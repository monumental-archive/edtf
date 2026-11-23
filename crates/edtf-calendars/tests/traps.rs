// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The trap list from issue #21 — every acceptance case the crate exists
//! to get right, pinned. Historically attested O.S./N.S. pairs live in
//! `tests/oracle.rs`; generative invariants in `tests/props.rs`.

#![expect(
    clippy::min_ident_chars,
    reason = "the conversions keep their published algorithms' own variable names (y, m, d, a); renaming them breaks the correspondence to the sources they are checked against"
)]
#![expect(clippy::panic, reason = "a panic in a test IS the failure signal")]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use edtf_calendars::{
    CalendarError, JulianDate, convert, gregorian_to_julian, is_julian_leap, julian_to_gregorian,
    parse_julian,
};
use edtf_core::{BoundDate, Edtf};

const fn j(year: i64, month: u8, day: u8) -> JulianDate {
    JulianDate { year, month, day }
}

fn edtf(text: &str) -> Edtf {
    Edtf::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn g(year: i64, month: u8, day: u8) -> BoundDate {
    BoundDate::new(year, month, day).unwrap_or_else(|e| panic!("{year}-{month}-{day}: {e}"))
}

/// The offset is not constant: 10 days at the 1582 reform, 11 from 1700,
/// 12 from 1800, 13 from 1900.
#[test]
fn offset_steps() {
    assert_eq!(julian_to_gregorian(j(1582, 10, 5)), Ok(g(1582, 10, 15)));
    assert_eq!(julian_to_gregorian(j(1700, 3, 1)), Ok(g(1700, 3, 12)));
    assert_eq!(julian_to_gregorian(j(1800, 3, 1)), Ok(g(1800, 3, 13)));
    assert_eq!(julian_to_gregorian(j(1900, 3, 1)), Ok(g(1900, 3, 14)));
    // The issue's own example: 1 March 1917 O.S. = 14 March 1917 N.S.
    assert_eq!(julian_to_gregorian(j(1917, 3, 1)), Ok(g(1917, 3, 14)));
    // And the next step, after Gregorian 2100 skips its leap day.
    assert_eq!(julian_to_gregorian(j(2100, 3, 1)), Ok(g(2100, 3, 15)));
}

/// Julian leap rule is every-4-years, no century exception: Julian
/// 1900-02-29 exists and converts, while Gregorian 1900-02-29 is not a
/// day at all: `BoundDate` refuses it before any conversion can be asked.
#[test]
fn julian_centuries_are_leap_years() {
    assert_eq!(julian_to_gregorian(j(1900, 2, 29)), Ok(g(1900, 3, 13)));
    assert_eq!(julian_to_gregorian(j(1700, 2, 29)), Ok(g(1700, 3, 11)));
    assert_eq!(julian_to_gregorian(j(2100, 2, 29)), Ok(g(2100, 3, 14)));
    assert_eq!(BoundDate::new(1900, 2, 29).ok(), None);
    // 2000 is leap in both calendars.
    assert_eq!(julian_to_gregorian(j(2000, 2, 29)), Ok(g(2000, 3, 13)));
    assert_eq!(gregorian_to_julian(g(2000, 2, 29)), Ok(j(2000, 2, 16)));
    // Julian 1900-02-30 is invalid in any calendar.
    assert_eq!(
        julian_to_gregorian(j(1900, 2, 30)),
        Err(CalendarError::InvalidDate)
    );
}

/// Proleptic in both directions: defined before 1582, the calendars
/// coincide across the 3rd century, and Julian is *ahead* before that.
#[test]
fn proleptic_before_the_reform() {
    // Offset zero: Gregorian 200-03-01 .. 300-02-28 aligns exactly.
    assert_eq!(julian_to_gregorian(j(200, 3, 1)), Ok(g(200, 3, 1)));
    assert_eq!(julian_to_gregorian(j(250, 6, 15)), Ok(g(250, 6, 15)));
    assert_eq!(julian_to_gregorian(j(300, 2, 28)), Ok(g(300, 2, 28)));
    // Julian ahead: 1 January 1 AD (O.S.) is 30 December 1 BC (N.S.).
    assert_eq!(julian_to_gregorian(j(1, 1, 1)), Ok(g(0, 12, 30)));
    assert_eq!(gregorian_to_julian(g(0, 1, 1)), Ok(j(0, 1, 3)));
}

/// Year boundaries shift: Julian late-December dates land in the next
/// Gregorian year.
#[test]
fn year_boundary_shifts() {
    // The classic Orthodox-Christmas off-by-one-year error.
    assert_eq!(julian_to_gregorian(j(1917, 12, 25)), Ok(g(1918, 1, 7)));
    // And a date that lands exactly on the Gregorian New Year.
    assert_eq!(julian_to_gregorian(j(1899, 12, 20)), Ok(g(1900, 1, 1)));
}

/// Astronomical year numbering throughout: year 0 exists and is a Julian
/// leap year.
#[test]
fn astronomical_numbering() {
    assert!(is_julian_leap(0));
    assert!(is_julian_leap(-4));
    assert!(!is_julian_leap(-1));
    assert_eq!(julian_to_gregorian(j(0, 2, 29)), Ok(g(0, 2, 27)));
    // Julian day zero: 1 January 4713 BC (O.S.) = 24 November 4714 BC
    // (proleptic Gregorian) — astronomical -4712 and -4713.
    assert_eq!(julian_to_gregorian(j(-4712, 1, 1)), Ok(g(-4713, 11, 24)));
}

/// Precision honesty: a Julian year or month never converts to a single
/// Gregorian year or month — `convert` returns an explicit interval.
#[test]
fn precision_honesty() {
    // The issue's example: Julian 1917 spans two Gregorian years.
    assert_eq!(convert(1917, None, None), Ok(edtf("1917-01-14/1918-01-13")));
    // Julian February 1900 straddles the 12→13 offset change *inside* the
    // month, and its last day is the century leap day.
    assert_eq!(
        convert(1900, Some(2), None),
        Ok(edtf("1900-02-13/1900-03-13"))
    );
    // Day precision converts exactly.
    assert_eq!(convert(1917, Some(12), Some(25)), Ok(edtf("1918-01-07")));
    // A day without a month is not a precision, it is a mistake.
    assert_eq!(
        convert(1917, None, Some(25)),
        Err(CalendarError::InvalidDate)
    );
    assert_eq!(
        convert(1917, Some(13), None),
        Err(CalendarError::InvalidDate)
    );
    assert_eq!(
        convert(1900, Some(2), Some(30)),
        Err(CalendarError::InvalidDate)
    );
}

/// A result EDTF cannot spell is refused.
///
/// A Gregorian day outside the years
/// −9999 to 9999 needs the `Y` form, which carries no month or day. The
/// edges sit where Julian and Gregorian years part: Julian 9999 ends in
/// Gregorian 10000, and Julian -9999 begins in Gregorian -10000.
#[test]
fn results_without_an_edtf_spelling_are_refused() {
    assert_eq!(
        convert(9999, Some(12), Some(31)),
        Err(CalendarError::OutOfRange)
    );
    assert_eq!(convert(9999, None, None), Err(CalendarError::OutOfRange));
    assert_eq!(
        convert(10000, Some(1), Some(1)),
        Err(CalendarError::OutOfRange)
    );
    assert_eq!(convert(10000, None, None), Err(CalendarError::OutOfRange));
    assert_eq!(convert(-9999, None, None), Err(CalendarError::OutOfRange));
    assert_eq!(
        convert(i64::MIN, None, None),
        Err(CalendarError::OutOfRange)
    );
    // The last spellable Julian day, and the first.
    assert_eq!(convert(9999, Some(10), Some(19)), Ok(edtf("9999-12-31")));
    assert_eq!(
        convert(9999, Some(10), Some(20)),
        Err(CalendarError::OutOfRange)
    );
    assert_eq!(convert(-9999, Some(3), Some(19)), Ok(edtf("-9999-01-01")));
    assert_eq!(
        convert(-9999, Some(3), Some(18)),
        Err(CalendarError::OutOfRange)
    );
}

/// The text forms: `Y`, `Y-MM`, `Y-MM-DD`.
///
/// Astronomical year, an optional `-` and no `+`. A leading `+` is the case a digit check alone refuses:
/// `i64::from_str` accepts it, so without the check `+1917` would read.
#[test]
fn parse_julian_reads_the_three_forms() {
    assert_eq!(parse_julian("1917"), Ok((1917, None, None)));
    assert_eq!(parse_julian("1917-10"), Ok((1917, Some(10), None)));
    assert_eq!(parse_julian("1917-10-25"), Ok((1917, Some(10), Some(25))));
    assert_eq!(parse_julian("-0044-03-15"), Ok((-44, Some(3), Some(15))));
    // Shape only: the calendar check is `convert`'s.
    assert_eq!(parse_julian("1917-13-99"), Ok((1917, Some(13), Some(99))));
    for bad in [
        "",
        "-",
        "+1917",
        "-+44-03-15",
        "1917-2-3",
        "191A",
        "1917-",
        "1917-10-25-01",
        "1917-aa",
        "--1917",
        " 1917",
    ] {
        assert_eq!(parse_julian(bad), Err(CalendarError::Malformed), "{bad:?}");
    }
    // A year of the right shape that will not fit `i64` is out of range,
    // and the most negative `i64` fits.
    assert_eq!(
        parse_julian("-9223372036854775808"),
        Ok((i64::MIN, None, None))
    );
    assert_eq!(
        parse_julian("9223372036854775808"),
        Err(CalendarError::OutOfRange)
    );
    assert_eq!(
        parse_julian("99999999999999999999999999999999999999999-01"),
        Err(CalendarError::OutOfRange)
    );
}

/// Errors and dates render for diagnostics; the negative-year form keeps the
/// astronomical sign and four-digit padding.
#[test]
fn display_forms() {
    assert_eq!(
        CalendarError::Malformed.to_string(),
        "expected a Julian date as Y, Y-MM or Y-MM-DD"
    );
    assert_eq!(
        CalendarError::InvalidDate.to_string(),
        "not a valid calendar date"
    );
    assert_eq!(
        CalendarError::OutOfRange.to_string(),
        "converted year out of range"
    );
    assert_eq!(j(1917, 10, 25).to_string(), "1917-10-25");
    assert_eq!(j(-44, 3, 15).to_string(), "-0044-03-15");
    // Year 0 is the sign boundary: it is unsigned, never "-0000".
    assert_eq!(j(0, 1, 1).to_string(), "0000-01-01");
}

/// Every rejection clause of the calendar-day check, separately.
#[test]
fn calendar_check_rejects_each_clause() {
    assert_eq!(
        convert(1917, Some(0), None),
        Err(CalendarError::InvalidDate)
    );
    assert_eq!(
        convert(1917, Some(1), Some(0)),
        Err(CalendarError::InvalidDate)
    );
    // Straight through the day-precision entry point too.
    assert_eq!(
        julian_to_gregorian(j(1917, 13, 1)),
        Err(CalendarError::InvalidDate)
    );
}

/// `i64::MIN` has no negation: the negative branch prints the magnitude as
/// unsigned, so the most negative year prints with one minus sign.
#[test]
fn julian_dates_print_every_year() {
    assert_eq!(j(i64::MIN, 1, 1).to_string(), "-9223372036854775808-01-01");
    assert_eq!(j(-5, 1, 1).to_string(), "-0005-01-01");
    assert_eq!(j(5, 1, 1).to_string(), "0005-01-01");
}
