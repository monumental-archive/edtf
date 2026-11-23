// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Data model for parsed EDTF expressions.
//!
//! The model preserves everything the string encodes beyond a yes/no answer:
//! per-component qualification, unspecified-digit masks, endpoint kinds, and
//! set semantics (see docs/spec-notes.md §6).
//!
//! A value that exists is valid (decision D31). Every type with a rule keeps
//! its state private and is built only through a constructor that checks
//! the rule and names it in a [`ValueError`] when it is broken; the parser
//! is one client of those constructors, and every other crate is another.

#![expect(
    clippy::min_ident_chars,
    reason = "y, m and d are the universal notation for a date's components, and this crate is about little else"
)]
#![expect(
    clippy::missing_inline_in_public_items,
    reason = "inlining is the compiler's call across a crate boundary, and the release profile enables fat LTO — annotating every public item would assert a decision this crate has not measured"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "these types are the public data model of an ISO 8601-2 value; the spec fixes their fields, so a non_exhaustive that promised future additions would be a promise this format cannot make"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "these enums enumerate what ISO 8601-2 defines; the spec fixes the variants, so non_exhaustive would promise additions the format cannot make"
)]
#![expect(
    clippy::absolute_paths,
    reason = "a one-use std path written in full at the call site is clearer than an import that only appears once"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching through a reference without restating & at every level is the idiomatic form the rest of this crate uses"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "every flagged operation is bounded where it stands: slice indices by a length guard on the line above, digit values to 0-9 by the match arm that binds them, and the JDN forms by being computed in i128 so any i64 year fits. The operations that genuinely could leave range already use checked_/saturating_ and return an error rather than wrapping"
)]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "the wildcard covers variants the arm above has already narrowed by construction; naming them would be dead code the compiler cannot check"
)]
#![expect(
    clippy::else_if_without_else,
    reason = "the chain is exhaustive over the values its guard admits"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "each index is preceded by the bounds check that justifies it, or indexes a fixed-size array whose length is in its type"
)]
#![expect(
    clippy::missing_trait_methods,
    reason = "the default implementations are what this type wants; overriding them to satisfy a lint would be code with no reason to exist"
)]
#![expect(
    clippy::multiple_inherent_impl,
    reason = "the impl blocks group by concern, which is how the module docs describe this type"
)]

use alloc::vec::Vec;

use crate::bounds::{date_span, day_candidates_of, is_leap, month_candidates_of};

/// Error returned when an input is not valid EDTF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    /// Human-readable reason the input was rejected.
    pub message: &'static str,
    /// Byte offset into the original input where the problem was detected.
    pub offset: usize,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "invalid EDTF at offset {}: {}",
            self.offset, self.message
        )
    }
}

impl core::error::Error for ParseError {}

/// Error returned when a constructor's arguments break a rule of the model.
///
/// The message names the rule. It is the same text the parser reports, at
/// an offset, for input that breaks the same rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueError {
    /// The rule broken.
    message: &'static str,
}

/// Every rule's message exists once, here; the parser reports the same text.
impl ValueError {
    /// A year digit outside 0-9.
    pub(crate) const YEAR_DIGIT: &'static str = "year digits must be 0-9";
    /// D21: a negative year is fully specified.
    pub(crate) const NEGATIVE_MASKED: &'static str =
        "negative years cannot contain unspecified digits";
    /// There is no negative zero year.
    pub(crate) const NEGATIVE_ZERO: &'static str = "-0000 is not a valid year";
    /// D30: one supported year range.
    pub(crate) const YEAR_RANGE: &'static str = "year out of supported range";
    /// D1: a `Y` year is one no four-digit year can spell.
    pub(crate) const Y_TOO_SMALL: &'static str = "Y-prefixed years require |year| > 9999";
    /// ISO 8601-2 4.7.3: a significand of zero denotes no year.
    pub(crate) const EXPONENT_ZERO: &'static str = "exponential year significand cannot be zero";
    /// ISO 8601-2 4.4.3: `S` is a precision on known digits.
    pub(crate) const SIGNIFICANT_MASKED: &'static str =
        "significant digits cannot combine with unspecified digits";
    /// ISO 8601-2 4.4.3: a four-digit year has one to four significant digits.
    pub(crate) const SIGNIFICANT_FOUR: &'static str =
        "significant digits must be 1-4 for a four-digit year";
    /// ISO 8601-2 4.7.4, D20: at most as many significant digits as written.
    pub(crate) const SIGNIFICANT_COUNT: &'static str =
        "significant digits exceed the year's digit count";
    /// A month or day digit outside 0-9.
    pub(crate) const FIELD_DIGIT: &'static str = "date field digits must be 0-9";
    /// ISO 8601-2 4.4.3: a significant-digit year has no month or day.
    pub(crate) const SIGNIFICANT_YEAR_ONLY: &'static str =
        "significant-digit years are year-precision only";
    /// ISO 8601-2 4.7.2: a `Y` year has no month or day.
    pub(crate) const Y_YEAR_ONLY: &'static str = "Y-prefixed years are year-precision only";
    /// A day is a day of some month.
    pub(crate) const DAY_WITHOUT_MONTH: &'static str = "a day requires a month";
    /// A month is a calendar month or a sub-year code (ISO 8601-2 4.8).
    pub(crate) const MONTH_RANGE: &'static str = "month must be 01-12 or a sub-year code 21-41";
    /// D14: a masked month matches calendar months only.
    pub(crate) const MONTH_MASK: &'static str = "no calendar month matches the unspecified digits";
    /// ISO 8601-2 4.8: a sub-year grouping has no days.
    pub(crate) const SUB_YEAR_DAY: &'static str = "sub-year groupings cannot carry a day";
    /// D11: a day has at least one valid calendar completion.
    pub(crate) const DAY_RANGE: &'static str = "day is out of range for the month";
    /// ISO 8601-1 5.3.1: hours run 00-23.
    pub(crate) const HOUR: &'static str = "hour must be 00-23 (24 is not allowed)";
    /// Minutes run 00-59.
    pub(crate) const MINUTE: &'static str = "minute must be 00-59";
    /// D3: seconds run 00-60, 60 admitting a leap second.
    pub(crate) const SECOND: &'static str = "second must be 00-60";
    /// ISO 8601-1 4.3.13: a shift from UTC is at most fourteen hours.
    pub(crate) const SHIFT_RANGE: &'static str = "time shift exceeds \u{b1}14:00";
    /// An hours-only shift (`±hh`) has no minutes to spell.
    pub(crate) const SHIFT_HOURS_ONLY: &'static str = "an hours-only time shift has no minutes";
    /// A.4.3: a date-time's date is a plain, complete, unqualified date.
    pub(crate) const DATETIME_DATE: &'static str =
        "a date-time's date must be a complete, unqualified YYYY-MM-DD date";
    /// D6: an interval has at least one dated end.
    pub(crate) const INTERVAL_UNDATED: &'static str = "interval needs at least one dated endpoint";
    /// D18: an interval's end does not precede its start.
    pub(crate) const INTERVAL_ORDER: &'static str = "interval end precedes interval start";
    /// ISO 8601-2 10.6: `date..` closes an interval, never opens it.
    pub(crate) const START_ON_OR_AFTER: &'static str =
        "'..'-suffixed date is only allowed as interval end";
    /// ISO 8601-2 10.6: `..date` opens an interval, never closes it.
    pub(crate) const END_ON_OR_BEFORE: &'static str =
        "'..'-prefixed date is only allowed as interval start";
    /// ISO 8601-2 6.1: a set has members.
    pub(crate) const SET_EMPTY: &'static str = "set must contain at least one element";
    /// D27: a set range endpoint is unmasked.
    pub(crate) const RANGE_MASKED: &'static str =
        "set range endpoints cannot contain unspecified digits";
    /// D27: a set range endpoint is unqualified.
    pub(crate) const RANGE_QUALIFIED: &'static str = "set range endpoints cannot be qualified";
    /// D27: a season has no successor.
    pub(crate) const RANGE_SEASON: &'static str = "set range endpoints cannot be seasons";
    /// D27: a set range endpoint is one concrete year.
    pub(crate) const RANGE_SIGNIFICANT: &'static str =
        "set range endpoints cannot carry significant digits";
    /// D27, ISO 8601-2 6.3 c: both ends of a set range share precision.
    pub(crate) const RANGE_PRECISION: &'static str = "set range endpoints must share precision";
    /// D18: a set range's end does not precede its start.
    pub(crate) const RANGE_ORDER: &'static str = "set range end precedes range start";
}

impl ValueError {
    /// An error naming the rule `message`.
    pub(crate) const fn new(message: &'static str) -> Self {
        Self { message }
    }

    /// The rule the arguments break.
    #[must_use]
    pub const fn message(self) -> &'static str {
        self.message
    }
}

impl core::fmt::Display for ValueError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message)
    }
}

impl core::error::Error for ValueError {}

/// The largest year magnitude: 18 digits (decision D30). With at most 18
/// digits a significant-digit sweep tops out at `10^18 - 1`, so every year
/// can be bounded, compared, ordered and enumerated in `i64`.
const MAX_YEAR_MAGNITUDE: i64 = 999_999_999_999_999_999;

/// Uncertainty (`?`), approximation (`~`), or both (`%`), per ISO 8601-2
/// §3.2.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Qualifier {
    /// The value's source is considered dubious (`?`).
    pub uncertain: bool,
    /// The value is an estimate (`~`).
    pub approximate: bool,
}

impl Qualifier {
    /// True if either flag is set.
    #[must_use]
    pub const fn is_qualified(self) -> bool {
        self.uncertain || self.approximate
    }

    /// Fold another component set into this one.
    pub(crate) const fn merge(&mut self, other: Self) {
        self.uncertain |= other.uncertain;
        self.approximate |= other.approximate;
    }
}

/// The three syntactic forms a calendar year can take.
///
/// A description, not a value: [`Year::new`] checks one and [`Year::kind`]
/// reads one back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YearKind {
    /// Four-digit year, possibly negative, possibly with `X` digits
    /// (`None` = unspecified digit).
    Standard {
        /// True for years before year 0000 (written with a leading `-`).
        negative: bool,
        /// The four digits, most significant first; `None` is an `X`.
        digits: [Option<u8>; 4],
    },
    /// `Y`-prefixed year beyond ±9999 (ISO 8601-2 §4.7.2).
    Big {
        /// The signed year value.
        value: i64,
    },
    /// `Y…E…` exponential year (ISO 8601-2 §4.7.3).
    Exponential {
        /// Signed significand; the year is `significand × 10^exponent`.
        significand: i64,
        /// Power-of-ten exponent.
        exponent: u32,
    },
}

/// A calendar year with optional significant-digit precision and qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Year {
    /// Which syntactic form the year uses, and its digits.
    kind: YearKind,
    /// The year's value, when every digit is specified.
    value: Option<i64>,
    /// `S`-suffix significant digits (ISO 8601-2 §4.4.3, §4.7.4).
    significant_digits: Option<u32>,
    /// Qualification applying to the year component.
    qualifier: Qualifier,
}

impl Year {
    /// A year of the given form, unqualified and without significant digits.
    ///
    /// # Errors
    ///
    /// [`ValueError`] when a digit is outside 0-9, a negative year is masked
    /// or is `-0000`, a significand is zero, or a `Y` year's value is not
    /// beyond ±9999 or is past the supported range (decision D30).
    pub fn new(kind: YearKind) -> Result<Self, ValueError> {
        let value = match kind {
            YearKind::Standard { negative, digits } => {
                if digits.iter().flatten().any(|d| *d > 9) {
                    return Err(ValueError::new(ValueError::YEAR_DIGIT));
                }
                let masked = digits.iter().any(Option::is_none);
                if negative && masked {
                    return Err(ValueError::new(ValueError::NEGATIVE_MASKED));
                }
                if negative && digits == [Some(0); 4] {
                    return Err(ValueError::new(ValueError::NEGATIVE_ZERO));
                }
                let magnitude = digits
                    .iter()
                    .flatten()
                    .fold(0_i64, |v, d| v * 10 + i64::from(*d));
                (!masked).then_some(if negative { -magnitude } else { magnitude })
            }
            YearKind::Big { value } => Some(y_value(value)?),
            YearKind::Exponential {
                significand,
                exponent,
            } => {
                if significand == 0 {
                    return Err(ValueError::new(ValueError::EXPONENT_ZERO));
                }
                let product = 10_i64
                    .checked_pow(exponent)
                    .and_then(|p| significand.checked_mul(p))
                    .ok_or(ValueError::new(ValueError::YEAR_RANGE))?;
                Some(y_value(product)?)
            }
        };
        Ok(Self {
            kind,
            value,
            significant_digits: None,
            qualifier: Qualifier::default(),
        })
    }

    /// The year `value` in the one form that spells it: four digits within
    /// ±9999, `Y`-prefixed beyond (ISO 8601-2 §4.7.2).
    ///
    /// # Errors
    ///
    /// [`ValueError`] when the value is past the supported range (D30).
    pub fn from_value(value: i64) -> Result<Self, ValueError> {
        if (-9999..=9999).contains(&value) {
            let magnitude = value.unsigned_abs();
            let digit = |place: u64| u8::try_from(magnitude.div_euclid(place).rem_euclid(10)).ok();
            Self::new(YearKind::Standard {
                negative: value < 0,
                digits: [digit(1000), digit(100), digit(10), digit(1)],
            })
        } else {
            Self::new(YearKind::Big { value })
        }
    }

    /// This year with `count` significant digits (ISO 8601-2 §4.4.3, §4.7.4).
    ///
    /// # Errors
    ///
    /// [`ValueError`] when the year has unspecified digits, or `count` is
    /// zero or more than the digits the year's form writes.
    pub const fn with_significant_digits(self, count: u32) -> Result<Self, ValueError> {
        if self.has_unspecified() {
            return Err(ValueError::new(ValueError::SIGNIFICANT_MASKED));
        }
        let message = match self.kind {
            YearKind::Standard {
                negative: _,
                digits: _,
            } => ValueError::SIGNIFICANT_FOUR,
            _ => ValueError::SIGNIFICANT_COUNT,
        };
        if count == 0 || count > self.width() {
            return Err(ValueError::new(message));
        }
        Ok(Self {
            significant_digits: Some(count),
            ..self
        })
    }

    /// This year with its qualifier replaced by `qualifier`.
    #[must_use]
    pub const fn with_qualifier(self, qualifier: Qualifier) -> Self {
        Self { qualifier, ..self }
    }

    /// Which syntactic form the year uses, and its digits.
    #[must_use]
    pub const fn kind(&self) -> YearKind {
        self.kind
    }

    /// `S`-suffix significant digits (ISO 8601-2 §4.4.3, §4.7.4).
    #[must_use]
    pub const fn significant_digits(&self) -> Option<u32> {
        self.significant_digits
    }

    /// Qualification applying to the year component.
    #[must_use]
    pub const fn qualifier(&self) -> Qualifier {
        self.qualifier
    }

    /// The numeric year value, when every digit is specified. `None` for a
    /// masked year; every other year's value fits `i64` (decision D30).
    #[must_use]
    pub const fn value(&self) -> Option<i64> {
        self.value
    }

    /// True if any digit is unspecified (`X`).
    #[must_use]
    pub const fn has_unspecified(&self) -> bool {
        self.value.is_none()
    }

    /// Digit width of the year's form, for significant-digit ranges: four
    /// for a four-digit year, every written digit for a `Y` year.
    pub(crate) const fn width(&self) -> u32 {
        match self.kind {
            YearKind::Standard {
                negative: _,
                digits: _,
            } => 4,
            YearKind::Big { value } => decimal_digits(value.unsigned_abs()),
            YearKind::Exponential {
                significand,
                exponent,
            } => decimal_digits(significand.unsigned_abs()) + exponent,
        }
    }
}

/// A `Y` year's value, checked against the one supported range (D30) and
/// against the four-digit range it must lie beyond (D1).
///
/// # Errors
///
/// [`ValueError`] naming whichever of the two ranges `value` misses.
fn y_value(value: i64) -> Result<i64, ValueError> {
    if !(-MAX_YEAR_MAGNITUDE..=MAX_YEAR_MAGNITUDE).contains(&value) {
        return Err(ValueError::new(ValueError::YEAR_RANGE));
    }
    if (-9999..=9999).contains(&value) {
        return Err(ValueError::new(ValueError::Y_TOO_SMALL));
    }
    Ok(value)
}

/// Number of decimal digits in `v` (at least one, for zero).
const fn decimal_digits(mut v: u64) -> u32 {
    let mut n = 1;
    while v >= 10 {
        v = v.div_euclid(10);
        n += 1;
    }
    n
}

/// A two-digit month or day slot; `None` digits are unspecified (`X`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateField {
    /// Tens digit then ones digit; `None` is an `X`.
    digits: [Option<u8>; 2],
    /// Qualification applying to this component.
    qualifier: Qualifier,
}

impl DateField {
    /// An unqualified field of these digits. Whether they make a month or a
    /// day is the date's rule ([`Date::new`]).
    ///
    /// # Errors
    ///
    /// [`ValueError`] when a digit is outside 0-9.
    pub fn new(digits: [Option<u8>; 2]) -> Result<Self, ValueError> {
        if digits.iter().flatten().any(|d| *d > 9) {
            return Err(ValueError::new(ValueError::FIELD_DIGIT));
        }
        Ok(Self {
            digits,
            qualifier: Qualifier::default(),
        })
    }

    /// An unqualified, fully specified field holding `value`.
    ///
    /// # Errors
    ///
    /// [`ValueError`] when `value` has more than two digits.
    pub fn from_value(value: u8) -> Result<Self, ValueError> {
        Self::new([Some(value.div_euclid(10)), Some(value.rem_euclid(10))])
    }

    /// This field with its qualifier replaced by `qualifier`.
    #[must_use]
    pub const fn with_qualifier(self, qualifier: Qualifier) -> Self {
        Self { qualifier, ..self }
    }

    /// Tens digit then ones digit; `None` is an `X`.
    #[must_use]
    pub const fn digits(self) -> [Option<u8>; 2] {
        self.digits
    }

    /// Qualification applying to this component.
    #[must_use]
    pub const fn qualifier(self) -> Qualifier {
        self.qualifier
    }

    /// The numeric value when both digits are specified.
    #[must_use]
    pub fn value(self) -> Option<u8> {
        Some(self.digits[0]? * 10 + self.digits[1]?)
    }

    /// True if either digit is unspecified (`X`).
    #[must_use]
    pub fn has_unspecified(self) -> bool {
        self.digits.iter().any(Option::is_none)
    }
}

/// Precision of a date expression (its lowest specified component).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// Year only (`1985`, `Y17E7`).
    Year,
    /// Year and calendar month (`1985-04`).
    Month,
    /// Month slot holds a sub-year grouping code 21–41 (ISO 8601-2 §4.8).
    Season,
    /// Complete calendar date (`1985-04-12`).
    Day,
}

/// A (possibly qualified, possibly partially unspecified) EDTF date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    /// The year component (always present).
    year: Year,
    /// Month 01–12 or sub-year grouping code 21–41.
    month: Option<DateField>,
    /// Calendar day of month.
    day: Option<DateField>,
}

impl Date {
    /// A date of these components.
    ///
    /// # Errors
    ///
    /// [`ValueError`] when a `Y` or significant-digit year carries a month,
    /// a day has no month, the month is neither 01-12 nor a sub-year code
    /// 21-41 (a masked month must match a calendar month, D14), a sub-year
    /// code carries a day, or no completion of the components is a real
    /// calendar day (D11).
    pub fn new(
        year: Year,
        month: Option<DateField>,
        day: Option<DateField>,
    ) -> Result<Self, ValueError> {
        if let Some(m) = month {
            Self::check_month(&year, m)?;
        }
        match (month, day) {
            (None, Some(_)) => return Err(ValueError::new(ValueError::DAY_WITHOUT_MONTH)),
            (Some(m), Some(d)) => {
                if m.value().is_some_and(|v| v > 12) {
                    return Err(ValueError::new(ValueError::SUB_YEAR_DAY));
                }
                if !day_has_valid_completion(year.kind, m, d) {
                    return Err(ValueError::new(ValueError::DAY_RANGE));
                }
            }
            _ => {}
        }
        Ok(Self { year, month, day })
    }

    /// The month rules a date checks before its day: only a four-digit year
    /// without significant digits has months, and a month is 01-12, a sub-year
    /// code 21-41, or a mask matching some calendar month (D14).
    ///
    /// The parser asks this first, to place a month error at the month.
    ///
    /// # Errors
    ///
    /// [`ValueError`] naming the month rule `m` breaks under `year`.
    pub(crate) fn check_month(year: &Year, m: DateField) -> Result<(), ValueError> {
        if !matches!(
            year.kind,
            YearKind::Standard {
                negative: _,
                digits: _
            }
        ) {
            return Err(ValueError::new(ValueError::Y_YEAR_ONLY));
        }
        if year.significant_digits.is_some() {
            return Err(ValueError::new(ValueError::SIGNIFICANT_YEAR_ONLY));
        }
        match m.value() {
            Some(v) if !((1..=12).contains(&v) || (21..=41).contains(&v)) => {
                Err(ValueError::new(ValueError::MONTH_RANGE))
            }
            None if month_candidates_of(m).is_empty() => {
                Err(ValueError::new(ValueError::MONTH_MASK))
            }
            _ => Ok(()),
        }
    }

    /// The year component (always present).
    #[must_use]
    pub const fn year(&self) -> Year {
        self.year
    }

    /// Month 01–12 or sub-year grouping code 21–41.
    #[must_use]
    pub const fn month(&self) -> Option<DateField> {
        self.month
    }

    /// Calendar day of month.
    #[must_use]
    pub const fn day(&self) -> Option<DateField> {
        self.day
    }

    /// This date with `q` merged into the qualifier of every component it
    /// has: whole-expression qualification (ISO 8601-2 §8.4).
    #[must_use]
    pub const fn qualified(self, q: Qualifier) -> Self {
        let mut out = self;
        out.year.qualifier.merge(q);
        if let Some(m) = &mut out.month {
            m.qualifier.merge(q);
        }
        if let Some(d) = &mut out.day {
            d.qualifier.merge(q);
        }
        out
    }

    /// The precision of this date (its lowest specified component).
    #[must_use]
    pub fn precision(&self) -> Precision {
        if self.day.is_some() {
            Precision::Day
        } else if let Some(m) = &self.month {
            match m.value() {
                Some(v) if v >= 21 => Precision::Season,
                _ => Precision::Month,
            }
        } else {
            Precision::Year
        }
    }

    /// Whether any component of this value satisfies `f`.
    fn any_component<F>(&self, f: F) -> bool
    where
        F: Fn(Qualifier) -> bool,
    {
        f(self.year.qualifier)
            || self.month.is_some_and(|m| f(m.qualifier))
            || self.day.is_some_and(|d| f(d.qualifier))
    }

    /// True if any component is marked uncertain (`?` or `%`).
    #[must_use]
    pub fn is_uncertain(&self) -> bool {
        self.any_component(|q| q.uncertain)
    }

    /// True if any component is marked approximate (`~` or `%`).
    #[must_use]
    pub fn is_approximate(&self) -> bool {
        self.any_component(|q| q.approximate)
    }

    /// True if any component contains an unspecified digit (`X`).
    #[must_use]
    pub fn has_unspecified(&self) -> bool {
        self.year.has_unspecified()
            || self.month.is_some_and(DateField::has_unspecified)
            || self.day.is_some_and(DateField::has_unspecified)
    }
}

/// Decision D11: a (possibly masked) year-month-day needs at least one valid
/// calendar completion.
///
/// Only a four-digit year reaches here, and only with
/// a calendar month or a mask of calendar months (`check_month`).
fn day_has_valid_completion(year: YearKind, m: DateField, d: DateField) -> bool {
    let months = month_candidates_of(m);
    let days = day_candidates_of(d);
    let mut leap_possible: Option<bool> = None;
    for &mm in &months {
        for &dd in &days {
            if month_admits_day(mm, dd, year, &mut leap_possible) {
                return true;
            }
        }
    }
    false
}

/// Whether month `mm` can hold day `dd`.
///
/// Deliberately a predicate rather than
/// a month-length function: February short-circuits to 28 for any day the
/// 28th admits, so leap-ness is only asked once a later day forces the
/// question, and `leap_possible` memoises that answer across the scan. A
/// month outside 1-12 holds no day.
fn month_admits_day(mm: u8, dd: u8, year: YearKind, leap_possible: &mut Option<bool>) -> bool {
    let max = match mm {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if dd <= 28 {
                28
            } else {
                let leap = *leap_possible.get_or_insert_with(|| year_leap_possible(year));
                if leap { 29 } else { 28 }
            }
        }
        _ => 0,
    };
    dd <= max
}

/// Whether any completion of this four-digit year could be a leap year. A
/// negative year is fully specified (D21), and leap-ness is symmetric under
/// negation, so the magnitude answers for it.
fn year_leap_possible(year: YearKind) -> bool {
    let YearKind::Standard {
        digits,
        negative: _,
    } = year
    else {
        return false;
    };
    (0..=9999_i64).any(|y| year_matches(digits, y) && is_leap(y))
}

/// Whether `y` matches the year's digits, masked positions free.
fn year_matches(digits: [Option<u8>; 4], y: i64) -> bool {
    let actual = [1000, 100, 10, 1].map(|place: i64| y.div_euclid(place).rem_euclid(10));
    digits
        .iter()
        .zip(actual)
        .all(|(pat, a)| pat.is_none_or(|p| i64::from(p) == a))
}

/// UTC designator or a numeric shift from UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeShift {
    /// Minutes from UTC, `None` for the `Z` designator.
    minutes: Option<i16>,
    /// True when written in the hours-only form `±hh`.
    hours_only: bool,
}

impl TimeShift {
    /// `Z`.
    pub const UTC: Self = Self {
        minutes: None,
        hours_only: false,
    };

    /// A numeric shift of `minutes` from UTC, spelt `±hh` when `hours_only`
    /// and `±hh:mm` otherwise.
    ///
    /// # Errors
    ///
    /// [`ValueError`] when the shift is beyond ±14:00, or is hours-only and
    /// not a whole number of hours.
    pub const fn offset(minutes: i16, hours_only: bool) -> Result<Self, ValueError> {
        if minutes.unsigned_abs() > 14 * 60 {
            return Err(ValueError::new(ValueError::SHIFT_RANGE));
        }
        if hours_only && minutes.rem_euclid(60) != 0 {
            return Err(ValueError::new(ValueError::SHIFT_HOURS_ONLY));
        }
        Ok(Self {
            minutes: Some(minutes),
            hours_only,
        })
    }

    /// True for the `Z` designator.
    #[must_use]
    pub const fn is_utc(self) -> bool {
        self.minutes.is_none()
    }

    /// Signed total offset from UTC in minutes, `None` for `Z`.
    #[must_use]
    pub const fn minutes(self) -> Option<i16> {
        self.minutes
    }

    /// True when written in the hours-only form `±hh`.
    #[must_use]
    pub const fn hours_only(self) -> bool {
        self.hours_only
    }
}

/// A complete time of day `hh:mm:ss` with optional shift (EDTF level 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Time {
    /// Clock hour 00–23.
    hour: u8,
    /// Clock minute 00–59.
    minute: u8,
    /// Clock second 00–60 (60 admits a leap second).
    second: u8,
    /// Optional UTC designator or shift.
    shift: Option<TimeShift>,
}

impl Time {
    /// A time of day.
    ///
    /// # Errors
    ///
    /// [`ValueError`] when the hour is past 23, the minute past 59 or the
    /// second past 60 (decision D3 admits a leap second).
    pub const fn new(
        hour: u8,
        minute: u8,
        second: u8,
        shift: Option<TimeShift>,
    ) -> Result<Self, ValueError> {
        if hour > 23 {
            return Err(ValueError::new(ValueError::HOUR));
        }
        if minute > 59 {
            return Err(ValueError::new(ValueError::MINUTE));
        }
        if second > 60 {
            return Err(ValueError::new(ValueError::SECOND));
        }
        Ok(Self {
            hour,
            minute,
            second,
            shift,
        })
    }

    /// This time with its shift replaced by `shift`.
    #[must_use]
    pub const fn with_shift(self, shift: Option<TimeShift>) -> Self {
        Self { shift, ..self }
    }

    /// Clock hour 00–23.
    #[must_use]
    pub const fn hour(self) -> u8 {
        self.hour
    }

    /// Clock minute 00–59.
    #[must_use]
    pub const fn minute(self) -> u8 {
        self.minute
    }

    /// Clock second 00–60 (60 admits a leap second).
    #[must_use]
    pub const fn second(self) -> u8 {
        self.second
    }

    /// Optional UTC designator or shift.
    #[must_use]
    pub const fn shift(self) -> Option<TimeShift> {
        self.shift
    }
}

/// A complete date and time of day (EDTF level 0 only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    /// The complete calendar date.
    date: Date,
    /// The complete time of day.
    time: Time,
}

impl DateTime {
    /// A date-time (Annex A.4.3).
    ///
    /// # Errors
    ///
    /// [`ValueError`] unless `date` is a plain, complete, unqualified
    /// `YYYY-MM-DD` date: a non-negative four-digit year, a calendar month
    /// and a day, no unspecified digit and no qualifier.
    pub fn new(date: Date, time: Time) -> Result<Self, ValueError> {
        let plain = matches!(
            date.year.kind,
            YearKind::Standard {
                negative: false,
                digits: _
            }
        ) && date.precision() == Precision::Day
            && !date.has_unspecified()
            && !date.is_uncertain()
            && !date.is_approximate();
        if !plain {
            return Err(ValueError::new(ValueError::DATETIME_DATE));
        }
        Ok(Self { date, time })
    }

    /// The complete calendar date.
    #[must_use]
    pub const fn date(&self) -> Date {
        self.date
    }

    /// The complete time of day.
    #[must_use]
    pub const fn time(&self) -> Time {
        self.time
    }
}

/// One side of a time interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalEndpoint {
    /// A concrete (possibly qualified/unspecified) date.
    Date(Date),
    /// `..` — open (unbounded).
    Open,
    /// Empty — unknown.
    Unknown,
    /// `..date` at the start: begins on or before this date (ISO 8601-2 §10.6).
    OnOrBefore(Date),
    /// `date..` at the end: ends on or after this date (ISO 8601-2 §10.6).
    OnOrAfter(Date),
}

impl IntervalEndpoint {
    /// True unless the endpoint is open (`..`) or unknown (empty).
    #[must_use]
    pub const fn is_dated(&self) -> bool {
        !matches!(self, Self::Open | Self::Unknown)
    }

    /// The date this value denotes, when it denotes exactly one.
    pub(crate) const fn date(&self) -> Option<&Date> {
        match self {
            Self::Date(d) | Self::OnOrBefore(d) | Self::OnOrAfter(d) => Some(d),
            _ => None,
        }
    }
}

/// A time interval `start/end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    /// The start endpoint.
    start: IntervalEndpoint,
    /// The end endpoint.
    end: IntervalEndpoint,
}

impl Interval {
    /// An interval from `start` to `end`.
    ///
    /// # Errors
    ///
    /// [`ValueError`] when `date..` starts the interval or `..date` ends it
    /// (ISO 8601-2 §10.6), neither end is dated (D6), or the end date
    /// precedes the start date (D18).
    pub fn new(start: IntervalEndpoint, end: IntervalEndpoint) -> Result<Self, ValueError> {
        if matches!(start, IntervalEndpoint::OnOrAfter(_)) {
            return Err(ValueError::new(ValueError::START_ON_OR_AFTER));
        }
        if matches!(end, IntervalEndpoint::OnOrBefore(_)) {
            return Err(ValueError::new(ValueError::END_ON_OR_BEFORE));
        }
        if !start.is_dated() && !end.is_dated() {
            return Err(ValueError::new(ValueError::INTERVAL_UNDATED));
        }
        // Comparable only when both endpoints are dates (cf. ISO 8601-2
        // §7.14.2 Example 2).
        if let (IntervalEndpoint::Date(a), IntervalEndpoint::Date(b)) = (&start, &end) {
            if out_of_order(a, b) {
                return Err(ValueError::new(ValueError::INTERVAL_ORDER));
            }
        }
        Ok(Self { start, end })
    }

    /// The start endpoint.
    #[must_use]
    pub const fn start(&self) -> IntervalEndpoint {
        self.start
    }

    /// The end endpoint.
    #[must_use]
    pub const fn end(&self) -> IntervalEndpoint {
        self.end
    }
}

/// D18: `a` cannot precede-or-touch `b` — its earliest possible day is later
/// than `b`'s latest possible day.
fn out_of_order(a: &Date, b: &Date) -> bool {
    date_span(a).0 > date_span(b).1
}

/// `{…}` (all members) vs `[…]` (one member) — ISO 8601-2 §6.1–6.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetKind {
    /// `{…}` — every member applies.
    AllMembers,
    /// `[…]` — exactly one (unknown) member applies.
    OneMember,
}

/// `a..b` inside a set: every value between the two, inclusive (ISO 8601-2
/// §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetRange {
    /// The first value.
    start: Date,
    /// The last value.
    end: Date,
}

impl SetRange {
    /// The range from `start` to `end`.
    ///
    /// # Errors
    ///
    /// [`ValueError`] unless both ends denote one concrete calendar value of
    /// the same precision — unmasked, unqualified, not a season, without
    /// significant digits (decision D27) — and the end does not precede the
    /// start (D18).
    pub fn new(start: Date, end: Date) -> Result<Self, ValueError> {
        range_endpoint(&start)?;
        range_endpoint(&end)?;
        if start.precision() != end.precision() {
            return Err(ValueError::new(ValueError::RANGE_PRECISION));
        }
        if out_of_order(&start, &end) {
            return Err(ValueError::new(ValueError::RANGE_ORDER));
        }
        Ok(Self { start, end })
    }

    /// The first value.
    #[must_use]
    pub const fn start(&self) -> Date {
        self.start
    }

    /// The last value.
    #[must_use]
    pub const fn end(&self) -> Date {
        self.end
    }
}

/// A `..` range endpoint must denote a single concrete calendar value at
/// year, month or day precision (decision D27).
///
/// Every expansion example in
/// ISO 8601-2 §6.3 c/§6.4 uses plain dates, a mask-carrying endpoint would
/// make the range a relation between value *sets*, seasons have no
/// spec-defined successor, and a qualifier on an endpoint has no defined
/// spread over the expanded members.
///
/// # Errors
///
/// [`ValueError`] naming the first of those rules `d` breaks.
fn range_endpoint(d: &Date) -> Result<(), ValueError> {
    let message = if d.has_unspecified() {
        ValueError::RANGE_MASKED
    } else if d.is_uncertain() || d.is_approximate() {
        ValueError::RANGE_QUALIFIED
    } else if d.precision() == Precision::Season {
        ValueError::RANGE_SEASON
    } else if d.year.significant_digits.is_some() {
        ValueError::RANGE_SIGNIFICANT
    } else {
        return Ok(());
    };
    Err(ValueError::new(message))
}

/// An element of a set, including `..` range notation (ISO 8601-2 §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetElement {
    /// A single date.
    Date(Date),
    /// `..date` — this date or any earlier one.
    OnOrBefore(Date),
    /// `date..` — this date or any later one.
    OnOrAfter(Date),
    /// `a..b` — every expression between the two, inclusive.
    Range(SetRange),
}

/// A set expression (EDTF level 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set {
    /// All-members (`{}`) or one-member (`[]`) semantics.
    kind: SetKind,
    /// The elements, in written order.
    elements: Vec<SetElement>,
}

impl Set {
    /// A set of `elements`, in written order.
    ///
    /// # Errors
    ///
    /// [`ValueError`] when `elements` is empty.
    pub fn new(kind: SetKind, elements: Vec<SetElement>) -> Result<Self, ValueError> {
        if elements.is_empty() {
            return Err(ValueError::new(ValueError::SET_EMPTY));
        }
        Ok(Self { kind, elements })
    }

    /// All-members (`{}`) or one-member (`[]`) semantics.
    #[must_use]
    pub const fn kind(&self) -> SetKind {
        self.kind
    }

    /// The elements, in written order.
    #[must_use]
    pub fn elements(&self) -> &[SetElement] {
        &self.elements
    }
}

/// Any EDTF expression.
///
/// Under the `serde` feature it serialises as its canonical EDTF text and
/// deserialises by parsing that text: the string is the standard's own
/// interchange form, and a value that parses is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edtf {
    /// A single date.
    Date(Date),
    /// A date with time of day (level 0 only).
    DateTime(DateTime),
    /// A time interval.
    Interval(Interval),
    /// A set of dates (level 2).
    Set(Set),
}

#[cfg(feature = "serde")]
impl serde::Serialize for Edtf {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Edtf {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let text = <alloc::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl Edtf {
    /// Parse an EDTF string (levels 0–2, ISO 8601-2:2019 Annex A).
    ///
    /// # Errors
    ///
    /// [`ParseError`] with a byte offset and message pointing at the first
    /// place the input stops being valid EDTF.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        crate::parser::parse(input)
    }

    /// The minimum EDTF conformance level (0, 1 or 2) able to express this
    /// value. Semantically equivalent spellings classify identically: e.g.
    /// `?1985-?04-?12` reports level 1 because `1985-04-12?` means the same.
    #[must_use]
    pub fn level(&self) -> u8 {
        match self {
            Self::Date(d) => date_level(d, false),
            Self::DateTime(_) => 0,
            Self::Interval(iv) => interval_level(iv),
            Self::Set(_) => 2,
        }
    }

    /// Whether any date inside this value satisfies `f`.
    fn any_date<F>(&self, f: F) -> bool
    where
        F: Fn(&Date) -> bool,
    {
        match self {
            Self::Date(d) => f(d),
            Self::DateTime(dt) => f(&dt.date),
            Self::Interval(iv) => iv.start.date().is_some_and(&f) || iv.end.date().is_some_and(&f),
            Self::Set(s) => s.elements.iter().any(|e| match e {
                SetElement::Date(d) | SetElement::OnOrBefore(d) | SetElement::OnOrAfter(d) => f(d),
                // A range's endpoints are unqualified and unmasked (D27), so
                // no predicate asked here is true of them.
                SetElement::Range(_) => false,
            }),
        }
    }

    /// True if any component anywhere is marked uncertain (`?` or `%`).
    #[must_use]
    pub fn is_uncertain(&self) -> bool {
        self.any_date(Date::is_uncertain)
    }

    /// True if any component anywhere is marked approximate (`~` or `%`).
    #[must_use]
    pub fn is_approximate(&self) -> bool {
        self.any_date(Date::is_approximate)
    }

    /// True if any component anywhere contains an unspecified digit (`X`).
    #[must_use]
    pub fn has_unspecified(&self) -> bool {
        self.any_date(Date::has_unspecified)
    }
}

impl core::str::FromStr for Edtf {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// The conformance level an interval requires.
fn interval_level(iv: &Interval) -> u8 {
    let mut level = 0;
    for endpoint in [&iv.start, &iv.end] {
        level = level.max(match endpoint {
            IntervalEndpoint::Open | IntervalEndpoint::Unknown => 1,
            IntervalEndpoint::OnOrBefore(d) | IntervalEndpoint::OnOrAfter(d) => {
                2_u8.max(date_level(d, true))
            }
            IntervalEndpoint::Date(d) => date_level(d, true),
        });
    }
    level
}

/// The conformance level a date requires.
fn date_level(d: &Date, in_interval: bool) -> u8 {
    let mut level = 0_u8;
    match d.year.kind {
        YearKind::Standard {
            negative,
            digits: _,
        } => {
            if negative {
                level = level.max(1);
            }
        }
        YearKind::Big { value: _ } => level = level.max(1),
        YearKind::Exponential {
            significand: _,
            exponent: _,
        } => level = level.max(2),
    }
    if d.year.significant_digits.is_some() {
        level = level.max(2);
    }
    if let Some(v) = d.month.and_then(DateField::value) {
        if (21..=24).contains(&v) {
            level = level.max(1);
        } else if (25..=41).contains(&v) {
            level = level.max(2);
        }
    }
    let masks = mask_level(d);
    if masks > 0 {
        // Unspecified digits inside interval endpoints are a level 2 feature
        // (ISO 8601-2 §10.4 is absent from the level 1 half of Annex A).
        level = level.max(if in_interval { 2 } else { masks });
    }
    level.max(qualification_level(d))
}

/// 0 = unqualified; 1 = uniform ("complete") qualification; 2 = mixed.
fn qualification_level(d: &Date) -> u8 {
    let mut quals = [Some(d.year.qualifier), None, None];
    quals[1] = d.month.map(|m| m.qualifier);
    quals[2] = d.day.map(|f| f.qualifier);
    let present: Vec<Qualifier> = quals.iter().flatten().copied().collect();
    if present.iter().all(|q| !q.is_qualified()) {
        return 0;
    }
    if present.iter().all(|q| *q == present[0]) {
        1
    } else {
        2
    }
}

/// 0 = no X digits; 1 = one of the level 1 shapes (Annex A.5.5); 2 = any other.
fn mask_level(d: &Date) -> u8 {
    let year_x: [bool; 4] = match d.year.kind {
        YearKind::Standard {
            digits,
            negative: _,
        } => [
            digits[0].is_none(),
            digits[1].is_none(),
            digits[2].is_none(),
            digits[3].is_none(),
        ],
        _ => [false; 4],
    };
    let year_full = !year_x.iter().any(|&x| x);
    let month_full = d.month.map(|m| !m.has_unspecified());
    let month_all_x = d.month.map(|m| m.digits.iter().all(Option::is_none));
    let day_all_x = d.day.map(|f| f.digits.iter().all(Option::is_none));
    let day_any_x = d.day.map(DateField::has_unspecified);
    let any_x =
        !year_full || d.month.is_some_and(DateField::has_unspecified) || day_any_x.unwrap_or(false);
    if !any_x {
        return 0;
    }
    // Level 1 shapes: `1985-04-XX`, `1985-XX-XX`, `2004-XX`, `201X`, `20XX`.
    let level1 = (year_full && month_full == Some(true) && day_all_x == Some(true))
        || (year_full && month_all_x == Some(true) && day_all_x == Some(true))
        || (year_full && month_all_x == Some(true) && d.day.is_none())
        || (d.month.is_none() && matches!(year_x, [false, false, false | true, true]));
    if level1 { 1 } else { 2 }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::missing_panics_doc,
        reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
    )]

    use super::*;

    /// A year-precision fixture, built by the constructors themselves.
    ///
    /// # Errors
    ///
    /// Whatever the constructors refuse; every fixture here is valid.
    fn year_date(year: i64, qualifier: Qualifier) -> Result<Date, ValueError> {
        Date::new(
            Year::from_value(year)?.with_qualifier(qualifier),
            None,
            None,
        )
    }

    /// A qualifier on the year alone is enough: it is the first of the
    /// three components asked, with no month or day to answer instead.
    #[test]
    fn a_year_only_qualifier_marks_the_date() {
        let uncertain = Qualifier {
            uncertain: true,
            approximate: false,
        };
        let flags = year_date(1985, uncertain).map(|d| (d.is_uncertain(), d.is_approximate()));
        assert_eq!(flags, Ok((true, false)));
    }

    /// A qualifier on the month alone is enough too, with no day after it:
    /// each component answers on its own, none needs another to agree.
    #[test]
    fn a_month_only_qualifier_marks_the_date() {
        let uncertain = Qualifier {
            uncertain: true,
            approximate: false,
        };
        let date = DateField::from_value(4).and_then(|month| {
            Date::new(
                Year::from_value(1985)?,
                Some(month.with_qualifier(uncertain)),
                None,
            )
        });
        let flags = date.map(|d| (d.is_uncertain(), d.is_approximate()));
        assert_eq!(flags, Ok((true, false)));
    }

    /// An unqualified value is not approximate.
    #[test]
    fn an_unqualified_value_is_not_approximate() {
        let flags = year_date(1985, Qualifier::default())
            .map(Edtf::Date)
            .map(|e| (e.is_approximate(), e.is_uncertain()));
        assert_eq!(flags, Ok((false, false)));
    }

    /// A month outside 1-12 holds no day: the completion scan only asks of
    /// calendar months, and anything else answers no rather than some
    /// month's length.
    #[test]
    fn month_admits_day_refuses_a_month_outside_1_12() {
        let year = YearKind::Standard {
            negative: false,
            digits: [Some(2), Some(0), Some(0), Some(1)],
        };
        assert!(!month_admits_day(21, 1, year, &mut None));
    }

    /// Only a four-digit year carries a month, so leap-ness is never asked
    /// of a `Y` year; asked anyway, it answers no rather than guess.
    #[test]
    fn a_y_year_is_never_leap_possible() {
        assert!(!year_leap_possible(YearKind::Big { value: 20_000 }));
    }
}
