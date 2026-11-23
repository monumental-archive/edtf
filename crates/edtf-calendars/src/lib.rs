// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Proleptic Julian (Old Style) → Gregorian conversion at the EDTF input
//! boundary.
//!
//! EDTF is proleptic-Gregorian by spec and stays that way — but pre-1918
//! Russian/Imperial sources, Orthodox liturgical dates, and most primary
//! material before a country's adoption date are written Julian (Old
//! Style). This crate converts proleptic Julian calendar dates to proleptic
//! Gregorian and hands the result to [`edtf_core`]'s model, so the
//! conversion is done once, deterministically, instead of hand-rolled per
//! ingest layer. Deterministic integer arithmetic (Julian Day Number
//! route), `#![no_std]`, zero dependencies beyond `edtf-core`.
//!
//! The traps this crate exists to get right, each pinned by tests:
//!
//! - The Julian–Gregorian offset is not constant: 10 days at the 1582 reform,
//!   11 from Gregorian 1700-03-01, 12 from 1800, 13 from 1900 — 1 March 1917
//!   O.S. is 14 March 1917 N.S.
//! - The Julian leap rule is every-4-years with no century exception: Julian
//!   1900-02-29 exists and converts (to Gregorian 1900-03-13), while Gregorian
//!   1900-02-29 stays invalid.
//! - Proleptic in both directions: defined before 1582 too. The offset shrinks
//!   going back, the calendars coincide across the 3rd century, and the Julian
//!   calendar is *ahead* before that (Julian 0001-01-01 is Gregorian
//!   0000-12-30).
//! - Year boundaries shift: Julian late-December dates land in the next
//!   Gregorian year — 25 December 1917 O.S. is 7 January 1918 N.S., the classic
//!   Orthodox-Christmas off-by-one-year error.
//! - Astronomical year numbering throughout (year 0 exists), consistent with
//!   `edtf-core`.
//! - **Precision honesty**: a Julian year or month does not convert to a single
//!   Gregorian year or month (Julian 1917 spans Gregorian
//!   1917-01-14..1918-01-13). [`convert`] returns sub-day-precision inputs as
//!   an EDTF interval from the first to the last Gregorian day — never a
//!   silently "converted" year.
//! - **Every value is EDTF**: [`convert`] builds its result through
//!   `edtf-core`'s constructors and refuses one EDTF cannot spell (a
//!   Gregorian day outside the years −9999 to 9999) with
//!   [`CalendarError::OutOfRange`]. [`parse_julian`] reads the text forms
//!   `Y`, `Y-MM` and `Y-MM-DD`.
//!
//! ```
//! use edtf_calendars::{JulianDate, convert, julian_to_gregorian, parse_julian};
//!
//! // The October Revolution: 25 October 1917 O.S. = 7 November 1917 N.S.
//! let g = julian_to_gregorian(JulianDate {
//!     year: 1917,
//!     month: 10,
//!     day: 25,
//! })
//! .unwrap();
//! assert_eq!(g.to_string(), "1917-11-07");
//!
//! // A Julian *year* is honestly a Gregorian range:
//! let (year, month, day) = parse_julian("1917").unwrap();
//! let value = convert(year, month, day).unwrap();
//! assert_eq!(value.to_string(), "1917-01-14/1918-01-13");
//! ```
//!
//! **Non-goals** (recorded in issue #21): other calendars (Islamic, Hebrew,
//! French Republican) until a real source demands them; England's 25-March
//! year start and dual-dating ("11 February 1731/32") — a known ingest
//! hazard the caller must resolve to an astronomical year *before* calling
//! this crate; and guessing which calendar a source used, which is the
//! application's vocabulary-backed qualifier, not arithmetic.
#![no_std]
#![expect(
    clippy::min_ident_chars,
    reason = "the conversions keep their published algorithms' own variable names (y, m, d, a); renaming them breaks the correspondence to the sources they are checked against"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "every flagged operation is bounded where it stands: slice indices by a length guard on the line above, digit values to 0-9 by the match arm that binds them, and the JDN forms by being computed in i128 so any i64 year fits. The operations that genuinely could leave range already use checked_/saturating_ and return an error rather than wrapping"
)]
#![expect(
    clippy::absolute_paths,
    reason = "a one-use std path written in full at the call site is clearer than an import that only appears once"
)]
#![expect(
    clippy::shadow_reuse,
    reason = "the algorithms shadow deliberately — the JDN forms rebind y and m as the published derivation does, step by step, and renaming each step would break the correspondence to the source"
)]
#![expect(
    clippy::missing_inline_in_public_items,
    reason = "inlining is the compiler's call across a crate boundary, and the release profile enables fat LTO — annotating every public item would assert a decision this crate has not measured"
)]
#![expect(
    clippy::as_conversions,
    reason = "the operands are proven in range by the guard or type immediately above each cast, and try_from at these sites would add an unreachable error path"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "these enums enumerate what ISO 8601-2 defines; the spec fixes the variants, so non_exhaustive would promise additions the format cannot make"
)]
#![expect(
    clippy::map_err_ignore,
    reason = "the source error carries no information the caller can act on; the replacement names the failing production instead"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "these types are the public data model of an ISO 8601-2 value; the spec fixes their fields, so a non_exhaustive that promised future additions would be a promise this format cannot make"
)]
#![expect(
    clippy::missing_trait_methods,
    reason = "the default implementations are what this type wants; overriding them to satisfy a lint would be code with no reason to exist"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching through a reference without restating & at every level is the idiomatic form the rest of this crate uses"
)]
#![expect(
    clippy::unreachable,
    reason = "an unreachable! whose comment names the caller-side check that makes it unreachable — a deliberate assertion of an invariant, not an unhandled case"
)]

use edtf_core::{BoundDate, Date, DateField, Edtf, Interval, IntervalEndpoint, ValueError, Year};

/// Error returned for malformed text, invalid calendar dates or
/// unrepresentable results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CalendarError {
    /// The text is not a Julian date in one of the three forms
    /// [`parse_julian`] reads: `Y`, `Y-MM` or `Y-MM-DD`.
    Malformed,
    /// The input is not a real calendar day (month outside 1–12, day
    /// beyond the month's length under the calendar's own leap rule, or a
    /// day given without a month).
    InvalidDate,
    /// The result has no EDTF spelling, or does not fit the numeric range
    /// this library computes with: [`convert`] refuses a Gregorian day
    /// outside the years −9999 to 9999, because a year past them needs
    /// EDTF's `Y` form, which carries no month or day (ISO 8601-2
    /// §4.7.2); [`parse_julian`] refuses a year too large for `i64`; and
    /// the day arithmetic refuses a converted year past `i64`.
    OutOfRange,
}

impl core::fmt::Display for CalendarError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "expected a Julian date as Y, Y-MM or Y-MM-DD",
            Self::InvalidDate => "not a valid calendar date",
            Self::OutOfRange => "converted year out of range",
        })
    }
}

impl core::error::Error for CalendarError {}

/// A proleptic Julian (Old Style) calendar date.
///
/// Year is astronomical numbering: 0 exists (and is a Julian leap year),
/// -1 precedes it. Ordering is calendrical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JulianDate {
    /// Astronomical year number.
    pub year: i64,
    /// Calendar month 1–12.
    pub month: u8,
    /// Calendar day of month 1–31.
    pub day: u8,
}

impl core::fmt::Display for JulianDate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.year < 0 {
            write!(
                f,
                "-{:04}-{:02}-{:02}",
                self.year.unsigned_abs(),
                self.month,
                self.day
            )
        } else {
            write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
        }
    }
}

/// Proleptic Julian leap rule: every fourth year, no century exception,
/// astronomical numbering (year 0 is a leap year, so are -4, -8, …).
#[must_use]
pub const fn is_julian_leap(year: i64) -> bool {
    year.rem_euclid(4) == 0
}

/// Days in `month` (1–12) under the given February length.
fn last_day(month: u8, leap: bool) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => unreachable!("caller validated month"),
    }
}

/// Validate a month/day pair against the given leap-year flag.
///
/// # Errors
///
/// [`CalendarError::InvalidDate`] when the month is outside 1..=12 or the
/// day is outside `1..=last_day` for that month.
fn check(month: u8, day: u8, leap: bool) -> Result<(), CalendarError> {
    if !(1..=12).contains(&month) || day == 0 || day > last_day(month, leap) {
        return Err(CalendarError::InvalidDate);
    }
    Ok(())
}

// Julian Day Number arithmetic (Fliegel–Van Flandern / Richards forms with
// floor division), computed in i128 so any i64 year is safe, valid for all
// proleptic dates in both calendars.

/// Julian Day Number of a proleptic Julian date.
const fn julian_jdn(y: i128, m: i128, d: i128) -> i128 {
    let a = (14 - m).div_euclid(12);
    let y = y + 4800 - a;
    let m = m + 12 * a - 3;
    d + (153 * m + 2).div_euclid(5) + 365 * y + y.div_euclid(4) - 32083
}

/// Split the era-day count `e` (per 4-year cycle) into (year-in-era, month,
/// day) — the shared tail of both JDN inversions.
const fn split(c: i128) -> (i128, u8, u8) {
    let d = (4 * c + 3).div_euclid(1461);
    let e = c - (1461 * d).div_euclid(4);
    let m = (5 * e + 2).div_euclid(153);
    let day = e - (153 * m + 2).div_euclid(5) + 1;
    let month = m + 3 - 12 * m.div_euclid(10);
    // month ∈ 3..=14 → 1..=12 after the +3-12·q fold; day ∈ 1..=31: both
    // are single-byte by construction of the civil-from-days algorithm.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "month in 1..=12 and day in 1..=31 by the algorithm's arithmetic"
    )]
    (d + m.div_euclid(10), month as u8, day as u8)
}

/// The proleptic Gregorian (year, month, day) a Julian Day Number denotes.
const fn jdn_to_gregorian(jdn: i128) -> (i128, u8, u8) {
    let a = jdn + 32044;
    let b = (4 * a + 3).div_euclid(146_097);
    let c = a - (146_097 * b).div_euclid(4);
    let (y, month, day) = split(c);
    (100 * b + y - 4800, month, day)
}

/// The proleptic Julian (year, month, day) a Julian Day Number denotes.
const fn jdn_to_julian(jdn: i128) -> (i128, u8, u8) {
    let (y, month, day) = split(jdn + 32082);
    (y - 4800, month, day)
}

/// Narrow a computed (year, month, day) triple to a [`BoundDate`].
///
/// # Errors
///
/// [`CalendarError::OutOfRange`] when the year does not fit `i64`. The
/// inversion yields a real Gregorian day, so the day itself is never refused.
fn to_bound_date(ymd: (i128, u8, u8)) -> Result<BoundDate, CalendarError> {
    let year = i64::try_from(ymd.0).map_err(|_| CalendarError::OutOfRange)?;
    BoundDate::new(year, ymd.1, ymd.2).map_err(|_| CalendarError::InvalidDate)
}

/// Convert a complete proleptic Julian date to its proleptic Gregorian day.
///
/// Rejects inputs that are not real Julian calendar days — but note that
/// Julian February 29 exists in *every* fourth year: `1900-02-29` (O.S.)
/// is valid here and converts to Gregorian `1900-03-13`.
///
/// # Errors
///
/// [`CalendarError::InvalidDate`] when the input is not a real Julian
/// calendar day; [`CalendarError::OutOfRange`] when the converted year
/// exceeds the numeric range this library computes with.
pub fn julian_to_gregorian(date: JulianDate) -> Result<BoundDate, CalendarError> {
    check(date.month, date.day, is_julian_leap(date.year))?;
    let jdn = julian_jdn(
        i128::from(date.year),
        i128::from(date.month),
        i128::from(date.day),
    );
    to_bound_date(jdn_to_gregorian(jdn))
}

/// Convert a complete proleptic Gregorian date to its proleptic Julian
/// (Old Style) date — the round-trip partner of [`julian_to_gregorian`].
///
/// Every [`BoundDate`] is a real Gregorian calendar day (`edtf-core` checks
/// it at construction), so Gregorian `1900-02-29` cannot reach here.
///
/// # Errors
///
/// [`CalendarError::OutOfRange`] when the converted year exceeds the
/// numeric range this library computes with.
pub fn gregorian_to_julian(date: BoundDate) -> Result<JulianDate, CalendarError> {
    let jdn = date.julian_day_number();
    let (year, month, day) = jdn_to_julian(jdn);
    Ok(JulianDate {
        year: i64::try_from(year).map_err(|_| CalendarError::OutOfRange)?,
        month,
        day,
    })
}

/// Read a Julian date written `Y`, `Y-MM` or `Y-MM-DD` into the parts
/// [`convert`] takes.
///
/// The year is astronomical: ASCII digits, any number of them, after an
/// optional `-` (no `+`). The month and day are two ASCII digits each.
/// Only the shape is read here; whether the parts name a real Julian day
/// is [`convert`]'s question.
///
/// ```
/// use edtf_calendars::{CalendarError, parse_julian};
///
/// assert_eq!(parse_julian("-0044-03-15"), Ok((-44, Some(3), Some(15))));
/// assert_eq!(parse_julian("1917-2"), Err(CalendarError::Malformed));
/// ```
///
/// # Errors
///
/// [`CalendarError::Malformed`] for any other text;
/// [`CalendarError::OutOfRange`] for a year of the right shape too large
/// for `i64`.
pub fn parse_julian(input: &str) -> Result<(i64, Option<u8>, Option<u8>), CalendarError> {
    let (negative, rest) = input
        .strip_prefix('-')
        .map_or((false, input), |rest| (true, rest));
    let mut parts = rest.split('-');
    let year_digits = parts.next().unwrap_or_default();
    if year_digits.is_empty() || !year_digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CalendarError::Malformed);
    }
    let two_digit = |part: Option<&str>| match part.map(str::as_bytes) {
        None => Ok(None),
        Some(&[tens @ b'0'..=b'9', units @ b'0'..=b'9']) => {
            Ok(Some((tens - b'0') * 10 + (units - b'0')))
        }
        Some(_) => Err(CalendarError::Malformed),
    };
    let month = two_digit(parts.next())?;
    let day = two_digit(parts.next())?;
    if parts.next().is_some() {
        return Err(CalendarError::Malformed);
    }
    // The shape is right, so a year that will not fit is a range error.
    let year = year_digits
        .bytes()
        .try_fold(0_i128, |v, b| {
            v.checked_mul(10)?.checked_add(i128::from(b - b'0'))
        })
        .and_then(|magnitude| i64::try_from(if negative { -magnitude } else { magnitude }).ok())
        .ok_or(CalendarError::OutOfRange)?;
    Ok((year, month, day))
}

/// The first and last Gregorian days of a possibly partial Julian date.
///
/// # Errors
///
/// As [`convert`], less the EDTF spelling.
fn gregorian_span(
    year: i64,
    month: Option<u8>,
    day: Option<u8>,
) -> Result<(BoundDate, BoundDate), CalendarError> {
    let (first, last) = match (month, day) {
        (Some(month), Some(day)) => {
            let only = JulianDate { year, month, day };
            (only, only)
        }
        (Some(month), None) => {
            if !(1..=12).contains(&month) {
                return Err(CalendarError::InvalidDate);
            }
            (
                JulianDate {
                    year,
                    month,
                    day: 1,
                },
                JulianDate {
                    year,
                    month,
                    day: last_day(month, is_julian_leap(year)),
                },
            )
        }
        (None, None) => (
            JulianDate {
                year,
                month: 1,
                day: 1,
            },
            JulianDate {
                year,
                month: 12,
                day: 31,
            },
        ),
        (None, Some(_)) => return Err(CalendarError::InvalidDate),
    };
    Ok((julian_to_gregorian(first)?, julian_to_gregorian(last)?))
}

/// The EDTF date naming the Gregorian day `day`.
///
/// Built through `edtf-core`'s constructors.
///
/// # Errors
///
/// [`ValueError`] when the year is past ±9999: it takes the `Y` form, and
/// [`Date::new`] refuses it a month. That refusal (or the year range,
/// D30) is the only one a real Gregorian day can meet.
fn edtf_date(day: BoundDate) -> Result<Date, ValueError> {
    Date::new(
        Year::from_value(day.year())?,
        Some(DateField::from_value(day.month())?),
        Some(DateField::from_value(day.day())?),
    )
}

/// Convert a possibly partial Julian date to the EDTF value it is.
///
/// - year + month + day → an [`Edtf::Date`], the exact Gregorian day;
/// - year + month → an [`Edtf::Interval`] over the Julian month;
/// - year only → an [`Edtf::Interval`] over the Julian year;
/// - a day without a month is [`CalendarError::InvalidDate`].
///
/// The interval is the whole point: Julian 1917 covers Gregorian
/// `1917-01-14/1918-01-13`, and Julian February 1900 covers Gregorian
/// `1900-02-13/1900-03-13` (the offset changes *inside* the month). This
/// function never pretends a Julian year or month is a Gregorian one.
///
/// The value is built through `edtf-core`'s constructors, so it is valid
/// EDTF by construction. [`julian_to_gregorian`] is the day arithmetic
/// alone, for any `i64` year.
///
/// ```
/// use edtf_calendars::{CalendarError, convert};
///
/// assert_eq!(convert(1917, Some(10), Some(25)).unwrap().to_string(), "1917-11-07");
/// // Julian 31 December 9999 is Gregorian 13 March 10000: no EDTF spelling.
/// assert_eq!(convert(9999, Some(12), Some(31)), Err(CalendarError::OutOfRange));
/// ```
///
/// # Errors
///
/// [`CalendarError::InvalidDate`] when the parts are not a real Julian
/// calendar day, month or year; [`CalendarError::OutOfRange`] when a
/// Gregorian day of the result falls outside the years −9999 to 9999,
/// which EDTF cannot spell with a month and day.
pub fn convert(year: i64, month: Option<u8>, day: Option<u8>) -> Result<Edtf, CalendarError> {
    let (first, last) = gregorian_span(year, month, day)?;
    let value = if day.is_some() {
        edtf_date(first).map(Edtf::Date)
    } else {
        edtf_date(first).and_then(|start| {
            Interval::new(
                IntervalEndpoint::Date(start),
                IntervalEndpoint::Date(edtf_date(last)?),
            )
            .map(Edtf::Interval)
        })
    };
    value.map_err(|_| CalendarError::OutOfRange)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::too_long_first_doc_paragraph,
        reason = "these describe test intent for a reader of the file, not a rustdoc summary"
    )]
    #![expect(
        clippy::missing_panics_doc,
        reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
    )]
    #![expect(
        clippy::let_underscore_untyped,
        reason = "the discarded value's type is fixed by the call it comes from"
    )]
    use super::*;

    // `last_day` is deliberately partial: every caller range-checks the
    // month first (`check` before the comparison, `convert` before the
    // month-span branch), so an out-of-range month is a caller bug, not a
    // calendar fact. These pin the guard's contract — misuse panics rather
    // than silently answering with some plausible-looking day count.

    #[test]
    #[should_panic(expected = "caller validated month")]
    fn last_day_below_range_panics() {
        let _ = last_day(0, false);
    }

    #[test]
    #[should_panic(expected = "caller validated month")]
    fn last_day_above_range_panics() {
        let _ = last_day(13, true);
    }

    /// The month-length table itself, direct: February is the only month the
    /// `leap` flag moves, and the flag is the *caller's* leap rule — this
    /// function takes the caller's leap rule and must not have an opinion
    /// of its own.
    #[test]
    fn last_day_table() {
        for month in [1, 3, 5, 7, 8, 10, 12] {
            assert_eq!(last_day(month, false), 31);
            assert_eq!(last_day(month, true), 31);
        }
        for month in [4, 6, 9, 11] {
            assert_eq!(last_day(month, false), 30);
            assert_eq!(last_day(month, true), 30);
        }
        assert_eq!(last_day(2, false), 28);
        assert_eq!(last_day(2, true), 29);
    }
}
