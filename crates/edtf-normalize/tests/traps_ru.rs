// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Russian-language acceptance traps: the same N-decisions exercised through
//! the RU table set — Roman-numeral centuries, multi-token eras ("до н. э."),
//! grammatical-case month/season forms, year-marker noise ("гг."), and the
//! locale-implied day-first numeric order (N5).

#![expect(
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::non_ascii_literal,
    reason = "the corpus is Russian prose dates; Cyrillic literals are the subject under test"
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
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use edtf_core::Edtf;
use edtf_normalize::{Language, NoMatchReason, Note, Options, Outcome, normalize_with};

fn ru() -> Options {
    Options {
        language: Language::Russian,
        ..Options::default()
    }
}

#[track_caller]
fn ok(input: &str, expected: &str, level: u8) {
    match normalize_with(input, ru()) {
        Outcome::Normalized(n) => {
            assert_eq!(n.edtf, expected, "input: {input:?}");
            let parsed = Edtf::parse(&n.edtf).expect("output must parse in core");
            assert_eq!(parsed, n.value, "value/edtf mismatch for {input:?}");
            assert_eq!(parsed.level(), level, "level mismatch for {input:?}");
        }
        other => panic!("expected Normalized for {input:?}, got {other:?}"),
    }
}

#[track_caller]
fn ambiguous(input: &str, expected: &[&str]) {
    match normalize_with(input, ru()) {
        Outcome::Ambiguous(a) => {
            let got: Vec<&str> = a.interpretations.iter().map(|i| i.edtf.as_str()).collect();
            assert_eq!(got, expected, "input: {input:?}");
        }
        other => panic!("expected Ambiguous for {input:?}, got {other:?}"),
    }
}

#[track_caller]
fn no_match(input: &str) {
    assert!(
        matches!(normalize_with(input, ru()), Outcome::NoMatch { reason: _ }),
        "expected NoMatch for {input:?}"
    );
}

#[test]
fn years_with_markers() {
    ok("1985", "1985", 0);
    ok("1985 г.", "1985", 0);
    ok("в 1985 году", "1985", 0);
    ok("1985 год", "1985", 0);
}

#[test]
fn qualifiers() {
    ok("около 1920", "1920~", 1);
    ok("около 1920 г.", "1920~", 1);
    ok("ок. 1920", "1920~", 1);
    ok("ок.1920", "1920~", 1);
    ok("примерно 1920", "1920~", 1);
    ok("возможно 1920", "1920?", 1);
    ok("возможно около 1920", "1920%", 1);
    ok("1920?", "1920?", 1); // valid EDTF passthrough
}

#[test]
fn bc_years_are_astronomical() {
    // N3: 500 до н. э. = astronomical -0499.
    ok("500 до н. э.", "-0499", 1);
    ok("500 до н.э.", "-0499", 1);
    ok("500 до нашей эры", "-0499", 1);
    ok("79 н. э.", "0079", 0);
    ok("около 64 до н. э.", "-0063~", 1);
}

#[test]
fn edtf_letters_read_in_either_case() {
    // A whole input is tried as EDTF before it is read as prose: "xxxx" is
    // the masked year, not the 40th century; forty is XL (N18).
    ok("xxxx", "XXXX", 2);
    ok("xxxx век", "39XX", 1);
    ok("xix", "18XX", 1);
    // Inside a phrase a one-token atom is tried as EDTF first too (N18).
    // "до" is strictly before (N8). The year-less XXXX has no unit of its
    // own, so it is refused; the 40th century reads.
    no_match("до xxxx");
    ok("до xxxx века", "[..3899]", 2);
    // An EDTF token keeps its letter case; a Roman numeral reads in either.
    ok("до XXXX века", "[..3899]", 2);
}

#[test]
fn cyrillic_lookalikes_read_as_edtf_letters() {
    // Russian х, у, е, т (either case) stand for X, Y, E, T: a row of the
    // language's table, folded only where the result parses (N18).
    ok("\u{445}\u{445}\u{445}\u{445}", "XXXX", 2); // хххх
    ok("\u{425}\u{425}\u{425}\u{425}", "XXXX", 2); // ХХХХ
    ok("198\u{445}", "198X", 1); // 198х
    ok("\u{443}17\u{435}7", "Y17E7", 2); // у17е7
    ok("1985-04-12\u{442}23:20:30", "1985-04-12T23:20:30", 0); // т
    ok("1914 \u{438}\u{43b}\u{438} 198\u{445}", "[1914,198X]", 2); // или
    match normalize_with("198\u{445}", ru()) {
        Outcome::Normalized(n) => {
            assert_eq!(n.notes, [Note::EdtfLettersNormalized]);
            assert!(n.notes.iter().any(|note| note.decision() == Some("N18")));
        }
        other => panic!("expected Normalized, got {other:?}"),
    }
    // Unchanged: a two-letter numeral is a century; so is a four-letter one
    // with a century word, from the Roman-digit row (N15).
    ok("\u{445}\u{445}", "19XX", 1); // хх
    ok(
        "\u{445}\u{445}\u{445}\u{445} \u{432}\u{435}\u{43a}",
        "39XX",
        1,
    ); // хххх век
    ok("\u{425}\u{406}\u{425} \u{432}\u{435}\u{43a}", "18XX", 1); // ХІХ век
}

#[test]
fn a_range_word_inside_an_era_phrase_is_not_the_split() {
    // "до" is both the range word and the start of "до н. э.": every
    // position is tried, left to right, and the first whose two sides
    // read wins (N3, N4).
    ok("от 44 г. до н. э. до 1914", "-0043/1914", 1);
    ok("от 100 г. до н. э. до 44 г. до н. э.", "-0099/-0043", 1);
}

#[test]
fn roman_centuries() {
    ok("XIX век", "18XX", 1);
    ok("XIX в.", "18XX", 1);
    ok("xix век", "18XX", 1);
    ok("19 век", "18XX", 1);
    ok("19-й век", "18XX", 1);
    ok("I век", "00XX", 1);
    // Cyrillic lookalike letters (Х is Cyrillic here).
    ok("ХIХ век", "18XX", 1);
    // A BC century is one year of its span (N2).
    ok("II век до н. э.", "[-0199..-0100]", 2);
    // Numerals STARTING with a subtractive pair (N15 — the underflow trap).
    ok("IV век", "03XX", 1);
    ok("IX век", "08XX", 1);
    ok("XL век", "39XX", 1);
    ok("IX-X вв.", "08XX/09XX", 2);
    // Roman provenance is citable (N15).
    match normalize_with("XIX век", ru()) {
        Outcome::Normalized(n) => {
            assert!(n.notes.contains(&Note::RomanCentury));
            assert!(n.notes.iter().any(|note| note.decision() == Some("N15")));
        }
        other => panic!("expected Normalized, got {other:?}"),
    }
}

#[test]
fn century_word_forms() {
    // Genitive plural, prepositional, and the "ст." abbreviation.
    ok("XIX-XX веков", "18XX/19XX", 2);
    ok("конец XIX - начало XX веков", "18XX/19XX", 2);
    ok("в XIX столетии", "18XX", 1);
    ok("XIX ст.", "18XX", 1);
    ok("в XIX-XX веках", "18XX/19XX", 2);
}

#[test]
fn prepositional_months() {
    // "в мае 1945 года" is the most common Russian month-year prose form.
    ok("в мае 1945 года", "1945-05", 0);
    ok("в январе 1985 году", "1985-01", 0);
    ok("в сентябре 1939 г.", "1939-09", 0);
    ok("в августе 1991", "1991-08", 0);
}

#[test]
fn year_marker_before_era() {
    // "44 г. до н. э." — the standard textbook form (N3).
    ok("44 г. до н. э.", "-0043", 1);
    ok("44 г. до н.э.", "-0043", 1);
    ok("79 г. н. э.", "0079", 0);
    ok("около 44 г. до н.э.", "-0043~", 1);
    ok("в 44 году до н. э.", "-0043", 1);
}

#[test]
fn decade_of_century() {
    // "60-е годы XIX века" — the idiom behind «шестидесятники» (N6).
    ok("60-е годы XIX века", "186X", 1);
    ok("20-е годы XX века", "192X", 1);
    ok("в 60-х годах XIX века", "186X", 1);
    ok("60-е гг. XIX в.", "186X", 1);
}

#[test]
fn century_parts() {
    // N1: начало/середина/конец are decade-rounded thirds; halves are
    // halves; each is one year of its span (ISO 8601-2 6.2).
    ok("начало XIX века", "[1800..1829]", 2);
    ok("в начале XIX века", "[1800..1829]", 2);
    ok("середина XIX века", "[1830..1869]", 2);
    ok("конец XIX века", "[1870..1899]", 2);
    ok("первая половина XVIII века", "[1700..1749]", 2);
    ok("вторая половина XVIII века", "[1750..1799]", 2);
    // A qualified part falls back to the century's mask (N1, D27).
    ok("около начала XIX века", "18XX~", 1);
}

#[test]
fn decades() {
    ok("1980-е", "198X", 1);
    ok("1980-е годы", "198X", 1);
    ok("1980-х годов", "198X", 1);
    ok("около 1920-х", "192X~", 1);
    ambiguous("80-е", &["188X", "198X"]);
    ambiguous("1900-е", &["190X", "19XX"]);
}

#[test]
fn months_and_days() {
    ok("январь 1985", "1985-01", 0);
    ok("января 1985", "1985-01", 0);
    ok("12 апреля 1985", "1985-04-12", 0);
    ok("12-го апреля 1985 года", "1985-04-12", 0);
    ok("апрель", "XXXX-04", 2);
}

#[test]
fn seasons() {
    ok("весна 2001", "2001-21", 1);
    ok("весной 2001", "2001-21", 1);
    ok("летом 1872", "1872-22", 1);
    ok("осенью 1916", "1916-23", 1);
    ok("зимой 1872", "1872-24", 1);
}

#[test]
fn numeric_dates_use_locale_day_first() {
    // N5: DD.MM.YYYY is the Russian convention — resolved, with the note.
    match normalize_with("12.04.1985", ru()) {
        Outcome::Normalized(n) => {
            assert_eq!(n.edtf, "1985-04-12");
            assert!(n.notes.contains(&Note::NumericResolvedByLocale));
        }
        other => panic!("expected Normalized, got {other:?}"),
    }
    // An explicit option still overrides the locale.
    let opts = Options {
        language: Language::Russian,
        numeric_order: Some(edtf_normalize::NumericOrder::MonthFirst),
        ..Options::default()
    };
    match normalize_with("12.04.1985", opts) {
        Outcome::Normalized(n) => assert_eq!(n.edtf, "1985-12-04"),
        other => panic!("expected Normalized, got {other:?}"),
    }
}

#[test]
fn ranges() {
    ok("1914-1918", "1914/1918", 0);
    ok("1914\u{2013}1918 гг.", "1914/1918", 0);
    ok("с 1914 по 1918", "1914/1918", 0);
    ok("с 1914 по 1918 год", "1914/1918", 0);
    ok("XVII-XIX вв.", "16XX/18XX", 2);
    // There is no century 0 (found by the normalize fuzz target).
    no_match("0-1в");
    no_match("0-XIX вв.");
    // Modifiers on range endpoints collapse to the whole century (N1 scope).
    ok("конец XIX - начало XX века", "18XX/19XX", 2);
}

#[test]
fn before_after() {
    // N8: standing alone, "до" is exclusive ("по" is the inclusive end).
    ok("до 1917", "[..1916]", 2);
    ok("ранее 1917", "[..1916]", 2);
    ok("до 1917 года", "[..1916]", 2);
    ok("не позднее 1917", "[..1917]", 2);
    ok("после 1917", "[1918..]", 2);
    ok("не ранее 1917", "[1917..]", 2);
    ok("начиная с 1917", "1917/..", 1);
    ok("с 1917", "1917/..", 1);
    ok("до июня 1940", "[..1940-05]", 2);
    // A season has no predecessor (ISO 8601-2 4.8.1).
    no_match("до весны 1920");
    // "до" as before-marker vs "до н. э." as era: both at once.
    ok("до 500 до н. э.", "[..-0500]", 2);
    // Inside a range "с … до …" and "от … до …" stay range words.
    ok("с 1914 до 1918", "1914/1918", 0);
    ok("от 1914 до 1918", "1914/1918", 0);
}

#[test]
fn alternatives() {
    // N14: one one-of set (ISO 8601-2 6.2) when every alternative is a date
    // or a set; each alternative a reading when one is a period.
    ok("1863 или 1864", "[1863,1864]", 2);
    // Each side is a whole reading; a stated year scopes the others (N14, N16).
    ambiguous("с 1914 по 1918 или 1920", &["1914/1918", "1920"]);
    ambiguous("1914 — 1918 или 1920", &["1914/1918", "1920"]);
    ok("июнь или июль 1940", "[1940-06,1940-07]", 2);
    ok("начало XIX века или 1850", "[1800..1829,1850]", 2);
}

#[test]
fn explicit_no_date() {
    // N12: the form decides what "unknown" means — and the reason lets it.
    no_match("неизвестно");
    no_match("без даты");
    no_match("б.д.");
    no_match("не датировано");
    assert!(matches!(
        normalize_with("без даты", ru()),
        Outcome::NoMatch {
            reason: NoMatchReason::ExplicitNoDate
        }
    ));
}

#[test]
fn ranges_inherit_years_and_winters_stay_honest() {
    // N16 / N17 through the Russian tables.
    ok("с июня по июль 1940", "1940-06/1940-07", 0);
    ambiguous("зима 1941-1942 гг.", &["1941-24", "1941-24/1942"]);
    // N4: BC starts refuse elided end years in Russian too.
    no_match("с 500 до н. э. по 18");
}

#[test]
fn no_substring_extraction() {
    no_match("это не дата");
    no_match("1863, напечатано в 1870");
}
