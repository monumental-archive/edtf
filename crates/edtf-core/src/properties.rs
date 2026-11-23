// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The crate's general rules, each asserted once (#367).
//!
//! Every rule is two functions: an *observation* (`observe_*`) that calls
//! the library and records what it answered, and a *check* (`check_*`)
//! that is pure and asserts the rule over that record. Three engines drive
//! the same checks: the proptest properties over generated values
//! (`proptests`), the deterministic string harness every `cargo test` runs
//! (`harness`), and the cargo-fuzz targets over parsed values
//! (`fuzz/fuzz_targets/roundtrip.rs` and `relate.rs`). None of them holds
//! an assertion of its own about these rules.
//!
//! The module exists only under `cfg(test)` and `cfg(fuzzing)`, the cfg
//! cargo-fuzz builds with: an ordinary build, and the documentation, never
//! see it. Each check's own assertions are pinned by a planted test that
//! hands it one fabricated bad observation (`planted`), so a check that
//! stopped asserting is a failing test, and a mutant of it is killed.

use core::iter::once;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    Bound, BoundDate, Bounds, Date, Edtf, IntervalEndpoint, Modality, ParseError, Qualifier,
    Relation, SetElement, Unenumerable,
};

#[cfg(test)]
mod harness;
#[cfg(test)]
mod proptests;

// ------------------------------------------------------------- text rule

/// What the parser's entry points answered for one input string.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TextObservation {
    /// The input.
    pub input: String,
    /// `Edtf::parse(input)`.
    pub parsed: Result<Edtf, ParseError>,
    /// `input.parse::<Edtf>()`.
    pub from_str: Result<Edtf, ParseError>,
    /// The refusal's message as displayed, when the input is refused.
    pub refusal: Option<String>,
    /// The free function `level(input)`.
    pub level: Option<u8>,
    /// `Edtf::level` of the parsed value, when the input is accepted.
    pub parsed_level: Option<u8>,
}

/// Observe the parser's entry points on `input`.
#[must_use]
pub fn observe_text(input: &str) -> TextObservation {
    let parsed = Edtf::parse(input);
    let refusal = parsed.as_ref().err().map(ToString::to_string);
    let parsed_level = parsed.as_ref().ok().map(Edtf::level);
    TextObservation {
        input: String::from(input),
        parsed,
        from_str: input.parse::<Edtf>(),
        refusal,
        level: crate::level(input),
        parsed_level,
    }
}

/// The entry points agree: `FromStr` is `Edtf::parse`, a refusal says
/// why, and the free `level` is the value's.
///
/// # Panics
///
/// When the rule is broken.
pub fn check_text(seen: &TextObservation) {
    let input = &seen.input;
    assert_eq!(
        seen.from_str, seen.parsed,
        "FromStr and Edtf::parse disagree on {input:?}"
    );
    if let Some(refusal) = seen.refusal.as_ref() {
        assert!(
            !refusal.is_empty(),
            "{input:?} is refused with an empty message"
        );
    }
    assert_eq!(
        seen.level, seen.parsed_level,
        "level({input:?}) disagrees with Edtf::level"
    );
}

// ------------------------------------------------------------ value rule

/// One value an expression enumerated, as observed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EnumeratedObservation {
    /// The value's canonical text.
    pub text: String,
    /// The value.
    pub value: Edtf,
    /// Its canonical text, parsed again.
    pub reparsed: Result<Edtf, ParseError>,
    /// Whether it still holds an unspecified digit.
    pub masked: bool,
    /// Its bounds.
    pub bounds: Bounds,
}

/// Everything one value answers: its canonical form, level, qualification,
/// bounds, enumeration and its relation to itself.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ValueObservation {
    /// The value's canonical text.
    pub text: String,
    /// The value.
    pub value: Edtf,
    /// Its canonical text, parsed again.
    pub reparsed: Result<Edtf, ParseError>,
    /// `is_valid` of its canonical text.
    pub valid: bool,
    /// Its level.
    pub level: u8,
    /// The reparsed value's level.
    pub reparsed_level: Option<u8>,
    /// Whether it is uncertain and whether approximate.
    pub qualification: (bool, bool),
    /// The same of the reparsed value.
    pub reparsed_qualification: Option<(bool, bool)>,
    /// Its bounds.
    pub bounds: Bounds,
    /// Whether it is an interval whose start, and whose end, is Unknown.
    pub unknown_ends: (bool, bool),
    /// The Julian Day Numbers of its bounds, when both are days.
    pub bound_day_numbers: Option<(i128, i128)>,
    /// Its bounds as displayed, when both are days.
    pub bound_texts: Option<(String, String)>,
    /// What the structure says `values()` answers (D24-D25): an interval,
    /// or a set holding an unbounded element, is unenumerable.
    pub expected_unenumerable: Option<Unenumerable>,
    /// What `values()` answered, up to the observation's cap.
    pub values: Result<Vec<EnumeratedObservation>, Unenumerable>,
    /// The refusal's message as displayed, when `values()` refused.
    pub unenumerable_reason: Option<String>,
    /// Whether the enumeration ended within the cap.
    pub exhausted: bool,
    /// Whether the value can equal itself.
    pub self_equal_possible: bool,
    /// The value's definite relation to itself, if any.
    pub self_definite: Option<Relation>,
}

/// Observe `value`, enumerating at most `cap` of its values.
#[must_use]
pub fn observe_value(value: &Edtf, cap: usize) -> ValueObservation {
    let text = value.to_string();
    let reparsed = Edtf::parse(&text);
    let bounds = value.bounds();
    let days = if let (Bound::Date(lo), Bound::Date(hi)) = (bounds.earliest, bounds.latest) {
        Some((lo, hi))
    } else {
        None
    };
    let (unknown_ends, expected_unenumerable) = match value.clone() {
        Edtf::Interval(iv) => (
            (
                matches!(iv.start(), IntervalEndpoint::Unknown),
                matches!(iv.end(), IntervalEndpoint::Unknown),
            ),
            Some(Unenumerable::Interval),
        ),
        Edtf::Set(set) => (
            (false, false),
            set.elements()
                .iter()
                .any(|e| matches!(e, SetElement::OnOrBefore(_) | SetElement::OnOrAfter(_)))
                .then_some(Unenumerable::UnboundedSetElement),
        ),
        Edtf::Date(_) | Edtf::DateTime(_) => ((false, false), None),
    };
    let enumeration = value.values();
    let unenumerable_reason = enumeration.as_ref().err().map(ToString::to_string);
    let mut exhausted = true;
    let values = enumeration.map(|mut stream| {
        let items = stream.by_ref().take(cap).map(observe_enumerated).collect();
        exhausted = stream.next().is_none();
        items
    });
    let relation = value.relation(value);
    let reparsed_level = reparsed.as_ref().ok().map(Edtf::level);
    let reparsed_qualification = reparsed
        .as_ref()
        .ok()
        .map(|again| (again.is_uncertain(), again.is_approximate()));
    ValueObservation {
        text,
        value: value.clone(),
        reparsed,
        valid: crate::is_valid(&value.to_string()),
        level: value.level(),
        reparsed_level,
        qualification: (value.is_uncertain(), value.is_approximate()),
        reparsed_qualification,
        bounds,
        unknown_ends,
        bound_day_numbers: days.map(|(lo, hi)| (lo.julian_day_number(), hi.julian_day_number())),
        bound_texts: days.map(|(lo, hi)| (lo.to_string(), hi.to_string())),
        expected_unenumerable,
        values,
        unenumerable_reason,
        exhausted,
        self_equal_possible: relation.is_possible(Relation::Equal),
        self_definite: relation.definite(),
    }
}

/// Observe one enumerated value.
fn observe_enumerated(value: Edtf) -> EnumeratedObservation {
    let text = value.to_string();
    let reparsed = Edtf::parse(&text);
    let masked = value.has_unspecified();
    let bounds = value.bounds();
    EnumeratedObservation {
        text,
        value,
        reparsed,
        masked,
        bounds,
    }
}

/// Every value keeps the crate's promises about it.
///
/// Its canonical form reparses to it, is valid and keeps its level and
/// qualification; the level is 0-2. A bound is Unknown only on the side
/// of an interval whose endpoint there is Unknown (D30); two day bounds
/// are ordered, ordered as their Julian Day Numbers, and display.
/// `values()` refuses exactly where the structure says (D24-D25), and
/// says why; every value it yields round-trips, is concrete and lies
/// within the bounds; a single date's values start at its earliest day,
/// ascend, and end at its latest day. A value can equal itself, and a
/// single-day value does.
///
/// # Panics
///
/// When the rule is broken.
pub fn check_value(seen: &ValueObservation) {
    check_canonical_form(seen);
    check_bounds(seen);
    check_enumeration(seen);
    check_self_relation(seen);
}

/// The canonical-form half of [`check_value`].
///
/// # Panics
///
/// When the rule is broken.
fn check_canonical_form(seen: &ValueObservation) {
    let text = &seen.text;
    assert_eq!(
        seen.reparsed.as_ref(),
        Ok(&seen.value),
        "canonical form {text:?} does not reparse to the value"
    );
    assert!(seen.valid, "is_valid refuses the canonical form {text:?}");
    assert!(
        seen.level <= 2,
        "{text:?} classified at level {}",
        seen.level
    );
    assert_eq!(
        seen.reparsed_level,
        Some(seen.level),
        "{text:?} changed level on reparse"
    );
    assert_eq!(
        seen.reparsed_qualification,
        Some(seen.qualification),
        "{text:?} changed qualification on reparse"
    );
}

/// The bounds half of [`check_value`].
///
/// # Panics
///
/// When the rule is broken.
fn check_bounds(seen: &ValueObservation) {
    let text = &seen.text;
    let Bounds { earliest, latest } = seen.bounds;
    assert_eq!(
        earliest == Bound::Unknown,
        seen.unknown_ends.0,
        "{text:?}: its earliest bound is {earliest:?}"
    );
    assert_eq!(
        latest == Bound::Unknown,
        seen.unknown_ends.1,
        "{text:?}: its latest bound is {latest:?}"
    );
    if let (Bound::Date(lo), Bound::Date(hi)) = (earliest, latest) {
        assert!(lo <= hi, "{text:?}: earliest {lo} is after latest {hi}");
        let (lo_day, hi_day) = seen.bound_day_numbers.unwrap_or_default();
        assert_eq!(
            lo <= hi,
            lo_day <= hi_day,
            "{text:?}: day order and JDN order disagree for {lo} and {hi}"
        );
        let (lo_text, hi_text) = seen.bound_texts.clone().unwrap_or_default();
        assert!(
            !lo_text.is_empty() && !hi_text.is_empty(),
            "{text:?}: a bound displays empty"
        );
    }
}

/// The enumeration half of [`check_value`].
///
/// # Panics
///
/// When the rule is broken.
fn check_enumeration(seen: &ValueObservation) {
    let text = &seen.text;
    assert_eq!(
        seen.values.as_ref().err().copied(),
        seen.expected_unenumerable,
        "{text:?}: values() disagrees with the structure"
    );
    if let Some(reason) = seen.unenumerable_reason.as_ref() {
        assert!(
            !reason.is_empty(),
            "{text:?}: unenumerable without a reason"
        );
    }
    let Ok(items) = seen.values.as_ref() else {
        return;
    };
    let Bounds { earliest, latest } = seen.bounds;
    for item in items {
        let shown = &item.text;
        assert_eq!(
            item.reparsed.as_ref(),
            Ok(&item.value),
            "{text:?} enumerated {shown:?}, which does not round-trip"
        );
        assert!(
            !item.masked,
            "{text:?} enumerated {shown:?}, which is still masked"
        );
        if let (Bound::Date(outer), Bound::Date(inner)) = (earliest, item.bounds.earliest) {
            assert!(
                outer <= inner,
                "{text:?} enumerated {shown:?} before its earliest bound"
            );
        }
        if let (Bound::Date(outer), Bound::Date(inner)) = (latest, item.bounds.latest) {
            assert!(
                inner <= outer,
                "{text:?} enumerated {shown:?} after its latest bound"
            );
        }
    }
    if matches!(seen.value, Edtf::Date(_)) {
        check_date_enumeration(seen, items);
    }
}

/// A single date's enumeration starts at its earliest day, ascends, and,
/// when it ended within the cap, ends at its latest day.
///
/// # Panics
///
/// When the rule is broken.
fn check_date_enumeration(seen: &ValueObservation, items: &[EnumeratedObservation]) {
    let text = &seen.text;
    assert!(
        !items.is_empty(),
        "{text:?}: a date enumerated nothing, though D11 guarantees a completion"
    );
    let (Some(first), Some(last)) = (items.first(), items.last()) else {
        return;
    };
    assert_eq!(
        first.bounds.earliest, seen.bounds.earliest,
        "{text:?}: enumeration does not start at the earliest bound"
    );
    for pair in items.windows(2) {
        if let (Some(before), Some(after)) = (pair.first(), pair.last()) {
            let ascends = matches!(
                (before.bounds.earliest, after.bounds.earliest),
                (Bound::Date(earlier), Bound::Date(later)) if earlier < later
            );
            assert!(
                ascends,
                "{text:?}: {} does not ascend from {}",
                after.text, before.text
            );
        }
    }
    if seen.exhausted {
        assert_eq!(
            last.bounds.latest, seen.bounds.latest,
            "{text:?}: enumeration does not end at the latest bound"
        );
    }
}

/// The self-relation half of [`check_value`].
///
/// # Panics
///
/// When the rule is broken.
fn check_self_relation(seen: &ValueObservation) {
    let text = &seen.text;
    assert!(
        seen.self_equal_possible,
        "{text:?} vs itself: Equal is ruled out"
    );
    if let (Bound::Date(lo), Bound::Date(hi)) = (seen.bounds.earliest, seen.bounds.latest) {
        if lo == hi {
            assert_eq!(
                seen.self_definite,
                Some(Relation::Equal),
                "{text:?}: a single day is not definitely equal to itself"
            );
        }
    }
}

// --------------------------------------------------------- bound-day rule

/// A bound day and its two calendar neighbours.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct BoundDayObservation {
    /// The day.
    pub day: BoundDate,
    /// Its Julian Day Number.
    pub number: i128,
    /// The next day and its number, unless the day is the last one.
    pub next: Option<(BoundDate, i128)>,
    /// The previous day and its number, unless the day is the first one.
    pub previous: Option<(BoundDate, i128)>,
}

/// Observe `day` and its neighbours.
#[must_use]
pub fn observe_bound_day(day: BoundDate) -> BoundDayObservation {
    BoundDayObservation {
        day,
        number: day.julian_day_number(),
        next: day.next_day().map(|d| (d, d.julian_day_number())),
        previous: day.previous_day().map(|d| (d, d.julian_day_number())),
    }
}

/// A bound day is a real day: it steps and orders like its Julian Day
/// Number (D31).
///
/// # Panics
///
/// When the rule is broken.
pub fn check_bound_day(seen: &BoundDayObservation) {
    let day = seen.day;
    if let Some((next, number)) = seen.next {
        assert!(
            day < next && Some(number) == seen.number.checked_add(1),
            "{day}: the next day {next} does not follow it"
        );
    }
    if let Some((previous, number)) = seen.previous {
        assert!(
            previous < day && Some(number) == seen.number.checked_sub(1),
            "{day}: the previous day {previous} does not precede it"
        );
    }
}

// ----------------------------------------------------------- letters rule

/// Every variant of a text with one of its letters in lower case, and
/// which of them the parser accepted.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LettersObservation {
    /// The text.
    pub text: String,
    /// The variants the parser accepted.
    pub accepted: Vec<String>,
}

/// Lower-case each letter of `text` in turn, and parse.
#[must_use]
pub fn observe_letters(text: &str) -> LettersObservation {
    let accepted = text
        .char_indices()
        .filter(|&(_, c)| c.is_ascii_uppercase())
        .map(|(at, c)| {
            let mut variant = String::from(text);
            variant.replace_range(at..=at, &c.to_ascii_lowercase().to_string());
            variant
        })
        .filter(|variant| crate::is_valid(variant))
        .collect();
    LettersObservation {
        text: String::from(text),
        accepted,
    }
}

/// EDTF letters are upper case only (D32): lowering any one of them
/// leaves no EDTF.
///
/// # Panics
///
/// When the rule is broken.
pub fn check_letters(seen: &LettersObservation) {
    assert!(
        seen.accepted.is_empty(),
        "{:?}: lower-case letters accepted in {:?}",
        seen.text,
        seen.accepted
    );
}

// -------------------------------------------------------- whitespace rule

/// The whitespace the rule inserts: space, tab, newline and no-break space.
const WHITESPACE: [char; 4] = [' ', '\t', '\n', '\u{a0}'];

/// Every variant of a text with whitespace inserted at one position, and
/// which of them the parser accepted.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WhitespaceObservation {
    /// The text.
    pub text: String,
    /// The variants the parser accepted.
    pub accepted: Vec<String>,
}

/// Insert each kind of whitespace at every position of `text`, and parse.
#[must_use]
pub fn observe_whitespace(text: &str) -> WhitespaceObservation {
    let positions = text
        .char_indices()
        .map(|(at, _)| at)
        .chain(once(text.len()));
    let accepted = positions
        .flat_map(|at| {
            WHITESPACE.map(|space| {
                let mut variant = String::from(text);
                variant.insert(at, space);
                variant
            })
        })
        .filter(|variant| crate::is_valid(variant))
        .collect();
    WhitespaceObservation {
        text: String::from(text),
        accepted,
    }
}

/// No whitespace anywhere (spec-notes §7 rule 11).
///
/// # Panics
///
/// When the rule is broken.
pub fn check_whitespace(seen: &WhitespaceObservation) {
    assert!(
        seen.accepted.is_empty(),
        "{:?}: whitespace accepted in {:?}",
        seen.text,
        seen.accepted
    );
}

// ----------------------------------------------------- qualification rule

/// A date's bounds, and its bounds under each qualification.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct QualificationObservation {
    /// The date's canonical text.
    pub text: String,
    /// The date's bounds.
    pub bounds: Bounds,
    /// The bounds of the date made uncertain, approximate, and both.
    pub qualified: [Bounds; 3],
}

/// Observe `date`'s bounds with and without each qualification.
#[must_use]
pub fn observe_qualification(date: Date) -> QualificationObservation {
    let qualifiers = [(true, false), (false, true), (true, true)];
    QualificationObservation {
        text: Edtf::Date(date).to_string(),
        bounds: Edtf::Date(date).bounds(),
        qualified: qualifiers.map(|(uncertain, approximate)| {
            Edtf::Date(date.qualified(Qualifier {
                uncertain,
                approximate,
            }))
            .bounds()
        }),
    }
}

/// Qualification never moves bounds (D23 a).
///
/// # Panics
///
/// When the rule is broken.
pub fn check_qualification(seen: &QualificationObservation) {
    for bounds in seen.qualified {
        assert_eq!(
            bounds, seen.bounds,
            "{:?}: qualification moved its bounds",
            seen.text
        );
    }
}

// --------------------------------------------------------- relation rule

/// Two values related both ways.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PairObservation {
    /// The left value's canonical text.
    pub left: String,
    /// The right value's canonical text.
    pub right: String,
    /// Each relation's modality, left to right, in `Relation::ALL` order.
    pub forward: [Modality; 6],
    /// Each relation's converse's modality, right to left, in the same
    /// order.
    pub converse: [Modality; 6],
    /// The possible relations, left to right.
    pub possible: Vec<Relation>,
    /// Whether each relation is definite, in `Relation::ALL` order.
    pub definite_each: [bool; 6],
    /// Whether each relation is possible, in `Relation::ALL` order.
    pub possible_each: [bool; 6],
    /// Whether each relation is impossible, in `Relation::ALL` order.
    pub impossible_each: [bool; 6],
    /// The definite relation, if any.
    pub definite: Option<Relation>,
    /// Whether the left value is definitely before the right.
    pub definite_before: bool,
    /// Each relation's name and its modality's name, in `Relation::ALL`
    /// order.
    pub names: [(&'static str, &'static str); 6],
    /// The relation's description.
    pub description: String,
    /// Whether any bound on either side is Unknown.
    pub unknown: bool,
    /// The left value's latest bound.
    pub left_latest: Bound,
    /// The right value's earliest bound.
    pub right_earliest: Bound,
}

/// Relate `left` to `right` and back.
#[must_use]
pub fn observe_pair(left: &Edtf, right: &Edtf) -> PairObservation {
    let forward = left.relation(right);
    let backward = right.relation(left);
    let any_unknown =
        |bounds: Bounds| bounds.earliest == Bound::Unknown || bounds.latest == Bound::Unknown;
    PairObservation {
        left: left.to_string(),
        right: right.to_string(),
        forward: Relation::ALL.map(|r| forward.modality(r)),
        converse: Relation::ALL.map(|r| backward.modality(r.converse())),
        possible: forward.possible().collect(),
        definite_each: Relation::ALL.map(|r| forward.is_definite(r)),
        possible_each: Relation::ALL.map(|r| forward.is_possible(r)),
        impossible_each: Relation::ALL.map(|r| forward.is_impossible(r)),
        definite: forward.definite(),
        definite_before: forward.is_definite(Relation::Before),
        names: Relation::ALL.map(|r| (r.as_str(), forward.modality(r).as_str())),
        description: forward.to_string(),
        unknown: any_unknown(left.bounds()) || any_unknown(right.bounds()),
        left_latest: left.bounds().latest,
        right_earliest: right.bounds().earliest,
    }
}

/// The relation algebra is sound (D23).
///
/// Swapping the operands maps every relation to its converse. Something
/// is possible; at most one relation is definite, and then it is the only
/// possibility and is Before, After or Equal. An Unknown bound leaves
/// every relation possible. Definite Before holds exactly when the left
/// value's latest day precedes the right's earliest and nothing is
/// Unknown. Impossible is not-possible; every name and the description
/// are non-empty.
///
/// # Panics
///
/// When the rule is broken.
pub fn check_pair(seen: &PairObservation) {
    let (left, right) = (&seen.left, &seen.right);
    assert_eq!(
        seen.forward, seen.converse,
        "{left} vs {right}: a relation and its converse disagree"
    );
    assert!(
        !seen.possible.is_empty(),
        "{left} vs {right}: no relation is possible"
    );
    let definite: Vec<Relation> = Relation::ALL
        .into_iter()
        .zip(seen.definite_each)
        .filter_map(|(relation, is)| is.then_some(relation))
        .collect();
    assert!(
        definite.len() <= 1,
        "{left} vs {right}: two definite relations"
    );
    assert_eq!(
        seen.definite,
        definite.first().copied(),
        "{left} vs {right}: definite() disagrees with is_definite"
    );
    if let Some(sole) = seen.definite {
        assert_eq!(
            seen.possible,
            [sole],
            "{left} vs {right}: a definite relation must be sole"
        );
        assert!(
            matches!(sole, Relation::Before | Relation::After | Relation::Equal),
            "{left} vs {right}: {sole:?} can never be definite"
        );
    }
    if seen.unknown {
        assert_eq!(
            seen.forward,
            [Modality::Possible; 6],
            "{left} vs {right}: an Unknown bound must leave every relation possible"
        );
    }
    let expected_before = !seen.unknown
        && matches!(
            (seen.left_latest, seen.right_earliest),
            (Bound::Date(hi), Bound::Date(lo)) if hi < lo
        );
    assert_eq!(
        seen.definite_before, expected_before,
        "{left} vs {right}: definite Before disagrees with the bounds"
    );
    for (possible, impossible) in seen.possible_each.iter().zip(seen.impossible_each) {
        assert_eq!(
            impossible, !possible,
            "{left} vs {right}: impossible is not not-possible"
        );
    }
    assert!(
        seen.names
            .iter()
            .all(|&(relation, modality)| !relation.is_empty() && !modality.is_empty()),
        "{left} vs {right}: a relation or modality has no name"
    );
    assert!(
        !seen.description.is_empty(),
        "{left} vs {right}: empty description"
    );
}

// ------------------------------------------------------------ order rule

/// Two dates, how they relate, and the interval between them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OrderObservation {
    /// The first date's canonical text.
    pub start: String,
    /// The second date's canonical text.
    pub end: String,
    /// The first date's bounds.
    pub start_bounds: Bounds,
    /// The second date's bounds.
    pub end_bounds: Bounds,
    /// Whether the first is definitely before the second.
    pub definite_before: bool,
    /// The bounds of `start/end`, when it parses as an interval.
    pub interval: Option<Bounds>,
}

/// Relate two dates and parse the interval from one to the other.
#[must_use]
pub fn observe_order(start: Date, end: Date) -> OrderObservation {
    let (from, to) = (Edtf::Date(start), Edtf::Date(end));
    let interval = match Edtf::parse(&format!("{from}/{to}")) {
        Ok(value @ Edtf::Interval(_)) => Some(value.bounds()),
        Ok(Edtf::Date(_) | Edtf::DateTime(_) | Edtf::Set(_)) | Err(_) => None,
    };
    OrderObservation {
        start: from.to_string(),
        end: to.to_string(),
        start_bounds: from.bounds(),
        end_bounds: to.bounds(),
        definite_before: from.relation(&to).is_definite(Relation::Before),
        interval,
    }
}

/// Order, relation and interval agree (D18, D23).
///
/// `A` is wholly before `B` exactly when the relation is definitely
/// before; `A/B` parses exactly when `A` is not wholly after `B`; and its
/// bounds are `A`'s earliest to `B`'s latest.
///
/// # Panics
///
/// When the rule is broken.
pub fn check_order(seen: &OrderObservation) {
    let (start, end) = (&seen.start, &seen.end);
    let wholly_before = matches!(
        (seen.start_bounds.latest, seen.end_bounds.earliest),
        (Bound::Date(hi), Bound::Date(lo)) if hi < lo
    );
    assert_eq!(
        seen.definite_before, wholly_before,
        "{start} vs {end}: definite Before disagrees with wholly before"
    );
    let wholly_after = matches!(
        (seen.start_bounds.earliest, seen.end_bounds.latest),
        (Bound::Date(lo), Bound::Date(hi)) if lo > hi
    );
    assert_eq!(
        seen.interval.is_some(),
        !wholly_after,
        "{start}/{end}: parses unless the start is wholly after the end"
    );
    if let Some(bounds) = seen.interval {
        assert_eq!(
            (bounds.earliest, bounds.latest),
            (seen.start_bounds.earliest, seen.end_bounds.latest),
            "{start}/{end}: the interval's bounds are not its ends'"
        );
    }
}

// ------------------------------------------------------------------ planted

/// One fabricated bad observation per assertion: each check must panic on
/// it with that assertion's message. A check that stopped asserting, or a
/// mutant of one, fails here.
#[cfg(test)]
mod planted {
    #![expect(
        clippy::missing_panics_doc,
        reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
    )]
    #![expect(clippy::panic, reason = "a panic in a test IS the failure signal")]

    use alloc::{string::String, vec, vec::Vec};

    use super::{
        EnumeratedObservation, ValueObservation, check_bound_day, check_letters, check_order,
        check_pair, check_qualification, check_text, check_value, check_whitespace,
        observe_bound_day, observe_letters, observe_order, observe_pair, observe_qualification,
        observe_text, observe_value, observe_whitespace,
    };
    use crate::{
        Bound, BoundDate, Bounds, Date, Edtf, Modality, ParseError, Relation, Unenumerable,
    };

    /// How much of an enumeration the planted observations take.
    const CAP: usize = 64;

    /// A fixture the parser accepts.
    fn parsed(text: &str) -> Edtf {
        Edtf::parse(text).unwrap_or_else(|e| panic!("fixture {text:?}: {e}"))
    }

    /// A date fixture the parser accepts.
    fn date(text: &str) -> Date {
        match parsed(text) {
            Edtf::Date(d) => d,
            Edtf::DateTime(_) | Edtf::Interval(_) | Edtf::Set(_) => {
                panic!("fixture {text:?} is not a date")
            }
        }
    }

    /// A bound-day fixture.
    fn day(year: i64, month: u8, day_of_month: u8) -> BoundDate {
        BoundDate::new(year, month, day_of_month).unwrap_or_else(|e| panic!("fixture day: {e}"))
    }

    /// A value's observation, enumerating up to the planted cap.
    fn value(text: &str) -> ValueObservation {
        observe_value(&parsed(text), CAP)
    }

    /// A refusal to fabricate with.
    const REFUSED: ParseError = ParseError {
        message: "planted",
        offset: 0,
    };

    /// A day bound.
    fn on(year: i64, month: u8, day_of_month: u8) -> Bound {
        Bound::Date(day(year, month, day_of_month))
    }

    /// The fixtures are honest: every check passes on what it observes.
    #[test]
    fn the_fixtures_pass_every_check() {
        for text in [
            "1985-04-12",
            "1985",
            "/1985",
            "1985/",
            "{1985,1990..1991}",
            "[..1985]",
        ] {
            check_text(&observe_text(text));
            check_value(&value(text));
            check_letters(&observe_letters(text));
            check_whitespace(&observe_whitespace(text));
            check_pair(&observe_pair(&parsed(text), &parsed("1985-04")));
        }
        check_text(&observe_text("nope"));
        check_bound_day(&observe_bound_day(day(1985, 4, 12)));
        check_qualification(&observe_qualification(date("1985-04")));
        check_order(&observe_order(date("1985"), date("1986")));
        check_order(&observe_order(date("1986"), date("1985")));
    }

    // ---------------------------------------------------------- text

    #[test]
    #[should_panic(expected = "FromStr and Edtf::parse disagree")]
    fn text_from_str_must_agree() {
        let mut seen = observe_text("1985");
        seen.from_str = Err(REFUSED);
        check_text(&seen);
    }

    #[test]
    #[should_panic(expected = "is refused with an empty message")]
    fn text_refusal_must_say_why() {
        let mut seen = observe_text("nope");
        seen.refusal = Some(String::new());
        check_text(&seen);
    }

    #[test]
    #[should_panic(expected = "disagrees with Edtf::level")]
    fn text_level_must_agree() {
        let mut seen = observe_text("1985");
        seen.level = Some(1);
        check_text(&seen);
    }

    // --------------------------------------------------------- value

    #[test]
    #[should_panic(expected = "does not reparse to the value")]
    fn value_must_round_trip() {
        let mut seen = value("1985");
        seen.reparsed = Ok(parsed("1986"));
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "is_valid refuses the canonical form")]
    fn value_canonical_form_must_be_valid() {
        let mut seen = value("1985");
        seen.valid = false;
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "classified at level 3")]
    fn value_level_must_be_0_to_2() {
        let mut seen = value("1985");
        seen.level = 3;
        seen.reparsed_level = Some(3);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "changed level on reparse")]
    fn value_level_must_survive_reparse() {
        let mut seen = value("1985");
        seen.reparsed_level = Some(1);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "changed qualification on reparse")]
    fn value_qualification_must_survive_reparse() {
        let mut seen = value("1985");
        seen.reparsed_qualification = Some((true, false));
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "its earliest bound is")]
    fn value_earliest_is_unknown_only_at_an_unknown_start() {
        let mut seen = value("/1985");
        seen.unknown_ends.0 = false;
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "its latest bound is")]
    fn value_latest_is_unknown_only_at_an_unknown_end() {
        let mut seen = value("1985/");
        seen.unknown_ends.1 = false;
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "is after latest")]
    fn value_bounds_must_be_ordered() {
        let mut seen = value("1985");
        seen.bounds = Bounds {
            earliest: on(1985, 12, 31),
            latest: on(1985, 1, 1),
        };
        seen.values = Ok(Vec::new());
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "day order and JDN order disagree")]
    fn value_day_order_must_be_jdn_order() {
        let mut seen = value("1985");
        seen.bound_day_numbers = Some((2, 1));
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "a bound displays empty")]
    fn value_bounds_must_display() {
        let mut seen = value("1985");
        seen.bound_texts = Some((String::new(), String::from("1985-12-31")));
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "values() disagrees with the structure")]
    fn value_enumeration_must_match_the_structure() {
        let mut seen = value("1985");
        seen.values = Err(Unenumerable::Interval);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "unenumerable without a reason")]
    fn value_unenumerable_must_say_why() {
        let mut seen = value("1985/1986");
        seen.unenumerable_reason = Some(String::new());
        check_value(&seen);
    }

    /// An enumerated value fabricated from fixture text.
    fn item(text: &str) -> EnumeratedObservation {
        let value = parsed(text);
        let (masked, bounds) = (value.has_unspecified(), value.bounds());
        EnumeratedObservation {
            text: String::from(text),
            value: value.clone(),
            reparsed: Ok(value),
            masked,
            bounds,
        }
    }

    #[test]
    #[should_panic(expected = "which does not round-trip")]
    fn value_enumerated_must_round_trip() {
        let mut seen = value("1985");
        let mut bad = item("1985");
        bad.reparsed = Err(REFUSED);
        seen.values = Ok(vec![bad]);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "which is still masked")]
    fn value_enumerated_must_be_concrete() {
        let mut seen = value("1985");
        let mut bad = item("1985");
        bad.masked = true;
        seen.values = Ok(vec![bad]);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "before its earliest bound")]
    fn value_enumerated_must_not_precede_the_bounds() {
        let mut seen = value("1985");
        seen.values = Ok(vec![item("1984")]);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "after its latest bound")]
    fn value_enumerated_must_not_follow_the_bounds() {
        let mut seen = value("{1985,1986}");
        seen.values = Ok(vec![item("1987")]);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "a date enumerated nothing")]
    fn value_date_must_enumerate_something() {
        let mut seen = value("1985");
        seen.values = Ok(Vec::new());
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "does not start at the earliest bound")]
    fn value_date_enumeration_must_start_at_the_earliest_bound() {
        let mut seen = value("198X");
        seen.values = Ok(vec![item("1981")]);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "does not ascend from")]
    fn value_date_enumeration_must_ascend() {
        let mut seen = value("198X");
        seen.values = Ok(vec![item("1980"), item("1980")]);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "does not end at the latest bound")]
    fn value_date_enumeration_must_end_at_the_latest_bound() {
        let mut seen = value("198X");
        seen.values = Ok(vec![item("1980"), item("1988")]);
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "Equal is ruled out")]
    fn value_must_possibly_equal_itself() {
        let mut seen = value("1985");
        seen.self_equal_possible = false;
        check_value(&seen);
    }

    #[test]
    #[should_panic(expected = "a single day is not definitely equal to itself")]
    fn value_single_day_must_equal_itself() {
        let mut seen = value("1985-04-12");
        seen.self_definite = None;
        check_value(&seen);
    }

    // ------------------------------------------------------ bound day

    #[test]
    #[should_panic(expected = "does not follow it")]
    fn bound_day_next_must_follow() {
        let mut seen = observe_bound_day(day(1985, 4, 12));
        seen.next = Some((day(1985, 4, 13), seen.number));
        check_bound_day(&seen);
    }

    #[test]
    #[should_panic(expected = "does not precede it")]
    fn bound_day_previous_must_precede() {
        let mut seen = observe_bound_day(day(1985, 4, 12));
        seen.previous = Some((day(1985, 4, 11), seen.number));
        check_bound_day(&seen);
    }

    // ------------------------------------------- letters, whitespace

    #[test]
    #[should_panic(expected = "lower-case letters accepted")]
    fn letters_must_be_upper_case() {
        let mut seen = observe_letters("198X");
        seen.accepted.push(String::from("198x"));
        check_letters(&seen);
    }

    #[test]
    #[should_panic(expected = "whitespace accepted")]
    fn whitespace_must_be_refused() {
        let mut seen = observe_whitespace("1985");
        seen.accepted.push(String::from("1985 "));
        check_whitespace(&seen);
    }

    // ---------------------------------------------- qualification

    #[test]
    #[should_panic(expected = "qualification moved its bounds")]
    fn qualification_must_not_move_bounds() {
        let mut seen = observe_qualification(date("1985"));
        seen.qualified[1].latest = on(1986, 1, 1);
        check_qualification(&seen);
    }

    // ------------------------------------------------------- pair

    /// A pair's observation.
    fn pair(left: &str, right: &str) -> super::PairObservation {
        observe_pair(&parsed(left), &parsed(right))
    }

    #[test]
    #[should_panic(expected = "a relation and its converse disagree")]
    fn pair_must_be_converse_symmetric() {
        let mut seen = pair("1985", "1990");
        seen.converse[0] = Modality::Impossible;
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "no relation is possible")]
    fn pair_must_admit_something() {
        let mut seen = pair("1985", "1990");
        seen.possible = Vec::new();
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "two definite relations")]
    fn pair_must_have_at_most_one_definite() {
        let mut seen = pair("1985", "1990");
        seen.definite_each = [true, true, false, false, false, false];
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "definite() disagrees with is_definite")]
    fn pair_definite_must_agree() {
        let mut seen = pair("1985", "1990");
        seen.definite = None;
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "a definite relation must be sole")]
    fn pair_definite_must_be_sole() {
        let mut seen = pair("1985", "1990");
        seen.possible.push(Relation::Overlaps);
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "can never be definite")]
    fn pair_only_before_after_equal_are_definite() {
        let mut seen = pair("1985", "1990");
        let contains = Relation::ALL
            .iter()
            .position(|r| *r == Relation::Contains)
            .unwrap_or_default();
        seen.definite_each = [false; 6];
        if let Some(slot) = seen.definite_each.get_mut(contains) {
            *slot = true;
        }
        seen.definite = Some(Relation::Contains);
        seen.possible = vec![Relation::Contains];
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "an Unknown bound must leave every relation possible")]
    fn pair_unknown_must_leave_everything_possible() {
        let mut seen = pair("1985/", "1990");
        seen.forward[0] = Modality::Impossible;
        seen.converse[0] = Modality::Impossible;
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "definite Before disagrees with the bounds")]
    fn pair_definite_before_must_match_the_bounds() {
        let mut seen = pair("1985", "1990");
        seen.right_earliest = on(1980, 1, 1);
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "impossible is not not-possible")]
    fn pair_impossible_must_be_not_possible() {
        let mut seen = pair("1985", "1990");
        seen.impossible_each[1] = false;
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "has no name")]
    fn pair_names_must_not_be_empty() {
        let mut seen = pair("1985", "1990");
        seen.names[0] = ("", "possible");
        check_pair(&seen);
    }

    #[test]
    #[should_panic(expected = "empty description")]
    fn pair_description_must_not_be_empty() {
        let mut seen = pair("1985", "1990");
        seen.description = String::new();
        check_pair(&seen);
    }

    // ------------------------------------------------------ order

    #[test]
    #[should_panic(expected = "definite Before disagrees with wholly before")]
    fn order_definite_before_must_be_wholly_before() {
        let mut seen = observe_order(date("1985"), date("1990"));
        seen.definite_before = false;
        check_order(&seen);
    }

    #[test]
    #[should_panic(expected = "parses unless the start is wholly after the end")]
    fn order_interval_must_parse_unless_wholly_after() {
        let mut seen = observe_order(date("1985"), date("1990"));
        seen.interval = None;
        check_order(&seen);
    }

    #[test]
    #[should_panic(expected = "the interval's bounds are not its ends'")]
    fn order_interval_bounds_must_be_its_ends() {
        let mut seen = observe_order(date("1985"), date("1990"));
        seen.interval = Some(Bounds {
            earliest: on(1985, 1, 1),
            latest: on(1985, 12, 31),
        });
        check_order(&seen);
    }
}
