// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generative invariants (issue #21's test ceremony): round-trip identity
//! in both directions, offset monotonicity, and span lengths that match
//! the Julian calendar's own day counts.
//!
//! The Gregorian JDN formula below is a test-local oracle, implemented
//! independently of the crate (same spirit as `edtf-core/tests/props.rs`
//! re-deriving the leap rule): day counting happens on the Gregorian day
//! scale only, so a bug in the crate's Julian arithmetic cannot cancel
//! itself out.

#![expect(
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::non_ascii_literal,
    reason = "proptest! expands to code carrying the generated corpus, which includes the Cyrillic month and century forms this crate normalises"
)]
#![expect(
    clippy::min_ident_chars,
    reason = "the conversions keep their published algorithms' own variable names (y, m, d, a); renaming them breaks the correspondence to the sources they are checked against"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic is over literal fixtures and generator-bounded values; an overflow here would fail the test, which is the signal"
)]
#![expect(
    clippy::unreachable,
    reason = "an unreachable! naming the invariant the fixture guarantees"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use edtf_calendars::{
    CalendarError, JulianDate, convert, gregorian_to_julian, is_julian_leap, julian_to_gregorian,
    parse_julian,
};
use edtf_core::{Bound, BoundDate, Edtf};
use proptest::prelude::*;

fn julian_last_day(month: u8, leap: bool) -> u8 {
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

/// The written-date offset: how many days later the Julian nominal date
/// `y-m-d` falls than the Gregorian nominal date `y-m-d`.
fn offset(y: i64, m: u8, d: u8) -> i128 {
    let g = julian_to_gregorian(JulianDate {
        year: y,
        month: m,
        day: d,
    })
    .expect("valid by construction");
    g.julian_day_number()
        - BoundDate::new(y, m, d)
            .expect("a day both calendars have")
            .julian_day_number()
}

const fn j(year: i64, month: u8, day: u8) -> JulianDate {
    JulianDate { year, month, day }
}

/// A valid proleptic Julian date across a wide year range.
fn julian_date() -> impl Strategy<Value = JulianDate> {
    (-1_000_000_i64..=1_000_000, 1_u8..=12).prop_flat_map(|(year, month)| {
        (1_u8..=julian_last_day(month, is_julian_leap(year))).prop_map(move |day| JulianDate {
            year,
            month,
            day,
        })
    })
}

/// The earliest and latest Gregorian days of `value`, which this crate only
/// ever builds closed.
fn bound_days(value: &Edtf) -> (BoundDate, BoundDate) {
    let bounds = value.bounds();
    let (Bound::Date(earliest), Bound::Date(latest)) = (bounds.earliest, bounds.latest) else {
        unreachable!("a converted value is a date or a closed interval")
    };
    (earliest, latest)
}

/// The first and last Gregorian days `convert` gives Julian year `y`.
fn year_span(y: i64) -> (BoundDate, BoundDate) {
    let value = convert(y, None, None).expect("year converts");
    bound_days(&value)
}

/// The independent oracle for `parse_julian`'s shape.
///
/// Scanned from the end, the text is up to two `-dd` groups after an
/// optional `-` and one or more digits. Returns the year's sign and digits
/// and the groups' values when the shape holds.
fn julian_shape(s: &str) -> Option<(bool, &str, Vec<u8>)> {
    let mut rest = s;
    let mut fields = Vec::new();
    for _ in 0_u8..2 {
        let &[.., b'-', tens @ b'0'..=b'9', units @ b'0'..=b'9'] = rest.as_bytes() else {
            break;
        };
        let Some(before) = rest.get(..rest.len() - 3) else {
            break;
        };
        // The group only counts if a year is left before it.
        if before.trim_start_matches('-').is_empty() {
            break;
        }
        fields.insert(0, (tens - b'0') * 10 + (units - b'0'));
        rest = before;
    }
    let (negative, digits) = rest.strip_prefix('-').map_or((false, rest), |d| (true, d));
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some((negative, digits, fields))
}

/// A string near the three forms (signs, letters, short and long fields,
/// extra groups), mixed with arbitrary text.
fn julian_text() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<String>(),
        "[-+]?[0-9]{0,25}(-[0-9a+]{0,3}){0,3}",
        "-?[0-9]{1,6}(-[0-9]{2}){0,2}",
    ]
}

const fn is_gregorian_leap(y: i64) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

/// A valid proleptic Gregorian date across a wide year range.
fn gregorian_date() -> impl Strategy<Value = BoundDate> {
    (-1_000_000_i64..=1_000_000, 1_u8..=12).prop_flat_map(|(year, month)| {
        (1_u8..=julian_last_day(month, is_gregorian_leap(year)))
            .prop_map(move |day| BoundDate::new(year, month, day).expect("a real Gregorian day"))
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Julian → Gregorian → Julian is the identity.
    #[test]
    fn round_trips_from_julian(d in julian_date()) {
        let g = julian_to_gregorian(d).expect("valid Julian date converts");
        prop_assert_eq!(gregorian_to_julian(g), Ok(d), "via {}", g);
    }

    /// Gregorian → Julian → Gregorian is the identity.
    #[test]
    fn round_trips_from_gregorian(d in gregorian_date()) {
        let j = gregorian_to_julian(d).expect("valid Gregorian date converts");
        prop_assert_eq!(julian_to_gregorian(j), Ok(d), "via {}", j);
    }

    /// The Julian–Gregorian offset never shrinks as time advances: for the
    /// same nominal month/day, a later year's offset is >= an earlier
    /// year's (day <= 28 so the nominal date exists in every year).
    #[test]
    fn offset_is_monotonic(
        y1 in -100_000_i64..=100_000, span in 0_i64..=100_000,
        m in 1_u8..=12, d in 1_u8..=28
    ) {
        let y2 = y1 + span;
        prop_assert!(offset(y1, m, d) <= offset(y2, m, d));
    }

    /// A year span covers exactly the Julian year's own day count, and its
    /// endpoints are the converted first and last days.
    #[test]
    fn year_span_has_julian_length(y in -9_000_i64..=9_000) {
        let (earliest, latest) = year_span(y);
        let days = latest.julian_day_number() - earliest.julian_day_number() + 1;
        let expected = if is_julian_leap(y) { 366 } else { 365 };
        prop_assert_eq!(days, expected, "Julian {} spans {}..{}", y, earliest, latest);
    }

    /// A month span covers exactly the Julian month's own day count.
    #[test]
    fn month_span_has_julian_length(y in -9_000_i64..=9_000, m in 1_u8..=12) {
        let (earliest, latest) =
            bound_days(&convert(y, Some(m), None).expect("month converts"));
        let days = latest.julian_day_number() - earliest.julian_day_number() + 1;
        prop_assert_eq!(days, i128::from(julian_last_day(m, is_julian_leap(y))));
    }

    /// Day-precision conversion agrees with the span endpoints: converting
    /// the first/last Julian day of a month lands on its span's bounds.
    #[test]
    fn day_conversion_agrees_with_spans(y in -9_000_i64..=9_000, m in 1_u8..=12) {
        let (earliest, latest) =
            bound_days(&convert(y, Some(m), None).expect("month converts"));
        let first = convert(y, Some(m), Some(1)).expect("day converts");
        prop_assert_eq!(bound_days(&first), (earliest, earliest));
        let last_day = julian_last_day(m, is_julian_leap(y));
        let last = convert(y, Some(m), Some(last_day)).expect("day converts");
        prop_assert_eq!(bound_days(&last), (latest, latest));
    }

    /// `convert` over any parts returns, and agrees with the arithmetic.
    ///
    /// An `Ok` value's bounds are the
    /// arithmetic's first and last Gregorian days of the Julian span, and
    /// its text parses back to it; an `OutOfRange` is a span whose
    /// arithmetic is refused or leaves the years −9999 to 9999.
    #[test]
    fn convert_returns_edtf_matching_the_arithmetic(
        y in prop_oneof![any::<i64>(), -10_100_i64..=10_100],
        m in prop_oneof![any::<Option<u8>>(), (1_u8..=12).prop_map(Some), Just(None)],
        d in prop_oneof![any::<Option<u8>>(), (1_u8..=31).prop_map(Some), Just(None)],
    ) {
        let result = convert(y, m, d);
        let leap = is_julian_leap(y);
        let span = match (m, d) {
            (Some(month), Some(day)) => Some((j(y, month, day), j(y, month, day))),
            (Some(month @ 1..=12), None) => {
                Some((j(y, month, 1), j(y, month, julian_last_day(month, leap))))
            }
            (None, None) => Some((j(y, 1, 1), j(y, 12, 31))),
            _ => None,
        };
        let Some((first, last)) = span else {
            prop_assert_eq!(result, Err(CalendarError::InvalidDate));
            return Ok(());
        };
        let days = julian_to_gregorian(first).and_then(|f| Ok((f, julian_to_gregorian(last)?)));
        match (result, days) {
            (Ok(value), Ok((f, l))) => {
                prop_assert_eq!(bound_days(&value), (f, l));
                prop_assert_eq!(Edtf::parse(&value.to_string()), Ok(value.clone()));
                prop_assert_eq!(matches!(value, Edtf::Date(_)), d.is_some());
            }
            (Err(CalendarError::OutOfRange), Ok((f, l))) => {
                let spelled = |g: BoundDate| (-9999..=9999).contains(&g.year());
                prop_assert!(!spelled(f) || !spelled(l), "{}..{} has a spelling", f, l);
            }
            (Err(e), Err(arithmetic)) => prop_assert_eq!(e, arithmetic),
            (other, arithmetic) => {
                return Err(TestCaseError::fail(format!("{other:?} for {arithmetic:?}")));
            }
        }
    }

    /// `parse_julian` reads exactly the three forms.
    ///
    /// Any text the oracle
    /// shapes as `Y`, `Y-MM` or `Y-MM-DD` reads to its parts (or is out of
    /// range when the year will not fit `i64`), and any other text is
    /// `Malformed`.
    #[test]
    fn parse_julian_accepts_exactly_the_three_forms(s in julian_text()) {
        let parsed = parse_julian(&s);
        match julian_shape(&s) {
            None => prop_assert_eq!(parsed, Err(CalendarError::Malformed)),
            Some((negative, digits, fields)) => {
                let year = digits
                    .parse::<i128>()
                    .ok()
                    .and_then(|v| i64::try_from(if negative { -v } else { v }).ok());
                let expected = year
                    .map(|y| (y, fields.first().copied(), fields.get(1).copied()))
                    .ok_or(CalendarError::OutOfRange);
                prop_assert_eq!(parsed, expected);
            }
        }
    }
}
