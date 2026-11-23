// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The proptest engine: atoms from every row of every language's tables,
//! composed by the table's range pairs, "or" words and before/after
//! phrases, and the constructive properties moved from `tests/props.rs`.

extern crate std;

use alloc::{format, string::String, vec, vec::Vec};

use edtf_core::{Edtf, SetElement, SetKind};
use proptest::{prelude::*, sample::select};

use super::*;
use crate::{normalize, tables::Lang};

fn en() -> Options {
    options(Language::English)
}

fn ru() -> Options {
    options(Language::Russian)
}

fn read(strings: &[&str]) -> Answer {
    Answer::Read(strings.iter().map(|s| String::from(*s)).collect())
}

// -----------------------------------------------------------------------
// Generators: atoms from every row of a language's tables

/// Years an atom states.
const YEARS: core::ops::RangeInclusive<u32> = 1000..=2100;

/// `n` (1-39) as a Roman numeral.
fn roman(mut n: u32) -> String {
    let mut out = String::new();
    for (value, letters) in [(10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")] {
        while n >= value {
            out.push_str(letters);
            n -= value;
        }
    }
    out
}

fn language() -> impl Strategy<Value = Language> {
    select(LANGUAGES.to_vec())
}

/// One atom: a row of the language's month, season, ordinal, era,
/// decade or century table, with the numbers it needs; or a plain
/// year or an all-numeric date, which the options read.
fn atom(language: Language) -> BoxedStrategy<String> {
    let lang: &'static Lang = lang_for(language);
    let months = select(lang.months.to_vec());
    let centuries = select(lang.century_words.to_vec());
    let mut kinds: Vec<BoxedStrategy<String>> = vec![
        (months.clone(), YEARS)
            .prop_map(|((m, _), y)| format!("{m} {y}"))
            .boxed(),
        (1_u8..=28, months.clone(), YEARS)
            .prop_map(|(d, (m, _), y)| format!("{d} {m} {y}"))
            .boxed(),
        months.prop_map(|(m, _)| String::from(m)).boxed(),
        (select(lang.seasons.to_vec()), YEARS)
            .prop_map(|((s, _), y)| format!("{s} {y}"))
            .boxed(),
        (
            1_u32..=21,
            select(lang.ordinal_suffixes.to_vec()),
            centuries.clone(),
        )
            .prop_map(|(n, suffix, word)| format!("{n}{suffix} {word}"))
            .boxed(),
        (1_u32..=21, centuries.clone())
            .prop_map(|(n, word)| format!("{n} {word}"))
            .boxed(),
        (1_u32..=3000, select(lang.eras.to_vec()))
            .prop_map(|(y, (era, _))| format!("{y} {}", era.join(" ")))
            .boxed(),
        (100_u32..=209, select(lang.decade_suffixes.to_vec()))
            .prop_map(|(p, suffix)| format!("{p}0{suffix}"))
            .boxed(),
        (1_u32..=9, select(lang.decade_suffixes.to_vec()))
            .prop_map(|(t, suffix)| format!("{t}0{suffix}"))
            .boxed(),
        YEARS.prop_map(|y| format!("{y}")).boxed(),
        (1_u32..=28, 1_u32..=12, YEARS, select(vec!['/', '.']))
            .prop_map(|(d, m, y, sep)| format!("{d}{sep}{m}{sep}{y}"))
            .boxed(),
    ];
    if !lang.ordinal_words.is_empty() {
        kinds.push(
            (select(lang.ordinal_words.to_vec()), centuries.clone())
                .prop_map(|((word, _), century)| format!("{word} {century}"))
                .boxed(),
        );
    }
    if lang.roman_centuries {
        kinds.push(
            (1_u32..=21, centuries)
                .prop_map(|(n, word)| format!("{} {word}", roman(n)))
                .boxed(),
        );
    }
    proptest::strategy::Union::new(kinds).boxed()
}

/// An atom, or the EDTF an atom or a before/after form of it emits,
/// given back as input: an EDTF token is an atom too (N18).
fn atom_or_edtf(language: Language) -> BoxedStrategy<String> {
    (atom(language), 0_usize..3)
        .prop_map(move |(atom, kind)| {
            let source = match kind {
                0 => return atom,
                1 => atom,
                _ => open_forms(&atom, language)
                    .into_iter()
                    .next()
                    .unwrap_or(atom),
            };
            let emitted = Answer::of(&normalize_with(&source, options(language))).strings();
            emitted.into_iter().next().unwrap_or(source)
        })
        .boxed()
}

/// A language and two of its atoms.
fn pair() -> impl Strategy<Value = (Language, String, String)> {
    language().prop_flat_map(|l| (Just(l), atom_or_edtf(l), atom_or_edtf(l)))
}

/// A language and one of its phrases: an atom, or two atoms composed by
/// a range form, an "or" form, or a before/after form.
fn phrase() -> impl Strategy<Value = (Language, String)> {
    pair().prop_flat_map(|(l, a, b)| {
        let mut all = vec![a.clone()];
        all.extend(range_forms(&a, &b, l));
        all.extend(or_forms(&a, &b, l));
        all.extend(open_forms(&a, l));
        (Just(l), select(all))
    })
}

/// A phrase under one of the option combinations of its language.
fn optioned() -> impl Strategy<Value = (Options, String)> {
    (phrase(), any::<bool>(), 0_usize..3).prop_map(|((l, s), century, order)| {
        let numeric_order = [
            None,
            Some(NumericOrder::DayFirst),
            Some(NumericOrder::MonthFirst),
        ];
        let options = Options {
            language: l,
            numeric_order: numeric_order.get(order).copied().flatten(),
            default_century: if century { Century::new(1900) } else { None },
        };
        (options, s)
    })
}

// -----------------------------------------------------------------------
// The rules, driven by proptest

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn range_forms_agree((l, a, b) in pair()) {
        if let Some(forms) = observe_range_forms(&a, &b, l) {
            check_range_forms_agree(&forms);
        }
    }

    #[test]
    fn or_composes((l, a, b) in pair()) {
        if let Some(observation) = observe_or(&a, &b, l) {
            check_or_composes(&observation);
        }
    }

    #[test]
    fn before_and_after_compose((l, a, b) in pair(), span in any::<bool>()) {
        let operand = if span {
            range_forms(&a, &b, l).into_iter().next().unwrap_or(a)
        } else {
            a
        };
        if let Some(observation) = observe_open(&operand, l) {
            check_open_composes(&observation);
        }
    }

    #[test]
    fn case_never_changes_an_answer((options, s) in optioned()) {
        check_case(&observe_case(&s, options));
    }

    #[test]
    fn emitted_strings_reread_as_themselves((options, s) in optioned()) {
        check_idempotent(&observe_reread(&s, options));
    }

    #[test]
    fn whitespace_never_changes_an_answer((options, s) in optioned()) {
        check_whitespace(&observe_whitespace(&s, options));
    }

    #[test]
    fn options_change_only_what_they_resolve(
        (l, s) in phrase(),
        first_year in (0_u16..=99).prop_map(|c| c * 100),
    ) {
        if let Some(century) = Century::new(first_year) {
            check_options(&observe_options(&s, l, century));
        }
    }

    #[test]
    fn no_masked_year_beside_a_stated_one((options, s) in optioned()) {
        check_no_masked_year(&observe_values(&s, options));
    }

    #[test]
    fn readings_are_honest((options, s) in optioned()) {
        check_honest(&observe_honest(&s, options));
    }
}

// -----------------------------------------------------------------------
// Constructive properties, moved from tests/props.rs: build the prose
// from a known date and expect the exact EDTF.

/// The one EDTF string `input` normalises to under `opts`.
fn assert_normalized(input: &str, expected: &str, opts: Options) {
    assert_eq!(
        Answer::of(&normalize_with(input, opts)),
        read(&[expected]),
        "input: {input:?}"
    );
    check_honest(&observe_honest(input, opts));
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
    "\u{44f}\u{43d}\u{432}\u{430}\u{440}\u{44f}",
    "\u{444}\u{435}\u{432}\u{440}\u{430}\u{43b}\u{44f}",
    "\u{43c}\u{430}\u{440}\u{442}\u{430}",
    "\u{430}\u{43f}\u{440}\u{435}\u{43b}\u{44f}",
    "\u{43c}\u{430}\u{44f}",
    "\u{438}\u{44e}\u{43d}\u{44f}",
    "\u{438}\u{44e}\u{43b}\u{44f}",
    "\u{430}\u{432}\u{433}\u{443}\u{441}\u{442}\u{430}",
    "\u{441}\u{435}\u{43d}\u{442}\u{44f}\u{431}\u{440}\u{44f}",
    "\u{43e}\u{43a}\u{442}\u{44f}\u{431}\u{440}\u{44f}",
    "\u{43d}\u{43e}\u{44f}\u{431}\u{440}\u{44f}",
    "\u{434}\u{435}\u{43a}\u{430}\u{431}\u{440}\u{44f}",
];

proptest! {
    #[test]
    fn plain_years_round_trip(y in 100_i32..=9999) {
        assert_normalized(&format!("{y}"), &format!("{y:04}"), en());
    }

    #[test]
    fn circa_years(y in 100_i32..=9999) {
        assert_normalized(&format!("circa {y}"), &format!("{y:04}~"), en());
        assert_normalized(&format!("\u{43e}\u{43a}\u{43e}\u{43b}\u{43e} {y}"), &format!("{y:04}~"), ru());
    }

    #[test]
    fn bc_years_use_astronomical_numbering(b in 2_i32..=9999) {
        // N3: year b BC = astronomical -(b-1).
        let expected = format!("-{:04}", b - 1);
        assert_normalized(&format!("{b} BC"), &expected, en());
        assert_normalized(&format!("{b} \u{434}\u{43e} \u{43d}. \u{44d}."), &expected, ru());
    }

    #[test]
    fn decades_mask_the_final_digit(p3 in 100_u16..=999) {
        prop_assume!(p3 % 10 != 0); // '..00s' forms are deliberately ambiguous (N6)
        assert_normalized(&format!("{p3}0s"), &format!("{p3}X"), en());
        assert_normalized(&format!("{p3}0-\u{435}"), &format!("{p3}X"), ru());
    }

    #[test]
    fn centuries_are_off_by_one(n in 2_u32..=20) {
        // N2: the nth century masks as (n-1)XX.
        assert_normalized(&format!("{n}th century"), &format!("{:02}XX", n - 1), en());
        assert_normalized(&format!("{n} \u{432}\u{435}\u{43a}"), &format!("{:02}XX", n - 1), ru());
    }

    #[test]
    fn month_year_forms(y in 1000_i32..=9999, m in 1_usize..=12) {
        let expected = format!("{y:04}-{m:02}");
        let (Some(en_month), Some(ru_month)) = (MONTHS_EN.get(m - 1), MONTHS_RU.get(m - 1)) else {
            panic!("month {m}");
        };
        assert_normalized(&format!("{en_month} {y}"), &expected, en());
        assert_normalized(&format!("{ru_month} {y} \u{433}\u{43e}\u{434}\u{430}"), &expected, ru());
    }

    #[test]
    fn unambiguous_numeric_dates(y in 1000_i32..=9999, m in 1_u32..=12, d in 13_u32..=28) {
        // d > 12 proves the order; d <= 28 is valid in every month (N5).
        let expected = format!("{y:04}-{m:02}-{d:02}");
        assert_normalized(&format!("{d}/{m}/{y}"), &expected, en());
        assert_normalized(&format!("{d}.{m}.{y}"), &expected, ru());
    }

    #[test]
    fn ambiguous_numeric_dates_always_report_both(
        y in 1000_i32..=9999, a in 1_u32..=12, b in 1_u32..=12
    ) {
        prop_assume!(a != b);
        let input = format!("{a}/{b}/{y}");
        assert_eq!(
            Answer::of(&normalize(&input)),
            read(&[&format!("{y:04}-{b:02}-{a:02}"), &format!("{y:04}-{a:02}-{b:02}")]),
            "{input:?}"
        );
    }

    #[test]
    fn year_ranges(a in 1000_i32..=9998, span in 1_i32..=500) {
        let b = (a + span).min(9999);
        assert_normalized(&format!("from {a} to {b}"), &format!("{a:04}/{b:04}"), en());
        assert_normalized(&format!("\u{441} {a} \u{43f}\u{43e} {b}"), &format!("{a:04}/{b:04}"), ru());
    }

    #[test]
    fn before_after_exclude_the_year(y in 1000_i32..=9998) {
        // N8: strictly before and after are one-of sets that exclude the
        // year; "until" and "since" are periods that reach it.
        assert_normalized(&format!("before {y}"), &format!("[..{:04}]", y - 1), en());
        assert_normalized(&format!("after {y}"), &format!("[{:04}..]", y + 1), en());
        assert_normalized(&format!("until {y}"), &format!("../{y:04}"), en());
        assert_normalized(&format!("since {y}"), &format!("{y:04}/.."), en());
        assert_normalized(&format!("\u{434}\u{43e} {y}"), &format!("[..{:04}]", y - 1), ru());
        assert_normalized(&format!("\u{43f}\u{43e}\u{441}\u{43b}\u{435} {y}"), &format!("[{:04}..]", y + 1), ru());
    }

    #[test]
    fn every_numeric_outcome_is_honest(y in 1000_i32..=9999, m in 1_u32..=12, d in 1_u32..=31) {
        // Over arbitrary numeric input, valid or not (February 30 and
        // friends fail closed).
        check_honest(&observe_honest(&format!("{d}/{m}/{y}"), en()));
    }
}

/// The years a one-of range `[a..b]` spans, or a panic naming the input.
fn one_of_range(input: &str) -> (i64, i64) {
    let value = match normalize(input) {
        Outcome::Normalized(n) => n.value,
        other => panic!("expected Normalized for {input:?}, got {other:?}"),
    };
    let Edtf::Set(set) = value else {
        panic!("expected a set for {input:?}, got {value:?}");
    };
    match (set.kind(), set.elements()) {
        (SetKind::OneMember, &[SetElement::Range(r)]) => (
            r.start()
                .year()
                .value()
                .unwrap_or_else(|| panic!("{input:?}")),
            r.end()
                .year()
                .value()
                .unwrap_or_else(|| panic!("{input:?}")),
        ),
        other => panic!("expected one one-of range for {input:?}, got {other:?}"),
    }
}

/// N1 over N2: every century's parts partition ISO's century.
///
/// For every century 2 to 20, early, mid and late, and first half and
/// second half, each cover (n-1)00 to (n-1)99 (ISO 8601-1 3.1.2.23),
/// exactly and without overlap, as one-of ranges (ISO 8601-2 6.2, 6.3 c).
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

#[test]
fn roman_numerals_are_written() {
    assert_eq!(roman(19), "XIX");
    assert_eq!(roman(4), "IV");
    assert_eq!(roman(21), "XXI");
}

