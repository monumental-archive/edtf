// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The normaliser's general rules, each asserted once (#368).
//!
//! Each rule is two functions: an *observation* (`observe_*`) that calls
//! the normaliser and records what it returned, and a *check* (`check_*`)
//! that is pure and asserts over the record. The proptest generators in
//! this module's tests and the `normalize` fuzz target drive the same
//! checks, and each assertion of each check is itself tested with one
//! fabricated bad observation.
//!
//! Compositions are built from the language tables: every range pair, "or"
//! word and before/after phrase of every language in [`LANGUAGES`], so a new
//! row or a new language is covered without a new test.
//!
//! The rules, as `docs/normalize-notes.md` states them:
//!
//! 1. Range forms agree ([`check_range_forms_agree`]).
//! 2. "Or" composes ([`check_or_composes`], N14).
//! 3. Before and after compose, with as many readings as there are
//!    distinct answers among the operand's readings ([`check_open_composes`],
//!    N8).
//! 4. Letter case never changes an answer ([`check_case`], N18).
//! 5. Every emitted string re-reads as itself ([`check_idempotent`], N13).
//! 6. Whitespace never changes an answer ([`check_whitespace`]).
//! 7. An option changes only what it resolves, or a refusal for an
//!    impossible date ([`check_options`], N5, N6).
//! 8. No masked year beside a stated one ([`check_no_masked_year`], N9).
//! 9. Readings are honest ([`check_honest`]).

use alloc::{
    format,
    string::{String, ToString as _},
    vec,
    vec::Vec,
};

use edtf_core::{Date, DateField, Edtf, IntervalEndpoint, Set, SetElement, SetKind, YearKind};

use crate::{
    Century, Language, NoMatchReason, Note, NumericOrder, Options, Outcome, normalize_with,
    tables::lang_for,
};

#[cfg(test)]
mod proptests;

/// Every language the normaliser has tables for.
pub const LANGUAGES: [Language; 2] = [Language::English, Language::Russian];

/// Where `language` stands in [`LANGUAGES`]. The match is exhaustive, so a
/// new language does not compile until it is listed there.
#[must_use]
pub const fn position(language: Language) -> usize {
    match language {
        Language::English => 0,
        Language::Russian => 1,
    }
}

/// What an outcome says, without its notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The readings' EDTF strings in order: one for `Normalized`.
    Read(Vec<String>),
    /// `NoMatch`, with its reason.
    Refused(NoMatchReason),
}

impl Answer {
    /// The answer an outcome gives.
    #[must_use]
    pub fn of(outcome: &Outcome) -> Self {
        match outcome.clone() {
            Outcome::Normalized(normalized) => Self::Read(vec![normalized.edtf]),
            Outcome::Ambiguous(ambiguous) => Self::Read(
                ambiguous
                    .interpretations
                    .into_iter()
                    .map(|interpretation| interpretation.edtf)
                    .collect(),
            ),
            Outcome::NoMatch { reason } => Self::Refused(reason),
        }
    }

    /// The EDTF strings read: none for a refusal.
    #[must_use]
    pub fn strings(self) -> Vec<String> {
        match self {
            Self::Read(strings) => strings,
            Self::Refused(_) => Vec::new(),
        }
    }
}

/// Inputs that were read, each with the answer it gave.
pub type Forms = Vec<(String, Answer)>;

/// The default options for `language`.
#[must_use]
pub fn options(language: Language) -> Options {
    Options {
        language,
        ..Options::default()
    }
}

/// Every option combination the normaliser reads, in every language.
#[must_use]
pub fn every_options() -> Vec<Options> {
    let mut all = Vec::new();
    for language in LANGUAGES {
        for numeric_order in [
            None,
            Some(NumericOrder::DayFirst),
            Some(NumericOrder::MonthFirst),
        ] {
            for default_century in [None, Century::new(1900)] {
                all.push(Options {
                    language,
                    numeric_order,
                    default_century,
                });
            }
        }
    }
    all
}

/// Each reading of an outcome: its value and its notes.
fn readings(outcome: &Outcome) -> Vec<(Edtf, Vec<Note>)> {
    match outcome.clone() {
        Outcome::Normalized(normalized) => vec![(normalized.value, normalized.notes)],
        Outcome::Ambiguous(ambiguous) => ambiguous
            .interpretations
            .into_iter()
            .map(|interpretation| (interpretation.value, interpretation.notes))
            .collect(),
        Outcome::NoMatch { reason: _ } => Vec::new(),
    }
}

/// Read every input under `options`.
fn observe_all(inputs: Vec<String>, options: Options) -> Forms {
    inputs
        .into_iter()
        .map(|input| {
            let answer = Answer::of(&normalize_with(&input, options));
            (input, answer)
        })
        .collect()
}

/// Whether an EDTF string carries no qualifier.
fn unqualified(edtf: &str) -> bool {
    !edtf.contains(['?', '~', '%'])
}

/// Whether a date's year is `XXXX` (N9).
const fn year_less(date: &Date) -> bool {
    matches!(
        date.year().kind(),
        YearKind::Standard {
            negative: _,
            digits: [None, None, None, None],
        }
    )
}

/// Whether a date is N9's year-less date: a month under an `XXXX` year.
const fn month_without_year(date: &Date) -> bool {
    year_less(date) && date.month().is_some()
}

/// Whether a date names a season (ISO 8601-2 4.8.1: codes 21-24 and up).
fn is_season(date: &Date) -> bool {
    date.month()
        .and_then(DateField::value)
        .is_some_and(|month| month > 12)
}

/// The date an interval endpoint carries, if any.
const fn endpoint_date(endpoint: IntervalEndpoint) -> Option<Date> {
    match endpoint {
        IntervalEndpoint::Date(date)
        | IntervalEndpoint::OnOrBefore(date)
        | IntervalEndpoint::OnOrAfter(date) => Some(date),
        IntervalEndpoint::Open | IntervalEndpoint::Unknown => None,
    }
}

// ---------------------------------------------------------------------------
// Compositions, from the language tables

/// Every two-sided range form of `left` and `right`: each range pair of the
/// language, and the hyphen, en dash and em dash, spaced and unspaced.
#[must_use]
pub fn range_forms(left: &str, right: &str, language: Language) -> Vec<String> {
    let mut forms: Vec<String> = lang_for(language)
        .range_pairs
        .iter()
        .map(|&(lead, sep)| {
            lead.map_or_else(
                || format!("{left} {sep} {right}"),
                |word| format!("{word} {left} {sep} {right}"),
            )
        })
        .collect();
    for dash in ['-', '\u{2013}', '\u{2014}'] {
        forms.push(format!("{left}{dash}{right}"));
        forms.push(format!("{left} {dash} {right}"));
    }
    forms
}

/// "`left` or `right`", once per "or" word of the language.
#[must_use]
pub fn or_forms(left: &str, right: &str, language: Language) -> Vec<String> {
    lang_for(language)
        .or_words
        .iter()
        .map(|word| format!("{left} {word} {right}"))
        .collect()
}

/// `operand` after each before/after phrase of the language.
#[must_use]
pub fn open_forms(operand: &str, language: Language) -> Vec<String> {
    lang_for(language)
        .open_phrases
        .iter()
        .map(|&(phrase, _)| format!("{} {operand}", phrase.join(" ")))
        .collect()
}

/// Assert every form gave the first form's answer.
///
/// # Panics
///
/// When two forms disagree.
fn check_agree(rule: &str, forms: &[(String, Answer)]) {
    if let Some(first) = forms.first() {
        for form in forms {
            assert_eq!(
                form.1, first.1,
                "{rule}: {:?} gives {:?}, {:?} gives {:?}",
                form.0, form.1, first.0, first.1
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 1. Range forms agree

/// Whether `input` reads alone as one unqualified date.
fn one_date(input: &str, language: Language) -> bool {
    match normalize_with(input, options(language)) {
        Outcome::Normalized(normalized) => {
            matches!(normalized.value, Edtf::Date(_)) && unqualified(&normalized.edtf)
        }
        Outcome::Ambiguous(_) | Outcome::NoMatch { reason: _ } => false,
    }
}

/// `token` with its EDTF letters written as EDTF writes them (N18): in
/// upper case, and each of the language's lookalike letters as the letter
/// it stands for, from the table row (`edtf_letters`).
fn edtf_letters(token: &str, language: Language) -> String {
    let lookalikes = lang_for(language).edtf_letters;
    token
        .chars()
        .map(|letter| {
            lookalikes
                .iter()
                .find(|&&(lookalike, _)| lookalike == letter)
                .map_or(letter, |&(_, edtf)| edtf)
        })
        .flat_map(char::to_uppercase)
        .collect()
}

/// Whether core reads `token`, its EDTF letters folded (N18), as a date
/// with a month, season or sub-year code: one unit, whose hyphens are its
/// own (N4, ISO 8601-1 3.2.6).
fn one_unit(token: &str, language: Language) -> bool {
    matches!(
        Edtf::parse(&edtf_letters(token, language)),
        Ok(Edtf::Date(date)) if date.month().is_some()
    )
}

/// Whether joining `left` and `right` by a hyphen writes an ISO date.
///
/// Core reads the token formed around the dash (`left`'s last token, the
/// hyphen, `right`'s first), its EDTF letters folded as N18 reads them, as
/// a date with a month, season or sub-year code: "1985-ХХ" is `1985-XX`.
///
/// That join is a different text (#397, ISO 8601-1 3.2.6), so the unspaced
/// forms are not compared. It is stated through core's parser and the
/// language's table, not the normaliser's reader, so the rule stays
/// independent of the code it tests.
fn joins_into_a_date(left: &str, right: &str, language: Language) -> bool {
    let token = format!(
        "{}-{}",
        left.split_whitespace().last().unwrap_or_default(),
        right.split_whitespace().next().unwrap_or_default()
    );
    one_unit(&token, language)
}

/// Whether `token` holds a range candidate of its own (N4).
///
/// That is a dash not inside one unit that does not open one of the
/// language's number suffixes (an `ordinal_suffixes` or `decade_suffixes`
/// entry starting with `-`, at the token's end).
///
/// Joined unspaced to the other side, such a token gives the text a
/// second candidate hyphen, and N4 reads every split of it that reads:
/// "1914-9-1989" is `1914/1989-09` or `1914-09/1989`. That join is a
/// different text from "from 1914-9 to 1989", so the unspaced forms are
/// not compared (#419).
fn holds_its_own_hyphen(token: &str, language: Language) -> bool {
    let lang = lang_for(language);
    let dashed = token.replace(['\u{2013}', '\u{2014}'], "-").to_lowercase();
    let bare = lang
        .ordinal_suffixes
        .iter()
        .chain(lang.decade_suffixes)
        .filter(|suffix| suffix.starts_with('-'))
        .find_map(|suffix| dashed.strip_suffix(suffix))
        .unwrap_or(&dashed);
    bare.contains('-') && !one_unit(bare, language)
}

/// Whether the unspaced join of `left` and `right` is another text.
///
/// It is when the join writes an ISO date across the dash
/// ([`joins_into_a_date`]), or a side's token at the join holds a range
/// candidate of its own ([`holds_its_own_hyphen`]); the spaced and worded
/// forms are then a different text from it.
fn unspaced_is_another_text(left: &str, right: &str, language: Language) -> bool {
    joins_into_a_date(left, right, language)
        || [
            left.split_whitespace().last(),
            right.split_whitespace().next(),
        ]
        .into_iter()
        .flatten()
        .any(|token| holds_its_own_hyphen(token, language))
}

/// Every range form of `left` and `right`, read.
///
/// `None` unless each reads alone as one unqualified date. The range words
/// and the spaced dashes are always read; the unspaced dashes only when the
/// join is the same text ([`unspaced_is_another_text`]).
#[must_use]
pub fn observe_range_forms(left: &str, right: &str, language: Language) -> Option<Forms> {
    let fused = unspaced_is_another_text(left, right, language);
    (one_date(left, language) && one_date(right, language)).then(|| {
        let forms = range_forms(left, right, language)
            .into_iter()
            .filter(|form| !(fused && is_unspaced(form, left, right)))
            .collect();
        observe_all(forms, options(language))
    })
}

/// Whether `form` is `left` and `right` joined by one dash, unspaced.
fn is_unspaced(form: &str, left: &str, right: &str) -> bool {
    form.strip_prefix(left)
        .and_then(|rest| rest.strip_suffix(right))
        .is_some_and(|dash| matches!(dash, "-" | "\u{2013}" | "\u{2014}"))
}

/// Rule 1: every range form of two dates gives the same outcome; an
/// unspaced form only where the join writes no ISO date across the dash
/// and adds the only range candidate to the text.
///
/// # Panics
///
/// When two forms disagree.
pub fn check_range_forms_agree(forms: &[(String, Answer)]) {
    check_agree("range forms disagree", forms);
}

// ---------------------------------------------------------------------------
// 2. "Or" composes (N14)

/// What "A or B" was expected to give, and what each form gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrObservation {
    /// The one-of set of A's and B's elements in written order, each
    /// written once; a set of one date is that date.
    pub expected: String,
    /// Each "or" form, read.
    pub forms: Forms,
}

/// The elements a one-of set holding `input`'s one reading would take,
/// when it reads alone as one unqualified reading that states its year: no
/// date in it is N9's year-less date, a month under `XXXX`.
fn or_elements(input: &str, language: Language) -> Option<Vec<SetElement>> {
    let Outcome::Normalized(normalized) = normalize_with(input, options(language)) else {
        return None;
    };
    if !unqualified(&normalized.edtf) {
        return None;
    }
    let elements = match normalized.value {
        Edtf::Date(date) => vec![SetElement::Date(date)],
        Edtf::Set(set) if set.kind() == SetKind::OneMember => set.elements().to_vec(),
        Edtf::Set(_) | Edtf::Interval(_) | Edtf::DateTime(_) => return None,
    };
    elements
        .iter()
        .all(|element| !element_date(element).is_some_and(|date| month_without_year(&date)))
        .then_some(elements)
}

/// The date a set element names that could be year-less. A set range
/// cannot hold unspecified digits (core refuses them), so it has none.
const fn element_date(element: &SetElement) -> Option<Date> {
    match *element {
        SetElement::Date(date) | SetElement::OnOrBefore(date) | SetElement::OnOrAfter(date) => {
            Some(date)
        }
        SetElement::Range(_) => None,
    }
}

/// Every "or" form of `left` and `right`, read, with the set they should
/// give, when each reads alone as one reading a one-of set can hold and
/// states its year; `None` otherwise.
#[must_use]
pub fn observe_or(left: &str, right: &str, language: Language) -> Option<OrObservation> {
    let mut elements: Vec<SetElement> = Vec::new();
    for side in [left, right] {
        for element in or_elements(side, language)? {
            if !elements.contains(&element) {
                elements.push(element);
            }
        }
    }
    let expected = match *elements.as_slice() {
        [SetElement::Date(date)] => Edtf::Date(date),
        _ => Edtf::Set(Set::new(SetKind::OneMember, elements).ok()?),
    };
    Some(OrObservation {
        expected: expected.to_string(),
        forms: observe_all(or_forms(left, right, language), options(language)),
    })
}

/// Rule 2: "A or B" is the one-of set of A's and B's elements.
///
/// # Panics
///
/// When a form gives anything else.
pub fn check_or_composes(observation: &OrObservation) {
    let expected = Answer::Read(vec![observation.expected.clone()]);
    for form in &observation.forms {
        assert_eq!(
            form.1, expected,
            "\"or\" does not compose: {:?} gives {:?}",
            form.0, form.1
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Before and after compose (N8)

/// One before/after form of an operand: what it gave, and how many
/// distinct answers the same phrase gives over the operand's readings, each
/// given back as its EDTF string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenForm {
    /// The form, `phrase operand`.
    pub input: String,
    /// What it gave.
    pub answer: Answer,
    /// The distinct answers among `phrase reading`, over the operand's
    /// readings.
    pub distinct: usize,
}

/// Each before/after form of an operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenObservation {
    /// One per row of the language's before/after phrases.
    pub forms: Vec<OpenForm>,
}

/// Whether a date can be a before/after operand's edge: it states a year
/// and is not a season, which has no predecessor (N8, N9).
fn dated_edge(date: &Date) -> bool {
    !year_less(date) && !is_season(date)
}

/// Whether a reading is a before/after operand: a dated date that is not a
/// season, or a span (an interval with two dated ends, or a one-element
/// one-of set).
fn is_operand(value: &Edtf) -> bool {
    match value.clone() {
        Edtf::Date(date) => dated_edge(&date),
        Edtf::Interval(interval) => match (interval.start(), interval.end()) {
            (IntervalEndpoint::Date(start), IntervalEndpoint::Date(end)) => {
                dated_edge(&start) && dated_edge(&end)
            }
            _ => false,
        },
        Edtf::Set(set) => {
            set.kind() == SetKind::OneMember
                && match *set.elements() {
                    [SetElement::Date(date)] => dated_edge(&date),
                    [SetElement::Range(range)] => {
                        dated_edge(&range.start()) && dated_edge(&range.end())
                    }
                    _ => false,
                }
        }
        Edtf::DateTime(_) => false,
    }
}

/// Every before/after form of `operand`, read.
///
/// Each form carries the distinct answers its phrase gives over the
/// operand's readings. `None` unless every reading of the operand alone is
/// a date that is not a season, or a span.
#[must_use]
pub fn observe_open(operand: &str, language: Language) -> Option<OpenObservation> {
    let opts = options(language);
    let outcome = normalize_with(operand, opts);
    if !readings(&outcome)
        .into_iter()
        .all(|(value, _)| is_operand(&value))
    {
        return None;
    }
    let alone = Answer::of(&outcome).strings();
    if alone.is_empty() {
        return None;
    }
    let forms = lang_for(language)
        .open_phrases
        .iter()
        .map(|&(words, _)| {
            let phrase = words.join(" ");
            let mut answers: Vec<Answer> = Vec::new();
            for reading in &alone {
                let answer = Answer::of(&normalize_with(&format!("{phrase} {reading}"), opts));
                if !answers.contains(&answer) {
                    answers.push(answer);
                }
            }
            let input = format!("{phrase} {operand}");
            let answer = Answer::of(&normalize_with(&input, opts));
            OpenForm {
                input,
                answer,
                distinct: answers.len(),
            }
        })
        .collect();
    Some(OpenObservation { forms })
}

/// Rule 3: each before/after meaning reads, with as many readings as there
/// are distinct answers among the operand's readings.
///
/// # Panics
///
/// When a form is refused or reads with another number of readings.
pub fn check_open_composes(observation: &OpenObservation) {
    for form in &observation.forms {
        assert!(
            form.answer.clone().strings().len() == form.distinct,
            "before/after does not compose: {:?} gives {:?}, not {} readings",
            form.input,
            form.answer,
            form.distinct
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Case (N18)

/// `input` as typed, upper-cased and lower-cased, read.
#[must_use]
pub fn observe_case(input: &str, options: Options) -> Forms {
    observe_all(
        vec![
            String::from(input),
            input.to_uppercase(),
            input.to_lowercase(),
        ],
        options,
    )
}

/// Rule 4: letter case never changes an answer.
///
/// # Panics
///
/// When two cases disagree.
pub fn check_case(forms: &[(String, Answer)]) {
    check_agree("case changes the answer", forms);
}

// ---------------------------------------------------------------------------
// 5. Idempotence (N13)

/// One emitted string, given back as input in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reread {
    /// The EDTF string the normaliser emitted.
    pub emitted: String,
    /// The language it was given back in.
    pub language: Language,
    /// What it gave.
    pub answer: Answer,
    /// Whether that answer is an N13 sub-year collision.
    pub collision: bool,
}

/// Every string `input` emits under `options`, given back as input in every
/// language.
#[must_use]
pub fn observe_reread(input: &str, options: Options) -> Vec<Reread> {
    let mut rereads = Vec::new();
    for emitted in Answer::of(&normalize_with(input, options)).strings() {
        for language in LANGUAGES {
            let outcome = normalize_with(&emitted, self::options(language));
            let collision = matches!(outcome, Outcome::Ambiguous(_))
                && readings(&outcome)
                    .into_iter()
                    .any(|(_, notes)| notes.contains(&Note::SeasonRangeCollision));
            rereads.push(Reread {
                emitted: emitted.clone(),
                language,
                answer: Answer::of(&outcome),
                collision,
            });
        }
    }
    rereads
}

/// Rule 5: every emitted string is normalised to itself, or, for an N13
/// collision, is among the readings.
///
/// # Panics
///
/// When an emitted string reads as anything else.
pub fn check_idempotent(rereads: &[Reread]) {
    for reread in rereads {
        let itself = reread.answer == Answer::Read(vec![reread.emitted.clone()]);
        let among = reread.collision && reread.answer.clone().strings().contains(&reread.emitted);
        assert!(
            itself || among,
            "emitted {:?} re-reads in {:?} as {:?}",
            reread.emitted,
            reread.language,
            reread.answer
        );
    }
}

// ---------------------------------------------------------------------------
// 6. Whitespace

/// `input` as typed, padded, with doubled spaces, a tab for each space and
/// U+00A0 for each space, read.
#[must_use]
pub fn observe_whitespace(input: &str, options: Options) -> Forms {
    observe_all(
        vec![
            String::from(input),
            format!("  {input}\t "),
            input.replace(' ', "  "),
            input.replace(' ', "\t"),
            input.replace(' ', "\u{a0}"),
        ],
        options,
    )
}

/// Rule 6: whitespace never changes an answer.
///
/// # Panics
///
/// When two spacings disagree.
pub fn check_whitespace(forms: &[(String, Answer)]) {
    check_agree("whitespace changes the answer", forms);
}

// ---------------------------------------------------------------------------
// 7. Options (N5, N6)

/// One option set away from the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// [`Options::numeric_order`].
    NumericOrder(NumericOrder),
    /// [`Options::default_century`].
    DefaultCentury(Century),
}

impl Choice {
    /// The notes of a default outcome this option exists to resolve.
    #[must_use]
    pub const fn resolves(self) -> &'static [Note] {
        match self {
            Self::NumericOrder(_) => &[Note::NumericOrderAmbiguous, Note::NumericResolvedByLocale],
            Self::DefaultCentury(_) => &[Note::DecadeAmbiguity],
        }
    }

    /// `options` with this option set.
    #[must_use]
    pub const fn apply(self, options: Options) -> Options {
        match self {
            Self::NumericOrder(order) => Options {
                numeric_order: Some(order),
                ..options
            },
            Self::DefaultCentury(century) => Options {
                default_century: Some(century),
                ..options
            },
        }
    }
}

/// An input's default answer and notes, and its answer under each option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsObservation {
    /// The input.
    pub input: String,
    /// Its answer under the default options.
    pub default: Answer,
    /// Every note of every reading of the default outcome.
    pub notes: Vec<Note>,
    /// Its answer under each single option.
    pub chosen: Vec<(Choice, Answer)>,
}

/// `input` under the default options of `language`, and under each numeric
/// order and `century`, one at a time.
#[must_use]
pub fn observe_options(input: &str, language: Language, century: Century) -> OptionsObservation {
    let default = normalize_with(input, options(language));
    let notes = readings(&default)
        .into_iter()
        .flat_map(|(_, notes)| notes)
        .collect();
    let chosen = [
        Choice::NumericOrder(NumericOrder::DayFirst),
        Choice::NumericOrder(NumericOrder::MonthFirst),
        Choice::DefaultCentury(century),
    ]
    .into_iter()
    .map(|choice| {
        let answer = Answer::of(&normalize_with(input, choice.apply(options(language))));
        (choice, answer)
    })
    .collect();
    OptionsObservation {
        input: String::from(input),
        default: Answer::of(&default),
        notes,
        chosen,
    }
}

/// Rule 7: an option changes only what it resolves (N5, N6).
///
/// An option may change an outcome only when the default is `NoMatch
/// ImpossibleDate` (the readings that failed are not visible), or when the
/// default carries the note that option resolves and the option's outcome
/// is not `NoMatch OutOfGrammar`.
///
/// # Panics
///
/// When an option changes any other outcome.
pub fn check_options(observation: &OptionsObservation) {
    for chosen in &observation.chosen {
        let (choice, answer) = (chosen.0, &chosen.1);
        assert!(
            *answer == observation.default
                || observation.default == Answer::Refused(NoMatchReason::ImpossibleDate)
                || *answer != Answer::Refused(NoMatchReason::OutOfGrammar)
                    && choice
                        .resolves()
                        .iter()
                        .any(|n| observation.notes.contains(n)),
            "{choice:?} changes {:?} from {:?} to {answer:?}, which carries none of {:?}",
            observation.input,
            observation.default,
            choice.resolves()
        );
    }
}

// ---------------------------------------------------------------------------
// 8. No masked year beside a stated one (N9)

/// The value of every reading of `input` under `options`.
#[must_use]
pub fn observe_values(input: &str, options: Options) -> Vec<Edtf> {
    readings(&normalize_with(input, options))
        .into_iter()
        .map(|(value, _)| value)
        .collect()
}

/// Rule 8: no reading is an interval with a year-less date at one end and a
/// stated year at the other.
///
/// A year-less date is N9's: a month under an `XXXX` year. A bare `XXXX`
/// says "year unknown", so `XXXX/1985` is honest.
///
/// # Panics
///
/// When one is.
pub fn check_no_masked_year(values: &[Edtf]) {
    for value in values {
        if let Edtf::Interval(interval) = value.clone() {
            if let (Some(start), Some(end)) = (
                endpoint_date(interval.start()),
                endpoint_date(interval.end()),
            ) {
                assert!(
                    (!month_without_year(&start) || year_less(&end))
                        && (!month_without_year(&end) || year_less(&start)),
                    "{value} has a month under XXXX at one end and a stated year at the other"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 9. Readings are honest

/// One reading as emitted, and what core makes of its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emitted {
    /// The EDTF string emitted.
    pub edtf: String,
    /// The value reported with it.
    pub value: Edtf,
    /// The string, parsed by core.
    pub reparsed: Option<Edtf>,
    /// The value, written by core.
    pub canonical: String,
}

/// What `input` gave under `options`, and what core makes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Honesty {
    /// The input.
    pub input: String,
    /// The input parsed as EDTF by core, if it is valid EDTF.
    pub valid: Option<Edtf>,
    /// Whether the outcome was `Ambiguous`.
    pub ambiguous: bool,
    /// Whether the outcome was `NoMatch`.
    pub refused: bool,
    /// Every reading.
    pub readings: Vec<Emitted>,
}

/// `input` under `options`, with each reading reparsed and rewritten by
/// core.
#[must_use]
pub fn observe_honest(input: &str, options: Options) -> Honesty {
    let outcome = normalize_with(input, options);
    let strings = Answer::of(&outcome).strings();
    let readings = strings
        .into_iter()
        .zip(readings(&outcome))
        .map(|(edtf, (value, _))| {
            let reparsed = Edtf::parse(&edtf).ok();
            let canonical = value.to_string();
            Emitted {
                edtf,
                value,
                reparsed,
                canonical,
            }
        })
        .collect();
    Honesty {
        input: String::from(input),
        valid: Edtf::parse(input).ok(),
        ambiguous: matches!(outcome, Outcome::Ambiguous(_)),
        refused: matches!(outcome, Outcome::NoMatch { reason: _ }),
        readings,
    }
}

/// Rule 9: every reading is canonical, reparses to its value, and no two
/// are equal; an ambiguity has two readings or more; and valid EDTF is read
/// as itself, or, for an N13 collision, among the readings.
///
/// # Panics
///
/// When any of those fails.
pub fn check_honest(observation: &Honesty) {
    let input = &observation.input;
    for (index, reading) in observation.readings.iter().enumerate() {
        assert_eq!(
            reading.reparsed.as_ref(),
            Some(&reading.value),
            "{input:?}: emitted {:?} does not reparse to its value",
            reading.edtf
        );
        assert_eq!(
            reading.canonical, reading.edtf,
            "{input:?}: emitted {:?} is not canonical",
            reading.edtf
        );
        assert!(
            observation
                .readings
                .iter()
                .take(index)
                .all(|earlier| earlier.value != reading.value),
            "{input:?}: reading {:?} reported twice",
            reading.edtf
        );
    }
    assert!(
        !observation.ambiguous || observation.readings.len() >= 2,
        "{input:?}: ambiguous with fewer than two readings"
    );
    if let Some(valid) = observation.valid.as_ref() {
        assert!(
            !observation.refused,
            "{input:?}: valid EDTF was not matched"
        );
        assert!(
            observation.ambiguous
                || observation
                    .readings
                    .iter()
                    .all(|reading| reading.value == *valid),
            "{input:?}: valid EDTF was read as another value"
        );
        assert!(
            observation
                .readings
                .iter()
                .any(|reading| reading.value == *valid),
            "{input:?}: valid EDTF was not among its readings"
        );
    }
}

#[cfg(test)]
mod planted {
    //! Each check's assertions, pinned: one fabricated bad observation per
    //! assertion, and a test that each observation observes.

    #![expect(
        clippy::missing_panics_doc,
        reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
    )]
    #![expect(clippy::panic, reason = "a panic in a test IS the failure signal")]

    extern crate std;

    use alloc::{string::String, vec, vec::Vec};

    use edtf_core::Edtf;

    use super::*;
    use crate::normalize;

    fn en() -> Options {
        options(Language::English)
    }

    fn ru() -> Options {
        options(Language::Russian)
    }

    fn read(strings: &[&str]) -> Answer {
        Answer::Read(strings.iter().copied().map(String::from).collect())
    }

    // -----------------------------------------------------------------------
    // The observations observe

    #[test]
    fn every_language_is_listed() {
        for (index, language) in LANGUAGES.into_iter().enumerate() {
            assert_eq!(position(language), index, "{language:?}");
        }
    }

    #[test]
    fn every_option_combination_is_listed() {
        let all = every_options();
        assert_eq!(all.len(), 12, "two languages, three orders, two centuries");
        assert!(all.iter().any(|opts| opts.language == Language::Russian
            && opts.numeric_order == Some(NumericOrder::MonthFirst)
            && opts.default_century == Century::new(1900)));
    }

    #[test]
    fn answers_name_every_reading() {
        assert_eq!(Answer::of(&normalize("1914")), read(&["1914"]));
        assert_eq!(
            Answer::of(&normalize("12/04/1985")),
            read(&["1985-04-12", "1985-12-04"])
        );
        assert_eq!(
            Answer::of(&normalize("unknown")),
            Answer::Refused(NoMatchReason::ExplicitNoDate)
        );
        assert_eq!(
            Answer::Refused(NoMatchReason::OutOfGrammar).strings(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn compositions_come_from_the_tables() {
        assert_eq!(
            range_forms("1914", "1920", Language::English),
            [
                "from 1914 to 1920",
                "between 1914 and 1920",
                "1914 to 1920",
                "1914 through 1920",
                "1914-1920",
                "1914 - 1920",
                "1914\u{2013}1920",
                "1914 \u{2013} 1920",
                "1914\u{2014}1920",
                "1914 \u{2014} 1920",
            ]
        );
        assert_eq!(
            or_forms("1914", "1920", Language::English),
            ["1914 or 1920"]
        );
        assert_eq!(
            open_forms("1914", Language::English),
            [
                "no later than 1914",
                "no earlier than 1914",
                "earlier than 1914",
                "later than 1914",
                "before 1914",
                "after 1914",
                "until 1914",
                "since 1914",
                "by 1914",
            ]
        );
    }

    #[test]
    fn an_unspaced_join_that_writes_a_date_is_not_compared() {
        let en = Language::English;
        assert!(joins_into_a_date("1000", "12 january 1914", en));
        assert!(joins_into_a_date("from 00XX", "21", en), "a season code");
        assert!(
            !joins_into_a_date("1914", "1920", en),
            "an interval, not a date"
        );
        assert!(
            !joins_into_a_date("1000", "13 january 1914", en),
            "no month 13"
        );
        assert!(!joins_into_a_date("1000", "january 1914", en));
        let forms: Vec<String> = observe_range_forms("1000", "12 january 1914", Language::English)
            .into_iter()
            .flatten()
            .map(|form| form.0)
            .collect();
        assert_eq!(
            forms,
            [
                "from 1000 to 12 january 1914",
                "between 1000 and 12 january 1914",
                "1000 to 12 january 1914",
                "1000 through 12 january 1914",
                "1000 - 12 january 1914",
                "1000 \u{2013} 12 january 1914",
                "1000 \u{2014} 12 january 1914",
            ]
        );
        assert!(is_unspaced("1914\u{2014}1920", "1914", "1920"));
        assert!(!is_unspaced("1914 - 1920", "1914", "1920"));
        assert!(!is_unspaced("1914 to 1920", "1914", "1920"));
    }

    /// The unspaced forms of `left` and `right`.
    fn unspaced_forms(left: &str, right: &str) -> Vec<String> {
        ['-', '\u{2013}', '\u{2014}']
            .into_iter()
            .map(|dash| format!("{left}{dash}{right}"))
            .collect()
    }

    /// `left` and `right` read alone as dates, every unspaced form is left
    /// out of rule 1, every other form is read, and they agree.
    fn assert_only_unspaced_left_out(left: &str, right: &str, language: Language) {
        let Some(forms) = observe_range_forms(left, right, language) else {
            panic!("{left:?} {right:?} are two dates");
        };
        let read: Vec<&String> = forms.iter().map(|form| &form.0).collect();
        let unspaced = unspaced_forms(left, right);
        let mut expected = range_forms(left, right, language);
        expected.retain(|form| !unspaced.contains(form));
        assert_eq!(
            read,
            expected.iter().collect::<Vec<_>>(),
            "{left:?} {right:?}"
        );
        check_range_forms_agree(&forms);
    }

    #[test]
    fn a_join_that_writes_a_date_in_lookalike_letters_is_not_compared() {
        // #419: the fuzz input "1985\nХХ " (Cyrillic Х). Folded as N18
        // reads it, the join is `1985-XX`, an ISO date.
        let ru = Language::Russian;
        assert!(joins_into_a_date("1985", "\u{425}\u{425} ", ru));
        assert!(
            joins_into_a_date("1985", "\u{445}\u{445}", ru),
            "either case"
        );
        assert!(joins_into_a_date("1985", "xx", Language::English), "Latin");
        assert!(
            !joins_into_a_date("1985", "\u{425}\u{425}", Language::English),
            "English has no lookalike row"
        );
        assert!(
            !joins_into_a_date("1985", "\u{425}I\u{425}", ru),
            "a Roman numeral"
        );
        assert_only_unspaced_left_out("1985", "\u{425}\u{425} ", ru);
    }

    #[test]
    fn a_join_beside_a_range_candidate_of_its_own_is_not_compared() {
        // #419: the fuzz input "1914-9\n1989". "1914-9" is not a date core
        // reads, so its hyphen is a candidate too (N4).
        let en = Language::English;
        let ru = Language::Russian;
        assert!(holds_its_own_hyphen("1914-9", en));
        assert!(holds_its_own_hyphen("1914\u{2013}9", en), "any dash");
        assert!(holds_its_own_hyphen("9-1989", en));
        assert!(!holds_its_own_hyphen("1914", en), "no hyphen");
        assert!(!holds_its_own_hyphen("1914-09", en), "one unit");
        assert!(!holds_its_own_hyphen("xxxx-01", en), "one unit, any case");
        assert!(!holds_its_own_hyphen("1980-\u{435}", ru), "a decade suffix");
        assert!(!holds_its_own_hyphen("19-\u{439}", ru), "an ordinal suffix");
        assert!(
            holds_its_own_hyphen("1980-\u{435}", en),
            "not English's suffix"
        );
        assert!(unspaced_is_another_text("1914-9", "1989", en));
        assert!(unspaced_is_another_text("1989", "9-1989", en));
        assert!(!unspaced_is_another_text("1914", "1989", en));
        assert!(!unspaced_is_another_text("1980-\u{435}", "1990", ru));
        assert_only_unspaced_left_out("1914-9", "1989", en);
        // The same text spaced still compares every other form.
        assert_eq!(
            observe_range_forms("1980-\u{435}", "1990", ru).map(|forms| forms.len()),
            Some(range_forms("1980-\u{435}", "1990", ru).len()),
            "a suffix's hyphen binds its number, so the join is the same text"
        );
    }

    #[test]
    fn range_forms_are_observed_between_two_dates_only() {
        let forms = observe_range_forms("1914", "1920", Language::English);
        assert_eq!(forms.as_ref().map(Vec::len), Some(10), "every form is read");
        assert!(
            forms
                .iter()
                .flatten()
                .all(|form| form.1 == read(&["1914/1920"]))
        );
        for (left, right) in [
            ("12/04/1985", "1990"),
            ("1990", "the 80s"),
            ("circa 1914", "1920"),
            ("1914", "1985/1990"),
            ("1914", "nonsense"),
        ] {
            assert_eq!(
                observe_range_forms(left, right, Language::English),
                None,
                "{left:?} {right:?}"
            );
        }
    }

    #[test]
    fn or_is_observed_with_the_set_it_should_give() {
        let observation = observe_or("1914", "[..1916]", Language::English);
        assert_eq!(
            observation
                .as_ref()
                .map(|observed| observed.expected.as_str()),
            Some("[1914,..1916]")
        );
        assert_eq!(
            observation.map(|observed| observed.forms),
            Some(vec![(
                String::from("1914 or [..1916]"),
                read(&["[1914,..1916]"])
            )])
        );
        assert_eq!(
            observe_or("1914", "1914", Language::English).map(|observed| observed.expected),
            Some(String::from("1914"))
        );
        for (left, right) in [
            ("June", "1914"),
            ("XXXX-01", "1914"),
            ("1914", "[1913,XXXX-01]"),
            ("[..XXXX-01]", "1914"),
            ("1914~", "1920"),
            ("1985/1990", "1914"),
            ("{1914,1915}", "1920"),
            ("1914", "nonsense"),
            ("12/04/1985", "1920"),
        ] {
            assert_eq!(
                observe_or(left, right, Language::English),
                None,
                "{left:?} {right:?}"
            );
        }
    }

    #[test]
    fn before_and_after_are_observed_for_operands_only() {
        let observation = observe_open("12/04/1985", Language::English);
        assert_eq!(
            observation
                .as_ref()
                .and_then(|observed| observed.forms.get(4)),
            Some(&OpenForm {
                input: String::from("before 12/04/1985"),
                answer: read(&["[..1985-04-11]", "[..1985-12-03]"]),
                distinct: 2,
            })
        );
        assert_eq!(observation.map(|observed| observed.forms.len()), Some(9));
        // Two readings of the operand, one answer: the phrase over each
        // reading gives `[0000..]` both times.
        let merged = observe_open("from 1st century to 10's", Language::English);
        assert_eq!(
            merged.as_ref().and_then(|observed| observed.forms.get(1)),
            Some(&OpenForm {
                input: String::from("no earlier than from 1st century to 10's"),
                answer: read(&["[0000..]"]),
                distinct: 1,
            })
        );
        for operand in [
            "1914",
            "1985/1990",
            "[1667]",
            "early 19th century",
            "[1667..1668]",
        ] {
            assert!(
                observe_open(operand, Language::English).is_some(),
                "{operand:?}"
            );
        }
        for operand in [
            "June",
            "XXXX-06",
            "spring 1985",
            "1985-21/1990",
            "XXXX/1990",
            "1985/XXXX",
            "1985/..",
            "[1667,1668]",
            "{1667}",
            "[..1667]",
            "[1667..XXXX]",
            "[XXXX..1667]",
            "1985-04-12T23:20:30",
            "nonsense",
        ] {
            assert_eq!(
                observe_open(operand, Language::English),
                None,
                "{operand:?}"
            );
        }
    }

    #[test]
    fn case_and_whitespace_are_observed() {
        assert_eq!(
            observe_case("Circa 1920", en()),
            [
                (String::from("Circa 1920"), read(&["1920~"])),
                (String::from("CIRCA 1920"), read(&["1920~"])),
                (String::from("circa 1920"), read(&["1920~"])),
            ]
        );
        let inputs: Vec<String> = observe_whitespace("circa 1920", en())
            .into_iter()
            .map(|(input, answer)| {
                assert_eq!(answer, read(&["1920~"]), "{input:?}");
                input
            })
            .collect();
        assert_eq!(
            inputs,
            [
                "circa 1920",
                "  circa 1920\t ",
                "circa  1920",
                "circa\t1920",
                "circa\u{a0}1920",
            ]
        );
    }

    #[test]
    fn rereads_are_observed_in_every_language() {
        let rereads = observe_reread("1914-21", en());
        assert_eq!(
            rereads
                .iter()
                .map(|reread| reread.collision)
                .collect::<Vec<_>>(),
            [true, true, false, false],
            "two readings, two languages; only the sub-year code collides"
        );
        let plain = observe_reread("1914", ru());
        assert_eq!(
            plain,
            [
                Reread {
                    emitted: String::from("1914"),
                    language: Language::English,
                    answer: read(&["1914"]),
                    collision: false,
                },
                Reread {
                    emitted: String::from("1914"),
                    language: Language::Russian,
                    answer: read(&["1914"]),
                    collision: false,
                },
            ]
        );
        assert!(
            observe_reread("12/04/1985", en())
                .iter()
                .all(|reread| !reread.collision),
            "an ambiguity that is not a collision"
        );
    }

    #[test]
    fn options_are_observed_one_at_a_time() {
        let Some(century) = Century::new(1700) else {
            panic!("1700 is a century");
        };
        let observation = observe_options("the 80s", Language::English, century);
        assert_eq!(observation.input, "the 80s");
        assert_eq!(observation.default, read(&["188X", "198X"]));
        assert!(observation.notes.contains(&Note::DecadeAmbiguity));
        assert_eq!(
            observation.chosen,
            [
                (
                    Choice::NumericOrder(NumericOrder::DayFirst),
                    read(&["188X", "198X"])
                ),
                (
                    Choice::NumericOrder(NumericOrder::MonthFirst),
                    read(&["188X", "198X"])
                ),
                (Choice::DefaultCentury(century), read(&["178X"])),
            ]
        );
        let numeric = observe_options("12.04.1985", Language::Russian, century);
        assert_eq!(
            numeric.chosen.get(1),
            Some(&(
                Choice::NumericOrder(NumericOrder::MonthFirst),
                read(&["1985-12-04"])
            ))
        );
        assert!(numeric.notes.contains(&Note::NumericResolvedByLocale));
        assert_eq!(
            Choice::NumericOrder(NumericOrder::DayFirst).resolves(),
            [Note::NumericOrderAmbiguous, Note::NumericResolvedByLocale]
        );
        assert_eq!(
            Choice::DefaultCentury(century).resolves(),
            [Note::DecadeAmbiguity]
        );
    }

    #[test]
    fn values_and_honesty_are_observed() {
        let values = observe_values("12/04/1985", en());
        assert_eq!(values.len(), 2);
        assert_eq!(observe_values("unknown", en()), Vec::new());
        let honesty = observe_honest("1985-04-12", en());
        assert_eq!(honesty.input, "1985-04-12");
        assert!(honesty.valid.is_some() && !honesty.ambiguous && !honesty.refused);
        assert_eq!(honesty.readings.len(), 1);
        let ambiguous = observe_honest("12/04/1985", en());
        assert!(ambiguous.ambiguous && ambiguous.valid.is_none());
        assert!(
            ambiguous
                .readings
                .iter()
                .all(|reading| reading.reparsed.as_ref() == Some(&reading.value)
                    && reading.canonical == reading.edtf)
        );
        assert!(observe_honest("unknown", en()).refused);
    }

    // -----------------------------------------------------------------------
    // Each assertion of each check, planted

    fn forms(answers: [Answer; 2]) -> Forms {
        let [first, second] = answers;
        vec![(String::from("a"), first), (String::from("b"), second)]
    }

    fn parsed(edtf: &str) -> Edtf {
        Edtf::parse(edtf).unwrap_or_else(|error| panic!("{edtf:?}: {error:?}"))
    }

    #[test]
    fn agreeing_forms_pass() {
        let same = forms([read(&["1914"]), read(&["1914"])]);
        check_range_forms_agree(&same);
        check_case(&same);
        check_whitespace(&same);
        check_range_forms_agree(&[]);
    }

    #[test]
    #[should_panic(expected = "range forms disagree: \"b\" gives")]
    fn range_forms_that_disagree_fail() {
        check_range_forms_agree(&forms([
            read(&["1914/1920"]),
            Answer::Refused(NoMatchReason::OutOfGrammar),
        ]));
    }

    #[test]
    #[should_panic(expected = "\"or\" does not compose: \"1914 or 1920\" gives")]
    fn an_or_that_does_not_compose_fails() {
        check_or_composes(&OrObservation {
            expected: String::from("[1914,1920]"),
            forms: vec![(String::from("1914 or 1920"), read(&["1914", "1920"]))],
        });
    }

    #[test]
    fn an_or_that_composes_passes() {
        check_or_composes(&OrObservation {
            expected: String::from("[1914,1920]"),
            forms: vec![(String::from("1914 or 1920"), read(&["[1914,1920]"]))],
        });
    }

    #[test]
    #[should_panic(
        expected = "before/after does not compose: \"before x\" gives Read([\"[..1913]\"]), not 2 readings"
    )]
    fn before_and_after_with_too_few_readings_fail() {
        check_open_composes(&OpenObservation {
            forms: vec![OpenForm {
                input: String::from("before x"),
                answer: read(&["[..1913]"]),
                distinct: 2,
            }],
        });
    }

    #[test]
    #[should_panic(expected = "before/after does not compose: \"before x\" gives Refused")]
    fn before_and_after_refused_fail() {
        check_open_composes(&OpenObservation {
            forms: vec![OpenForm {
                input: String::from("before x"),
                answer: Answer::Refused(NoMatchReason::OutOfGrammar),
                distinct: 1,
            }],
        });
    }

    #[test]
    fn before_and_after_that_compose_pass() {
        check_open_composes(&OpenObservation {
            forms: vec![OpenForm {
                input: String::from("before x"),
                answer: read(&["[..1899]", "[..1799]"]),
                distinct: 2,
            }],
        });
    }

    #[test]
    #[should_panic(expected = "case changes the answer: \"b\" gives")]
    fn case_that_changes_an_answer_fails() {
        check_case(&forms([read(&["1920~"]), read(&["1920"])]));
    }

    #[test]
    #[should_panic(expected = "whitespace changes the answer: \"b\" gives")]
    fn whitespace_that_changes_an_answer_fails() {
        check_whitespace(&forms([read(&["1920~"]), read(&["1920"])]));
    }

    fn reread(answer: Answer, collision: bool) -> Reread {
        Reread {
            emitted: String::from("1914-21"),
            language: Language::English,
            answer,
            collision,
        }
    }

    #[test]
    fn rereads_as_itself_or_among_a_collision_pass() {
        check_idempotent(&[
            reread(read(&["1914-21"]), false),
            reread(read(&["1914-21", "1914/1921"]), true),
        ]);
    }

    #[test]
    #[should_panic(
        expected = "emitted \"1914-21\" re-reads in English as Read([\"1914-21\", \"1914/1921\"])"
    )]
    fn a_reread_among_readings_that_do_not_collide_fails() {
        check_idempotent(&[reread(read(&["1914-21", "1914/1921"]), false)]);
    }

    #[test]
    #[should_panic(
        expected = "emitted \"1914-21\" re-reads in English as Read([\"1914/1921\", \"1914-21-01\"])"
    )]
    fn a_collision_without_the_string_fails() {
        check_idempotent(&[reread(read(&["1914/1921", "1914-21-01"]), true)]);
    }

    fn options_observation(chosen: Answer, notes: Vec<Note>) -> OptionsObservation {
        OptionsObservation {
            input: String::from("x"),
            default: read(&["188X", "198X"]),
            notes,
            chosen: vec![(
                Choice::DefaultCentury(Century::new(1900).unwrap_or_else(|| panic!("a century"))),
                chosen,
            )],
        }
    }

    #[test]
    fn options_that_resolve_their_note_pass() {
        check_options(&options_observation(
            read(&["198X"]),
            vec![Note::DecadeAmbiguity],
        ));
        check_options(&options_observation(read(&["188X", "198X"]), Vec::new()));
    }

    #[test]
    #[should_panic(
        expected = "changes \"x\" from Read([\"188X\", \"198X\"]) to Read([\"198X\"]), which carries none of [DecadeAmbiguity]"
    )]
    fn an_option_that_changes_another_outcome_fails() {
        check_options(&options_observation(
            read(&["198X"]),
            vec![Note::NumericOrderAmbiguous],
        ));
    }

    #[test]
    fn stated_years_on_both_ends_pass() {
        check_no_masked_year(&[
            parsed("1985-06/1985-12"),
            parsed("XXXX-06/XXXX-07"),
            parsed("XXXX/1985"),
            parsed("1985/XXXX"),
            parsed("../1985"),
            parsed("1985"),
        ]);
    }

    #[test]
    #[should_panic(
        expected = "XXXX-06/1985-04-12 has a month under XXXX at one end and a stated year at the other"
    )]
    fn a_masked_year_beside_a_stated_one_fails() {
        check_no_masked_year(&[parsed("XXXX-06/1985-04-12")]);
    }

    fn emitted(edtf: &str) -> Emitted {
        let value = parsed(edtf);
        Emitted {
            edtf: String::from(edtf),
            value: value.clone(),
            reparsed: Some(value),
            canonical: String::from(edtf),
        }
    }

    fn honesty(readings: Vec<Emitted>) -> Honesty {
        Honesty {
            input: String::from("x"),
            valid: None,
            ambiguous: readings.len() > 1,
            refused: readings.is_empty(),
            readings,
        }
    }

    #[test]
    fn honest_readings_pass() {
        check_honest(&honesty(vec![emitted("1914")]));
        check_honest(&honesty(vec![emitted("1985-04-12"), emitted("1985-12-04")]));
        check_honest(&honesty(Vec::new()));
        let mut collision = honesty(vec![emitted("1914-21"), emitted("1914/1921")]);
        collision.valid = Some(parsed("1914-21"));
        check_honest(&collision);
        let mut itself = honesty(vec![emitted("1914")]);
        itself.valid = Some(parsed("1914"));
        check_honest(&itself);
    }

    #[test]
    #[should_panic(expected = "\"x\": emitted \"1914\" does not reparse to its value")]
    fn a_reading_that_does_not_reparse_fails() {
        let mut reading = emitted("1914");
        reading.reparsed = Some(parsed("1915"));
        check_honest(&honesty(vec![reading]));
    }

    #[test]
    #[should_panic(expected = "\"x\": emitted \"1914\" is not canonical")]
    fn a_reading_that_is_not_canonical_fails() {
        let mut reading = emitted("1914");
        reading.canonical = String::from("+1914");
        check_honest(&honesty(vec![reading]));
    }

    #[test]
    #[should_panic(expected = "\"x\": reading \"1914\" reported twice")]
    fn a_reading_reported_twice_fails() {
        check_honest(&honesty(vec![
            emitted("1915"),
            emitted("1914"),
            emitted("1914"),
        ]));
    }

    #[test]
    #[should_panic(expected = "\"x\": ambiguous with fewer than two readings")]
    fn an_ambiguity_of_one_fails() {
        let mut observation = honesty(vec![emitted("1914")]);
        observation.ambiguous = true;
        check_honest(&observation);
    }

    #[test]
    #[should_panic(expected = "\"x\": valid EDTF was not matched")]
    fn valid_edtf_refused_fails() {
        let mut observation = honesty(Vec::new());
        observation.valid = Some(parsed("1914"));
        check_honest(&observation);
    }

    #[test]
    #[should_panic(expected = "\"x\": valid EDTF was read as another value")]
    fn valid_edtf_read_as_another_fails() {
        let mut observation = honesty(vec![emitted("1914"), emitted("1915")]);
        observation.ambiguous = false;
        observation.valid = Some(parsed("1914"));
        check_honest(&observation);
    }

    #[test]
    #[should_panic(expected = "\"x\": valid EDTF was not among its readings")]
    fn valid_edtf_missing_from_an_ambiguity_fails() {
        let mut observation = honesty(vec![emitted("1915"), emitted("1916")]);
        observation.valid = Some(parsed("1914"));
        check_honest(&observation);
    }
}
