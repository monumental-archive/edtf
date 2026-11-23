// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generative invariants (issue #21's test ceremony): round-trip identity
//! in both directions, offset monotonicity, and span lengths that match
//! the Julian calendar's own day counts.
//!
//! The arithmetic is also searched over its whole input domain (#345):
//! every `i64` year, edges weighted in, crossed with every `u8` month and
//! day. There the oracles are test-local: the Julian leap rule, month
//! lengths and a Julian day count are re-derived below, independently of
//! the crate (same spirit as `edtf-core/tests/props.rs` re-deriving the
//! leap rule), and the Gregorian side is `edtf-core`'s own
//! `BoundDate::julian_day_number`, so a bug in the crate's Julian
//! arithmetic cannot cancel itself out.

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
    CalendarError, Converted, JulianDate, convert, gregorian_to_julian, is_julian_leap,
    julian_to_gregorian,
};
use edtf_core::BoundDate;
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
    fn year_span_has_julian_length(y in -100_000_i64..=100_000) {
        let Ok(Converted::Span { earliest, latest }) = convert(y, None, None) else {
            return Err(TestCaseError::fail("year must convert to a span"));
        };
        let days = latest.julian_day_number() - earliest.julian_day_number() + 1;
        let expected = if is_julian_leap(y) { 366 } else { 365 };
        prop_assert_eq!(days, expected, "Julian {} spans {}..{}", y, earliest, latest);
    }

    /// A month span covers exactly the Julian month's own day count.
    #[test]
    fn month_span_has_julian_length(y in -100_000_i64..=100_000, m in 1_u8..=12) {
        let Ok(Converted::Span { earliest, latest }) = convert(y, Some(m), None) else {
            return Err(TestCaseError::fail("month must convert to a span"));
        };
        let days = latest.julian_day_number() - earliest.julian_day_number() + 1;
        prop_assert_eq!(days, i128::from(julian_last_day(m, is_julian_leap(y))));
    }

    /// Day-precision conversion agrees with the span endpoints: converting
    /// the first/last Julian day of a month lands on its span's bounds.
    #[test]
    fn day_conversion_agrees_with_spans(y in -100_000_i64..=100_000, m in 1_u8..=12) {
        let Ok(Converted::Span { earliest, latest }) = convert(y, Some(m), None) else {
            return Err(TestCaseError::fail("month must convert to a span"));
        };
        prop_assert_eq!(convert(y, Some(m), Some(1)), Ok(Converted::Day(earliest)));
        let last = julian_last_day(m, is_julian_leap(y));
        prop_assert_eq!(convert(y, Some(m), Some(last)), Ok(Converted::Day(latest)));
    }
}

/// The Julian leap rule, re-derived: every fourth year, no century
/// exception, astronomical numbering. Deliberately not the crate's
/// `is_julian_leap`, so a change to that rule is a counter-example here.
const fn julian_leap(y: i64) -> bool {
    y.rem_euclid(4) == 0
}

/// Whether the fields name a proleptic Julian calendar date.
fn names_julian_date(year: i64, month: u8, day: u8) -> bool {
    (1..=12).contains(&month) && day >= 1 && day <= julian_last_day(month, julian_leap(year))
}

/// Julian Day Number of a proleptic Julian date, by counting days.
///
/// Not the crate's Fliegel–Van Flandern form: the days before year
/// `y` (365 a year plus one per leap year from 0 up to `y`, which is
/// `⌊(y + 3) / 4⌋` for either sign), then the days before the month, then
/// the day. Julian 0000-01-01 is JDN 1 721 058.
fn julian_day_count(year: i64, month: u8, day: u8) -> i128 {
    let y = i128::from(year);
    let before_month: i128 = (1..month)
        .map(|m| i128::from(julian_last_day(m, julian_leap(year))))
        .sum();
    1_721_058 + 365 * y + (y + 3).div_euclid(4) + before_month + i128::from(day) - 1
}

/// The JDNs of the first and last Gregorian days whose year fits `i64`.
fn gregorian_jdn_limits() -> (i128, i128) {
    let first = BoundDate::new(i64::MIN, 1, 1).expect("1 January exists in every year");
    let last = BoundDate::new(i64::MAX, 12, 31).expect("31 December exists in every year");
    (first.julian_day_number(), last.julian_day_number())
}

/// Any `i64` year, with the edges weighted in: the `i64` limits, the
/// Julian years where the Gregorian year leaves `i64`, the years around
/// zero and the reform, and leap and century years.
fn any_year() -> impl Strategy<Value = i64> {
    // Where the Gregorian year leaves `i64`. Only a centre for sampling;
    // the properties judge the result by the oracles above.
    let edge = |d| {
        gregorian_to_julian(d)
            .expect("the Julian year of an i64 Gregorian limit fits i64")
            .year
    };
    let low = edge(BoundDate::new(i64::MIN, 1, 1).expect("1 January exists"));
    let high = edge(BoundDate::new(i64::MAX, 12, 31).expect("31 December exists"));
    prop_oneof![
        4 => any::<i64>(),
        2 => i64::MIN..=i64::MIN + 400,
        2 => i64::MAX - 400..=i64::MAX,
        2 => low - 400..=low + 400,
        2 => high - 400..=high + 400,
        2 => -10_000_i64..=10_000,
        2 => (i64::MIN.div_euclid(400) + 1..=i64::MAX.div_euclid(400)).prop_map(|c| c * 400),
        2 => (i64::MIN.div_euclid(100) + 1..=i64::MAX.div_euclid(100)).prop_map(|c| c * 100),
    ]
}

/// Any month and day value.
///
/// Most draws are `0..=13` and `0..=32`, where
/// the valid values and the first invalid ones on each side sit; February
/// and the days 28 to 32, where the leap rule and the month lengths
/// decide, are weighted in; the rest are any `u8`.
fn any_fields() -> impl Strategy<Value = (u8, u8)> {
    let month = prop_oneof![3 => 0_u8..=13, 1 => Just(2_u8), 1 => any::<u8>()];
    let day = prop_oneof![2 => 0_u8..=32, 2 => 28_u8..=32, 1 => any::<u8>()];
    (month, day)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8192))]

    /// `julian_to_gregorian` returns for any `i64` year and any `u8`
    /// month and day, and refuses with `InvalidDate` exactly when the
    /// fields name no Julian calendar date.
    #[test]
    fn julian_to_gregorian_is_total_and_refuses_exactly_non_dates(
        year in any_year(), (month, day) in any_fields()
    ) {
        let result = julian_to_gregorian(JulianDate { year, month, day });
        prop_assert_eq!(
            result == Err(CalendarError::InvalidDate),
            !names_julian_date(year, month, day),
            "Julian {}-{}-{} gave {:?}", year, month, day, result
        );
    }

    /// Every day `julian_to_gregorian` returns is the Julian date's own
    /// day, and `gregorian_to_julian` takes it back to the date given,
    /// over the whole `i64` year range.
    #[test]
    fn every_converted_day_round_trips_over_all_years(
        year in any_year(), (month, day) in any_fields()
    ) {
        let date = JulianDate { year, month, day };
        if let Ok(g) = julian_to_gregorian(date) {
            prop_assert_eq!(
                g.julian_day_number(), julian_day_count(year, month, day),
                "Julian {} gave {}", date, g
            );
            prop_assert_eq!(gregorian_to_julian(g), Ok(date), "via {}", g);
        }
    }

    /// The only other refusal is `OutOfRange`, and it comes exactly when
    /// the Julian date's Gregorian year leaves `i64`.
    #[test]
    fn out_of_range_only_past_the_i64_years(
        year in any_year(), (month, day) in any_fields()
    ) {
        let (first, last) = gregorian_jdn_limits();
        let named = names_julian_date(year, month, day);
        let past = named && !(first..=last).contains(&julian_day_count(year, month, day));
        let result = julian_to_gregorian(JulianDate { year, month, day });
        prop_assert_eq!(
            result == Err(CalendarError::OutOfRange), past,
            "Julian {}-{}-{} gave {:?}", year, month, day, result
        );
        prop_assert!(
            matches!(result, Ok(_) | Err(CalendarError::OutOfRange)) || !named,
            "Julian {}-{}-{} gave {:?}", year, month, day, result
        );
    }
}
