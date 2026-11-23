// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spec-derived tests beyond the legacy corpus: Annex A examples, the
//! reject list (spec-notes §5), and the D1-D17 decisions (spec-notes §9).

#![expect(
    clippy::unwrap_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::min_ident_chars,
    reason = "the test bodies use the same y/m/d date-component names as the code they exercise"
)]
#![expect(clippy::panic, reason = "a panic in a test IS the failure signal")]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "the wildcard covers variants the assertion has already excluded"
)]
#![expect(
    clippy::shadow_unrelated,
    reason = "short-lived rebinding inside one assertion chain"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use edtf_core::{
    Bound, Date, DateField, DateTime, Edtf, Interval, IntervalEndpoint, Precision, Qualifier, Set,
    SetKind, SetRange, Time, TimeShift, ValueError, Year, YearKind, is_valid, level,
};

fn assert_level(s: &str, want: u8) {
    assert_eq!(level(s), Some(want), "level of {s:?}");
}

fn assert_invalid(s: &str) {
    assert!(!is_valid(s), "{s:?} should be invalid");
}

#[test]
fn level0_dates_and_datetimes() {
    assert_level("1985-04-12", 0);
    assert_level("1985-04", 0);
    assert_level("1985", 0);
    assert_level("0000", 0); // year zero exists
    assert_level("2000-02-29", 0); // leap day, leap year
    assert_level("1985-04-12T23:20:30", 0);
    assert_level("1985-04-12T23:59:60Z", 0); // leap second accepted (D3)
    assert_level("1985-04-12T23:20:30+14:00", 0);
}

#[test]
fn level1_features() {
    assert_level("-1985", 1);
    assert_level("-0001", 1); // year before 0000
    assert_level("Y170000002", 1);
    assert_level("Y-170000002", 1);
    assert_level("2001-21", 1);
    assert_level("2001-24", 1);
    assert_level("1985-04-12?", 1);
    assert_level("1985-04?", 1);
    assert_level("1985~", 1);
    assert_level("1985%", 1);
    assert_level("1985-04-XX", 1);
    assert_level("1985-XX-XX", 1);
    assert_level("2004-XX", 1);
    assert_level("201X", 1);
    assert_level("20XX", 1);
    assert_level("1985-04-12/..", 1);
    assert_level("../1985-04-12", 1);
    assert_level("1986-04/", 1);
    assert_level("/1985", 1);
    assert_level("1984?/2004%", 1);
    assert_level("../1985-04-12?", 1);
    assert_level("1985-04-12~/", 1);
}

#[test]
fn level2_features() {
    assert_level("Y-17E7", 2);
    assert_level("Y3388E2S3", 2);
    assert_level("1950S2", 2);
    assert_level("Y171010000S3", 2);
    assert_level("2001-34", 2); // quarter 2
    assert_level("2001-41", 2); // semestral 2
    assert_level("2004-06~-11", 2);
    assert_level("2004?-06-11", 2);
    assert_level("?2004-06-~11", 2);
    assert_level("2004-%06-11", 2);
    assert_level("2004-06?-~11", 2); // mixed group + individual (8.4.6)
    assert_level("156X-12-25", 2);
    assert_level("XXXX-12-XX", 2);
    assert_level("1XXX-XX", 2);
    assert_level("1560-X2", 2);
    assert_level("XXXX", 2);
    assert_level("2XXX", 2);
    assert_level("1XX3", 2);
    assert_level("{1960,1961-12}", 2);
    assert_level("[1667,1760-12]", 2);
    assert_level("[..1984]", 2);
    assert_level("[1984..]", 2);
    assert_level("[1670..1672]", 2);
    assert_level("{..1983-12-31,1984-10-10..1984-11-01,1984-11-05..}", 2);
    assert_level("2004-06-~01/2004-06-~20", 2);
    assert_level("..2004-06-01/2004-06-20..", 2);
    assert_level("2004-06-XX/2004-07-03", 2); // X in interval endpoint (D16)
}

#[test]
fn calendar_validation() {
    assert_invalid("1985-02-30");
    assert_invalid("1985-04-31");
    assert_invalid("2001-02-29"); // not a leap year
    assert_invalid("1900-02-29"); // centennial, not leap
    assert!(is_valid("2000-02-29")); // divisible by 400
    assert!(is_valid("1985-0X-31")); // months 01,03,05,07,08 complete it (D11)
    assert_invalid("1985-02-3X"); // no completion: Feb has no day 30-39
    assert_invalid("X900-02-29"); // no X900 year is a leap year
    assert!(is_valid("XXXX-02-29")); // some leap year exists
    assert_invalid("1985-00");
    assert_invalid("1985-13");
    assert_invalid("2004-20"); // 13-20 unassigned
    assert_invalid("1985-04-00");
    assert_invalid("1985-04-32");
    assert_invalid("2001-21-05"); // seasons cannot carry a day
}

#[test]
fn reject_list_not_edtf() {
    assert_invalid("19850412"); // basic format
    assert_invalid("1985-102"); // ordinal date
    assert_invalid("1985-W15"); // week date
    assert_invalid("P1Y2M"); // duration
    assert_invalid("R12/1985/1990"); // recurring interval
    assert_invalid("1985Y4M12D"); // explicit form
    assert_invalid("198"); // bare decade
    assert_invalid("19"); // bare century
    assert_invalid("2018-01-15/02-20"); // end-component elision
    assert_invalid("1985-4-12"); // single-digit month
    assert_invalid("1985-04-12T23:20"); // reduced-precision time
    assert_invalid("1985-04-12T24:00:00"); // hour 24
    assert_invalid("1985-04-12T23:20:30+15:00"); // shift beyond ±14:00
    assert_invalid("1985-04-12T23:20:30-00:00"); // negative zero shift (D4)
    assert_invalid("1985-04-12T23:20:30-00");
    assert_invalid("hello");
    assert_invalid("");
    assert_invalid("1985 -04"); // whitespace
    assert_invalid("..1984"); // bare '..' outside sets/intervals
}

#[test]
fn decisions() {
    assert_invalid("Y1985"); // D1: Y-form needs |year| > 9999
    assert_invalid("Y-1985");
    assert_invalid("Y12E2"); // D1: resolves to 1200
    assert_invalid("-0000"); // D2
    assert_invalid("../.."); // D6: no dated endpoint
    assert_invalid("/");
    assert_invalid("../");
    assert_invalid("/..");
    assert_invalid("1950S0"); // D7: precision bounds
    assert_invalid("1950S5");
    assert!(is_valid("1950S4"));
    assert_invalid("195XS2"); // S cannot combine with X
    assert_invalid("-12345"); // D13: >4-digit negative needs Y-form
    assert!(is_valid("-1985-04-12")); // D17: negative year with month/day
    assert_level("-1985-04-12", 1);
    assert_invalid("Y20000-04"); // Y-years are year-only

    // D20: no leading zeros in Y-year significands. Found by cargo-fuzz:
    // "Y08470847E1S9" used to parse with a 9-digit S-budget, then its
    // canonical zero-stripped form failed to reparse.
    assert_invalid("Y08470847E1S9");
    assert_invalid("Y-01694194E1S9");
    assert_invalid("Y018470");
    assert_invalid("Y012E3");
    assert!(is_valid("Y8470847E1S8"));
    assert_invalid("{}");
    assert_invalid("{1960,}");
    assert_invalid("{1960, 1961}"); // space
    assert_invalid("[..]");
    assert_invalid("1985-04-12/1985-04-12T10:00:00"); // datetime interval endpoint

    // D32: EDTF letters are upper case only.
    assert_invalid("198x");
    assert_invalid("y17e7");
    assert_invalid("1950s2");
    assert_invalid("1985-04-12t23:20:30z");
}

#[test]
fn model_accessors() {
    let d = match Edtf::parse("2004-06~-11").unwrap() {
        Edtf::Date(d) => d,
        other => panic!("expected date, got {other:?}"),
    };
    assert!(d.year().qualifier().approximate);
    assert!(d.month().unwrap().qualifier().approximate);
    assert!(!d.day().unwrap().qualifier().is_qualified());
    assert_eq!(d.precision(), Precision::Day);
    assert_eq!(d.year().value(), Some(2004));

    let season = match Edtf::parse("2001-21").unwrap() {
        Edtf::Date(d) => d,
        other => panic!("expected date, got {other:?}"),
    };
    assert_eq!(season.precision(), Precision::Season);

    let iv = match Edtf::parse("1985-04-12/..").unwrap() {
        Edtf::Interval(iv) => iv,
        other => panic!("expected interval, got {other:?}"),
    };
    assert!(matches!(iv.end(), IntervalEndpoint::Open));

    let set = match Edtf::parse("[1667,1760-12]").unwrap() {
        Edtf::Set(s) => s,
        other => panic!("expected set, got {other:?}"),
    };
    assert_eq!(set.kind(), SetKind::OneMember);
    assert_eq!(set.elements().len(), 2);
}

#[test]
fn error_offsets_point_at_the_problem() {
    fn offset_of(s: &str) -> usize {
        Edtf::parse(s).unwrap_err().offset
    }
    assert_eq!(offset_of("1985-13"), 5); // the bad month
    assert_eq!(offset_of("1985-02-30"), 8); // the bad day
    assert_eq!(offset_of("1985-04-12T25:00:00"), 11); // the bad hour
    assert_eq!(offset_of("1985-04-12T23:20:30+15:00"), 19); // the bad shift
    assert_eq!(offset_of("1985 -04"), 4); // the space
    assert_eq!(offset_of("2004/2003"), 5); // the out-of-order end
    assert_eq!(offset_of("{1960,1985-00}"), 11); // bad month inside a set
    assert_eq!(offset_of("../1985-13"), 8); // bad month after '..' prefix
}

#[test]
fn rejection_edges_carry_precise_errors() {
    fn message_of(s: &str) -> String {
        Edtf::parse(s).unwrap_err().to_string()
    }
    // One year range for every spelling (D30): an exponent too large is
    // the same error as the year it would produce.
    assert!(message_of("Y1E100001").contains("year out of supported range"));
    // Shift minutes are a separate field from shift magnitude (±14:00).
    assert!(message_of("1985-04-12T10:00:00+05:60").contains("shift minutes must be 00-59"));
    // A set range is a single '..' between two dates.
    assert!(message_of("[1985..1990..1995]").contains("multiple '..'"));
}

/// Decision D30: a year has at most 18 digits, whichever way it is spelled,
/// and every year that parses can be bounded, compared and ordered.
#[test]
fn years_have_one_supported_range() {
    fn message_of(s: &str) -> String {
        Edtf::parse(s).map_or_else(|e| e.message.to_owned(), |v| format!("valid: {v}"))
    }
    for s in [
        "Y999999999999999999",
        "Y-999999999999999999",
        "Y9E17",
        "Y99E16",
    ] {
        assert!(is_valid(s), "{s:?} is at or inside the limit");
    }
    for s in [
        "Y1000000000000000000",
        "Y1E18",
        "Y-1E18",
        "Y9223372036854775807",
        "Y1E19",
        "Y1E30",
        "Y1E100000",
        "Y10000000000000000000",
        "Y99999999999999999999",
        "Y1E100001",
    ] {
        assert_eq!(message_of(s), "year out of supported range", "{s:?}");
    }
    // The error sits at the year's first digit, after any sign.
    for (s, offset) in [
        ("Y10000000000000000000", 1),
        ("Y99999999999999999999", 1),
        ("Y1E100001", 1),
        ("Y-1E18", 2),
    ] {
        assert_eq!(Edtf::parse(s).unwrap_err().offset, offset, "{s:?}");
    }
    // A significant-digit sweep stays inside the limit because the year does.
    assert_invalid("Y9E18S1");
    assert_invalid("Y99E17S1");
    let b = Edtf::parse("Y9E17S1").unwrap().bounds();
    let (Bound::Date(lo), Bound::Date(hi)) = (b.earliest, b.latest) else {
        panic!("Y9E17S1 must have date bounds, got {b:?}");
    };
    assert_eq!(
        (lo.to_string(), hi.to_string()),
        (
            "900000000000000000-01-01".to_owned(),
            "999999999999999999-12-31".to_owned()
        )
    );
    // No interval or set can hide an uncomputable year any more.
    for s in [
        "Y1E30/1985",
        "Y2E30/Y1E30",
        "Y8E20202/Y9E18S1",
        "[Y1E19,1985]",
        "{Y1E19..Y1E19}",
    ] {
        assert_invalid(s);
    }
    assert_eq!(
        message_of("Y9E17/Y8E17"),
        "interval end precedes interval start"
    );
}

#[test]
fn component_flags_reach_into_sets() {
    // The `any_date` sweep must see every set element shape.
    assert!(Edtf::parse("[1985?]").unwrap().is_uncertain());
    assert!(Edtf::parse("[..1985~]").unwrap().is_approximate());
    assert!(Edtf::parse("[198X]").unwrap().has_unspecified());
    // Range endpoints reject qualifiers and masks at parse time, so the
    // sweep across a range can only ever come back clean.
    assert!(!Edtf::parse("[1985..1990]").unwrap().is_uncertain());
}

#[test]
fn edtf_implements_fromstr() {
    let v: Edtf = "1985-04".parse().unwrap();
    assert_eq!(v.to_string(), "1985-04");
    "1985-13".parse::<Edtf>().unwrap_err();
}

#[test]
fn component_flags_reach_into_interval_endpoints() {
    // Open/unknown endpoints carry no date: the sweep must fall through to
    // the other endpoint, and short-circuit when the first already matches.
    assert!(Edtf::parse("../1985?").unwrap().is_uncertain());
    assert!(Edtf::parse("1985?/..").unwrap().is_uncertain());
    assert!(!Edtf::parse("../1985").unwrap().is_uncertain());
}

#[test]
fn summary_spells_every_kind_precision_and_bound() {
    // Kinds, and the precision word only dates and datetimes carry.
    for (input, kind, precision) in [
        ("1985", "date", Some("year")),
        ("1985-04", "date", Some("month")),
        ("2001-21", "date", Some("season")),
        ("1985-04-12", "date", Some("day")),
        ("1985-04-12T10:00:00Z", "datetime", Some("day")),
        ("1985/1987", "interval", None),
        ("[1985,1987]", "set", None),
    ] {
        let s = Edtf::parse(input).unwrap().summary();
        assert_eq!(s.kind, kind, "{input}");
        assert_eq!(s.precision, precision, "{input}");
    }
    // The four bound cases: a day, either infinity, and unknown (None).
    let s = Edtf::parse("../1985-04").unwrap().summary();
    assert_eq!(s.earliest.as_deref(), Some("-infinity"));
    assert_eq!(s.latest.as_deref(), Some("1985-04-30"));
    let s = Edtf::parse("1985-04-12/..").unwrap().summary();
    assert_eq!(s.earliest.as_deref(), Some("1985-04-12"));
    assert_eq!(s.latest.as_deref(), Some("infinity"));
    let s = Edtf::parse("1985/").unwrap().summary();
    assert_eq!(s.latest, None);
    // Canonical form, level and the three flags ride along.
    let s = Edtf::parse("?2004-?06-?11").unwrap().summary();
    assert_eq!(s.canonical, "2004-06-11?");
    assert_eq!(s.level, 1);
    assert!(s.uncertain && !s.approximate && !s.unspecified);
    let s = Edtf::parse("156X~").unwrap().summary();
    assert!(!s.uncertain && s.approximate && s.unspecified);
}

/// Under the `serde` feature a value is its canonical EDTF text.
///
/// It serialises as the string and deserialises by parsing it, so JSON can
/// only build what the parser accepts (decision D31). The object is the
/// field-by-field shape the model once had, holding month 16: it is
/// refused, not deserialised into a value whose bounds would panic.
#[cfg(feature = "serde")]
#[test]
fn serde_is_the_canonical_string() {
    let value: Edtf = serde_json::from_str("\"2004-06-11%\"").unwrap();
    assert_eq!(value, Edtf::parse("2004-06-11%").unwrap());
    assert_eq!(serde_json::to_string(&value).unwrap(), "\"2004-06-11%\"");
    let messy: Edtf = serde_json::from_str("\"?2004-?06-?11\"").unwrap();
    assert_eq!(serde_json::to_string(&messy).unwrap(), "\"2004-06-11?\"");

    let refused = serde_json::from_str::<Edtf>("\"2004-16-11%\"").unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("month must be 01-12 or a sub-year code 21-41"),
        "{refused}"
    );
    let object = r#"{"Date":{"year":{"kind":{"Standard":{"negative":false,"digits":[2,0,0,4]}},"significant_digits":null,"qualifier":{"uncertain":true,"approximate":true}},"month":{"digits":[1,6],"qualifier":{"uncertain":true,"approximate":true}},"day":{"digits":[1,1],"qualifier":{"uncertain":true,"approximate":true}}}}"#;
    assert_eq!(serde_json::from_str::<Edtf>(object).ok(), None);
}

/// Assert each result is refused with exactly its rule's message.
fn assert_rules(cases: Vec<(Result<(), ValueError>, &str)>) {
    for (result, message) in cases {
        assert_eq!(
            result.map_err(ValueError::message),
            Err(message),
            "expected the rule {message:?}"
        );
    }
}

fn year(value: i64) -> Year {
    Year::from_value(value).unwrap()
}

fn field(value: u8) -> DateField {
    DateField::from_value(value).unwrap()
}

fn date(y: i64, m: Option<u8>, d: Option<u8>) -> Date {
    Date::new(year(y), m.map(field), d.map(field)).unwrap()
}

/// A four-digit year of these digits.
///
/// # Errors
///
/// The rule the digits break, which is what the tests below ask for.
fn standard(negative: bool, digits: [Option<u8>; 4]) -> Result<Year, ValueError> {
    Year::new(YearKind::Standard { negative, digits })
}

/// Every rule a year's constructors check, named in the error: the same
/// text the parser reports for input that breaks the same rule (D31).
#[test]
fn year_constructors_name_the_rule_their_arguments_break() {
    let exponential = |significand, exponent| {
        Year::new(YearKind::Exponential {
            significand,
            exponent,
        })
        .map(drop)
    };
    let masked = standard(false, [Some(1), Some(9), None, None]);
    assert_rules(vec![
        (
            standard(false, [Some(10), Some(0), Some(0), Some(0)]).map(drop),
            "year digits must be 0-9",
        ),
        (
            standard(true, [Some(1), None, Some(0), Some(0)]).map(drop),
            "negative years cannot contain unspecified digits",
        ),
        (
            standard(true, [Some(0); 4]).map(drop),
            "-0000 is not a valid year",
        ),
        (
            Year::new(YearKind::Big { value: i64::MAX }).map(drop),
            "year out of supported range",
        ),
        (
            Year::new(YearKind::Big { value: -9999 }).map(drop),
            "Y-prefixed years require |year| > 9999",
        ),
        (
            exponential(0, 5),
            "exponential year significand cannot be zero",
        ),
        (exponential(2, 64), "year out of supported range"),
        (exponential(1, 18), "year out of supported range"),
        (exponential(9, 3), "Y-prefixed years require |year| > 9999"),
        (
            Year::from_value(i64::MIN).map(drop),
            "year out of supported range",
        ),
        (
            masked.and_then(|y| y.with_significant_digits(2)).map(drop),
            "significant digits cannot combine with unspecified digits",
        ),
        (
            year(1985).with_significant_digits(5).map(drop),
            "significant digits must be 1-4 for a four-digit year",
        ),
        (
            year(1985).with_significant_digits(0).map(drop),
            "significant digits must be 1-4 for a four-digit year",
        ),
        (
            year(12_345).with_significant_digits(6).map(drop),
            "significant digits exceed the year's digit count",
        ),
    ]);
    assert_eq!(exponential(17, 7), Ok(()));
}

/// Every rule a date's constructors check, named in the error (D31).
#[test]
fn date_constructors_name_the_rule_their_arguments_break() {
    let sig = year(1985).with_significant_digits(2).unwrap();
    let masked_month = DateField::new([Some(2), None]).unwrap();
    assert_rules(vec![
        (
            DateField::new([Some(0), Some(10)]).map(drop),
            "date field digits must be 0-9",
        ),
        (
            DateField::from_value(100).map(drop),
            "date field digits must be 0-9",
        ),
        (
            Date::new(year(20_000), Some(field(1)), None).map(drop),
            "Y-prefixed years are year-precision only",
        ),
        (
            Date::new(sig, Some(field(1)), None).map(drop),
            "significant-digit years are year-precision only",
        ),
        (
            Date::new(year(1985), None, Some(field(1))).map(drop),
            "a day requires a month",
        ),
        (
            Date::new(year(1985), Some(field(13)), None).map(drop),
            "month must be 01-12 or a sub-year code 21-41",
        ),
        (
            Date::new(year(1985), Some(field(0)), None).map(drop),
            "month must be 01-12 or a sub-year code 21-41",
        ),
        (
            Date::new(year(1985), Some(masked_month), None).map(drop),
            "no calendar month matches the unspecified digits",
        ),
        (
            Date::new(year(1985), Some(field(21)), Some(field(1))).map(drop),
            "sub-year groupings cannot carry a day",
        ),
        (
            Date::new(year(1985), Some(field(2)), Some(field(29))).map(drop),
            "day is out of range for the month",
        ),
        (
            Date::new(year(1985), Some(field(4)), Some(field(0))).map(drop),
            "day is out of range for the month",
        ),
    ]);
    // The rules refuse only what they name: a leap day in a leap year builds.
    assert_eq!(
        Date::new(year(2000), Some(field(2)), Some(field(29))).map(drop),
        Ok(())
    );
}

/// Every rule a time's and a date-time's constructors check (D31).
#[test]
fn time_constructors_name_the_rule_their_arguments_break() {
    let midnight = Time::new(0, 0, 0, None).unwrap();
    let datetime_date = "a date-time's date must be a complete, unqualified YYYY-MM-DD date";
    assert_rules(vec![
        (
            Time::new(24, 0, 0, None).map(drop),
            "hour must be 00-23 (24 is not allowed)",
        ),
        (Time::new(0, 60, 0, None).map(drop), "minute must be 00-59"),
        (Time::new(0, 0, 61, None).map(drop), "second must be 00-60"),
        (
            TimeShift::offset(-841, false).map(drop),
            "time shift exceeds \u{b1}14:00",
        ),
        (
            TimeShift::offset(90, true).map(drop),
            "an hours-only time shift has no minutes",
        ),
        (
            DateTime::new(date(1985, Some(4), None), midnight).map(drop),
            datetime_date,
        ),
        (
            DateTime::new(date(-1985, Some(4), Some(12)), midnight).map(drop),
            datetime_date,
        ),
    ]);
    assert_eq!(TimeShift::offset(-840, true).map(drop), Ok(()));
}

/// Every rule an interval's and a set's constructors check (D31).
#[test]
fn interval_and_set_constructors_name_the_rule_their_arguments_break() {
    let y = |v| date(v, None, None);
    let masked = Date::new(
        standard(false, [Some(1), Some(9), Some(8), None]).unwrap(),
        None,
        None,
    );
    let uncertain = Qualifier {
        uncertain: true,
        approximate: false,
    };
    let sig = Date::new(year(1985).with_significant_digits(2).unwrap(), None, None);
    assert_rules(vec![
        (
            Interval::new(IntervalEndpoint::OnOrAfter(y(1985)), IntervalEndpoint::Open).map(drop),
            "'..'-suffixed date is only allowed as interval end",
        ),
        (
            Interval::new(
                IntervalEndpoint::Open,
                IntervalEndpoint::OnOrBefore(y(1985)),
            )
            .map(drop),
            "'..'-prefixed date is only allowed as interval start",
        ),
        (
            Interval::new(IntervalEndpoint::Open, IntervalEndpoint::Unknown).map(drop),
            "interval needs at least one dated endpoint",
        ),
        (
            Interval::new(
                IntervalEndpoint::Date(y(1986)),
                IntervalEndpoint::Date(y(1985)),
            )
            .map(drop),
            "interval end precedes interval start",
        ),
        (
            Set::new(SetKind::OneMember, Vec::new()).map(drop),
            "set must contain at least one element",
        ),
        (
            masked.and_then(|a| SetRange::new(a, y(1990))).map(drop),
            "set range endpoints cannot contain unspecified digits",
        ),
        (
            SetRange::new(y(1985), y(1990).qualified(uncertain)).map(drop),
            "set range endpoints cannot be qualified",
        ),
        (
            SetRange::new(date(1985, Some(21), None), date(1990, Some(21), None)).map(drop),
            "set range endpoints cannot be seasons",
        ),
        (
            sig.and_then(|a| SetRange::new(a, y(1990))).map(drop),
            "set range endpoints cannot carry significant digits",
        ),
        (
            SetRange::new(y(1985), date(1990, Some(1), None)).map(drop),
            "set range endpoints must share precision",
        ),
        (
            SetRange::new(y(1990), y(1985)).map(drop),
            "set range end precedes range start",
        ),
    ]);
}
