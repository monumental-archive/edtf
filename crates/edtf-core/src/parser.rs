// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Hand-written recursive-descent parser and validator for EDTF levels 0–2.
//!
//! Grammar and validation rules follow docs/spec-notes.md, which cites the
//! ISO 8601-1/-2:2019 sections for every production. The D-decisions are
//! implemented as resolved there.
//!
//! Every error carries the byte offset (into the original input) where the
//! problem was detected; sub-parsers receive a `base` offset so positions
//! stay absolute inside intervals, sets and date-times.

#![expect(
    clippy::min_ident_chars,
    reason = "a byte scanner's b, c, s and i, alongside a date's y, m and d, are this grammar's notation; spelling them out makes the productions harder to follow, not easier"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "every flagged operation is bounded where it stands: slice indices by a length guard on the line above, digit values to 0-9 by the match arm that binds them, and the JDN forms by being computed in i128 so any i64 year fits. The operations that genuinely could leave range already use checked_/saturating_ and return an error rather than wrapping"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "each index is preceded by the bounds check that justifies it, or indexes a fixed-size array whose length is in its type"
)]
#![expect(
    clippy::as_conversions,
    reason = "the operands are proven in range by the guard or type immediately above each cast, and try_from at these sites would add an unreachable error path"
)]
#![expect(
    clippy::string_slice,
    reason = "the slice boundaries are byte offsets the parser itself produced and has already proven to be char boundaries"
)]
#![expect(
    clippy::missing_asserts_for_indexing,
    reason = "the length guard immediately above the index is the assertion"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) states the intended visibility even where the module tree makes it redundant today"
)]
#![expect(
    clippy::single_char_lifetime_names,
    reason = "'a is the conventional name for the single borrowed input's lifetime"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "edtf-core declares every module private and exports only named types, so nothing flagged here is reachable from outside the crate and there is no published error contract to document"
)]

use alloc::vec::Vec;

use crate::types::{
    Date, DateField, DateTime, Edtf, Interval, IntervalEndpoint, ParseError, Qualifier, Set,
    SetElement, SetKind, SetRange, Time, TimeShift, ValueError, Year, YearKind,
};

/// Parse a whole EDTF expression, or report where it stopped being one.
pub(crate) fn parse(input: &str) -> Result<Edtf, ParseError> {
    if input.is_empty() {
        return Err(err(0, "empty input"));
    }
    if let Some(pos) = input.bytes().position(|b| !b.is_ascii()) {
        return Err(err(pos, "EDTF permits ASCII characters only"));
    }
    if let Some(pos) = input
        .bytes()
        .position(|b| b == b' ' || b.is_ascii_control())
    {
        return Err(err(pos, "whitespace is not allowed"));
    }
    match input.as_bytes()[0] {
        b'{' | b'[' => parse_set(input),
        _ if input.contains('/') => parse_interval(input),
        _ if input.contains('T') => parse_datetime(input),
        _ => parse_date_at(input, 0).map(Edtf::Date),
    }
}

/// A parse error at `offset` with a fixed message.
const fn err(offset: usize, message: &'static str) -> ParseError {
    ParseError { message, offset }
}

/// Byte offset of `part` (a slice borrowed from `whole`) within `whole`.
fn offset_in(whole: &str, part: &str) -> usize {
    (part.as_ptr() as usize).saturating_sub(whole.as_ptr() as usize)
}

// ---------------------------------------------------------------- cursor

/// A cursor over the input's bytes, carrying its offset in the original.
struct Cur<'a> {
    /// The bytes still being scanned.
    b: &'a [u8],
    /// Index of the next byte in `b`.
    i: usize,
    /// Offset of `b[0]` within the original input.
    base: usize,
}

impl<'a> Cur<'a> {
    /// A cursor over `s`, whose first byte sits at `base` in the original.
    const fn new(s: &'a str, base: usize) -> Self {
        Self {
            b: s.as_bytes(),
            i: 0,
            base,
        }
    }

    /// The current position, in the original input's coordinates.
    const fn pos(&self) -> usize {
        self.base + self.i
    }

    /// A parse error at the current position.
    const fn fail(&self, message: &'static str) -> ParseError {
        err(self.pos(), message)
    }

    /// The next byte without consuming it.
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// Consume and return the next byte.
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }

    /// Consume the next byte if it is `c`; report whether it was.
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    /// Whether the cursor has reached the end of its input.
    const fn eof(&self) -> bool {
        self.i >= self.b.len()
    }

    /// Consume a trailing qualifier character, if one is present.
    fn take_qualifier(&mut self) -> Option<Qualifier> {
        let q = match self.peek()? {
            b'?' => Qualifier {
                uncertain: true,
                approximate: false,
            },
            b'~' => Qualifier {
                uncertain: false,
                approximate: true,
            },
            b'%' => Qualifier {
                uncertain: true,
                approximate: true,
            },
            _ => return None,
        };
        self.i += 1;
        Some(q)
    }

    /// Consume consecutive digits into an i64; returns (value, digit count).
    fn take_number(&mut self) -> Result<(i64, usize), ParseError> {
        let start = self.i;
        let mut v: i64 = 0;
        while let Some(d @ b'0'..=b'9') = self.peek() {
            self.i += 1;
            v = v
                .checked_mul(10)
                .and_then(|x| x.checked_add(i64::from(d - b'0')))
                .ok_or_else(|| err(self.base + start, "number out of supported range"))?;
        }
        Ok((v, self.i - start))
    }

    /// Two characters, each a digit or `X`.
    fn take_two(&mut self, what: &'static str) -> Result<[Option<u8>; 2], ParseError> {
        let mut out = [None; 2];
        for slot in &mut out {
            match self.bump() {
                Some(d @ b'0'..=b'9') => *slot = Some(d - b'0'),
                Some(b'X') => *slot = None,
                _ => {
                    return Err(ParseError {
                        message: match what {
                            "month" => "month must be two digits (or X)",
                            _ => "day must be two digits (or X)",
                        },
                        offset: self.base + self.i.saturating_sub(1),
                    });
                }
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- dates

#[expect(
    clippy::too_many_lines,
    reason = "one linear scan; splitting scatters the offset bookkeeping"
)]
/// Parse a date whose first byte sits at `base` in the original input.
pub(crate) fn parse_date_at(s: &str, base: usize) -> Result<Date, ParseError> {
    // Every caller (dispatcher, interval endpoints, set elements) rejects
    // empty slices with its own message first; a graceful error beats a
    // panic if that invariant ever breaks.
    if s.is_empty() {
        return Err(err(base, "empty date"));
    }
    if s.as_bytes()[0] == b'Y' {
        return parse_prefixed_year(s, base);
    }
    let mut c = Cur::new(s, base);

    // Year: optional individual qualifier, optional '-', exactly four digit/X.
    let mut year_qual = Qualifier::default();
    if let Some(q) = c.take_qualifier() {
        year_qual.merge(q);
    }
    let negative = c.eat(b'-');
    let year_off = c.pos();
    let mut digits = [None; 4];
    for slot in &mut digits {
        match c.bump() {
            Some(d @ b'0'..=b'9') => *slot = Some(d - b'0'),
            Some(b'X') => *slot = None,
            _ => {
                return Err(err(
                    c.pos().saturating_sub(1),
                    "year must have exactly four digits (or X)",
                ));
            }
        }
    }
    let mut year = Year::new(YearKind::Standard { negative, digits }).map_err(|e| {
        // A sign that makes no year is reported at the sign itself.
        at(
            e,
            year_off - usize::from(e.message() == ValueError::NEGATIVE_ZERO),
        )
    })?;

    // Optional S<digits> significant-digit suffix (year-precision only). A
    // missing count is a count of zero, which no year admits.
    if c.eat(b'S') {
        let marker = c.pos() - 1;
        let count_off = c.pos();
        let (n, _) = c.take_number()?;
        year = year
            .with_significant_digits(u32::try_from(n).unwrap_or(u32::MAX))
            .map_err(|e| {
                at(
                    e,
                    if e.message() == ValueError::SIGNIFICANT_MASKED {
                        marker
                    } else {
                        count_off
                    },
                )
            })?;
    }

    // Optional trailing qualifier (group: year, or complete if year-only).
    if let Some(q) = c.take_qualifier() {
        year_qual.merge(q);
    }

    if c.eof() {
        return finish_date(
            year.with_qualifier(year_qual),
            None,
            None,
            year_off,
            year_off,
        );
    }
    if year.significant_digits().is_some() {
        return Err(c.fail(ValueError::SIGNIFICANT_YEAR_ONLY));
    }
    if !c.eat(b'-') {
        return Err(c.fail("expected '-' before month"));
    }

    // Month.
    let mut month_qual = Qualifier::default();
    if let Some(q) = c.take_qualifier() {
        month_qual.merge(q);
    }
    let month_off = c.pos();
    let month_digits = c.take_two("month")?;
    if let Some(q) = c.take_qualifier() {
        // Group qualification: applies to this component and everything left.
        month_qual.merge(q);
        year_qual.merge(q);
    }

    if c.eof() {
        let month = field(month_digits, month_qual, month_off)?;
        return finish_date(
            year.with_qualifier(year_qual),
            Some(month),
            None,
            month_off,
            month_off,
        );
    }
    if !c.eat(b'-') {
        return Err(c.fail("expected '-' before day"));
    }

    // Day.
    let mut day_qual = Qualifier::default();
    if let Some(q) = c.take_qualifier() {
        day_qual.merge(q);
    }
    let day_off = c.pos();
    let day_digits = c.take_two("day")?;
    if let Some(q) = c.take_qualifier() {
        // Trailing qualifier after the last component = complete qualification.
        day_qual.merge(q);
        month_qual.merge(q);
        year_qual.merge(q);
    }
    if !c.eof() {
        return Err(c.fail("unexpected characters after day"));
    }
    finish_date(
        year.with_qualifier(year_qual),
        Some(field(month_digits, month_qual, month_off)?),
        Some(field(day_digits, day_qual, day_off)?),
        month_off,
        day_off,
    )
}

/// The parser's error for a constructor's `e`, at `offset`.
const fn at(e: ValueError, offset: usize) -> ParseError {
    err(offset, e.message())
}

/// A month or day field of scanned digits, qualified.
fn field(
    digits: [Option<u8>; 2],
    qualifier: Qualifier,
    off: usize,
) -> Result<DateField, ParseError> {
    DateField::new(digits)
        .map(|f| f.with_qualifier(qualifier))
        .map_err(|e| at(e, off))
}

/// Build the date, placing a month error at the month and any other at the
/// day: the month's rules are asked first so their offset is the month's.
fn finish_date(
    year: Year,
    month: Option<DateField>,
    day: Option<DateField>,
    month_off: usize,
    day_off: usize,
) -> Result<Date, ParseError> {
    if let Some(m) = month {
        Date::check_month(&year, m).map_err(|e| at(e, month_off))?;
    }
    Date::new(year, month, day).map_err(|e| at(e, day_off))
}

/// `Y…` years (ISO 8601-2 §4.7.2-4.7.4): year precision only, |value| > 9999.
///
/// The year's rules, its range among them (decision D30), are the model's:
/// a written-out run too wide for `i64` and an exponent too wide for `u32`
/// are handed on as values past that range, so every way of writing too
/// large a year is the same error.
fn parse_prefixed_year(s: &str, base: usize) -> Result<Date, ParseError> {
    let mut c = Cur::new(s, base);
    c.eat(b'Y');
    let negative = c.eat(b'-');
    let value_off = c.pos();
    let out_of_range = || err(value_off, ValueError::YEAR_RANGE);
    let (mantissa, mantissa_len) = c.take_number().map_err(|_too_wide| out_of_range())?;
    if mantissa_len == 0 {
        return Err(c.fail("expected digits after 'Y'"));
    }
    // Leading zeros are only meaningful in four-digit years (P1 4.5); in a
    // Y-year they would also desynchronize the S-digit budget from the
    // canonical (zero-stripped) rendering (decision D20).
    if mantissa > 0 && mantissa_len as u64 != u64::from(mantissa.ilog10() + 1) {
        return Err(err(
            value_off,
            "leading zeros are not allowed in Y-prefixed years",
        ));
    }
    let signed = if negative { -mantissa } else { mantissa };
    let kind = if c.eat(b'E') {
        let exp_off = c.pos();
        let (exponent, exp_len) = c.take_number().map_err(|_too_wide| out_of_range())?;
        if exp_len == 0 {
            return Err(err(exp_off, "expected digits after exponent 'E'"));
        }
        YearKind::Exponential {
            significand: signed,
            exponent: u32::try_from(exponent).unwrap_or(u32::MAX),
        }
    } else {
        YearKind::Big { value: signed }
    };
    let mut year = Year::new(kind).map_err(|e| at(e, value_off))?;
    if c.eat(b'S') {
        let count_off = c.pos();
        let (n, _) = c.take_number()?;
        year = year
            .with_significant_digits(u32::try_from(n).unwrap_or(u32::MAX))
            .map_err(|e| at(e, count_off))?;
    }
    let mut qualifier = Qualifier::default();
    if let Some(q) = c.take_qualifier() {
        qualifier.merge(q);
    }
    if !c.eof() {
        return Err(c.fail(ValueError::Y_YEAR_ONLY));
    }
    Date::new(year.with_qualifier(qualifier), None, None).map_err(|e| at(e, value_off))
}

// ---------------------------------------------------------------- datetime

/// Parse a date-time expression.
fn parse_datetime(s: &str) -> Result<Edtf, ParseError> {
    // The dispatcher only routes strings containing 'T' here; a graceful
    // error beats a library panic if that invariant ever breaks.
    let Some((date_part, time_part)) = s.split_once('T') else {
        return Err(err(0, "datetime requires 'T'"));
    };
    let date = parse_plain_complete_date(date_part)?;
    let time = parse_time(time_part, date_part.len() + 1)?;
    DateTime::new(date, time)
        .map(Edtf::DateTime)
        .map_err(|e| at(e, 0))
}

/// Date-times exist only at level 0: a plain, complete, unqualified
/// YYYY-MM-DD (Annex A.4.3).
fn parse_plain_complete_date(s: &str) -> Result<Date, ParseError> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(err(0, "date-times require a complete YYYY-MM-DD date"));
    }
    let digit = |i: usize| -> Result<Option<u8>, ParseError> {
        match b[i] {
            d @ b'0'..=b'9' => Ok(Some(d - b'0')),
            _ => Err(err(i, "date-times require a plain all-digit date")),
        }
    };
    let digits = [digit(0)?, digit(1)?, digit(2)?, digit(3)?];
    let year = Year::new(YearKind::Standard {
        negative: false,
        digits,
    })
    .map_err(|e| at(e, 0))?;
    let month = field([digit(5)?, digit(6)?], Qualifier::default(), 5)?;
    let day = field([digit(8)?, digit(9)?], Qualifier::default(), 8)?;
    finish_date(year, Some(month), Some(day), 5, 8)
}

/// Parse the time half of a date-time.
fn parse_time(s: &str, base: usize) -> Result<Time, ParseError> {
    let b = s.as_bytes();
    if b.len() < 8 || b[2] != b':' || b[5] != b':' {
        return Err(err(base, "time must be hh:mm:ss"));
    }
    let two = |i: usize| -> Result<u8, ParseError> {
        match (b[i], b[i + 1]) {
            (t @ b'0'..=b'9', o @ b'0'..=b'9') => Ok((t - b'0') * 10 + (o - b'0')),
            _ => Err(err(base + i, "time components must be two digits")),
        }
    };
    let hour = two(0)?;
    let minute = two(3)?;
    let second = two(6)?;
    let time = Time::new(hour, minute, second, None).map_err(|e| {
        let component = if e.message() == ValueError::MINUTE {
            3
        } else if e.message() == ValueError::SECOND {
            6
        } else {
            0
        };
        at(e, base + component)
    })?;
    let rest = &s[8..];
    let shift = if rest.is_empty() {
        None
    } else if rest == "Z" {
        Some(TimeShift::UTC)
    } else {
        Some(parse_shift(rest, base + 8)?)
    };
    Ok(time.with_shift(shift))
}

/// Parse a UTC designator or numeric time-zone shift.
fn parse_shift(s: &str, base: usize) -> Result<TimeShift, ParseError> {
    let b = s.as_bytes();
    let negative = match b[0] {
        b'+' => false,
        b'-' => true,
        _ => return Err(err(base, "time shift must be Z, +hh, +hh:mm or negative")),
    };
    let (hours, minutes, hours_only) = match b.len() {
        3 => {
            let h = shift_two(b, 1, base)?;
            (h, 0, true)
        }
        6 if b[3] == b':' => {
            let h = shift_two(b, 1, base)?;
            let m = shift_two(b, 4, base)?;
            (h, m, false)
        }
        _ => return Err(err(base, "time shift must be \u{b1}hh or \u{b1}hh:mm")),
    };
    if minutes > 59 {
        return Err(err(base + 4, "shift minutes must be 00-59"));
    }
    let total = i16::from(hours) * 60 + i16::from(minutes);
    if negative && total == 0 {
        return Err(err(base, "negative zero time shift is not allowed"));
    }
    TimeShift::offset(if negative { -total } else { total }, hours_only).map_err(|e| at(e, base))
}

/// Read a two-digit shift component at `i`.
const fn shift_two(b: &[u8], i: usize, base: usize) -> Result<u8, ParseError> {
    match (b[i], b[i + 1]) {
        (t @ b'0'..=b'9', o @ b'0'..=b'9') => Ok((t - b'0') * 10 + (o - b'0')),
        _ => Err(err(base + i, "time shift components must be two digits")),
    }
}

// ---------------------------------------------------------------- intervals

/// Parse an interval expression.
fn parse_interval(s: &str) -> Result<Edtf, ParseError> {
    let mut parts = s.splitn(3, '/');
    let start_str = parts.next().unwrap_or("");
    let end_str = parts
        .next()
        .ok_or_else(|| err(s.len(), "interval requires '/'"))?;
    if parts.next().is_some() {
        return Err(err(
            offset_in(s, end_str) + end_str.len(),
            "interval must contain exactly one '/'",
        ));
    }
    let start = parse_endpoint(start_str, offset_in(s, start_str), true)?;
    let end = parse_endpoint(end_str, offset_in(s, end_str), false)?;
    Interval::new(start, end).map(Edtf::Interval).map_err(|e| {
        at(
            e,
            if e.message() == ValueError::INTERVAL_ORDER {
                offset_in(s, end_str)
            } else {
                0
            },
        )
    })
}

/// Parse one interval endpoint. A `..` on the wrong side is refused here,
/// at its text, before the date beside it is read.
fn parse_endpoint(s: &str, base: usize, is_start: bool) -> Result<IntervalEndpoint, ParseError> {
    if s.is_empty() {
        return Ok(IntervalEndpoint::Unknown);
    }
    if s == ".." {
        return Ok(IntervalEndpoint::Open);
    }
    if let Some(rest) = s.strip_prefix("..") {
        if !is_start {
            return Err(err(base, ValueError::END_ON_OR_BEFORE));
        }
        return Ok(IntervalEndpoint::OnOrBefore(parse_date_at(rest, base + 2)?));
    }
    if let Some(rest) = s.strip_suffix("..") {
        if is_start {
            return Err(err(base + rest.len(), ValueError::START_ON_OR_AFTER));
        }
        return Ok(IntervalEndpoint::OnOrAfter(parse_date_at(rest, base)?));
    }
    Ok(IntervalEndpoint::Date(parse_date_at(s, base)?))
}

// ---------------------------------------------------------------- sets

/// Parse a set expression.
fn parse_set(s: &str) -> Result<Edtf, ParseError> {
    let b = s.as_bytes();
    let (kind, close) = match b[0] {
        b'{' => (SetKind::AllMembers, b'}'),
        _ => (SetKind::OneMember, b']'),
    };
    if b.len() < 2 || b[b.len() - 1] != close {
        return Err(err(s.len().saturating_sub(1), "unterminated set"));
    }
    let inner = &s[1..s.len() - 1];
    if inner.is_empty() {
        return Err(err(1, ValueError::SET_EMPTY));
    }
    if let Some(pos) = inner
        .bytes()
        .position(|c| matches!(c, b'{' | b'}' | b'[' | b']'))
    {
        return Err(err(1 + pos, "sets cannot nest"));
    }
    let mut elements = Vec::new();
    for part in inner.split(',') {
        let part_off = offset_in(s, part);
        if part.is_empty() {
            return Err(err(part_off, "empty set element"));
        }
        elements.push(parse_set_element(part, part_off)?);
    }
    Set::new(kind, elements)
        .map(Edtf::Set)
        .map_err(|e| at(e, 1))
}

/// Parse one set element, which may itself be a range.
fn parse_set_element(p: &str, base: usize) -> Result<SetElement, ParseError> {
    if p == ".." {
        return Err(err(base, "'..' alone is not a set element"));
    }
    if let Some(rest) = p.strip_prefix("..") {
        return Ok(SetElement::OnOrBefore(parse_date_at(rest, base + 2)?));
    }
    if let Some(rest) = p.strip_suffix("..") {
        return Ok(SetElement::OnOrAfter(parse_date_at(rest, base)?));
    }
    if let Some(idx) = p.find("..") {
        let (a, b) = (&p[..idx], &p[idx + 2..]);
        let end_off = base + idx + 2;
        if b.contains("..") {
            return Err(err(end_off, "set element cannot contain multiple '..'"));
        }
        let from = parse_date_at(a, base)?;
        let to = parse_date_at(b, end_off)?;
        // A range of the start alone breaks only the start's own rules
        // (D27), so asking it first places those errors at the start.
        SetRange::new(from, from).map_err(|e| at(e, base))?;
        return SetRange::new(from, to)
            .map(SetElement::Range)
            .map_err(|e| at(e, end_off));
    }
    Ok(SetElement::Date(parse_date_at(p, base)?))
}

// ------------------------------------------------------------ guard tests
//
// The parser's own callers already exclude the inputs below, so these
// defensive arms are unreachable through `parse`. They are the contract for
// misuse of the private/`pub(crate)` entry points — a positioned error or a
// panic, never a silent wrong answer — and only a white-box test can pin it.

#[cfg(test)]
mod tests {
    #![expect(
        clippy::missing_panics_doc,
        reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
    )]
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a panic here is the failure signal, not a crash path"
    )]

    use super::*;

    /// Every caller filters empty slices first; the fallback must still be a
    /// positioned error rather than an index panic on the first byte.
    #[test]
    fn empty_date_slice_errors_at_its_base_offset() {
        let e = parse_date_at("", 7).unwrap_err();
        assert_eq!(e.message, "empty date");
        assert_eq!(e.offset, 7);
    }

    /// A digit run too wide for `i64` is real input, not only a guard.
    ///
    /// It has to be reported at the run's first digit rather than wrap around. An
    /// `S` count keeps `take_number`'s own message; in a year the same
    /// overflow is the one year-range error (decision D30).
    #[test]
    fn an_oversized_digit_run_errors_at_the_first_digit() {
        assert_err(
            "Y17E7S99999999999999999999",
            "number out of supported range",
            6,
        );
    }

    /// An exponent is limited only by the year it produces (decision D30):
    /// a run too wide for `i64`, and one that fits `i64` but not `u32`, are
    /// both the year-range error at the year's first digit.
    #[test]
    fn an_oversized_exponent_is_the_year_range_error() {
        assert_err("Y1E99999999999999999999", "year out of supported range", 1);
        assert_err("Y1E4294967296", "year out of supported range", 1);
    }

    /// The dispatcher only routes strings containing `/` here; without one the
    /// missing second half is reported at end of input.
    #[test]
    fn interval_without_a_slash_errors_at_end_of_input() {
        let e = parse_interval("1985").unwrap_err();
        assert_eq!(e.message, "interval requires '/'");
        assert_eq!(e.offset, 4);
    }

    /// The dispatcher only routes `T`-bearing strings here; without one the
    /// split fails and the error must say so.
    #[test]
    fn datetime_without_t_errors() {
        let e = parse_datetime("1985-04-12").unwrap_err();
        assert_eq!(e.message, "datetime requires 'T'");
        assert_eq!(e.offset, 0);
    }

    /// A date-time's month and day are the date's: the same rules, the same
    /// messages, at the month and the day of the `YYYY-MM-DD` it requires.
    #[test]
    fn datetime_month_and_day_follow_the_date_rules() {
        let month = "month must be 01-12 or a sub-year code 21-41";
        assert_err("2020-13-01T00:00:00", month, 5);
        assert_err(
            "2020-21-01T00:00:00",
            "sub-year groupings cannot carry a day",
            8,
        );
        assert_err(
            "2019-02-29T00:00:00",
            "day is out of range for the month",
            8,
        );
    }

    /// Assert `input` fails with exactly this message at exactly this offset.
    fn assert_err(input: &str, message: &str, offset: usize) {
        let e = parse(input).unwrap_err();
        assert_eq!((e.message, e.offset), (message, offset), "{input:?}");
    }

    /// A space and a control character are each refused on their own, at
    /// their own byte, before any grammar runs.
    #[test]
    fn whitespace_and_control_bytes_are_each_refused() {
        assert_err("1985-01-01 ", "whitespace is not allowed", 10);
        assert_err("1985\t", "whitespace is not allowed", 4);
    }

    /// Error offsets inside a date are absolute: each component's position is
    /// added to the slice's base, so a date that is not the whole input (an
    /// interval end, a set element) reports where it really sits.
    #[test]
    fn date_errors_are_positioned_from_the_slice_base() {
        assert_err("1985-1", "month must be two digits (or X)", 5);
        assert_err("1985/1986-1", "month must be two digits (or X)", 10);
        assert_err("-0000", "-0000 is not a valid year", 0);
        assert_err(
            "198XS2",
            "significant digits cannot combine with unspecified digits",
            4,
        );
    }

    /// A masked month or year digit admits only the completions matching its
    /// specified digit: `X4` is April alone (no 31st), and a year ending in 1
    /// is never leap while one ending in 4 can be.
    #[test]
    fn masked_digits_admit_only_matching_completions() {
        assert_err("1985-X4-31", "day is out of range for the month", 8);
        assert_err("XXX1-02-29", "day is out of range for the month", 8);
        parse("XXX4-02-29").unwrap();
    }

    /// The S-digit count must be at least 1 and within the digit count, and
    /// a missing count is refused rather than read as no suffix.
    #[test]
    fn y_year_significant_digits_beyond_the_digit_count_are_refused() {
        let msg = "significant digits exceed the year's digit count";
        assert_err("Y12345S6", msg, 7);
        assert_err("Y12345S", msg, 7);
        assert_err("Y12345S0", msg, 7);
    }

    /// Every separator of a date-time is checked, each on its own: a wrong
    /// byte in any one position is enough to refuse the shape.
    #[test]
    fn datetime_separators_are_each_required() {
        let date = "date-times require a complete YYYY-MM-DD date";
        assert_err("1985X04-12T10:00:00", date, 0);
        assert_err("1985-04X12T10:00:00", date, 0);
        assert_err("1985-04-1T10:00:00", date, 0);
        assert_err("1985-04-12T10x00:00", "time must be hh:mm:ss", 11);
        assert_err("1985-04-12T10:00x00", "time must be hh:mm:ss", 11);
        assert_err(
            "1985-04-12T10:00:00+01x30",
            "time shift must be \u{b1}hh or \u{b1}hh:mm",
            19,
        );
    }

    /// Time and shift errors sit at the offending component, counted from
    /// the start of the whole input.
    #[test]
    fn time_errors_are_positioned_at_their_component() {
        assert_err(
            "1985-04-12T10:0a:00",
            "time components must be two digits",
            14,
        );
        assert_err("1985-04-12T10:60:00", "minute must be 00-59", 14);
        assert_err("1985-04-12T10:00:61", "second must be 00-60", 17);
        assert_err(
            "1985-04-12T10:00:00+01:60",
            "shift minutes must be 00-59",
            23,
        );
        let two = "time shift components must be two digits";
        assert_err("1985-04-12T10:00:00+0a", two, 20);
        assert_err("1985-04-12T10:00:00+01:0a", two, 23);
    }

    /// Interval errors sit where the offending part starts or ends.
    #[test]
    fn interval_errors_are_positioned_at_the_offending_part() {
        assert_err("1985/1986/1987", "interval must contain exactly one '/'", 9);
        assert_err(
            "..1985-13/1990",
            "month must be 01-12 or a sub-year code 21-41",
            7,
        );
        assert_err(
            "1985../1990",
            "'..'-suffixed date is only allowed as interval end",
            4,
        );
    }

    /// An interval may start and end on the same day: only an end strictly
    /// before the start is out of order.
    #[test]
    fn a_one_day_interval_is_in_order() {
        parse("1985-01-01/1985-01-01").unwrap();
    }

    /// `{}` is a terminated set with nothing in it, not an unterminated one.
    #[test]
    fn an_empty_set_is_refused_as_empty_not_unterminated() {
        assert_err("{}", "set must contain at least one element", 1);
        assert_err("{", "unterminated set", 0);
    }

    /// Set errors sit at the offending element or range end, counted from
    /// the start of the whole input.
    #[test]
    fn set_errors_are_positioned_at_the_offending_element() {
        assert_err("{1985,{1986}}", "sets cannot nest", 6);
        let month = "month must be 01-12 or a sub-year code 21-41";
        assert_err("{..1985-13}", month, 8);
        assert_err("{1980,1985..1986-13}", month, 17);
        assert_err(
            "{1980,1985..1986..1987}",
            "set element cannot contain multiple '..'",
            12,
        );
        assert_err(
            "{1980,1985..1986~}",
            "set range endpoints cannot be qualified",
            12,
        );
        assert_err(
            "{1980,1985..1986-01}",
            "set range endpoints must share precision",
            12,
        );
        assert_err(
            "{1980,1986..1985}",
            "set range end precedes range start",
            12,
        );
    }
}
