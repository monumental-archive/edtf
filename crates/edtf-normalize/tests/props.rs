// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Property tests: the "valid by construction" guarantee from issue #20.
//!
//! Generators are constructive (build the prose from a known date, then
//! assert the exact expected EDTF), mirroring edtf-core's props.rs strategy —
//! nothing is filtered, so shrinking stays meaningful. Every asserted output
//! is round-tripped through `edtf_core::Edtf::parse` and its level checked;
//! that re-parse is what makes the guarantee real, since core has no
//! validating constructors.

#![expect(
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::cast_possible_wrap,
    reason = "generator and oracle ranges are bounded by construction"
)]
#![expect(
    clippy::non_ascii_literal,
    reason = "Russian prose dates are the input this normaliser exists to read; Cyrillic literals are the subject under test, not stray non-ASCII"
)]
#![expect(
    clippy::min_ident_chars,
    reason = "the test bodies use the same y/m/d date-component names as the code they exercise"
)]
#![expect(
    clippy::default_numeric_fallback,
    reason = "literal fixtures whose type the assertion already fixes"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic is over literal fixtures and generator-bounded values; an overflow here would fail the test, which is the signal"
)]
#![expect(
    clippy::as_conversions,
    reason = "casting fixture values the test itself bounded"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "indexing a fixture the test itself constructed; an out-of-range index is a failing test, not a crash path"
)]
#![expect(clippy::panic, reason = "a panic in a test IS the failure signal")]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "the wildcard covers variants the assertion has already excluded"
)]
#![expect(
    clippy::integer_division_remainder_used,
    reason = "same integer calendar arithmetic as the code under test"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use edtf_core::{Edtf, Set, SetElement, SetKind};
use edtf_normalize::{Language, Options, Outcome, normalize, normalize_with};
use proptest::prelude::*;

/// Assert a Normalized outcome and re-parse the output in core.
fn assert_normalized(input: &str, expected: &str, opts: Options) {
    match normalize_with(input, opts) {
        Outcome::Normalized(n) => {
            assert_eq!(n.edtf, expected, "input: {input:?}");
            let parsed = Edtf::parse(&n.edtf).expect("output must parse in core");
            assert_eq!(parsed, n.value, "value/edtf mismatch for {input:?}");
        }
        other => panic!("expected Normalized for {input:?}, got {other:?}"),
    }
}

fn en() -> Options {
    Options::default()
}

fn ru() -> Options {
    Options {
        language: Language::Russian,
        ..Options::default()
    }
}

const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const MONTHS_RU: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];

proptest! {
    #[test]
    fn plain_years_round_trip(y in 100_i32..=9999) {
        assert_normalized(&format!("{y}"), &format!("{y:04}"), en());
    }

    #[test]
    fn circa_years(y in 100_i32..=9999) {
        assert_normalized(&format!("circa {y}"), &format!("{y:04}~"), en());
        assert_normalized(&format!("около {y}"), &format!("{y:04}~"), ru());
    }

    #[test]
    fn bc_years_use_astronomical_numbering(b in 2_u32..=9999) {
        // N3: year b BC = astronomical -(b-1).
        let astro = -(b as i32 - 1);
        assert_normalized(&format!("{b} BC"), &format!("-{:04}", -astro), en());
        assert_normalized(&format!("{b} до н. э."), &format!("-{:04}", -astro), ru());
    }

    #[test]
    fn decades_mask_the_final_digit(p3 in 100_u16..=999) {
        prop_assume!(p3 % 10 != 0); // '..00s' forms are deliberately ambiguous (N6)
        assert_normalized(&format!("{p3}0s"), &format!("{p3}X"), en());
        assert_normalized(&format!("{p3}0-е"), &format!("{p3}X"), ru());
    }

    #[test]
    fn centuries_are_off_by_one(n in 2_u32..=20) {
        // N2: the nth century masks as (n-1)XX.
        assert_normalized(&format!("{n}th century"), &format!("{:02}XX", n - 1), en());
        assert_normalized(&format!("{n} век"), &format!("{:02}XX", n - 1), ru());
    }

    #[test]
    fn month_year_forms(y in 1000_i32..=9999, m in 0_usize..12) {
        let expected = format!("{y:04}-{:02}", m + 1);
        assert_normalized(&format!("{} {y}", MONTHS_EN[m]), &expected, en());
        assert_normalized(&format!("{} {y} года", MONTHS_RU[m]), &expected, ru());
    }

    #[test]
    fn unambiguous_numeric_dates(y in 1000_i32..=9999, m in 1_u32..=12, d in 13_u32..=28) {
        // d > 12 proves the order; d ≤ 28 is valid in every month (N5).
        let expected = format!("{y:04}-{m:02}-{d:02}");
        assert_normalized(&format!("{d}/{m}/{y}"), &expected, en());
        assert_normalized(&format!("{d}.{m}.{y}"), &expected, ru());
    }

    #[test]
    fn ambiguous_numeric_dates_always_report_both(
        y in 1000_i32..=9999, a in 1_u32..=12, b in 1_u32..=12
    ) {
        prop_assume!(a != b);
        match normalize(&format!("{a}/{b}/{y}")) {
            Outcome::Ambiguous(amb) => {
                assert_eq!(amb.interpretations.len(), 2);
                for i in &amb.interpretations {
                    let parsed = Edtf::parse(&i.edtf).expect("interpretation must parse");
                    assert_eq!(parsed, i.value);
                }
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn year_ranges(a in 1000_i32..=9998, span in 1_i32..=500) {
        let b = (a + span).min(9999);
        assert_normalized(&format!("from {a} to {b}"), &format!("{a:04}/{b:04}"), en());
        assert_normalized(&format!("с {a} по {b}"), &format!("{a:04}/{b:04}"), ru());
    }

    #[test]
    fn before_after_exclude_the_year(y in 1000_i32..=9998) {
        // N8: strictly before and after are one-of sets that exclude the
        // year; "until" and "since" are periods that reach it.
        assert_normalized(&format!("before {y}"), &format!("[..{:04}]", y - 1), en());
        assert_normalized(&format!("after {y}"), &format!("[{:04}..]", y + 1), en());
        assert_normalized(&format!("until {y}"), &format!("../{y:04}"), en());
        assert_normalized(&format!("since {y}"), &format!("{y:04}/.."), en());
        assert_normalized(&format!("до {y}"), &format!("[..{:04}]", y - 1), ru());
        assert_normalized(&format!("после {y}"), &format!("[{:04}..]", y + 1), ru());
    }

    #[test]
    fn every_outcome_is_internally_consistent(y in 1000_i32..=9999, m in 1_u32..=12, d in 1_u32..=31) {
        // Over arbitrary numeric input (valid or not): whatever comes back,
        // every emitted string parses in core and equals its value.
        match normalize(&format!("{d}/{m}/{y}")) {
            Outcome::Normalized(n) => {
                let parsed = Edtf::parse(&n.edtf).expect("must parse");
                assert_eq!(parsed, n.value);
            }
            Outcome::Ambiguous(amb) => {
                assert!(amb.interpretations.len() >= 2, "ambiguity needs 2+ readings");
                for i in &amb.interpretations {
                    let parsed = Edtf::parse(&i.edtf).expect("must parse");
                    assert_eq!(parsed, i.value);
                }
            }
            Outcome::NoMatch { reason: _ } => {} // Feb 30 and friends fail closed
        }
    }
}

/// The years a one-of range `[a..b]` spans, or a panic naming the input.
fn one_of_range(input: &str) -> (i64, i64) {
    match normalize(input) {
        Outcome::Normalized(n) => match n.value {
            Edtf::Set(Set {
                kind: SetKind::OneMember,
                elements,
            }) => match elements.as_slice() {
                &[SetElement::Range(a, b)] => (
                    a.year.value().expect("a plain year"),
                    b.year.value().expect("a plain year"),
                ),
                other => panic!("expected one range for {input:?}, got {other:?}"),
            },
            other => panic!("expected a one-of set for {input:?}, got {other:?}"),
        },
        other => panic!("expected Normalized for {input:?}, got {other:?}"),
    }
}

/// N1 over N2: every century's parts partition ISO's century.
///
/// For every century 2 to 20, early ∪ mid ∪ late and first half ∪ second
/// half each cover (n-1)00 to (n-1)99 (ISO 8601-1 3.1.2.23), exactly and
/// without overlap, as one-of ranges (ISO 8601-2 6.2, 6.3 c).
#[test]
fn century_parts_partition_the_century() {
    for n in 2_i64..=20 {
        let first = (n - 1) * 100;
        let last = first + 99;
        for parts in [
            &["early", "mid", "late"][..],
            &["first half of the", "second half of the"],
        ] {
            let mut next = first;
            for part in parts {
                let (a, b) = one_of_range(&format!("{part} {n}th century"));
                assert_eq!(
                    a, next,
                    "{part} {n}th century starts where the last part ended"
                );
                assert!(b >= a, "{part} {n}th century");
                next = b + 1;
            }
            assert_eq!(next - 1, last, "the {n}th century's parts end at {last}");
        }
    }
}
