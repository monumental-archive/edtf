// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The proptest engine: generate random *valid* EDTF values as structured
//! data and hold every one of them to the rules in `properties`.
//!
//! This is the complementary bug class to the string-side engines (the
//! deterministic `harness` and the cargo-fuzz targets): values the parser
//! may never produce but the type system can represent, and rule
//! violations on shapes random strings rarely reach.
//!
//! Generators are constructive rather than filtered: masks are applied to an
//! already-valid concrete date (so at least that completion stays valid,
//! decision D11), ranges are compare-and-swapped into order using the same
//! bounds comparison the parser applies (decision D18), and time shifts are
//! only generated in shapes `Display` can spell.

#![expect(
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "generator and oracle ranges are bounded by construction"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic is over literal fixtures and generator-bounded values; an overflow here would fail the test, which is the signal"
)]
#![expect(
    clippy::integer_division_remainder_used,
    reason = "same integer calendar arithmetic as the code under test"
)]
#![expect(
    clippy::as_conversions,
    reason = "casting fixture values the test itself bounded"
)]
#![expect(
    clippy::non_ascii_literal,
    reason = "proptest! expands to code carrying the generated corpus, which includes the Cyrillic month and century forms this crate normalises"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching through a reference without restating & at every level"
)]
#![expect(
    clippy::shadow_reuse,
    reason = "rebinding a value through the transformation under test"
)]
#![expect(
    clippy::absolute_paths,
    reason = "a one-use std path written in full at the call site"
)]
#![expect(
    clippy::integer_division,
    reason = "same integer calendar arithmetic as the code under test"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "indexing a fixture the test itself constructed; an out-of-range index is a failing test, not a crash path"
)]
#![expect(
    clippy::unreachable,
    reason = "an unreachable! naming the invariant the fixture guarantees"
)]

use alloc::{string::ToString as _, vec, vec::Vec};

use proptest::prelude::*;

use super::{
    check_bound_day, check_letters, check_order, check_pair, check_qualification, check_value,
    check_whitespace, observe_bound_day, observe_letters, observe_order, observe_pair,
    observe_qualification, observe_value, observe_whitespace,
};
use crate::{
    Bound, BoundDate, Date, DateField, DateTime, Edtf, Interval, IntervalEndpoint, Qualifier, Set,
    SetElement, SetKind, SetRange, Time, TimeShift, Year, YearKind,
};

// ------------------------------------------------------------ calendar

const fn is_leap(y: i64) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

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
        _ => unreachable!("month is 1-12"),
    }
}

const fn year_digits(y: u16) -> [Option<u8>; 4] {
    [
        Some((y / 1000 % 10) as u8),
        Some((y / 100 % 10) as u8),
        Some((y / 10 % 10) as u8),
        Some((y % 10) as u8),
    ]
}

const fn field_digits(v: u8) -> [Option<u8>; 2] {
    [Some(v / 10), Some(v % 10)]
}

/// Turn digits into `X`s where `bits` says so.
fn mask<const N: usize>(mut digits: [Option<u8>; N], bits: [bool; N]) -> [Option<u8>; N] {
    for (d, m) in digits.iter_mut().zip(bits) {
        if m {
            *d = None;
        }
    }
    digits
}

/// A year the generator knows is valid, with its significant digits and
/// qualifier.
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn year_of(kind: YearKind, significant_digits: Option<u32>, qualifier: Qualifier) -> Year {
    let year = Year::new(kind).expect("the generator builds valid years");
    significant_digits
        .map_or(Ok(year), |count| year.with_significant_digits(count))
        .expect("the generator builds valid significant digits")
        .with_qualifier(qualifier)
}

/// A month or day field the generator knows is valid, qualified.
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn field_of(digits: [Option<u8>; 2], qualifier: Qualifier) -> DateField {
    DateField::new(digits)
        .expect("the generator builds valid digits")
        .with_qualifier(qualifier)
}

/// A date the generator knows is valid.
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn date_of(year: Year, month: Option<DateField>, day: Option<DateField>) -> Date {
    Date::new(year, month, day).expect("the generator builds valid dates")
}

/// The parser's interval/range ordering rule (decision D18): out of order
/// only when `a`'s earliest possible day is later than `b`'s latest.
fn dates_out_of_order(a: &Date, b: &Date) -> bool {
    let lo = Edtf::Date(*a).bounds().earliest;
    let hi = Edtf::Date(*b).bounds().latest;
    match (lo, hi) {
        (Bound::Date(lo), Bound::Date(hi)) => lo > hi,
        _ => false,
    }
}

// ------------------------------------------------------------ strategies

fn qualifier() -> impl Strategy<Value = Qualifier> {
    (any::<bool>(), any::<bool>()).prop_map(|(uncertain, approximate)| Qualifier {
        uncertain,
        approximate,
    })
}

/// Four-digit year, possibly negative (then unmasked, and never -0000) or
/// masked (then non-negative) — the parser's standard-year rules.
fn standard_year() -> impl Strategy<Value = Year> {
    (any::<bool>(), 0_u16..=9999, any::<[bool; 4]>(), qualifier()).prop_map(
        |(negative, y, bits, qualifier)| {
            let negative = negative && y != 0;
            let digits = if negative {
                year_digits(y)
            } else {
                mask(year_digits(y), bits)
            };
            year_of(YearKind::Standard { negative, digits }, None, qualifier)
        },
    )
}

/// Standard year with an `S` suffix: unmasked, 1-4 significant digits.
fn sig_standard_year() -> impl Strategy<Value = Year> {
    (any::<bool>(), 0_u16..=9999, 1_u32..=4, qualifier()).prop_map(|(negative, y, s, qualifier)| {
        year_of(
            YearKind::Standard {
                negative: negative && y != 0,
                digits: year_digits(y),
            },
            Some(s),
            qualifier,
        )
    })
}

/// `Y`-prefixed year: |value| > 9999, significant digits within the digit
/// count.
fn big_year() -> impl Strategy<Value = Year> {
    (
        10_000_i64..=9_999_999_999_999,
        any::<bool>(),
        proptest::option::of(1_u32..=13),
        qualifier(),
    )
        .prop_map(|(mag, negative, sig, qualifier)| {
            let digit_count = mag.ilog10() + 1;
            year_of(
                YearKind::Big {
                    value: if negative { -mag } else { mag },
                },
                sig.map(|s| s.min(digit_count)),
                qualifier,
            )
        })
}

/// `Y…E…` year: non-zero significand, |value| > 9999, significant digits
/// within significand digits + exponent.
fn exp_year() -> impl Strategy<Value = Year> {
    (
        1_i64..=999_999,
        any::<bool>(),
        0_u32..=12,
        proptest::option::of(1_u32..=18),
        qualifier(),
    )
        .prop_filter("|value| must exceed 9999", |(mag, _, exp, ..)| {
            i128::from(*mag) * 10_i128.pow(*exp) > 9999
        })
        .prop_map(|(mag, negative, exponent, sig, qualifier)| {
            let digit_count = mag.ilog10() + 1 + exponent;
            year_of(
                YearKind::Exponential {
                    significand: if negative { -mag } else { mag },
                    exponent,
                },
                sig.map(|s| s.min(digit_count)),
                qualifier,
            )
        })
}

/// A concrete, valid calendar (year, month, day) triple.
fn ymd() -> impl Strategy<Value = (u16, u8, u8)> {
    (0_u16..=9999, 1_u8..=12).prop_flat_map(|(y, m)| {
        let hi = last_day(m, is_leap(i64::from(y)));
        (Just(y), Just(m), 1_u8..=hi)
    })
}

fn year_only_date() -> impl Strategy<Value = Date> {
    prop_oneof![
        4 => standard_year(),
        1 => sig_standard_year(),
        1 => big_year(),
        1 => exp_year(),
    ]
    .prop_map(|year| date_of(year, None, None))
}

/// Standard year plus an explicit sub-year grouping code 21-41 (no day).
fn season_date() -> impl Strategy<Value = Date> {
    (standard_year(), 21_u8..=41, qualifier())
        .prop_map(|(year, code, q)| date_of(year, Some(field_of(field_digits(code), q)), None))
}

/// Year-month or year-month-day, starting from a concrete valid date and
/// masking digits — the concrete completion keeps every mask valid (D11).
fn month_day_date() -> impl Strategy<Value = Date> {
    (
        ymd(),
        any::<bool>(),
        any::<bool>(),
        any::<[bool; 4]>(),
        any::<[bool; 2]>(),
        any::<[bool; 2]>(),
        (qualifier(), qualifier(), qualifier()),
    )
        .prop_map(
            |((y, m, d), negative, with_day, ybits, mbits, dbits, (yq, mq, dq))| {
                let negative = negative && y != 0;
                let digits = if negative {
                    year_digits(y)
                } else {
                    mask(year_digits(y), ybits)
                };
                date_of(
                    year_of(YearKind::Standard { negative, digits }, None, yq),
                    Some(field_of(mask(field_digits(m), mbits), mq)),
                    with_day.then(|| field_of(mask(field_digits(d), dbits), dq)),
                )
            },
        )
}

fn date() -> impl Strategy<Value = Date> {
    prop_oneof![
        2 => year_only_date(),
        1 => season_date(),
        4 => month_day_date(),
    ]
}

/// Time shifts only in the shapes `Display` can spell: `hours_only`
/// requires a whole-hour offset.
fn shift() -> impl Strategy<Value = Option<TimeShift>> {
    prop_oneof![
        1 => Just(None),
        1 => Just(Some(TimeShift::UTC)),
        1 => (-14_i16..=14).prop_map(|h| TimeShift::offset(h * 60, true).ok()),
        1 => (-840_i16..=840).prop_map(|minutes| TimeShift::offset(minutes, false).ok()),
    ]
}

/// Date-times exist only at level 0: plain, complete, unqualified date.
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn datetime() -> impl Strategy<Value = DateTime> {
    (ymd(), (0_u8..=23, 0_u8..=59, 0_u8..=60, shift())).prop_map(
        |((y, m, d), (hour, minute, second, shift))| {
            let plain = Qualifier::default();
            let date = date_of(
                year_of(
                    YearKind::Standard {
                        negative: false,
                        digits: year_digits(y),
                    },
                    None,
                    plain,
                ),
                Some(field_of(field_digits(m), plain)),
                Some(field_of(field_digits(d), plain)),
            );
            let time = Time::new(hour, minute, second, shift).expect("a valid time");
            DateTime::new(date, time).expect("a plain complete date")
        },
    )
}

/// A generated value the constructors accept.
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn interval() -> impl Strategy<Value = Interval> {
    let start = prop_oneof![
        5 => date().prop_map(IntervalEndpoint::Date),
        1 => Just(IntervalEndpoint::Open),
        1 => Just(IntervalEndpoint::Unknown),
        2 => date().prop_map(IntervalEndpoint::OnOrBefore),
    ];
    let end = prop_oneof![
        5 => date().prop_map(IntervalEndpoint::Date),
        1 => Just(IntervalEndpoint::Open),
        1 => Just(IntervalEndpoint::Unknown),
        2 => date().prop_map(IntervalEndpoint::OnOrAfter),
    ];
    (start, end)
        .prop_filter("interval needs a dated endpoint", |(s, e)| {
            s.is_dated() || e.is_dated()
        })
        .prop_map(|(start, end)| {
            // Comparable concrete endpoints get compare-and-swapped into
            // order; a swap is always sufficient because a single date's
            // bounds are themselves ordered.
            if let (IntervalEndpoint::Date(a), IntervalEndpoint::Date(b)) = (&start, &end) {
                if dates_out_of_order(a, b) {
                    return Interval::new(IntervalEndpoint::Date(*b), IntervalEndpoint::Date(*a))
                        .expect("swapped into order");
                }
            }
            Interval::new(start, end).expect("a dated, ordered interval")
        })
}

/// A concrete, unqualified, unmasked date at the given precision
/// (0 = year, 1 = month, 2 = day) — the only shape D27 admits as a `..`
/// range endpoint. Years may be negative (never -0000).
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn plain_date(y: i64, m: u8, d: u8, precision: u8) -> Date {
    let plain = Qualifier::default();
    date_of(
        Year::from_value(y).expect("a four-digit year"),
        (precision >= 1).then(|| field_of(field_digits(m), plain)),
        (precision >= 2).then(|| field_of(field_digits(d), plain)),
    )
}

/// D27 range elements: both endpoints concrete, unqualified, non-season,
/// same-precision, compare-and-swapped into order at that precision.
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn range_element() -> impl Strategy<Value = SetElement> {
    (ymd(), ymd(), any::<[bool; 2]>(), 0_u8..=2).prop_map(
        |((y1, m1, d1), (y2, m2, d2), neg, precision)| {
            let sy1 = if neg[0] && y1 != 0 {
                -i64::from(y1)
            } else {
                i64::from(y1)
            };
            let sy2 = if neg[1] && y2 != 0 {
                -i64::from(y2)
            } else {
                i64::from(y2)
            };
            let key = |y: i64, m: u8, d: u8| match precision {
                0 => (y, 0, 0),
                1 => (y, m, 0),
                _ => (y, m, d),
            };
            let (a, b) = if key(sy1, m1, d1) <= key(sy2, m2, d2) {
                ((sy1, m1, d1), (sy2, m2, d2))
            } else {
                ((sy2, m2, d2), (sy1, m1, d1))
            };
            SetElement::Range(
                SetRange::new(
                    plain_date(a.0, a.1, a.2, precision),
                    plain_date(b.0, b.1, b.2, precision),
                )
                .expect("a D27 range in order"),
            )
        },
    )
}

fn set_element() -> impl Strategy<Value = SetElement> {
    prop_oneof![
        5 => date().prop_map(SetElement::Date),
        1 => date().prop_map(SetElement::OnOrBefore),
        1 => date().prop_map(SetElement::OnOrAfter),
        3 => range_element(),
    ]
}

/// A generated value the constructors accept.
///
/// # Panics
///
/// When the generator, not the code under test, is wrong: it only asks
/// for values the constructors accept.
fn set() -> impl Strategy<Value = Set> {
    (
        any::<bool>(),
        proptest::collection::vec(set_element(), 1..=4),
    )
        .prop_map(|(all, elements)| {
            let kind = if all {
                SetKind::AllMembers
            } else {
                SetKind::OneMember
            };
            Set::new(kind, elements).expect("a set with members")
        })
}

fn edtf() -> impl Strategy<Value = Edtf> {
    prop_oneof![
        5 => date().prop_map(Edtf::Date),
        1 => datetime().prop_map(Edtf::DateTime),
        2 => interval().prop_map(Edtf::Interval),
        2 => set().prop_map(Edtf::Set),
    ]
}

// ------------------------------------------------------------ properties

/// How much of a (possibly astronomically large) value stream a property
/// consumes per case.
const VALUES_CAP: usize = 600;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Every generated value keeps the value rule: canonical form, level,
    /// qualification, bounds, enumeration and self-relation.
    #[test]
    fn every_value_keeps_the_value_rule(v in edtf()) {
        check_value(&observe_value(&v, VALUES_CAP));
    }

    /// Every pair of generated values relates soundly.
    #[test]
    fn every_pair_relates_soundly(a in edtf(), b in edtf()) {
        check_pair(&observe_pair(&a, &b));
    }

    /// Order, relation and interval agree (D18, D23).
    #[test]
    fn order_relation_and_interval_agree(a in date(), b in date()) {
        check_order(&observe_order(a, b));
    }

    /// Qualification never moves bounds (D23 a).
    #[test]
    fn qualification_never_moves_bounds(d in date()) {
        check_qualification(&observe_qualification(d));
    }

    /// EDTF letters are upper case only (D32).
    #[test]
    fn letters_are_upper_case_only(v in edtf()) {
        check_letters(&observe_letters(&v.to_string()));
    }
}

proptest! {
    // Each case parses four variants per character of the canonical form.
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// No whitespace anywhere (spec-notes §7 rule 11).
    #[test]
    fn whitespace_is_never_accepted(v in edtf()) {
        check_whitespace(&observe_whitespace(&v.to_string()));
    }
}

// ----------------------------------------------------- values() oracles

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A set of concrete singleton elements enumerates exactly those
    /// elements, verbatim, in written order (D29).
    #[test]
    fn set_of_singletons_yields_elements_in_order(
        parts in proptest::collection::vec((ymd(), 0_u8..=2), 1..=4),
        all in any::<bool>(),
    ) {
        let dates: Vec<Date> = parts
            .iter()
            .map(|((y, m, d), p)| plain_date(i64::from(*y), *m, *d, *p))
            .collect();
        let kind = if all { SetKind::AllMembers } else { SetKind::OneMember };
        let set = Edtf::Set(
            Set::new(kind, dates.iter().copied().map(SetElement::Date).collect())
                .expect("a set with members"),
        );
        let got: Vec<Edtf> = set.values().expect("a set of dates enumerates").collect();
        let want: Vec<Edtf> = dates.into_iter().map(Edtf::Date).collect();
        prop_assert_eq!(got, want, "{}", set);
    }

    /// Mask completions equal brute-force enumeration of the whole digit
    /// domain (restricted to <= 2 masked year digits to keep the oracle
    /// small), including qualifier copy-through (D28).
    #[test]
    fn masked_date_values_equal_brute_force(d in month_day_date()) {
        let YearKind::Standard { negative: _, digits } = d.year().kind() else {
            unreachable!("month_day_date generates standard years")
        };
        prop_assume!(digits.iter().filter(|x| x.is_none()).count() <= 2);
        let field_admits = |f: &DateField, v: u8| {
            let [tens, ones] = f.digits();
            tens.is_none_or(|p| p == v / 10) && ones.is_none_or(|p| p == v % 10)
        };
        let years: Vec<i64> = d.year().value().map_or_else(
            || {
                (0_i64..=9999)
                    .filter(|y| {
                        let ds = year_digits(*y as u16);
                        (0..4).all(|i| digits[i].is_none() || digits[i] == ds[i])
                    })
                    .collect()
            },
            |v| vec![v],
        );
        let month = d.month().expect("month_day_date always has a month");
        let yq = d.year().qualifier();
        let mut want: Vec<Edtf> = Vec::new();
        for y in years {
            let year = Year::from_value(y).expect("a four-digit year").with_qualifier(yq);
            for m in (1..=12_u8).filter(|m| field_admits(&month, *m)) {
                let mf = Some(field_of(field_digits(m), month.qualifier()));
                match d.day() {
                    None => want.push(Edtf::Date(date_of(year, mf, None))),
                    Some(day) => {
                        for dd in (1..=31_u8).filter(|x| field_admits(&day, *x)) {
                            if dd > last_day(m, is_leap(y)) {
                                continue;
                            }
                            let df = Some(field_of(field_digits(dd), day.qualifier()));
                            want.push(Edtf::Date(date_of(year, mf, df)));
                        }
                    }
                }
            }
        }
        let got: Vec<Edtf> = Edtf::Date(d).values().expect("a date enumerates").collect();
        prop_assert_eq!(got, want, "input {}", Edtf::Date(d));
    }
}

// ------------------------------------------- constructors (decision D31)

/// A digit argument: usually 0-9 or an `X`, sometimes any byte at all.
fn digit_arg() -> impl Strategy<Value = Option<u8>> {
    proptest::option::weighted(0.85, prop_oneof![9 => 0_u8..=9, 1 => any::<u8>()])
}

/// A year's arguments: any form, any value, any significant-digit count,
/// any qualifier, through either constructor.
fn year_args() -> BoxedStrategy<(Result<Year, ()>, Option<u32>, Qualifier)> {
    let kind = prop_oneof![
        3 => (any::<bool>(), [digit_arg(), digit_arg(), digit_arg(), digit_arg()])
            .prop_map(|(negative, digits)| YearKind::Standard { negative, digits }),
        1 => prop_oneof![any::<i64>(), -10_000_000_i64..=10_000_000]
            .prop_map(|value| YearKind::Big { value }),
        1 => (prop_oneof![any::<i64>(), -999_i64..=999], prop_oneof![any::<u32>(), 0_u32..=20])
            .prop_map(|(significand, exponent)| YearKind::Exponential {
                significand,
                exponent,
            }),
    ];
    let value = prop_oneof![any::<i64>(), -20_000_i64..=20_000];
    (
        prop_oneof![
            3 => kind.prop_map(|k| Year::new(k).map_err(drop)),
            1 => value.prop_map(|v| Year::from_value(v).map_err(drop)),
        ],
        proptest::option::weighted(0.2, prop_oneof![0_u32..=20, any::<u32>()]),
        qualifier(),
    )
        .boxed()
}

/// A month or day field's arguments, through either constructor.
fn field_args() -> BoxedStrategy<Result<DateField, ()>> {
    prop_oneof![
        [digit_arg(), digit_arg()].prop_map(|d| DateField::new(d).map_err(drop)),
        prop_oneof![0_u8..=42, any::<u8>()].prop_map(|v| DateField::from_value(v).map_err(drop)),
    ]
    .boxed()
}

/// A date's arguments: a year and, often, a month and a day.
type DateArgs = (
    (Result<Year, ()>, Option<u32>, Qualifier),
    Option<(Result<DateField, ()>, Qualifier)>,
    Option<(Result<DateField, ()>, Qualifier)>,
);

fn date_args() -> BoxedStrategy<DateArgs> {
    (
        year_args(),
        proptest::option::weighted(0.6, (field_args(), qualifier())),
        proptest::option::weighted(0.5, (field_args(), qualifier())),
    )
        .boxed()
}

/// Every constructor of the date, called on its arguments; `None` where one
/// refuses them.
fn build_date(((year, sig, yq), month, day): DateArgs) -> Option<Date> {
    let mut year = year.ok()?;
    if let Some(count) = sig {
        year = year.with_significant_digits(count).ok()?;
    }
    let field = |f: Option<(Result<DateField, ()>, Qualifier)>| match f {
        None => Ok(None),
        Some((f, q)) => f.map(|f| Some(f.with_qualifier(q))),
    };
    Date::new(
        year.with_qualifier(yq),
        field(month).ok()?,
        field(day).ok()?,
    )
    .ok()
}

/// A plain complete date's arguments, for a date-time: any year value, any
/// month and day byte, rarely a qualifier.
fn plain_date_args() -> impl Strategy<Value = (i64, u8, u8, Qualifier)> {
    (
        prop_oneof![9 => -10_i64..=10_000, 1 => any::<i64>()],
        prop_oneof![9 => 0_u8..=13, 1 => any::<u8>()],
        prop_oneof![9 => 0_u8..=32, 1 => any::<u8>()],
        prop_oneof![9 => Just(Qualifier::default()), 1 => qualifier()],
    )
}

/// A time's arguments: any bytes, and any shift.
fn time_args() -> impl Strategy<Value = (u8, u8, u8, u8, i16, bool)> {
    (
        prop_oneof![0_u8..=25, any::<u8>()],
        prop_oneof![0_u8..=61, any::<u8>()],
        prop_oneof![0_u8..=61, any::<u8>()],
        0_u8..=2,
        prop_oneof![-900_i16..=900, any::<i16>()],
        any::<bool>(),
    )
}

/// Every value the constructors build from these arguments.
fn build_values(
    dates: &[DateArgs; 4],
    (year, month, day, qualifier): (i64, u8, u8, Qualifier),
    (hour, minute, second, shift_form, minutes, hours_only): (u8, u8, u8, u8, i16, bool),
    (start_form, end_form, all, members): (u8, u8, bool, [bool; 4]),
) -> Vec<Edtf> {
    let [d0, d1, d2, d3] = dates.map(build_date);
    let mut out: Vec<Edtf> = [d0, d1, d2, d3]
        .iter()
        .flatten()
        .copied()
        .map(Edtf::Date)
        .collect();

    let plain = Year::from_value(year).ok().and_then(|y| {
        let field = |v: u8| {
            DateField::from_value(v)
                .map(|f| f.with_qualifier(qualifier))
                .ok()
        };
        Date::new(y.with_qualifier(qualifier), field(month), field(day)).ok()
    });
    let shift = match shift_form {
        0 => Ok(None),
        1 => Ok(Some(TimeShift::UTC)),
        _ => TimeShift::offset(minutes, hours_only).map(Some),
    };
    if let (Some(date), Ok(shift)) = (plain, shift) {
        if let Ok(dt) = Time::new(hour, minute, second, shift).and_then(|t| DateTime::new(date, t))
        {
            out.push(Edtf::DateTime(dt));
        }
    }

    let endpoint = |form: u8, date: Option<Date>| match (form, date) {
        (0, Some(x)) => IntervalEndpoint::Date(x),
        (1, Some(x)) => IntervalEndpoint::OnOrBefore(x),
        (2, Some(x)) => IntervalEndpoint::OnOrAfter(x),
        (3, _) => IntervalEndpoint::Unknown,
        _ => IntervalEndpoint::Open,
    };
    if let Ok(iv) = Interval::new(endpoint(start_form, d0), endpoint(end_form, d1)) {
        out.push(Edtf::Interval(iv));
    }

    let range = d2.zip(d3).and_then(|(a, b)| SetRange::new(a, b).ok());
    let candidates = [
        d0.map(SetElement::Date),
        d1.map(SetElement::OnOrBefore),
        d2.map(SetElement::OnOrAfter),
        range.map(SetElement::Range),
    ];
    let elements: Vec<SetElement> = candidates
        .into_iter()
        .zip(members)
        .filter_map(|(e, keep)| e.filter(|_| keep))
        .collect();
    let kind = if all {
        SetKind::AllMembers
    } else {
        SetKind::OneMember
    };
    if let Ok(set) = Set::new(kind, elements) {
        out.push(Edtf::Set(set));
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// A value that exists is valid (decision D31).
    ///
    /// Every constructor is called on arbitrary arguments — any `i64`, any
    /// `u8`, any digit mask — and returns; every value it builds keeps the
    /// value rule and relates soundly to a fixed one. A bound day the
    /// constructor accepts is a real day.
    #[test]
    fn constructors_build_only_valid_values(
        dates in [date_args(), date_args(), date_args(), date_args()],
        plain in plain_date_args(),
        time in time_args(),
        shape in (0_u8..=4, 0_u8..=4, any::<bool>(), any::<[bool; 4]>()),
        day in (prop_oneof![any::<i64>(), -10_000_i64..=10_000], any::<u8>(), any::<u8>()),
    ) {
        let fixed = Edtf::parse("1985-04").expect("a fixture");
        for value in build_values(&dates, plain, time, shape) {
            check_value(&observe_value(&value, 3));
            check_pair(&observe_pair(&value, &fixed));
        }
        let (year, month, day_of_month) = day;
        if let Ok(bd) = BoundDate::new(year, month, day_of_month) {
            check_bound_day(&observe_bound_day(bd));
        }
    }
}
