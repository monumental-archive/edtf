// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Edge-of-grammar inputs: each test pins a refusal or acceptance path that
//! the trap list does not reach — attached modifiers, malformed numerics,
//! range endpoints that must fail closed, and preprocessing quirks. Same
//! N-decision authority as `traps.rs`.

#![expect(
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::non_ascii_literal,
    reason = "Russian prose dates are the input this normaliser exists to read; Cyrillic literals are the subject under test, not stray non-ASCII"
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
    clippy::missing_assert_message,
    reason = "the assertion's expression is its message"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use edtf_core::Edtf;
use edtf_normalize::{
    Century, Language, NoMatchReason, Note, NumericOrder, Options, Outcome, normalize,
    normalize_with,
};

fn ru() -> Options {
    Options {
        language: Language::Russian,
        ..Options::default()
    }
}

/// Assert a Normalized outcome with the given canonical EDTF.
#[track_caller]
fn ok_with(input: &str, opts: Options, expected: &str) {
    match normalize_with(input, opts) {
        Outcome::Normalized(n) => {
            assert_eq!(n.edtf, expected, "input: {input:?}");
            assert_eq!(Edtf::parse(&n.edtf).expect("output must parse"), n.value);
        }
        other => panic!("expected Normalized for {input:?}, got {other:?}"),
    }
}

#[track_caller]
fn ok(input: &str, expected: &str) {
    ok_with(input, Options::default(), expected);
}

/// Assert an Ambiguous outcome offering exactly these EDTF readings, in order.
#[track_caller]
fn ambiguous(input: &str, expected: &[&str]) {
    match normalize(input) {
        Outcome::Ambiguous(a) => {
            let got: Vec<&str> = a.interpretations.iter().map(|i| i.edtf.as_str()).collect();
            assert_eq!(got, expected, "input: {input:?}");
        }
        other => panic!("expected Ambiguous for {input:?}, got {other:?}"),
    }
}

#[track_caller]
fn no_match_with(input: &str, opts: Options) {
    assert!(
        matches!(normalize_with(input, opts), Outcome::NoMatch { reason: _ }),
        "expected NoMatch for {input:?}"
    );
}

#[track_caller]
fn no_match(input: &str) {
    no_match_with(input, Options::default());
}

// ---------------------------------------------------------------------------
// Years and eras

#[test]
fn bc_years_beyond_the_edtf_year_range_fail_closed() {
    // 20000 BC would be astronomical -19999, outside the four-digit model.
    no_match("20000 BC");
}

#[test]
fn era_qualified_non_years_fail_closed() {
    no_match("19th BC"); // era after something that is not a plain year
}

// ---------------------------------------------------------------------------
// Roman numerals (Cyrillic lookalikes included, N15)

#[test]
fn cyrillic_lookalike_roman_numerals() {
    // A real Russian keyboard produces Cyrillic Х/І in "ХІХ век" (N15).
    ok_with("Х\u{456}Х век", ru(), "18XX");
    // The full lookalike alphabet maps (л=L, с=C, д=D, м=M) but values
    // above 99 are not centuries — refuse, never guess.
    no_match_with("см век", ru());
    no_match_with("дм век", ru());
    // L (50) alone is a syntactically fine century ordinal.
    ok_with("хл век", ru(), "39XX");
}

// ---------------------------------------------------------------------------
// Decades and modifiers

#[test]
fn right_single_quote_marks_a_bare_decade() {
    // U+2019 is what word processors type for the apostrophe in "’80s".
    ambiguous("\u{2019}80s", &["188X", "198X"]);
}

#[test]
fn attached_approximate_prefix_requires_digits() {
    // "c" reads as circa only before a digit; "cat" is not "c. at".
    no_match("cat");
}

#[test]
fn decade_suffix_needs_digits_and_a_whole_decade() {
    no_match("abcs"); // suffix without digits
    no_match("180s"); // three-digit "decade" is neither decade nor century
}

#[test]
fn attached_modifiers() {
    // "mid-1930s": attached single-word modifier, dropped over a decade (N1).
    ok("mid-1930s", "193X");
    // A dangling hyphen after the modifier is not an attached form.
    no_match("mid- 1930s");
}

#[test]
fn modifier_over_an_ambiguous_decade_keeps_both_readings() {
    // "early 1900s": the modifier is dropped (N1) but the decade/century
    // ambiguity (N6) survives.
    ambiguous("early 1900s", &["190X", "19XX"]);
}

// ---------------------------------------------------------------------------
// Numeric tokens

#[test]
fn mixed_numeric_separators_fail_closed() {
    no_match("12/04-1985");
}

#[test]
fn impossible_on_both_numeric_readings_fails_closed() {
    // 13.13.1985: neither day-first nor month-first can place a 13th month.
    no_match("13.13.1985");
}

#[test]
fn equal_numeric_fields_are_order_irrelevant() {
    // Both orderings render the same date, so the order is irrelevant, not
    // two identical readings (N5).
    ok("04/04/1985", "1985-04-04");
    assert_eq!(notes_of("04/04/1985"), [Note::NumericOrderIrrelevant]);
}

#[test]
fn year_then_single_digit_month() {
    ok("1985/4", "1985-04");
}

#[test]
fn hyphenated_year_pair_rejects_non_ranges() {
    no_match("1914-13"); // not a month, not a later elided year
    no_match("1914-100"); // three digits: neither a month nor an elided year
}

#[test]
fn season_range_collisions_cover_every_season_name() {
    // N13: 21-24 collide with the four season codes.
    ambiguous("1915-21", &["1915-21", "1915/1921"]);
    ambiguous("1915-22", &["1915-22", "1915/1922"]);
    ambiguous("1915-23", &["1915-23", "1915/1923"]);
    ambiguous("1915-24", &["1915-24", "1915/1924"]);
}

// ---------------------------------------------------------------------------
// Open-ended ranges, alternatives

#[test]
fn open_range_over_a_collision_reports_both_readings() {
    // "1914-21" is spring 1914 or the span 1914-1921 (N13). Strictly
    // before a season has no form, so "before" refuses the input (N14);
    // on or before reads both.
    no_match_because(
        "before 1914-21",
        Options::default(),
        NoMatchReason::OutOfGrammar,
    );
    ambiguous("no later than 1914-21", &["[..1914-21]", "[..1921]"]);
}

#[test]
fn open_ranges_take_a_span_by_its_near_edge() {
    // Before a span is before its start; after it is after its end (N8).
    ok("before 1914-1918", "[..1913]");
    ok("after 1914-1918", "[1919..]");
    ok("after 1914-18", "[1919..]");
    ok("until 1914-1918", "../1918");
    ok("since 1914-1918", "1914/..");
    ok("before june-july 1940", "[..1940-05]");
    ok("after 2nd century bc", "[-0099..]");
    ok("before early 19th century", "[..1799]");
    assert!(!notes_of("before early 19th century").contains(&Note::ModifierDropped));
    ok("after late 19th century", "[1900..]");
    ok("before circa 1914-1918", "[..1913~]");
    ok("circa before 1917", "[..1916~]");
    // N17's one-winter reading has no predecessor (4.8.1), so strictly
    // before refuses the input (N14).
    no_match("before winter 1941-42");
    for input in ["before after 1900", "before before 1900", "after 1918-1914"] {
        no_match_because(input, Options::default(), NoMatchReason::OutOfGrammar);
    }
    // Both readings of "1900s" start in 1900: one answer.
    ok("before 1900s", "[..1899]");
    ambiguous("after 1900s", &["[1910..]", "[2000..]"]);
}

#[test]
fn edtf_letters_read_in_either_case() {
    // EDTF notation is read in either letter case, as a whole input and as
    // a token inside a phrase (N18).
    ok("198x", "198X");
    assert_eq!(notes_of("198x"), [Note::EdtfLettersNormalized]);
    ok("y17e7", "Y17E7");
    ok("1985-04-12t10:00:00z", "1985-04-12T10:00:00Z");
    ok("198x/199x", "198X/199X");
    ok("[198x,1990]", "[198X,1990]");
    ok("198X", "198X");
    assert_eq!(notes_of("198X"), [Note::AlreadyValidEdtf]);
    // The case note is there exactly when the fold changed the text.
    ok("before 198x", "[..1979]");
    assert_eq!(
        notes_of("before 198x"),
        [Note::EdtfLettersNormalized, Note::BeforeAfter]
    );
    ok("before 198X", "[..1979]");
    assert_eq!(notes_of("before 198X"), [Note::BeforeAfter]);
    ok("until y17e7", "../Y17E7");
    ok("before 1985-xx", "[..1984-12]");
    ok("before -0500", "[..-0501]");
    // An unmasked date is its own unit, in its own year form; a step from
    // a Y-year is a Y-year (N8).
    ok("no later than y17e7", "[..Y17E7]");
    ok("no earlier than y17e7", "[Y17E7..]");
    ok("before y17e7", "[..Y169999999]");
    ok("after y17e7", "[Y170000001..]");
    ok("before circa y17e7", "[..Y169999999~]");
    ok("after y-17e7", "[Y-169999999..]");
    ok("198X or 1990", "[198X,1990]");
    ok("1985~ or 1990", "[1985~,1990]");
    ok("2004-06~-11 or 2005", "[2004-06~-11,2005]");
    ok("198x to 1995", "198X/1995");
    // No EDTF form says "before" a time of day.
    no_match_because(
        "before 1985-04-12t10:00:00",
        Options::default(),
        NoMatchReason::OutOfGrammar,
    );
    ambiguous("1900s", &["190X", "19XX"]);
    ok("c1860", "1860~");
    no_match_because("xix", Options::default(), NoMatchReason::OutOfGrammar);
    // A colliding sub-year token stays the range grammar's question (N13).
    no_match_because(
        "1915-21 to 1990",
        Options::default(),
        NoMatchReason::OutOfGrammar,
    );
}

#[test]
fn sub_year_codes_read_inside_a_phrase() {
    // A sub-year code with no elided reading is simply that date (N13).
    // A season has no predecessor or successor (4.8.1), so strictly before
    // or after one is refused; the other rows use the season itself (N8).
    ok("no later than 1985-21", "[..1985-21]");
    ok("since 1985-24", "1985-24/..");
    ok("1985-21 or 1990", "[1985-21,1990]");
    ok("1985-21 to 1990", "1985-21/1990");
    ok("1985-21 - 1990-22", "1985-21/1990-22");
    // A colliding token gives one answer per reading as an alternative: a
    // set beside the season, the alternatives beside the span (N14).
    ambiguous("1914-21 or 1930", &["[1914-21,1930]", "1914/1921", "1930"]);
    // It is refused as a range endpoint.
    for input in [
        "before 1985-21",
        "after 1985-24",
        "before 1914-21",
        "1915-21 to 1990",
        "1900 to 1915-21",
        "before 1985-42",
        "before 1985-13",
    ] {
        no_match_because(input, Options::default(), NoMatchReason::OutOfGrammar);
    }
    // Two identical alternatives are one reading.
    ok("1863 or 1863", "1863");
}

#[test]
fn or_needs_an_expression_on_both_sides() {
    no_match("1863 or");
    no_match("or 1863");
}

#[test]
fn or_reports_each_reading_of_an_alternative() {
    // "1900s" is itself two readings: one set per combination (N14).
    ambiguous("1863 or 1900s", &["[1863,190X]", "[1863,19XX]"]);
}

// ---------------------------------------------------------------------------
// Two-sided ranges

#[test]
fn dangling_hyphen_is_not_a_range() {
    no_match("1914 -");
}

#[test]
fn bare_ordinal_left_endpoint_requires_a_century_right() {
    // The right side must be a century mask for "17-..." to inherit
    // century-ness; an ambiguous decade is not one.
    no_match("17-1900s");
}

#[test]
fn single_letter_roman_century_works_as_a_left_endpoint() {
    // A lone "х" is too weak to parse alone (N15) but inherits century-ness
    // from the right endpoint: X-XIX centuries.
    ok_with("х-х\u{456}х вв.", ru(), "09XX/18XX");
}

#[test]
fn ambiguous_left_endpoint_with_unparseable_right_fails_closed() {
    no_match("1900s-45");
}

#[test]
fn unparseable_endpoints_fail_closed() {
    no_match("abc-def");
}

#[test]
fn ambiguous_endpoint_ranges_keep_every_reading() {
    // Left ambiguous, right fixed.
    ambiguous("the 80s to 1990", &["188X/1990", "198X/1990"]);
    // Right ambiguous, left fixed.
    ambiguous("1750 to the 80s", &["1750/188X", "1750/198X"]);
}

#[test]
fn ambiguous_endpoint_with_one_reversed_reading_fails_closed() {
    // "the 80s to 1950": the 1980s reading is a reversed interval; a prose
    // error poisons the whole input rather than promoting the survivor (N14).
    assert!(matches!(
        normalize("the 80s to 1950"),
        Outcome::NoMatch { reason: _ }
    ));
}

#[test]
fn doubly_ambiguous_ranges_fail_closed() {
    // Every combination of two ambiguous endpoints is read; one of them
    // (198X to 189X) is reversed, which refuses the whole input (N14).
    no_match("the 80s to the 90s");
}

// ---------------------------------------------------------------------------
// Qualifiers and empty inputs

#[test]
fn spaced_trailing_question_mark() {
    ok("1860 ?", "1860?");
}

#[test]
fn a_lone_question_mark_is_not_a_date() {
    no_match("?");
}

#[test]
fn non_decade_years_with_a_decade_suffix_fail_closed() {
    no_match("1985s"); // only whole decades take the plural suffix
}

#[test]
fn era_years_outside_the_model_fail_closed() {
    no_match("0 AD"); // no year zero in era phrasing
    no_match("10000 AD"); // five digits only ever fit BC (astronomical -9999)
}

// ---------------------------------------------------------------------------
// Token-classifier extremes

#[test]
fn oversized_and_empty_numbers_fail_closed() {
    no_match("600000 BC"); // six digits never form a year token
    no_match("s"); // a decade suffix with no digits at all
}

#[test]
fn numeric_field_widths_are_enforced() {
    no_match("32/04/1985"); // 32 can only be a day, and no month fits
    no_match("123/4/1985"); // three-digit first field
    no_match("12/345/1985"); // three-digit second field
    no_match("004.1985"); // three-digit month
    no_match("13.2008"); // 13 is not a month
    no_match("1914.1918"); // year pairs only elide across '-'
}

#[test]
fn top_level_collision_requires_digits_on_both_sides() {
    no_match("1985-2a");
}

#[test]
fn sub_year_codes_that_cannot_elide_pass_through_as_edtf() {
    // 1985-21 reads only as spring 1985: 1921 < 1985 is no range (N13).
    ok("1985-21", "1985-21");
}

// ---------------------------------------------------------------------------
// Range word-splits and endpoint shapes

#[test]
fn range_words_need_both_sides() {
    no_match("to 1900");
    no_match("1900 to");
    no_match("- 1914"); // leading hyphen leaves an empty left endpoint
}

#[test]
fn spaced_elision_only_collides_on_bare_year_left_endpoints() {
    // A month (or day) on the left kills the sub-year reading: only the
    // elided range survives (N13 scope).
    ok("may 1915 - 21", "1915-05/1921");
    ok("12 may 1915 - 21", "1915-05-12/1921");
}

#[test]
fn range_endpoints_refuse_interval_readings() {
    // "1915-21" as an endpoint offers a sub-year date AND a year range; the
    // range reading cannot nest inside another range, so the pair fails
    // closed rather than promoting the survivor (N14).
    no_match("1900 to 1915-21");
    no_match("1915-21 to 1990");
}

#[test]
fn winter_needs_a_bare_successor_year_for_the_cross_year_reading() {
    // A month or day on the right endpoint forces the plain-range reading:
    // no one-winter interpretation exists (N17 scope).
    ok("winter 1941 - may 1942", "1941-24/1942-05");
    ok("winter 1941 - 12 may 1942", "1941-24/1942-05-12");
}

// ---------------------------------------------------------------------------
// Decade-of-century phrasing

#[test]
fn decade_of_century_in_both_registers() {
    // Noise between decade and century, Roman century (N6/N15)…
    ok_with("60-е годы XIX века", ru(), "186X");
    // …and the Arabic-ordinal variant.
    ok_with("60-е 19 века", ru(), "186X");
}

#[test]
fn bare_decade_of_bare_roman_century() {
    // No noise words at all between decade and century.
    ok_with("60-е XIX", ru(), "186X");
}

#[test]
fn collision_check_needs_digits_in_the_year_too() {
    no_match("198a-21");
    // An attached qualifier is not a year digit: it scopes the whole input,
    // so both range endpoints carry it (N10, N13).
    ambiguous("c100-21", &["0100-21~", "0100~/0121~"]);
}

// ---------------------------------------------------------------------------
// Boundaries the mutation leg found unpinned (#297)

/// Assert a `NoMatch` outcome with exactly this reason.
#[track_caller]
fn no_match_because(input: &str, opts: Options, reason: NoMatchReason) {
    assert_eq!(
        normalize_with(input, opts),
        Outcome::NoMatch { reason },
        "input: {input:?}"
    );
}

/// The notes of a `Normalized` outcome.
#[track_caller]
fn notes_of(input: &str) -> Vec<Note> {
    match normalize(input) {
        Outcome::Normalized(n) => n.notes,
        other => panic!("expected Normalized for {input:?}, got {other:?}"),
    }
}

#[test]
fn no_match_reasons_cite_their_decisions() {
    assert_eq!(NoMatchReason::OutOfGrammar.decision(), "N11");
    assert_eq!(NoMatchReason::ExplicitNoDate.decision(), "N12");
    assert_eq!(NoMatchReason::ImpossibleDate.decision(), "N14");
}

#[test]
fn season_collision_readings_name_the_season() {
    // Each sub-year code in a NNNN-NN collision is named in its reading
    // (N13); a code past the four seasons gets the generic name.
    for (code, name) in [
        (21_u8, "spring"),
        (22, "summer"),
        (23, "autumn"),
        (24, "winter"),
        (25, "sub-year grouping"),
    ] {
        let input = format!("1914-{code}");
        match normalize(&input) {
            Outcome::Ambiguous(a) => {
                assert_eq!(
                    a.interpretations.first().map(|i| i.reading.as_str()),
                    Some(format!("sub-year code {code} ({name})").as_str()),
                    "input: {input:?}"
                );
            }
            other => panic!("expected Ambiguous for {input:?}, got {other:?}"),
        }
    }
}

#[test]
fn numeric_fields_are_at_most_five_digits() {
    // A zero-padded five-digit field still reads as its value…
    ok("1985/04/00012", "1985-04-12");
    // …but six digits is not a date field at all.
    no_match_because(
        "1985/04/000012",
        Options::default(),
        NoMatchReason::OutOfGrammar,
    );
}

#[test]
fn subtractive_roman_numerals_reach_c_d_and_m() {
    // The signed accumulation reads any numeral whose total is 1-99, so the
    // C, D and M values are live even though no century is 100+ (N15).
    ok_with("xc век", ru(), "89XX");
    ok_with("ccccxd век", ru(), "89XX");
    ok_with("dccccxm век", ru(), "89XX");
}

#[test]
fn bare_roman_century_is_only_for_roman_languages() {
    // English does not write centuries in Roman numerals (N15).
    no_match_because("xix", Options::default(), NoMatchReason::OutOfGrammar);
}

#[test]
fn out_of_range_numeric_fields_are_out_of_grammar() {
    // A month of 13 is not a field the grammar reads, so this is not an
    // impossible calendar date (N11, not N14).
    no_match_because(
        "1985/13/05",
        Options::default(),
        NoMatchReason::OutOfGrammar,
    );
}

#[test]
fn hyphenated_year_ranges_as_or_alternatives() {
    // An "or" side is read as a whole input, so the range grammar (full and
    // elided end year, N4) answers it (N14).
    ambiguous("1914-1918 or 1920", &["1914/1918", "1920"]);
    ambiguous("1914-18 or 1920", &["1914/1918", "1920"]);
}

#[test]
fn decade_of_a_bc_century_fails_closed() {
    // The decade-of-century reading (N6) builds a positive decade mask; a BC
    // century cannot take one (N2), so the era refuses it.
    no_match_because(
        "60-е годы XIX века до н. э.",
        ru(),
        NoMatchReason::OutOfGrammar,
    );
}

#[test]
fn bare_left_ordinal_needs_a_century_on_the_right() {
    // "17-19th centuries" inherits century-ness; "17-1850" has none to
    // inherit.
    no_match_because("17-1850", Options::default(), NoMatchReason::OutOfGrammar);
}

#[test]
fn elision_needs_an_ad_start() {
    // Any AD start elides; only a BC start refuses (N4).
    ok("100-18", "0100/0118");
    ok("0001-18", "0001/0018"); // year 1 is the first AD year
}

#[test]
fn distributed_endpoint_year_replaces_the_masked_note() {
    // The year-less endpoint inherits a year, so its MissingYearMasked note
    // gives way to EndpointYearDistributed (N16).
    assert_eq!(
        notes_of("june to july 1940"),
        [Note::EndpointYearDistributed]
    );
}

#[test]
fn a_typed_masked_year_inherits_like_a_prose_one() {
    // Year-less is the value's `XXXX` year, not a note: an EDTF atom typed
    // with one inherits the stated year as a prose month does (N9, N16).
    ok("from XXXX-01 to 1985", "1985-01/1985");
    ok("from 1985 to XXXX-03", "1985/1985-03");
    assert_eq!(
        notes_of("from XXXX-01 to 1985"),
        [Note::EndpointYearDistributed]
    );
    assert_eq!(normalize("XXXX-01 or 1985"), normalize("january or 1985"));
    // A bare `XXXX` is an unknown year, not a year-less date: it has
    // nothing for a stated year to scope (N9).
    ok("from XXXX to 1985", "XXXX/1985");
    ok("XXXX or 1985", "[XXXX,1985]");
    // Two year-less ends keep their masks, typed or prose (N16).
    ok("XXXX-01 to june", "XXXX-01/XXXX-06");
}

#[test]
fn qualifier_distribution_is_noted_only_for_qualified_intervals() {
    // N10's note marks a whole-expression qualifier spread over interval
    // endpoints: not an unqualified interval, not a qualified date.
    assert_eq!(notes_of("1914-1918"), []);
    assert_eq!(notes_of("circa 1850"), []);
    assert_eq!(notes_of("circa 1850 to 1860"), [Note::QualifierDistributed]);
}

#[test]
fn collision_check_needs_digits_in_both_halves() {
    // "+101" parses as a number but is not four ASCII digits: no N13
    // collision reading may be built from it.
    no_match_because("+101-21", Options::default(), NoMatchReason::OutOfGrammar);
}

// ---------------------------------------------------------------------------
// One reading grammar on every path (#312)

#[test]
fn or_sides_are_whole_readings() {
    // An alternative is anything a whole input could be, and "or" never
    // binds inside a range (N14).
    ambiguous("1914-1914 or 1920", &["1914/1914", "1920"]);
    ambiguous("1914 - 1918 or 1920", &["1914/1918", "1920"]);
    ambiguous("from 1914 to 1918 or 1920", &["1914/1918", "1920"]);
    ok("before 1863 or 1864", "[..1862,1864]");
    ok("early 19th century or 1850", "[1800..1829,1850]");
    ok("1863 or 1864 or 1865", "[1863,1864,1865]");
    ambiguous("1914-1918 or 1920", &["1914/1918", "1920"]);
    // A qualified century part falls back to its mask inside the set (N1).
    ok("1863 or circa early 19th century", "[1863,18XX~]");
    for input in ["1918-1914 or 1920", "1914-14 or 1920"] {
        no_match_because(input, Options::default(), NoMatchReason::OutOfGrammar);
    }
}

#[test]
fn alternatives_share_a_stated_year() {
    // A stated year scopes the year-less alternatives (N16).
    ok("June or July 1940", "[1940-06,1940-07]");
    ok("June 1940 or July", "[1940-06,1940-07]");
    ok("12 June or 13 July 1940", "[1940-06-12,1940-07-13]");
    let inheriting = notes_of("June or July 1940");
    assert!(inheriting.contains(&Note::EndpointYearDistributed));
    assert!(!inheriting.contains(&Note::MissingYearMasked));
    // Agreeing stated years scope the year-less side together.
    ok(
        "June or July 1940 or August 1940",
        "[1940-06,1940-07,1940-08]",
    );
    for input in ["June or July 1940 or August 1941", "June or 1940s"] {
        no_match_because(input, Options::default(), NoMatchReason::OutOfGrammar);
    }
    ok("May or June", "[XXXX-05,XXXX-06]");
    ambiguous("June or 1914-1918", &["XXXX-06", "1914/1918"]);
}

#[test]
fn year_ranges_read_the_same_on_every_path() {
    // One endpoint pairing reads the unspaced, spaced and range-word forms
    // (N4).
    for input in ["0050-60", "0050 - 60", "from 0050 to 60"] {
        ok(input, "0050/0060");
    }
    for input in ["1914-1914", "1914 - 1914", "1914 to 1914"] {
        ok(input, "1914/1914");
    }
    for input in ["1 bc - 18", "44 bc to 18"] {
        no_match_because(input, Options::default(), NoMatchReason::OutOfGrammar);
    }
    ok("0000-21", "0000-21");
    assert_eq!(notes_of("0000-21"), [Note::AlreadyValidEdtf]);
    ambiguous("0010-25", &["0010-25", "0010/0025"]);
}

// ---------------------------------------------------------------------------
// One reader for every part of a phrase (#360)

#[test]
fn trailing_qualifier_symbols_scope_the_whole_expression() {
    // `?`, `~` and `%` scope alike: a trailing symbol on a range or an "or"
    // list qualifies all of it (N10).
    ok("1868-1871?", "1868?/1871?");
    ok("1868-1871~", "1868~/1871~");
    ok("1868-1871%", "1868%/1871%");
    ok("1914 or 1985~", "[1914~,1985~]");
    // Valid EDTF carries its own qualifiers and passes through untouched.
    ok("1984?/2004~", "1984?/2004~");
    assert_eq!(notes_of("1984?/2004~"), [Note::AlreadyValidEdtf]);
}

#[test]
fn a_range_end_reads_as_it_reads_alone() {
    // Year inheritance (N16) runs on every combination, so XXXX never
    // leaks into a range that states its year (N9); a combination that
    // fails refuses the whole input (N14).
    no_match_because(
        "June - 12/04/1985",
        Options::default(),
        NoMatchReason::ImpossibleDate,
    );
    ok_with(
        "June - 12/04/1985",
        Options {
            numeric_order: Some(NumericOrder::MonthFirst),
            ..Options::default()
        },
        "1985-06/1985-12-04",
    );
    no_match("June - the 80s");
    no_match_with(
        "June - the 80s",
        Options {
            default_century: Century::new(1900),
            ..Options::default()
        },
    );
    // A range ambiguous on both sides reports every combination.
    ambiguous(
        "12/04/1985 - 11/05/1986",
        &[
            "1985-04-12/1986-05-11",
            "1985-04-12/1986-11-05",
            "1985-12-04/1986-05-11",
            "1985-12-04/1986-11-05",
        ],
    );
    ok("early 1930s to late 1940s", "193X/194X");
    no_match("5th century BC to 3rd century BC");
}

#[test]
fn any_edtf_value_is_an_alternative_and_a_span_an_operand() {
    // An EDTF token holds any value: an alternative may be one (N18); a span
    // is an operand by its near edge (N8).
    ambiguous("1914 or 1985/1990", &["1914", "1985/1990"]);
    ok("1914 or [1667,1668]", "[1914,1667,1668]");
    ok("before 1985/1990", "[..1984]");
    ok("after 1985/1990", "[1991..]");
    // A one-element set is its element.
    ok("before [1667]", "[..1666]");
    // A set, an open interval or a datetime is no before/after operand, and
    // an interval is no range end.
    for input in [
        "before [1667,1668]",
        "before ../1985",
        "before 1985-04-12T23:20:30",
        "1985/1990 - 1995",
    ] {
        no_match(input);
    }
}
