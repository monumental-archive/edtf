// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Earliest/latest calendar-day bounds for EDTF expressions.
//!
//! Every EDTF expression denotes a region of the time axis. `bounds()`
//! reports the earliest and latest proleptic-Gregorian calendar day that
//! region touches — the primitive that range queries (`edtf_min`/`edtf_max`
//! in SQL, filtering in an application) are built from.
//!
//! Conventions (documented, spec-derived where the spec speaks):
//! - Bounds are day-granular; time-of-day refines within a day and does not
//!   change them.
//! - Qualification (`?~%`) does not move bounds: `1985~` still bounds to the
//!   calendar year 1985 (ISO 8601-2 §8.4.2 NOTE — approximation widens
//!   confidence, not the written value).
//! - Unspecified digits bound to the full set of matching completions; `XXXX`
//!   is "a four-digit year" (ISO 8601-2 §9.2.1.2 c 4), so it bounds to
//!   0000-01-01..9999-12-31.
//! - Season/sub-year boundaries use the fixed month table in
//!   `docs/spec-notes.md` D12 (ISO leaves seasons location-dependent).
//! - Open interval ends (`..`) are infinite; unknown ends (empty) are
//!   [`Bound::Unknown`].

#![expect(
    clippy::min_ident_chars,
    reason = "y, m and d are the universal notation for a date's components, and this crate is about little else"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "every flagged operation is bounded where it stands: slice indices by a length guard on the line above, digit values to 0-9 by the match arm that binds them, and the JDN forms by being computed in i128 so any i64 year fits. The operations that genuinely could leave range already use checked_/saturating_ and return an error rather than wrapping"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching through a reference without restating & at every level is the idiomatic form the rest of this crate uses"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) states the intended visibility even where the module tree makes it redundant today"
)]
#![expect(
    clippy::absolute_paths,
    reason = "a one-use std path written in full at the call site is clearer than an import that only appears once"
)]
#![expect(
    clippy::as_conversions,
    reason = "the operands are proven in range by the guard or type immediately above each cast, and try_from at these sites would add an unreachable error path"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "each index is preceded by the bounds check that justifies it, or indexes a fixed-size array whose length is in its type"
)]
#![expect(
    clippy::integer_division_remainder_used,
    reason = "calendar arithmetic is integer division by definition — the leap rules are /4, /100 and /400, and a float would be wrong"
)]
#![expect(
    clippy::unreachable,
    reason = "an unreachable! whose comment names the caller-side check that makes it unreachable — a deliberate assertion of an invariant, not an unhandled case"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "these types are the public data model of an ISO 8601-2 value; the spec fixes their fields, so a non_exhaustive that promised future additions would be a promise this format cannot make"
)]
#![expect(
    clippy::missing_inline_in_public_items,
    reason = "inlining is the compiler's call across a crate boundary, and the release profile enables fat LTO — annotating every public item would assert a decision this crate has not measured"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "these enums enumerate what ISO 8601-2 defines; the spec fixes the variants, so non_exhaustive would promise additions the format cannot make"
)]
#![expect(
    clippy::integer_division,
    reason = "calendar arithmetic is integer division by definition — the leap rules are /4, /100 and /400, and a float would be wrong"
)]
#![expect(
    clippy::too_long_first_doc_paragraph,
    reason = "the items are crate-private, so these paragraphs render in no rustdoc summary; they are written to be read next to the code they describe"
)]

use crate::types::{
    Date, DateField, Edtf, Interval, IntervalEndpoint, Set, SetElement, ValueError, YearKind,
};

/// The rule a [`BoundDate`] keeps: it names a real proleptic-Gregorian day.
const BOUND_DAY: &str = "not a proleptic-Gregorian calendar day";

/// A concrete proleptic-Gregorian calendar day used as a bound.
///
/// Ordering is calendrical (year, then month, then day), and it agrees with
/// [`BoundDate::julian_day_number`]: every `BoundDate` is a real day. Year is
/// astronomical numbering: 0 is a leap year, -1 precedes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct BoundDate {
    /// Astronomical year number.
    year: i64,
    /// Calendar month 1–12.
    month: u8,
    /// Calendar day of month 1–31.
    day: u8,
}

impl BoundDate {
    /// The proleptic-Gregorian day `year`-`month`-`day`.
    ///
    /// ```
    /// use edtf_core::BoundDate;
    ///
    /// assert!(BoundDate::new(2000, 2, 29).is_ok());
    /// assert!(BoundDate::new(1900, 2, 29).is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// [`ValueError`] when the month is outside 1–12 or the day outside the
    /// month's length under the Gregorian leap rule.
    pub fn new(year: i64, month: u8, day: u8) -> Result<Self, ValueError> {
        if !(1..=12).contains(&month) || day == 0 || day > last_day(month, is_leap(year)) {
            return Err(ValueError::new(BOUND_DAY));
        }
        Ok(Self { year, month, day })
    }

    /// Astronomical year number.
    #[must_use]
    pub const fn year(self) -> i64 {
        self.year
    }

    /// Calendar month 1–12.
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Calendar day of month 1–31.
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// The Julian Day Number of this proleptic-Gregorian day: the count of
    /// days since noon, 1 January 4713 BC in the proleptic Julian calendar
    /// (astronomical year -4712), so `2000-01-01` is 2451545 and `-4713-11-24`
    /// is 0. Days before that origin are negative.
    ///
    /// This is the property any consumer mapping a bound into another date
    /// system needs (a Postgres `date` is a day count from 2000-01-01), and
    /// it is how `edtf-calendars` crosses into the Julian calendar.
    ///
    /// The function is total: it computes in `i128` with floor division
    /// (Fliegel–Van Flandern / Richards), so every `i64` year is safe.
    #[must_use]
    #[expect(
        clippy::shadow_reuse,
        reason = "the JDN form rebinds y and m as the published derivation does, step by step; renaming each step would break the correspondence to the source"
    )]
    pub const fn julian_day_number(self) -> i128 {
        let y = self.year as i128;
        let m = self.month as i128;
        let d = self.day as i128;
        let a = (14 - m).div_euclid(12);
        let y = y + 4800 - a;
        let m = m + 12 * a - 3;
        d + (153 * m + 2).div_euclid(5) + 365 * y + y.div_euclid(4) - y.div_euclid(100)
            + y.div_euclid(400)
            - 32045
    }

    /// The proleptic-Gregorian day after this one, or `None` past the last
    /// day of year `i64::MAX`.
    ///
    /// ```
    /// use edtf_core::BoundDate;
    ///
    /// let leap = BoundDate::new(2000, 2, 28).unwrap();
    /// assert_eq!(leap.next_day(), BoundDate::new(2000, 2, 29).ok());
    /// ```
    #[must_use]
    pub fn next_day(self) -> Option<Self> {
        if self.day < last_day(self.month, is_leap(self.year)) {
            Some(Self {
                day: self.day + 1,
                ..self
            })
        } else if self.month < 12 {
            Some(Self {
                year: self.year,
                month: self.month + 1,
                day: 1,
            })
        } else {
            Some(Self {
                year: self.year.checked_add(1)?,
                month: 1,
                day: 1,
            })
        }
    }

    /// The proleptic-Gregorian day before this one, or `None` before the
    /// first day of year `i64::MIN`. The mirror of [`BoundDate::next_day`].
    ///
    /// ```
    /// use edtf_core::BoundDate;
    ///
    /// let march = BoundDate::new(1900, 3, 1).unwrap();
    /// assert_eq!(march.previous_day(), BoundDate::new(1900, 2, 28).ok());
    /// ```
    #[must_use]
    pub fn previous_day(self) -> Option<Self> {
        if self.day > 1 {
            Some(Self {
                day: self.day - 1,
                ..self
            })
        } else if self.month > 1 {
            let month = self.month - 1;
            Some(Self {
                year: self.year,
                month,
                day: last_day(month, is_leap(self.year)),
            })
        } else {
            Some(Self {
                year: self.year.checked_sub(1)?,
                month: 12,
                day: 31,
            })
        }
    }

    /// The first day of year `year`.
    const fn first_of_year(year: i64) -> Self {
        Self {
            year,
            month: 1,
            day: 1,
        }
    }

    /// The last day of year `year`.
    const fn last_of_year(year: i64) -> Self {
        Self {
            year,
            month: 12,
            day: 31,
        }
    }
}

impl core::fmt::Display for BoundDate {
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

/// One side of a [`Bounds`] result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// Unbounded in the past (open interval start, `..date` endpoints).
    NegativeInfinity,
    /// A concrete calendar day.
    Date(BoundDate),
    /// Unbounded in the future (open interval end, `date..` endpoints).
    PositiveInfinity,
    /// Not determinable: an unknown interval end (an empty endpoint), and
    /// nothing else. Every date has computable bounds (decision D30).
    Unknown,
}

/// The earliest and latest calendar day an expression touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Earliest calendar day (inclusive).
    pub earliest: Bound,
    /// Latest calendar day (inclusive).
    pub latest: Bound,
}

impl Edtf {
    /// Compute the earliest/latest calendar-day bounds of this expression.
    #[must_use]
    pub fn bounds(&self) -> Bounds {
        match self {
            Self::Date(d) => date_bounds(d),
            Self::DateTime(dt) => date_bounds(&dt.date()),
            Self::Interval(iv) => interval_bounds(iv),
            Self::Set(s) => set_bounds(s),
        }
    }
}

/// The earliest and latest calendar day a single date expression touches.
fn date_bounds(d: &Date) -> Bounds {
    let (earliest, latest) = date_span(d);
    Bounds {
        earliest: Bound::Date(earliest),
        latest: Bound::Date(latest),
    }
}

/// The earliest and latest calendar day a single date touches. Every date
/// has both (decision D30): a year's value and its significant-digit sweep
/// fit `i64`, and every date has a calendar completion (D11).
pub(crate) fn date_span(d: &Date) -> (BoundDate, BoundDate) {
    let year = d.year();
    match (year.kind(), year.value()) {
        (YearKind::Standard { negative, digits }, _) if year.significant_digits().is_none() => {
            let edge = |ascending| {
                year_completions(negative, digits, ascending)
                    .find_map(|y| extremum_in_year(d, y, ascending))
                    .unwrap_or_else(|| unreachable!("a date has a calendar completion (D11)"))
            };
            (edge(true), edge(false))
        }
        // A year with significant digits, or a `Y` year, is year-precision:
        // its sweep is its span.
        (_, Some(value)) => {
            let (lo, hi) = significant_range(value, year.significant_digits(), year.width());
            (BoundDate::first_of_year(lo), BoundDate::last_of_year(hi))
        }
        (_, None) => unreachable!("only a four-digit year without significant digits is masked"),
    }
}

/// The year range denoted by a value with `S<precision>` significant digits
/// over a `width`-digit form (ISO 8601-2 §4.4.3): keep the leading
/// `precision` digits, sweep the rest 0..9. A year has at most 18 digits
/// (decision D30), so the sweep tops out at `10^18 - 1` and fits `i64`.
pub(crate) const fn significant_range(
    value: i64,
    precision: Option<u32>,
    width: u32,
) -> (i64, i64) {
    let Some(p) = precision else {
        return (value, value);
    };
    let modulus = 10_i64.pow(width - p);
    let mag = value.abs();
    let lo_mag = mag - mag.rem_euclid(modulus);
    let hi_mag = lo_mag + (modulus - 1);
    if value < 0 {
        (-hi_mag, -lo_mag)
    } else {
        (lo_mag, hi_mag)
    }
}

/// First and last month of each sub-year grouping code (docs/spec-notes.md
/// D12). `wraps` means the grouping ends in the following year.
fn season_months(code: u8) -> (u8, u8, bool) {
    match code {
        21 | 25 | 31 => (3, 5, false),  // spring (N), autumn (S)
        22 | 26 | 32 => (6, 8, false),  // summer (N), winter (S)
        23 | 27 | 29 => (9, 11, false), // autumn (N), spring (S)
        24 | 28 | 30 => (12, 2, true),  // winter (N), summer (S)
        33 => (1, 3, false),
        34 => (4, 6, false),
        35 => (7, 9, false),
        36 => (10, 12, false),
        37 => (1, 4, false),
        38 => (5, 8, false),
        39 => (9, 12, false),
        40 => (1, 6, false),
        41 => (7, 12, false),
        _ => unreachable!("validated season code"),
    }
}

/// Completions of a four-digit year, ascending or descending: the year
/// itself when every digit is given, else every year its digits match. A
/// masked year is non-negative (D21). The masked positions form an odometer
/// (most significant first), so consecutive counter values yield
/// consecutive matching years in order.
fn year_completions(
    negative: bool,
    digits: [Option<u8>; 4],
    ascending: bool,
) -> impl Iterator<Item = i64> {
    let mut masked = [0_usize; 4];
    let mut n: u32 = 0;
    for (i, d) in digits.iter().enumerate() {
        if d.is_none() {
            masked[n as usize] = i;
            n += 1;
        }
    }
    let count = 10_u32.pow(n);
    (0..count).map(move |i| {
        let mut rem = if ascending { i } else { count - 1 - i };
        let mut filled = digits.map(|d| d.unwrap_or(0));
        for pos in masked[..n as usize].iter().rev() {
            filled[*pos] = (rem % 10) as u8;
            rem /= 10;
        }
        let magnitude = filled.iter().fold(0, |acc, d| acc * 10 + i64::from(*d));
        if negative { -magnitude } else { magnitude }
    })
}

/// Earliest (`ascending`) or latest day within year `y` matching the
/// month/day pattern of `d`, if the year admits one. Almost every pattern
/// resolves in the first year asked; only a February 29 that year lacks
/// sends the caller on to the next completion. A season starts on the
/// first day of its first month and ends on the last day of its last, in
/// the next year when it wraps (D12).
fn extremum_in_year(d: &Date, y: i64, ascending: bool) -> Option<BoundDate> {
    let Some(month) = d.month() else {
        return Some(if ascending {
            BoundDate::first_of_year(y)
        } else {
            BoundDate::last_of_year(y)
        });
    };
    if let Some(code) = month.value().filter(|code| *code > 12) {
        let (first, last, wraps) = season_months(code);
        if ascending {
            return Some(BoundDate {
                year: y,
                month: first,
                day: 1,
            });
        }
        let year = if wraps { y + 1 } else { y };
        return Some(BoundDate {
            year,
            month: last,
            day: last_day(last, is_leap(year)),
        });
    }
    let mut months = month_candidates_of(month);
    if !ascending {
        months.reverse();
    }
    for m in months {
        let Some(day) = d.day() else {
            return Some(BoundDate {
                year: y,
                month: m,
                day: if ascending {
                    1
                } else {
                    last_day(m, is_leap(y))
                },
            });
        };
        let mut days = day_candidates_of(day);
        if !ascending {
            days.reverse();
        }
        if let Some(dd) = days.into_iter().find(|dd| *dd <= last_day(m, is_leap(y))) {
            return Some(BoundDate {
                year: y,
                month: m,
                day: dd,
            });
        }
    }
    None
}

/// Every month value a (possibly masked) month field admits: its own value
/// when specified, else the calendar months 01–12 its digits match (D14).
pub(crate) fn month_candidates_of(f: DateField) -> alloc::vec::Vec<u8> {
    f.value().map_or_else(
        || (1..=12).filter(|v| field_matches(f, *v)).collect(),
        |v| alloc::vec![v],
    )
}

/// Every day value a (possibly masked) day field admits: its own value when
/// it is 1–31, else the days 1–31 its digits match.
pub(crate) fn day_candidates_of(f: DateField) -> alloc::vec::Vec<u8> {
    match f.value() {
        Some(v) if (1..=31).contains(&v) => alloc::vec![v],
        Some(_) => alloc::vec::Vec::new(),
        None => (1..=31).filter(|v| field_matches(f, *v)).collect(),
    }
}

/// Whether a concrete value matches a field's digits, masked positions free.
fn field_matches(f: DateField, v: u8) -> bool {
    let [tens, ones] = f.digits();
    tens.is_none_or(|p| p == v / 10) && ones.is_none_or(|p| p == v % 10)
}

/// Proleptic Gregorian leap rule on the astronomical year number.
pub(crate) const fn is_leap(y: i64) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

/// Last day of a month, honouring the leap flag for February.
pub(crate) fn last_day(month: u8, leap: bool) -> u8 {
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
        _ => unreachable!("month is 1-12"),
    }
}

/// Bounds of an interval: its start's earliest and its end's latest.
fn interval_bounds(iv: &Interval) -> Bounds {
    let earliest = match iv.start() {
        IntervalEndpoint::Open | IntervalEndpoint::OnOrBefore(_) => Bound::NegativeInfinity,
        IntervalEndpoint::Unknown => Bound::Unknown,
        IntervalEndpoint::Date(d) | IntervalEndpoint::OnOrAfter(d) => Bound::Date(date_span(&d).0),
    };
    let latest = match iv.end() {
        IntervalEndpoint::Open | IntervalEndpoint::OnOrAfter(_) => Bound::PositiveInfinity,
        IntervalEndpoint::Unknown => Bound::Unknown,
        IntervalEndpoint::Date(d) | IntervalEndpoint::OnOrBefore(d) => Bound::Date(date_span(&d).1),
    };
    Bounds { earliest, latest }
}

/// Bounds of a set: the widest span any element touches. A set has at least
/// one element, and no element's bound is unknown.
fn set_bounds(s: &Set) -> Bounds {
    // `None` is an infinite side: `..date` has no earliest day, `date..` no
    // latest.
    let sides = s.elements().iter().map(|e| match e {
        SetElement::Date(d) => {
            let (lo, hi) = date_span(d);
            (Some(lo), Some(hi))
        }
        SetElement::OnOrBefore(d) => (None, Some(date_span(d).1)),
        SetElement::OnOrAfter(d) => (Some(date_span(d).0), None),
        SetElement::Range(r) => (Some(date_span(&r.start()).0), Some(date_span(&r.end()).1)),
    });
    // `Option`'s order puts `None` first, which is right for the earliest
    // side; the latest side ranks an infinite end above every day.
    let earliest = sides.clone().map(|(lo, _)| lo).min().flatten();
    let latest = sides
        .map(|(_, hi)| (hi.is_none(), hi))
        .max()
        .and_then(|(_, hi)| hi);
    Bounds {
        earliest: earliest.map_or(Bound::NegativeInfinity, Bound::Date),
        latest: latest.map_or(Bound::PositiveInfinity, Bound::Date),
    }
}

/// White-box tests for the invariants the code above asserts.
///
/// Every value is built through the model's constructors, so it is valid
/// (decision D31); each test pins a guard that a valid value never reaches,
/// by calling the private function a caller would have to misuse.
#[cfg(test)]
mod tests {
    #![expect(
        clippy::missing_panics_doc,
        reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
    )]
    #![expect(
        clippy::let_underscore_untyped,
        reason = "the discarded value's type is fixed by the call it comes from"
    )]
    use super::{BoundDate, last_day, season_months, significant_range};

    // The sign split sits exactly at zero: year 0 sweeps upward like any
    // non-negative year, and a negative year mirrors its magnitude's sweep,
    // both ends negated, so the range still reads low to high.
    #[test]
    fn significant_range_splits_on_the_sign_at_zero() {
        assert_eq!(significant_range(0, Some(2), 4), (0, 99));
        assert_eq!(significant_range(-1985, Some(2), 4), (-1999, -1900));
    }

    // `season_months` is total over the 21..=41 codes validation admits;
    // anything else must panic rather than return a wrong month pair.
    #[test]
    #[should_panic(expected = "validated season code")]
    fn season_months_rejects_a_code_outside_the_table() {
        let _ = season_months(42);
    }

    /// A fixture day, built by the constructor itself.
    fn bd(year: i64, month: u8, day: u8) -> Option<BoundDate> {
        BoundDate::new(year, month, day).ok()
    }

    /// A month end, a year end, 29 February in a leap year and its absence
    /// in 1900: each step lands on the true calendar neighbour, and each
    /// step undoes the other.
    #[test]
    fn next_day_and_previous_day_step_the_calendar() {
        let pairs = [
            (bd(1985, 4, 12), bd(1985, 4, 13)),
            (bd(1985, 4, 30), bd(1985, 5, 1)),
            (bd(1985, 12, 31), bd(1986, 1, 1)),
            (bd(2000, 2, 28), bd(2000, 2, 29)),
            (bd(2000, 2, 29), bd(2000, 3, 1)),
            (bd(1900, 2, 28), bd(1900, 3, 1)),
            (bd(0, 1, 1), bd(0, 1, 2)),
            (bd(-1, 12, 31), bd(0, 1, 1)),
        ];
        for (day, next) in pairs {
            assert_ne!(day, None);
            assert_ne!(next, None);
            assert_eq!(day.and_then(BoundDate::next_day), next, "{day:?}");
            assert_eq!(next.and_then(BoundDate::previous_day), day, "{next:?}");
        }
        let (last, first) = (bd(i64::MAX, 12, 31), bd(i64::MIN, 1, 1));
        assert_ne!(last, None);
        assert_ne!(first, None);
        assert_eq!(last.and_then(BoundDate::next_day), None);
        assert_eq!(first.and_then(BoundDate::previous_day), None);
    }

    #[test]
    #[should_panic(expected = "month is 1-12")]
    fn last_day_rejects_a_month_outside_1_12() {
        let _ = last_day(13, false);
    }
}
