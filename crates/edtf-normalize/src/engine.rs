// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The language-neutral pattern grammar.
//!
//! Preprocessing, qualifier stripping, and the ordered match arms.
//! Everything language-specific comes in through a [`Lang`] table set.
//! Whole-input only — no substring extraction (N11); anything unmatched is
//! `NoMatch`, never a guess.
//!
//! Values are built through `edtf_core`'s validating constructors and
//! rendered via its canonical `Display` ([`render`]). A constructor's
//! refusal is the calendar check: it turns e.g. a February 31 into
//! [`Expr::Impossible`], and the input fails closed (N14). A value core
//! built is valid EDTF by construction (D31), so nothing is re-parsed.

#![expect(
    clippy::min_ident_chars,
    reason = "a byte scanner's b, c, s and i, alongside a date's y, m and d, are this grammar's notation; spelling them out makes the productions harder to follow, not easier"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "every flagged operation is bounded where it stands: slice indices by a length guard on the line above, digit values to 0-9 by the match arm that binds them, and the JDN forms by being computed in i128 so any i64 year fits. The operations that genuinely could leave range already use checked_/saturating_ and return an error rather than wrapping"
)]
#![expect(
    clippy::default_numeric_fallback,
    reason = "the inferred integer type is the one the surrounding signature already fixes; spelling it out at each literal adds noise, not information"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "each index is preceded by the bounds check that justifies it, or indexes a fixed-size array whose length is in its type"
)]
#![expect(
    clippy::shadow_reuse,
    reason = "the algorithms shadow deliberately — the JDN forms rebind y and m as the published derivation does, step by step, and renaming each step would break the correspondence to the source"
)]
#![expect(
    clippy::integer_division_remainder_used,
    reason = "calendar arithmetic is integer division by definition — the leap rules are /4, /100 and /400, and a float would be wrong"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching through a reference without restating & at every level is the idiomatic form the rest of this crate uses"
)]
#![expect(
    clippy::integer_division,
    reason = "calendar arithmetic is integer division by definition — the leap rules are /4, /100 and /400, and a float would be wrong"
)]
#![expect(
    clippy::string_slice,
    reason = "the slice boundaries are byte offsets the parser itself produced and has already proven to be char boundaries"
)]
#![expect(
    clippy::shadow_unrelated,
    reason = "short-lived rebinding inside one expression chain, where a second name would say nothing"
)]
#![expect(
    clippy::single_char_lifetime_names,
    reason = "'a is the conventional name for the single borrowed input's lifetime"
)]
#![expect(
    clippy::as_conversions,
    reason = "the operands are proven in range by the guard or type immediately above each cast, and try_from at these sites would add an unreachable error path"
)]
#![expect(
    clippy::cognitive_complexity,
    reason = "the function is one flat match over the grammar's productions; splitting it would scatter a single reading of the spec across call sites"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) states the intended visibility even where the module tree makes it redundant today"
)]
#![expect(
    clippy::shadow_same,
    reason = "rebinding a value to itself after a narrowing conversion"
)]

use core::iter;

use alloc::{
    format,
    string::{String, ToString as _},
    vec,
    vec::Vec,
};

use edtf_core::{
    Bound, BoundDate, Date, DateField, Edtf, Interval, IntervalEndpoint, Qualifier, Set,
    SetElement, SetKind, SetRange, ValueError, Year, YearKind,
};

use crate::{
    Ambiguous, Interpretation, NoMatchReason, Normalized, Note, NumericOrder, Options, Outcome,
    tables::{Lang, Open, Span, lang_for, season_name},
};

// ---------------------------------------------------------------------------
// Internal shapes

/// A matched expression before qualification and rendering.
#[derive(Debug, Clone)]
enum Expr {
    /// A single date.
    Date(Date),
    /// An interval, from the prose grammar or an EDTF token.
    Interval(Interval),
    /// One year of a span, `[lo..hi]` (ISO 8601-2 6.2, 6.3 c): a century
    /// part, or a BC century (N1, N2). `mask` is the looser date a qualifier
    /// falls back to, since a range inside a set cannot carry one (D27).
    Part {
        /// The span's first year.
        lo: Date,
        /// The span's last year.
        hi: Date,
        /// The century's digit mask, where it has one (`18XX`).
        mask: Option<Date>,
    },
    /// A one-of set the grammar built (6.2): before/after (N8) or "or"
    /// alternatives (N14).
    OneOf(Vec<SetElement>),
    /// Any other EDTF value an EDTF token holds: a set or a datetime. It
    /// reads as a whole input and as an "or" alternative only (N18).
    Value(Edtf),
    /// A reading the calendar refuses: core's constructors would not build
    /// it (February 30, a reversed span). It is a reading all the same, so
    /// the whole input fails closed as an impossible date (N14).
    Impossible,
}

impl From<Edtf> for Expr {
    fn from(value: Edtf) -> Self {
        match value {
            Edtf::Date(d) => Self::Date(d),
            Edtf::Interval(iv) => Self::Interval(iv),
            other @ (Edtf::DateTime(_) | Edtf::Set(_)) => Self::Value(other),
        }
    }
}

/// One reading of an ambiguous input.
#[derive(Debug, Clone)]
struct Candidate {
    /// The expression this reading denotes.
    expr: Expr,
    /// How the reading is described to a caller ("day-month-year", …).
    reading: String,
    /// Why this reading exists.
    notes: Vec<Note>,
}

/// Every reading a phrase, or a part of one, admits: never empty. One
/// candidate is a plain answer; several are reported, never picked from.
type Reading = Vec<Candidate>;

/// A reading with one candidate.
fn one(expr: Expr, notes: Vec<Note>) -> Reading {
    vec![Candidate {
        expr,
        reading: String::new(),
        notes,
    }]
}

// ---------------------------------------------------------------------------
// Construction helpers: every value is built through core's constructors

/// Single decimal digit as `u8`.
///
/// Callers reduce mod 10, or divide down to one digit, so the value is
/// always < 10 and the narrowing cannot truncate.
const fn digit(v: u32) -> u8 {
    (v % 10) as u8
}

/// The expression for a date core built, or the impossible reading for
/// one it refused (N14).
fn checked(date: Result<Date, ValueError>) -> Expr {
    date.map_or(Expr::Impossible, Expr::Date)
}

/// A four-digit `Year` for `y`, or `None` outside the standard range.
fn year_of(y: i32) -> Option<Year> {
    if !(-9999..=9999).contains(&y) {
        return None;
    }
    Year::from_value(i64::from(y)).ok()
}

/// A `Year` for any value, in the one form core accepts for it: four
/// digits within ±9999, `Y`-prefixed beyond (ISO 8601-2 4.7.2).
fn year_any(y: i64) -> Option<Year> {
    Year::from_value(y).ok()
}

/// A year-precision `Date`.
fn date_y(y: i32) -> Option<Date> {
    Date::new(year_of(y)?, None, None).ok()
}

/// A date of `year` with month `m` and, when given, day `d`: no reading
/// when a part has more than two digits, the impossible reading when the
/// calendar has no such month or day (N14).
fn dated(year: Year, m: u8, d: Option<u8>) -> Option<Expr> {
    let month = DateField::from_value(m).ok()?;
    let day = match d {
        Some(d) => Some(DateField::from_value(d).ok()?),
        None => None,
    };
    Some(checked(Date::new(year, Some(month), day)))
}

/// A month-precision date, or `None` if the year is out of range.
fn date_ym(y: i32, m: u8) -> Option<Expr> {
    dated(year_of(y)?, m, None)
}

/// A day-precision date, or `None` if the year is out of range.
fn date_ymd(y: i32, m: u8, d: u8) -> Option<Expr> {
    dated(year_of(y)?, m, Some(d))
}

/// `XXXX` year for the missing-year forms (N9).
fn masked_year() -> Option<Year> {
    Year::new(YearKind::Standard {
        negative: false,
        digits: [None; 4],
    })
    .ok()
}

/// `198X` from the three leading digits (198).
fn decade_date(prefix3: u16) -> Expr {
    let year = Year::new(YearKind::Standard {
        negative: false,
        digits: [
            Some(digit(u32::from(prefix3) / 100)),
            Some(digit(u32::from(prefix3) / 10)),
            Some(digit(u32::from(prefix3))),
            None,
        ],
    });
    year.map_or(Expr::Impossible, |y| checked(Date::new(y, None, None)))
}

/// `18XX` from the two leading digits. A BC century has no mask (D21).
fn century_date(prefix2: u8) -> Option<Date> {
    let year = Year::new(YearKind::Standard {
        negative: false,
        digits: [Some(prefix2 / 10), Some(prefix2 % 10), None, None],
    })
    .ok()?;
    Date::new(year, None, None).ok()
}

/// An interval expression between two dates, or the impossible reading
/// when the end precedes the start (D18).
fn interval(a: Date, b: Date) -> Expr {
    Interval::new(IntervalEndpoint::Date(a), IntervalEndpoint::Date(b))
        .map_or(Expr::Impossible, Expr::Interval)
}

// ---------------------------------------------------------------------------
// Token classifiers

/// Parse an all-digit token as a number.
fn num(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The token without a single trailing full stop.
fn strip_dot(t: &str) -> &str {
    t.trim_end_matches('.')
}

/// Compare a token to a dotless table word, ignoring '.' in the token
/// ("н.э." matches "нэ", "b.c." matches "bc").
fn eq_dotless(token: &str, word: &str) -> bool {
    let mut w = word.chars();
    for c in token.chars() {
        if c == '.' {
            continue;
        }
        if w.next() != Some(c) {
            return false;
        }
    }
    w.next().is_none()
}

/// Look a token up in a word table, case-insensitively.
fn table_u8(t: &str, table: &[(&str, u8)]) -> Option<u8> {
    let t = strip_dot(t);
    table.iter().find(|(n, _)| *n == t).map(|&(_, v)| v)
}

/// "19", "19th", "19-й", or "nineteenth" as a cardinal.
fn ordinal_num(t: &str, lang: &Lang) -> Option<u32> {
    if let Some(v) = num(t) {
        return Some(v);
    }
    for suffix in lang.ordinal_suffixes {
        if let Some(v) = t.strip_suffix(suffix).and_then(num) {
            return Some(v);
        }
    }
    lang.ordinal_words
        .iter()
        .find(|(w, _)| *w == t)
        .map(|&(_, v)| v)
}

/// Roman numeral 1–99 ("xix" → 19).
///
/// Latin letters are read in either case: preprocessing keeps an EDTF
/// token as typed, and "XXXX" is one ("XXXX века" → `39XX`). The
/// language's lookalike letters are read too, from its `roman_letters`
/// table — Russian sources routinely type "ХІХ век" with Cyrillic Х (N15).
/// Every letter is lower-cased first, so a kept token in upper-case
/// lookalikes ("ХХХХ века") reads as well.
fn roman_num(t: &str, lang: &Lang) -> Option<u32> {
    let mut vals = Vec::new();
    for c in t.chars().flat_map(char::to_lowercase) {
        vals.push(match c {
            'i' => 1,
            'v' => 5,
            'x' => 10,
            'l' => 50,
            'c' => 100,
            'd' => 500,
            'm' => 1000,
            c => lang
                .roman_letters
                .iter()
                .find(|&&(l, _)| l == c)
                .map(|&(_, v)| v)?,
        });
    }
    // Signed accumulation: numerals STARTING with a subtractive pair (IV,
    // IX, XL, XC) go negative on the first symbol and recover on the next.
    let mut total: i64 = 0;
    for (i, &v) in vals.iter().enumerate() {
        // mutants:equivalent replace + with *: `vals[i * 1..]` is `vals[i..]`, which adds only vals[i] itself to the look-ahead, and `n > v` is false for n == v.
        if vals[i + 1..].iter().any(|&n| n > v) {
            total -= i64::from(v);
        } else {
            total += i64::from(v);
        }
    }
    u32::try_from(total).ok().filter(|t| (1..=99).contains(t))
}

/// Ordinal in century position: 1 to 99.
///
/// Digits, suffixed digits, ordinal words, and Roman numerals where the
/// language uses them. The bool records the Roman-numeral provenance
/// (surfaced as `Note::RomanCentury`, N15). The range is held here, not by
/// each caller, so a bare left endpoint ("0-1в") can never reach the
/// century arithmetic as century 0.
fn century_ordinal(t: &str, lang: &Lang) -> Option<(u32, bool)> {
    if let Some(n) = ordinal_num(t, lang) {
        return (1..=99).contains(&n).then_some((n, false));
    }
    if lang.roman_centuries {
        return Some((roman_num(t, lang)?, true));
    }
    None
}

/// Day-of-month token: 1–2 digits with optional ordinal suffix.
fn day_num(t: &str, lang: &Lang) -> Option<u8> {
    let digits = lang
        .ordinal_suffixes
        .iter()
        .find_map(|s| t.strip_suffix(s))
        .unwrap_or(t);
    if digits.len() > 2 {
        return None;
    }
    let v = num(digits)?;
    u8::try_from(v).ok().filter(|d| (1..=31).contains(d))
}

/// Year token: 3–4 digits, nonzero.
fn year_num(t: &str) -> Option<i32> {
    if t.len() < 3 || t.len() > 4 {
        return None;
    }
    let v = num(t)?;
    (v >= 1).then(|| i32::try_from(v).ok())?
}

// ---------------------------------------------------------------------------
// Preprocessing and qualifier stripping

/// The one place input is read as EDTF (N18).
///
/// The text is tried as typed, then folded: the language's lookalike
/// letters read as the EDTF letters they stand for, and ASCII letters
/// upper-cased. The flag says whether the fold changed the text.
fn edtf_token(t: &str, lang: &Lang) -> Option<(Edtf, bool)> {
    let upper: String = t
        .chars()
        .map(|c| {
            lang.edtf_letters
                .iter()
                .find(|&&(l, _)| l == c)
                .map_or(c, |&(_, e)| e)
                .to_ascii_uppercase()
        })
        .collect();
    let folded = (upper != t).then_some((upper.as_str(), true));
    iter::once((t, false))
        .chain(folded)
        .find_map(|(text, fold)| Some((Edtf::parse(text).ok()?, fold)))
}

/// Unify dashes and collapse whitespace; lower-case every prose token.
///
/// Per whitespace-separated token: an EDTF token is kept as typed (its letters and commas are notation, N18); any other is
/// lower-cased, its apostrophes unified and its commas dropped. A token
/// the range splitter cuts (`range_hyphens`) is taken piece by piece, so
/// an EDTF piece keeps its case as a whole EDTF token does (N4).
fn preprocess(input: &str, opts: Options, lang: &Lang) -> String {
    let dashed: String = input
        .chars()
        .map(|c| match c {
            '\u{2013}' | '\u{2014}' => '-',
            c => c,
        })
        .collect();
    let mut s = String::with_capacity(dashed.len());
    for tok in dashed.split_whitespace() {
        s.push(' ');
        if edtf_token(tok, lang).is_some() {
            s.push_str(tok);
            continue;
        }
        // A token the range splitter cuts is read piece by piece, so an
        // EDTF piece ("XXXX-01" in "XXXX-01-1985") is kept as typed too.
        let mut start = 0;
        for end in range_hyphens(tok, opts, lang)
            .into_iter()
            .chain(iter::once(tok.len()))
        {
            let piece = &tok[start..end];
            if edtf_token(piece, lang).is_some() {
                s.push_str(piece);
            } else {
                for ch in piece.chars() {
                    match ch {
                        '\u{2019}' => s.push('\''),
                        ',' => s.push(' '),
                        c => s.extend(c.to_lowercase()),
                    }
                }
            }
            s.push_str(tok.get(end..=end).unwrap_or_default());
            start = end + 1;
        }
    }
    let mut out = String::with_capacity(s.len());
    for tok in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(tok);
    }
    out
}

/// Strip noise and qualifier words from around an expression.
///
/// Leading noise ("the", "в"), trailing noise ("гг.", "года"), leading and
/// trailing qualifier words, attached approximate prefixes ("c1860",
/// "ок.1920") and a trailing `?`, `~` or `%` on the last token ("1868-1871?").
/// Returns the remainder and the accumulated whole-expression qualifier
/// (N10). Text that is one EDTF token ("1984?/2004~") carries its own
/// qualifiers and is returned untouched (N18).
fn strip_qualifiers(s: &str, lang: &Lang) -> (String, Qualifier) {
    let mut toks: Vec<&str> = s.split(' ').filter(|t| !t.is_empty()).collect();
    let mut q = Qualifier::default();
    if let [t] = toks.as_slice() {
        if edtf_token(t, lang).is_some() {
            return (String::from(*t), q);
        }
    }
    'outer: loop {
        // mutants:equivalent replace > with >=: dropping a lone qualifier or noise word leaves an empty remainder, and neither "" nor any such word parses in any caller (none is a number, ordinal, Roman numeral or no-date word).
        if toks.len() > 1 {
            let head = strip_dot(toks[0].trim_end_matches(':'));
            if lang.noise_leading.contains(&head) {
                toks.remove(0);
                continue;
            }
            if lang.approx_leading.contains(&head) {
                q.approximate = true;
                toks.remove(0);
                continue;
            }
            if lang.uncertain_leading.contains(&head) {
                q.uncertain = true;
                toks.remove(0);
                continue;
            }
        }
        if let Some(&t0) = toks.first() {
            for p in lang.approx_attached {
                if let Some(rest) = t0.strip_prefix(p) {
                    if rest.starts_with(|c: char| c.is_ascii_digit()) {
                        q.approximate = true;
                        toks[0] = rest;
                        continue 'outer;
                    }
                }
            }
        }
        // mutants:equivalent replace > with >=: dropping a lone trailing qualifier or noise word leaves an empty remainder, and neither "" nor any such word parses in any caller.
        if toks.len() > 1 {
            let tail = strip_dot(toks[toks.len() - 1]);
            if lang.noise_trailing.contains(&tail) {
                toks.pop();
                continue;
            }
            if lang.approx_trailing.contains(&tail) {
                q.approximate = true;
                toks.pop();
                continue;
            }
            if lang.uncertain_trailing.contains(&tail) {
                q.uncertain = true;
                toks.pop();
                continue;
            }
        }
        if let Some(last) = toks.last_mut() {
            if let Some(rest) = last.strip_suffix(['?', '~', '%']) {
                // The symbol is the one ASCII byte just stripped.
                let symbol = last.as_bytes()[rest.len()];
                q.uncertain |= symbol != b'~';
                q.approximate |= symbol != b'?';
                *last = rest;
                if rest.is_empty() {
                    toks.pop();
                }
                continue;
            }
        }
        break;
    }
    (toks.join(" "), q)
}

// ---------------------------------------------------------------------------
// Eras

/// Split a trailing era phrase ("bc", "до н. э.") off the token list.
/// `Some(true)` = BC. Table order is longest-first, so "до н э" wins
/// over "н э".
fn split_era<'a>(toks: &[&'a str], lang: &Lang) -> Option<(bool, Vec<&'a str>)> {
    for (phrase, bc) in lang.eras {
        // mutants:equivalent replace > with >=: a token list that IS an era phrase leaves an empty core, which parses to None; no era phrase parses on its own either.
        if toks.len() > phrase.len() {
            let tail = &toks[toks.len() - phrase.len()..];
            if tail.iter().zip(*phrase).all(|(t, w)| eq_dotless(t, w)) {
                return Some((*bc, toks[..toks.len() - phrase.len()].to_vec()));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Decades

#[derive(Clone, Copy)]
/// What a decade token denotes, before a century is chosen.
enum DecadeParse {
    /// "1860s", "1980-е" — three leading digits fixed.
    Exact(u16),
    /// "1900s" — decade `190X` or century `19XX` (N6).
    CenturyAmbig(u16, u8),
    /// "80s", "80-е" — tens digit only; century unknown (N6).
    Bare(u8),
}

/// Read a decade token ("60s", "1960s", "60-е"), if it is one.
fn decade_tok(t: &str, lang: &Lang) -> Option<DecadeParse> {
    let t = t.strip_prefix('\'').unwrap_or(t);
    let digits = lang
        .decade_suffixes
        .iter()
        .find_map(|s| t.strip_suffix(s))?;
    let v = num(digits)?;
    if v % 10 != 0 {
        return None;
    }
    match digits.len() {
        4 => {
            let prefix3 = u16::try_from(v / 10).ok()?;
            if v % 100 == 0 {
                Some(DecadeParse::CenturyAmbig(
                    prefix3,
                    u8::try_from(v / 100).ok()?,
                ))
            } else {
                Some(DecadeParse::Exact(prefix3))
            }
        }
        2 => Some(DecadeParse::Bare(u8::try_from(v / 10).ok()?)),
        _ => None,
    }
}

/// Whether `year` is a century's first year in `0..=9900`, the domain of
/// [`crate::Century`] (N6).
pub(crate) const fn is_century_first_year(year: u16) -> bool {
    year <= 9900 && year % 100 == 0
}

/// Turn a decade reading into a candidate set, honouring the options.
fn decade_single(parse: DecadeParse, opts: Options) -> Reading {
    match parse {
        DecadeParse::Exact(p3) => one(decade_date(p3), Vec::new()),
        DecadeParse::CenturyAmbig(p3, p2) => vec![
            Candidate {
                expr: decade_date(p3),
                reading: format!("decade ({p3}X)"),
                notes: vec![Note::DecadeAmbiguity],
            },
            Candidate {
                expr: century_date(p2).map_or(Expr::Impossible, Expr::Date),
                reading: format!("century ({p2}XX)"),
                notes: vec![Note::DecadeAmbiguity],
            },
        ],
        DecadeParse::Bare(tens) => opts.default_century.map_or_else(
            || {
                let mk = |p3: u16| Candidate {
                    expr: decade_date(p3),
                    reading: format!("the {p3}0s"),
                    notes: vec![Note::DecadeAmbiguity],
                };
                vec![mk(180 + u16::from(tens)), mk(190 + u16::from(tens))]
            },
            |century| {
                let p3 = century.first_year() / 10 + u16::from(tens);
                one(decade_date(p3), vec![Note::DefaultCenturyApplied])
            },
        ),
    }
}

// ---------------------------------------------------------------------------
// Centuries

/// "19th century", "19c", "XIX век", bare "xix" (Roman-numeral languages
/// only) → century number, plus the Roman-numeral provenance flag (N15).
/// Era is split off by the caller.
fn century_toks(toks: &[&str], lang: &Lang) -> Option<(u32, bool)> {
    let word = |t: &str| lang.century_words.contains(&strip_dot(t));
    // Attached single-letter century word: "19c", "19в".
    let attached = |t: &str| -> Option<(u32, bool)> {
        let t = strip_dot(t);
        let d = lang
            .century_words
            .iter()
            .filter(|w| w.chars().count() == 1)
            .find_map(|w| t.strip_suffix(w))?;
        century_ordinal(d, lang)
    };
    let (n, roman) = match toks {
        [t] => attached(t).or_else(|| {
            // A bare Roman numeral reads as a century where the language
            // writes centuries that way ("XIX—XX вв." endpoints) — but a
            // lone letter with no century word is too weak a signal.
            (lang.roman_centuries && t.chars().count() >= 2)
                .then(|| roman_num(t, lang).map(|n| (n, true)))
                .flatten()
        })?,
        [o, w] if word(w) => century_ordinal(o, lang)?,
        _ => return None,
    };
    Some((n, roman))
}

/// Astronomical year span of century `n`: 19th CE = 1800..=1899 (ISO
/// 8601-1 3.1.2.23: a century begins with a year divisible by 100),
/// 2nd BC = -0199..=-0100, 1st BC = -0099..=0000 (year 0 exists; N2/N3).
#[expect(
    clippy::cast_possible_wrap,
    reason = "n <= 99 (century_ordinal range filter); i32::try_from is not usable in const fn"
)]
const fn century_span(n: u32, bc: bool) -> (i32, i32) {
    let n = n as i32;
    if bc {
        (-(100 * n - 1), -(100 * (n - 1)))
    } else {
        (100 * (n - 1), 100 * (n - 1) + 99)
    }
}

/// Whole century as an expression: the digit mask where there is one.
fn century_expr(n: u32, bc: bool) -> Option<(Expr, Vec<Note>)> {
    if bc {
        // edtf-core rejects unspecified digits in negative years (D21), and
        // the 1st century BC would need year -0000 anyway — a BC century is
        // a one-of range of its years, and the CenturyMask note would be a
        // lie for it (N2).
        let notes = vec![Note::AstronomicalYear, Note::BcCentury];
        let (s, e) = century_span(n, true);
        return Some((
            Expr::Part {
                lo: date_y(s)?,
                hi: date_y(e)?,
                mask: None,
            },
            notes,
        ));
    }
    let prefix2 = u8::try_from(n - 1).ok()?;
    Some((Expr::Date(century_date(prefix2)?), vec![Note::CenturyMask]))
}

/// Early/mid/late/half of a century as a decade-rounded one-of range (N1).
#[expect(
    clippy::many_single_char_names,
    reason = "s/e/a/b are interval-endpoint arithmetic; longer names obscure the span algebra"
)]
fn century_part_expr(span: Span, n: u32, bc: bool) -> Option<(Expr, Vec<Note>)> {
    let (s, e) = century_span(n, bc);
    let (a, b) = match span {
        Span::Early => (s, s + 29),
        Span::Mid => (s + 30, s + 69),
        Span::Late => (s + 70, e),
        Span::FirstHalf => (s, s + 49),
        Span::SecondHalf => (s + 50, e),
    };
    let mut notes = vec![Note::CenturyPart];
    if bc {
        notes.push(Note::AstronomicalYear);
    }
    // A BC century has no mask (D21), so a qualified BC part is refused.
    let mask = if bc {
        None
    } else {
        Some(century_date(u8::try_from(n - 1).ok()?)?)
    };
    Some((
        Expr::Part {
            lo: date_y(a)?,
            hi: date_y(b)?,
            mask,
        },
        notes,
    ))
}

// ---------------------------------------------------------------------------
// Numeric tokens ("12/04/1985", "7/2008", "1985/4")

/// The separator an all-numeric date uses, if it uses exactly one.
fn numeric_sep(t: &str) -> Option<char> {
    let mut sep = None;
    for c in ['/', '-', '.'] {
        if t.contains(c) {
            if sep.is_some() {
                return None; // mixed separators
            }
            sep = Some(c);
        }
    }
    sep
}

/// A day-month-year triple as an expression, if the parts are a real date.
fn dmy(d: u32, m: u32, y: i32) -> Option<Expr> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (m, d) = (u8::try_from(m).ok()?, u8::try_from(d).ok()?);
    date_ymd(y, m, d)
}

#[expect(
    clippy::many_single_char_names,
    reason = "y/m/d/a/b are date-field names; the arms ARE the grammar"
)]
/// Read an all-numeric date token, reporting every ordering it admits.
fn numeric_token(t: &str, opts: Options, lang: &Lang) -> Option<Reading> {
    let sep = numeric_sep(t)?;
    let parts: Vec<&str> = t.split(sep).collect();
    let vals: Vec<u32> = parts.iter().map(|p| num(p)).collect::<Option<_>>()?;
    match (parts.as_slice(), vals.as_slice()) {
        // Year first is order-unambiguous: 1985/04/12.
        ([py, _, _], &[y, m, d]) if py.len() == 4 => Some(one(
            dmy(d, m, i32::try_from(y).ok()?)?,
            vec![Note::NumericUnambiguous],
        )),
        // Two small fields then a 4-digit year: the DMY/MDY minefield (N5).
        ([pa, pb, py], &[a, b, y]) if py.len() == 4 && pa.len() <= 2 && pb.len() <= 2 => {
            let y = i32::try_from(y).ok()?;
            let day_first = dmy(a, b, y);
            let month_first = dmy(b, a, y);
            match (a > 12, b > 12) {
                (true, false) => Some(one(day_first?, vec![Note::NumericUnambiguous])),
                (false, true) => Some(one(month_first?, vec![Note::NumericUnambiguous])),
                (true, true) => None,
                (false, false) if a == b => {
                    Some(one(day_first?, vec![Note::NumericOrderIrrelevant]))
                }
                (false, false) => {
                    let (order, note) = match (opts.numeric_order, lang.implied_numeric_order) {
                        (Some(o), _) => (Some(o), Note::NumericResolvedByOption),
                        (None, Some(o)) => (Some(o), Note::NumericResolvedByLocale),
                        (None, None) => (None, Note::NumericOrderAmbiguous),
                    };
                    match order {
                        Some(NumericOrder::DayFirst) => Some(one(day_first?, vec![note])),
                        Some(NumericOrder::MonthFirst) => Some(one(month_first?, vec![note])),
                        None => {
                            let cand = |e: Option<Expr>, reading: &str| {
                                e.map(|expr| Candidate {
                                    expr,
                                    reading: String::from(reading),
                                    notes: vec![Note::NumericOrderAmbiguous],
                                })
                            };
                            let readings: Vec<Candidate> = [
                                cand(day_first, "day-month-year"),
                                cand(month_first, "month-day-year"),
                            ]
                            .into_iter()
                            .flatten()
                            .collect();
                            (!readings.is_empty()).then_some(readings)
                        }
                    }
                }
            }
        }
        // Month and year: 7/2008, 04.1985.
        ([pa, py], &[m, y]) if pa.len() <= 2 && py.len() == 4 && (1..=12).contains(&m) => {
            let (y, m) = (i32::try_from(y).ok()?, u8::try_from(m).ok()?);
            Some(one(date_ym(y, m)?, Vec::new()))
        }
        // Year then small field: 1985/4, 1985-4 (a full "1985-04" is an EDTF
        // token, read before the prose arms).
        ([py, pb], &[y, m]) if py.len() == 4 && pb.len() <= 2 && (1..=12).contains(&m) => {
            let (y, m) = (i32::try_from(y).ok()?, u8::try_from(m).ok()?);
            Some(one(date_ym(y, m)?, Vec::new()))
        }
        _ => None,
    }
}

/// Whether a sub-year-code token is also an elided range ("1914-21").
///
/// Such a token is not an atom: the range grammar reads it, giving both readings where a span
/// may stand and refusing it as a range endpoint, which cannot hold a span
/// (N13).
fn sub_year_collides(t: &str, opts: Options, lang: &Lang) -> bool {
    is_sub_year_token(t) && endpoint_pair(&t[..4], &t[5..], opts, lang).is_some()
}

/// `NNNN-NN` with NN a sub-year code 21–41: four digits, a hyphen, two
/// digits (N13).
fn is_sub_year_token(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..].iter().all(u8::is_ascii_digit)
        && t[5..]
            .parse::<u8>()
            .is_ok_and(|code| (21..=41).contains(&code))
}

// ---------------------------------------------------------------------------
// The single-expression grammar

/// Match one date-ish expression: the atom reader every construct reads its
/// parts through. `in_range` suppresses span-producing forms (century
/// parts) so range endpoints stay dates (N1 scope).
#[expect(
    clippy::too_many_lines,
    reason = "the ordered arms ARE the grammar (N11); order carries meaning"
)]
fn parse_single(s: &str, opts: Options, lang: &Lang, in_range: bool) -> Option<Reading> {
    // An empty token list (callers never produce one) falls through every
    // arm to the final `_ => None`.
    let toks: Vec<&str> = s.split(' ').filter(|t| !t.is_empty()).collect();

    // A one-token atom is tried as EDTF before any prose arm, as a whole
    // input and inside a phrase alike (N18). A colliding sub-year token is
    // the range grammar's question instead (N13). The as-typed note is
    // kept only where the token is the whole input (`combine`, `run`).
    if let [t] = toks.as_slice() {
        if sub_year_collides(t, opts, lang) {
            return None;
        }
        if let Some((value, folded)) = edtf_token(t, lang) {
            let note = if folded {
                Note::EdtfLettersNormalized
            } else {
                Note::AlreadyValidEdtf
            };
            return Some(one(Expr::from(value), vec![note]));
        }
    }

    // Trailing era phrase, if any ("500 bc", "xix век до н. э.").
    let (era, mut core_toks) = match split_era(&toks, lang) {
        Some((bc, rest)) => (Some(bc), rest),
        None => (None, toks.clone()),
    };
    // A year marker may sit BEFORE the era phrase ("44 г. до н. э.") — strip
    // trailing noise again now that the era is off (N3 scope).
    // mutants:equivalent replace > with >=: popping a lone trailing noise word ("года") leaves an empty core; neither the word nor an empty core parses.
    while core_toks.len() > 1
        && lang
            .noise_trailing
            .contains(&strip_dot(core_toks[core_toks.len() - 1]))
    {
        core_toks.pop();
    }
    let bc = era == Some(true);

    // Part-of-century / decade modifier, spaced or attached ("mid-1930s").
    if let Some((span, rest)) = split_modifier(&core_toks, lang) {
        if era.is_none() {
            if let [t] = rest.as_slice() {
                if let Some(d) = decade_tok(t, lang) {
                    // Sub-decade parts would be false precision: the modifier
                    // is dropped and recorded (N1).
                    return Some(with_note(decade_single(d, opts), Note::ModifierDropped));
                }
            }
        }
        if let Some((n, roman)) = century_toks(&rest, lang) {
            let (expr, mut notes) = if in_range {
                let (e, mut ns) = century_expr(n, bc)?;
                ns.push(Note::ModifierDropped);
                (e, ns)
            } else {
                century_part_expr(span, n, bc)?
            };
            if roman {
                notes.push(Note::RomanCentury);
            }
            return Some(one(expr, notes));
        }
        return None;
    }

    if let Some((n, roman)) = century_toks(&core_toks, lang) {
        let (expr, mut notes) = century_expr(n, bc)?;
        if roman {
            notes.push(Note::RomanCentury);
        }
        return Some(one(expr, notes));
    }

    // Decade of an explicit century: "60-е годы XIX века" → 186X (N6).
    if era.is_none() && core_toks.len() >= 2 {
        if let Some(DecadeParse::Bare(tens)) = decade_tok(core_toks[0], lang) {
            let mut rest = core_toks[1..].to_vec();
            // mutants:equivalent replace > with >=: stripping a lone noise word after a decade leaves no century tokens; no noise word reads as a century either.
            while rest.len() > 1 && lang.noise_trailing.contains(&strip_dot(rest[0])) {
                rest.remove(0);
            }
            if let Some((n, roman)) = century_toks(&rest, lang) {
                let prefix3 = (u16::try_from(n).ok()? - 1) * 10 + u16::from(tens);
                let mut notes = vec![Note::DecadeOfCentury];
                if roman {
                    notes.push(Note::RomanCentury);
                }
                return Some(one(decade_date(prefix3), notes));
            }
        }
    }

    // Era-qualified plain year: "500 bc" → -0499, "79 ce" → 0079 (N3).
    if let Some(bc) = era {
        if let [t] = core_toks.as_slice() {
            let v = num(t)?;
            if v >= 1 && t.len() <= if bc { 5 } else { 4 } {
                let y = i32::try_from(if bc { 1 - i64::from(v) } else { i64::from(v) }).ok()?;
                let notes = if bc {
                    vec![Note::AstronomicalYear]
                } else {
                    Vec::new()
                };
                return Some(one(Expr::Date(date_y(y)?), notes));
            }
        }
        return None;
    }

    match core_toks.as_slice() {
        [t] => {
            if let Some(y) = year_num(t) {
                return Some(one(Expr::Date(date_y(y)?), Vec::new()));
            }
            if let Some(d) = decade_tok(t, lang) {
                return Some(decade_single(d, opts));
            }
            if let Some(m) = table_u8(t, lang.months) {
                // "January" → XXXX-01 (N9).
                return Some(one(
                    dated(masked_year()?, m, None)?,
                    vec![Note::MissingYearMasked],
                ));
            }
            numeric_token(t, opts, lang)
        }
        [a, b] => {
            // "spring 2001" / "весной 2001" / "2001 spring" (N7).
            for (st, yt) in [(a, b), (b, a)] {
                if let (Some(code), Some(y)) = (table_u8(st, lang.seasons), year_num(yt)) {
                    return Some(one(dated(year_of(y)?, code, None)?, vec![Note::SeasonCode]));
                }
            }
            // "June 1940" / "1940 June".
            for (mt, yt) in [(a, b), (b, a)] {
                if let (Some(m), Some(y)) = (table_u8(mt, lang.months), year_num(yt)) {
                    return Some(one(date_ym(y, m)?, Vec::new()));
                }
            }
            // "January 12" / "12 January" → XXXX-01-12 (N9).
            for (mt, dt) in [(a, b), (b, a)] {
                if let (Some(m), Some(d)) = (table_u8(mt, lang.months), day_num(dt, lang)) {
                    return Some(one(
                        dated(masked_year()?, m, Some(d))?,
                        vec![Note::MissingYearMasked],
                    ));
                }
            }
            None
        }
        [a, b, c] => {
            // "12 April 1985" / "April 12, 1985" / "12-го апреля 1985".
            for (dt, mt) in [(a, b), (b, a)] {
                if let (Some(d), Some(m), Some(y)) =
                    (day_num(dt, lang), table_u8(mt, lang.months), year_num(c))
                {
                    return Some(one(date_ymd(y, m, d)?, Vec::new()));
                }
            }
            None
        }
        _ => None,
    }
}

/// Split a leading part-of-century modifier phrase off the token list, then
/// drop any noise between it and the rest ("first half OF THE 19th century").
fn split_modifier<'a>(toks: &[&'a str], lang: &Lang) -> Option<(Span, Vec<&'a str>)> {
    let matched = lang.modifiers.iter().find_map(|(phrase, span)| {
        // mutants:equivalent replace > with >=: a bare modifier phrase gives an empty remainder (None); unmatched, the phrase alone also parses to None.
        (toks.len() > phrase.len() && toks.iter().zip(*phrase).all(|(t, w)| strip_dot(t) == *w))
            .then(|| (*span, toks[phrase.len()..].to_vec()))
    });
    let (span, mut rest) = matched.or_else(|| {
        // Attached single-word modifier: "mid-1930s", "late-19th".
        let (head, tail_toks) = toks.split_first()?;
        let (word, tail) = head.split_once('-')?;
        let span = lang
            .modifiers
            .iter()
            .find(|(p, _)| p.len() == 1 && p[0] == word)
            .map(|&(_, s)| s)?;
        if tail.is_empty() {
            return None;
        }
        let mut out = vec![tail];
        out.extend_from_slice(tail_toks);
        Some((span, out))
    })?;
    // mutants:equivalent replace > with >=: stripping a lone noise word after the modifier leaves nothing; the noise word is no decade or century either.
    while rest.len() > 1 && lang.noise_leading.contains(&strip_dot(rest[0])) {
        rest.remove(0);
    }
    Some((span, rest))
}

/// The reading with one more note on every candidate.
fn with_note(mut reading: Reading, note: Note) -> Reading {
    for c in &mut reading {
        c.notes.push(note);
    }
    reading
}

// ---------------------------------------------------------------------------
// Open-ended and two-sided ranges

/// Match a leading before/after phrase from a longest-first table, with
/// the meaning its row carries.
fn match_head<'a>(toks: &[&'a str], phrases: &[(&[&str], Open)]) -> Option<(Open, Vec<&'a str>)> {
    for (phrase, open) in phrases {
        // mutants:equivalent replace > with >=: a bare open-range phrase gives an empty remainder, which parses to None, as does the unmatched phrase alone.
        if toks.len() > phrase.len() && toks.iter().zip(*phrase).all(|(t, w)| strip_dot(t) == *w) {
            return Some((*open, toks[phrase.len()..].to_vec()));
        }
    }
    None
}

/// Everything a whole input, or one "or" alternative, can be (N4, N8, N14).
///
/// A date, a two-sided range, an open range or an EDTF token. It is never a
/// range endpoint, so the single-expression grammar may read a span here.
fn parse_reading(s: &str, opts: Options, lang: &Lang) -> Option<Reading> {
    let toks: Vec<&str> = s.split(' ').collect();
    parse_single(s, opts, lang, false)
        .or_else(|| parse_open(&toks, opts, lang))
        .or_else(|| parse_range(s, &toks, opts, lang))
}

/// The one way a construct reads its parts (N14, N16).
///
/// Walks every combination of the parts' readings. Each combination takes
/// year inheritance first (`distribute_year`), then the construct's own
/// `build` step; a combination that fails refuses the whole input, because
/// promoting the survivors would fake certainty the prose never had. A part
/// is never the whole input, so an as-typed EDTF token inside a construct
/// carries no note (N18).
fn combine(
    parts: &[Reading],
    build: impl Fn(Vec<Candidate>) -> Option<Reading>,
) -> Option<Reading> {
    let mut combos: Vec<Vec<Candidate>> = vec![Vec::new()];
    for part in parts {
        let mut next = Vec::with_capacity(combos.len() * part.len());
        for prefix in &combos {
            for c in part {
                let mut c = c.clone();
                c.notes.retain(|n| *n != Note::AlreadyValidEdtf);
                let mut combo = prefix.clone();
                combo.push(c);
                next.push(combo);
            }
        }
        combos = next;
    }
    let mut out = Vec::new();
    for mut combo in combos {
        // An interval or other EDTF value neither gives nor takes a year.
        let mut dates: Vec<(&mut Expr, &mut Vec<Note>)> = combo
            .iter_mut()
            .filter(|c| matches!(c.expr, Expr::Date(_)))
            .map(|c| (&mut c.expr, &mut c.notes))
            .collect();
        distribute_year(&mut dates)?;
        out.extend(build(combo)?);
    }
    Some(out)
}

/// Before and after (N8): six meanings, one per row of the language's
/// `open_phrases` table.
///
/// Strictly before and after, and on or before and after, are one-of sets
/// (ISO 8601-2 6.3 a, b); "until" and "since" state a period, an open
/// interval.
///
/// The operand is any reading with a start and an end: a date, a span (a
/// one-of range, or an interval with two dated ends) or a one-element set.
/// An open span ("before after 1900"), a set of several, or a datetime
/// gives nothing (N8, N18).
fn parse_open(toks: &[&str], opts: Options, lang: &Lang) -> Option<Reading> {
    let (open, rest) = match_head(toks, lang.open_phrases)?;
    let (inner, q) = strip_qualifiers(&rest.join(" "), lang);
    let operand = parse_reading(&inner, opts, lang)?;
    combine(&[operand], |parts| {
        let [mut c] = <[Candidate; 1]>::try_from(parts).ok()?;
        c.expr = open_expr(open, &c.expr, q)?;
        c.notes.push(Note::BeforeAfter);
        Some(vec![c])
    })
}

/// An operand's first and last date, and whether it is one date.
fn operand_ends(e: &Expr) -> Option<(Date, Date, bool)> {
    let only = |elements: &[SetElement]| match elements {
        [SetElement::Date(d)] => Some((*d, *d, true)),
        [SetElement::Range(r)] => Some((r.start(), r.end(), false)),
        _ => None,
    };
    match e {
        Expr::Date(d) => Some((*d, *d, true)),
        Expr::Interval(iv) => match (iv.start(), iv.end()) {
            (IntervalEndpoint::Date(a), IntervalEndpoint::Date(b)) => Some((a, b, false)),
            _ => None,
        },
        Expr::Part { lo, hi, mask: _ } => Some((*lo, *hi, false)),
        Expr::OneOf(elements) => only(elements),
        Expr::Value(Edtf::Set(set)) => only(set.elements()),
        Expr::Value(_) | Expr::Impossible => None,
    }
}

/// The before/after expression for one operand (N8).
///
/// `lo` is the operand's earliest day and `hi` its latest, each cut to the
/// precision of the date it comes from; a step moves one unit at that
/// precision. Every unit carries the operand's qualifier and `q`.
fn open_expr(open: Open, operand: &Expr, q: Qualifier) -> Option<Expr> {
    // An impossible operand stays impossible under any opening (N14).
    if matches!(operand, Expr::Impossible) {
        return Some(Expr::Impossible);
    }
    let (start, end, single) = operand_ends(operand)?;
    // A year-less end ("June" is `XXXX-06`, N9) has no unit of its own:
    // one read off core's 0000–9999 bounds would state a year the prose
    // never gave. Each set row reads one end and refuses only when that
    // end is year-less; "until" and "since" take it as written.
    let known = |d: Date| {
        let year_less = matches!(
            d.year().kind(),
            YearKind::Standard {
                negative: _,
                digits: [None, None, None, None],
            }
        );
        (!year_less).then_some(d)
    };
    let element = match open {
        Open::Before => SetElement::OnOrBefore(step(&known(start)?, false, q)?),
        Open::OnOrBefore => SetElement::OnOrBefore(unit(&known(end)?, true, q)?),
        Open::After => SetElement::OnOrAfter(step(&known(end)?, true, q)?),
        Open::OnOrAfter => SetElement::OnOrAfter(unit(&known(start)?, false, q)?),
        Open::Until | Open::Since => {
            let until = open == Open::Until;
            let x = if single {
                start
            } else if until {
                unit(&end, true, q)?
            } else {
                unit(&start, false, q)?
            };
            let x = x.qualified(q);
            let (start, end) = if until {
                (IntervalEndpoint::Open, IntervalEndpoint::Date(x))
            } else {
                (IntervalEndpoint::Date(x), IntervalEndpoint::Open)
            };
            return Interval::new(start, end).ok().map(Expr::Interval);
        }
    };
    Some(Expr::OneOf(vec![element]))
}

/// Whether a date names a season, which has no predecessor or successor
/// (ISO 8601-2 4.8.1 gives codes 21–24 no dates).
fn is_season(d: &Date) -> bool {
    d.month().and_then(DateField::value).is_some_and(|m| m > 12)
}

/// Whether every digit of `d` is specified and its year carries no
/// significant digits: a date that is its own `lo` and `hi` (N8).
fn unmasked(d: &Date) -> bool {
    d.year().significant_digits().is_none() && !d.has_unspecified()
}

/// The first (or last) unit of `d` at its own precision, qualified. A
/// season, or any unmasked date, is its own unit, in its own year form
/// (`Y17E7` stays `Y17E7`).
fn unit(d: &Date, last: bool, q: Qualifier) -> Option<Date> {
    if is_season(d) || unmasked(d) {
        return Some(d.qualified(q));
    }
    cut(edge(d, last)?, d, q)
}

/// The unit just before `d`'s first (or just after its last), qualified,
/// its year in the form its value takes (`Y17E7` → `Y169999999`,
/// `Y10000` → `9999`). A season has none.
fn step(d: &Date, forward: bool, q: Qualifier) -> Option<Date> {
    if is_season(d) {
        return None;
    }
    let day = edge(d, forward)?;
    let next = if forward {
        day.next_day()
    } else {
        day.previous_day()
    };
    cut(next?, d, q)
}

/// The earliest (or latest) calendar day `d` touches.
fn edge(d: &Date, last: bool) -> Option<BoundDate> {
    let bounds = Edtf::Date(*d).bounds();
    match if last { bounds.latest } else { bounds.earliest } {
        Bound::Date(day) => Some(day),
        Bound::NegativeInfinity | Bound::PositiveInfinity | Bound::Unknown => None,
    }
}

/// `like`'s qualifier, every component's merged, with `q`.
fn carried(like: &Date, q: Qualifier) -> Qualifier {
    let mut carried = q;
    for field in [
        Some(like.year().qualifier()),
        like.month().map(DateField::qualifier),
        like.day().map(DateField::qualifier),
    ]
    .into_iter()
    .flatten()
    {
        merge(&mut carried, field);
    }
    carried
}

/// `day` cut to the precision of `like`, its year in the form its value
/// takes (`year_any`), carrying `like`'s qualifier and `q`.
fn cut(day: BoundDate, like: &Date, q: Qualifier) -> Option<Date> {
    let year = year_any(day.year())?;
    let (month, day) = match (like.month(), like.day()) {
        (None, _) => (None, None),
        (Some(_), None) => (Some(DateField::from_value(day.month()).ok()?), None),
        (Some(_), Some(_)) => (
            Some(DateField::from_value(day.month()).ok()?),
            Some(DateField::from_value(day.day()).ok()?),
        ),
    };
    Some(
        Date::new(year, month, day)
            .ok()?
            .qualified(carried(like, q)),
    )
}

/// "1863 or 1864 or 1865" → `[1863,1864,1865]`, ISO's one-of set (N14).
///
/// Every "or" separates whole alternatives, each read as a whole input
/// would be. Qualifiers, the whole input's `outer` included, and a stated
/// year scoping the year-less ones (N16) apply before the set is built.
/// An alternative with several readings gives one set per combination.
/// When an alternative is a period, a datetime or an all-members set, no
/// single expression exists and each alternative is reported as a reading.
fn parse_or(toks: &[&str], opts: Options, lang: &Lang, outer: Qualifier) -> Option<Reading> {
    let sides: Vec<String> = toks
        .split(|t| lang.or_words.contains(t))
        .map(|side| side.join(" "))
        .collect();
    if sides.len() < 2 {
        return None;
    }
    let mut parts = Vec::with_capacity(sides.len());
    for side in sides {
        let (inner, mut q) = strip_qualifiers(&side, lang);
        merge(&mut q, outer);
        let mut reading = parse_reading(&inner, opts, lang)?;
        for c in &mut reading {
            qualify_candidate(c, q)?;
        }
        parts.push(reading);
    }
    combine(&parts, |alternatives| {
        if let Some(set) = one_of(&alternatives) {
            let mut notes: Vec<Note> = Vec::new();
            for n in alternatives.iter().flat_map(|c| c.notes.iter().copied()) {
                if !notes.contains(&n) {
                    notes.push(n);
                }
            }
            notes.push(Note::OrAlternatives);
            let named: Vec<&str> = alternatives
                .iter()
                .map(|c| c.reading.as_str())
                .filter(|r| !r.is_empty())
                .collect();
            return Some(vec![Candidate {
                expr: set,
                reading: named.join(" or "),
                notes,
            }]);
        }
        Some(
            alternatives
                .into_iter()
                .map(|mut c| {
                    c.notes.push(Note::OrAlternatives);
                    c.reading = String::from("alternative");
                    c
                })
                .collect(),
        )
    })
}

/// The one-of set the alternatives make, when every one is a date or a
/// one-of set (whose elements are taken in place). Equal elements are
/// written once, and a set left with one plain date is that date.
fn one_of(alternatives: &[Candidate]) -> Option<Expr> {
    let mut elements: Vec<SetElement> = Vec::new();
    for c in alternatives {
        let mine = match &c.expr {
            Expr::Date(d) => vec![SetElement::Date(*d)],
            Expr::Part { lo, hi, mask: _ } => match SetRange::new(*lo, *hi) {
                Ok(r) => vec![SetElement::Range(r)],
                Err(_) => return Some(Expr::Impossible),
            },
            Expr::OneOf(els) => els.clone(),
            Expr::Value(Edtf::Set(set)) if set.kind() == SetKind::OneMember => {
                set.elements().to_vec()
            }
            // One impossible alternative makes the set impossible (N14).
            Expr::Impossible => return Some(Expr::Impossible),
            Expr::Interval(_) | Expr::Value(_) => return None,
        };
        for e in mine {
            if !elements.contains(&e) {
                elements.push(e);
            }
        }
    }
    Some(match elements.as_slice() {
        [SetElement::Date(d)] => Expr::Date(*d),
        _ => Expr::OneOf(elements),
    })
}

/// Two-sided ranges: "from X to Y", "с X по Y", "X to Y", "X-Y".
///
/// The range word is tried at every whitespace token, left to right, and
/// the first position whose two sides read wins ("от 44 г. до н. э. до
/// 1914", where the era phrase holds the separator word). Failing that,
/// the hyphen is tried at every candidate (`range_hyphens`): a hyphen
/// inside a date unit is never one (N4). One split that reads gives its
/// readings; several give every split's readings, left to right (N14).
fn parse_range(s: &str, toks: &[&str], opts: Options, lang: &Lang) -> Option<Reading> {
    for (lead, sep) in lang.range_pairs {
        let inner = match lead {
            Some(l) if toks.first() == Some(l) => &toks[1..],
            Some(_) => continue,
            None => toks,
        };
        let found = inner.iter().enumerate().find_map(|(pos, t)| {
            (t == sep)
                .then(|| {
                    endpoint_pair(
                        &inner[..pos].join(" "),
                        &inner[pos + 1..].join(" "),
                        opts,
                        lang,
                    )
                })
                .flatten()
        });
        if found.is_some() {
            return found;
        }
    }
    let splits: Reading = range_hyphens(s, opts, lang)
        .into_iter()
        .filter_map(|i| endpoint_pair(s[..i].trim(), s[i + 1..].trim(), opts, lang))
        .flatten()
        .collect();
    (!splits.is_empty()).then_some(splits)
}

/// The hyphens that may separate a range: every `-` outside a date unit.
///
/// A date unit is the longest run, starting at a token start or just
/// after a candidate hyphen and ending at a hyphen, a space or the end,
/// that core reads as one date with a month, season or sub-year code
/// (N4): a hyphen inside an ISO 8601 extended-format date separates its
/// components, never a range (ISO 8601-1 §3.2.6). A bare year has no
/// component hyphen, so it is not a unit and a minus before it stays a
/// candidate ("1985 -1990"). A colliding sub-year token is not a unit;
/// it is the range grammar's question (N13).
fn range_hyphens(s: &str, opts: Options, lang: &Lang) -> Vec<usize> {
    let mut hyphens = Vec::new();
    let mut i = 0;
    let mut unit_may_start = true;
    while let Some(c) = s[i..].chars().next() {
        if unit_may_start {
            if let Some(end) = date_unit(&s[i..], opts, lang) {
                i += end;
                unit_may_start = false;
                continue;
            }
        }
        unit_may_start = matches!(c, '-' | ' ');
        if c == '-' {
            hyphens.push(i);
        }
        i += c.len_utf8();
    }
    hyphens
}

/// The length of the longest date unit `s` starts with (`range_hyphens`).
fn date_unit(s: &str, opts: Options, lang: &Lang) -> Option<usize> {
    s.char_indices()
        .filter(|&(_, c)| matches!(c, '-' | ' '))
        .map(|(i, _)| i)
        .chain(iter::once(s.len()))
        .rfind(|&end| {
            let t = &s[..end];
            !sub_year_collides(t, opts, lang)
                && matches!(edtf_token(t, lang), Some((Edtf::Date(d), _)) if d.month().is_some())
        })
}

/// Two readings' descriptions, joined for a range built from them.
fn join_readings(left: &str, right: &str) -> String {
    let named: Vec<&str> = [left, right]
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect();
    named.join(" to ")
}

/// Parse both endpoints of a range.
///
/// Per-endpoint qualifiers ("1856-ca. 1865" → `1856/1865~`), elided end
/// years ("1914-18", N4), and bare-ordinal left centuries ("17-19th
/// centuries" → `16XX/18XX`). An endpoint with several readings pairs each
/// with the other side's; a range ambiguous on both sides reports every
/// combination.
#[expect(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "endpoint pairing is one decision table; l/r/a/b/e its algebra"
)]
fn endpoint_pair(left: &str, right: &str, opts: Options, lang: &Lang) -> Option<Reading> {
    let (ls, lq) = strip_qualifiers(left, lang);
    let (rs, rq) = strip_qualifiers(right, lang);
    let lres = parse_single(&ls, opts, lang, true);
    let rres = parse_single(&rs, opts, lang, true);

    let pair: [Reading; 2] = match (lres, rres) {
        (Some(l), Some(r)) => [l, r],
        // "17-19th centuries", "XVII-XIX вв.": a bare ordinal on the left
        // inherits century-ness from the right endpoint.
        (None, Some(r)) => {
            let [
                Candidate {
                    expr: Expr::Date(rd),
                    notes: rnotes,
                    reading: _,
                },
            ] = r.as_slice()
            else {
                return None;
            };
            if !rnotes.contains(&Note::CenturyMask) || ls.contains(' ') {
                return None;
            }
            let (n, roman) = century_ordinal(&ls, lang)?;
            let bc = matches!(
                rd.year().kind(),
                YearKind::Standard {
                    negative: true,
                    digits: _
                }
            );
            let (expr, mut ns) = century_expr(n, bc)?;
            if roman {
                ns.push(Note::RomanCentury);
            }
            [one(expr, ns), r]
        }
        // "1914 - 18": the right side elides the left year's century (N4).
        (Some(l), None) => {
            let [
                Candidate {
                    expr: Expr::Date(ld),
                    reading: _,
                    notes: _,
                },
            ] = l.as_slice()
            else {
                return None;
            };
            let y = i32::try_from(ld.year().value()?).ok()?;
            // Elision needs an AD start. A BC start (year 0 is 1 BC, N3)
            // has no deterministic elided reading — truncating arithmetic
            // would fabricate one (N4).
            if y < 1 {
                return None;
            }
            let v = num(&rs).filter(|_| rs.len() <= 2)?;
            let v_u8 = u8::try_from(v).ok()?;
            let elided = y / 100 * 100 + i32::from(v_u8);
            // Mirror the unspaced NNNN-NN limits: 01–12 read as months and
            // are never ranges (N4); 21–41 collide with sub-year codes (N13).
            if elided <= y || v <= 12 {
                return None;
            }
            // No grammar arm builds a day without a month, so a month-less
            // left endpoint is a bare year — no day check needed.
            if (21..=41).contains(&v) && ld.month().is_none() {
                let season = dated(ld.year(), v_u8, None)?;
                let season = if let Expr::Date(d) = season {
                    Expr::Date(d.qualified(lq))
                } else {
                    season
                };
                let (a, b) = (ld.qualified(lq), date_y(elided)?.qualified(rq));
                return Some(vec![
                    Candidate {
                        expr: season,
                        reading: format!("sub-year code {v} ({})", season_name(v_u8)),
                        notes: vec![Note::SeasonRangeCollision],
                    },
                    Candidate {
                        expr: interval(a, b),
                        reading: format!("year range {y}/{elided}"),
                        notes: vec![Note::ElidedEndYear, Note::SeasonRangeCollision],
                    },
                ]);
            }
            let r = one(Expr::Date(date_y(elided)?), vec![Note::ElidedEndYear]);
            [l, r]
        }
        (None, None) => return None,
    };

    let build = |ld: &Date, rd: &Date| -> Option<Expr> {
        let (ld, rd) = (ld.qualified(lq), rd.qualified(rq));
        if let (Some(a), Some(b)) = (ld.year().value(), rd.year().value()) {
            if a > b {
                return None; // reversed ranges are prose errors, not dates
            }
        }
        Some(interval(ld, rd))
    };
    // "June-July 1940": an endpoint without a year inherits the other
    // endpoint's stated year — the year is elided, not missing (N16);
    // `combine` applies that before this step.
    combine(&pair, |parts| {
        let [l, r] = <[Candidate; 2]>::try_from(parts).ok()?;
        // A range endpoint is a date: an interval, a set or a datetime
        // cannot stand as one (N1, N18).
        let (ld, rd) = match (&l.expr, &r.expr) {
            (Expr::Date(ld), Expr::Date(rd)) => (ld, rd),
            // An impossible endpoint makes the range impossible (N14).
            (Expr::Impossible, Expr::Date(_) | Expr::Impossible)
            | (Expr::Date(_), Expr::Impossible) => {
                return Some(one(Expr::Impossible, Vec::new()));
            }
            _ => return None,
        };
        // "winter 1941-42": one boundary-spanning winter, or winter 1941
        // through the whole of 1942? Both readings, honestly (N17).
        if let Some(both) = cross_year_season(ld, &l.notes, rd, &r.notes, lq, &build) {
            return Some(both);
        }
        let expr = build(ld, rd)?;
        let reading = join_readings(&l.reading, &r.reading);
        let mut notes = l.notes;
        notes.extend(r.notes);
        Some(vec![Candidate {
            expr,
            reading,
            notes,
        }])
    })
}

/// A month or day whose year is not given: a fully masked `XXXX` year with
/// a month (N9).
///
/// It holds whether a prose month masked the year or the input typed it as
/// EDTF. A bare `XXXX` is a year that is unknown, not a year-less date: it
/// has nothing for a stated year to scope. A partly masked year (`XX85`)
/// states a year that is not fully specified.
const fn year_less(d: &Date) -> bool {
    d.month().is_some()
        && matches!(
            d.year().kind(),
            YearKind::Standard {
                negative: _,
                digits: [None, None, None, None],
            }
        )
}

/// N16: a stated year scopes every year-less date beside it — both range
/// endpoints, or every "or" alternative.
///
/// A date is year-less by its value (`year_less`), not by how it was read:
/// a note marks only a prose month's mask, so testing one would let a typed
/// `XXXX` year leak beside a stated one.
///
/// When some but not all dates are year-less, every date that states a year
/// must state the same fully specified one; each year-less date takes it
/// and is noted as such. Fails (→ `NoMatch`) when the stated years disagree
/// or are not fully specified: pairing a masked month with a masked decade
/// would compound the guessing.
fn distribute_year(dates: &mut [(&mut Expr, &mut Vec<Note>)]) -> Option<()> {
    let masked = |e: &Expr| matches!(e, Expr::Date(d) if year_less(d));
    let n_masked = dates.iter().filter(|(e, _)| masked(e)).count();
    if n_masked == 0 || n_masked == dates.len() {
        return Some(());
    }
    let mut stated: Option<(i64, YearKind)> = None;
    for (e, _) in &*dates {
        if masked(e) {
            continue;
        }
        let Expr::Date(d) = e else { continue };
        let v = d.year().value()?;
        match stated {
            None => stated = Some((v, d.year().kind())),
            Some((s, _)) if s == v => {}
            Some(_) => return None,
        }
    }
    let (_, kind) = stated?;
    for (e, n) in &mut *dates {
        let Expr::Date(d) = e else { continue };
        if year_less(d) {
            // The stated year can make the date impossible: February 29
            // in a common year (N14).
            let year = Year::new(kind).ok()?.with_qualifier(d.year().qualifier());
            **e = checked(Date::new(year, d.month(), d.day()));
            n.retain(|note| *note != Note::MissingYearMasked);
            n.push(Note::EndpointYearDistributed);
        }
    }
    Some(())
}

/// N17: "winter 1941-42" (and "winter 1941-1942") — EDTF's winter code 24
/// wraps the year boundary, so the prose may name ONE winter or a
/// winter-to-year range. Emit both readings.
fn cross_year_season(
    ld: &Date,
    ln: &[Note],
    rd: &Date,
    rn: &[Note],
    lq: Qualifier,
    build: &impl Fn(&Date, &Date) -> Option<Expr>,
) -> Option<Reading> {
    const WINTER: u8 = 24;
    // No grammar arm builds a day without a month, so a month-less right
    // endpoint is a bare year — no day check needed.
    if ld.month().and_then(DateField::value) != Some(WINTER) || rd.month().is_some() {
        return None;
    }
    let (a, b) = (ld.year().value()?, rd.year().value()?);
    if b != a + 1 {
        return None;
    }
    let season = ld.qualified(lq);
    let mut season_notes = ln.to_vec();
    season_notes.push(Note::CrossYearSeason);
    let range = build(ld, rd)?;
    let mut range_notes = ln.to_vec();
    range_notes.extend(rn.iter().copied());
    range_notes.push(Note::CrossYearSeason);
    Some(vec![
        Candidate {
            expr: Expr::Date(season),
            reading: String::from("one winter spanning the year boundary"),
            notes: season_notes,
        },
        Candidate {
            expr: range,
            reading: format!("winter {a} through the whole of {b}"),
            notes: range_notes,
        },
    ])
}

// ---------------------------------------------------------------------------
// Qualification and rendering

/// Fold a qualifier into an accumulating one.
const fn merge(into: &mut Qualifier, q: Qualifier) {
    into.uncertain |= q.uncertain;
    into.approximate |= q.approximate;
}

/// Distribute a whole-expression qualifier over every present component;
/// uniform qualification renders as the trailing form ("1940-06~") via
/// core's canonical Display (N10).
const fn qualify_date(d: &mut Date, q: Qualifier) {
    *d = d.qualified(q);
}

/// Distribute a whole-expression qualifier over every date a reading
/// covers (N10).
///
/// A range inside a set cannot carry one (D27): a century
/// part falls back to its century's mask, recording the dropped modifier
/// (N1), and a span with no mask fails. So does a set or a datetime an
/// EDTF token holds, which has no place for one.
fn qualify_candidate(c: &mut Candidate, q: Qualifier) -> Option<()> {
    if !q.is_qualified() {
        return Some(());
    }
    match &mut c.expr {
        Expr::Date(d) => qualify_date(d, q),
        Expr::Interval(iv) => {
            let [mut start, mut end] = [iv.start(), iv.end()];
            for ep in [&mut start, &mut end] {
                match ep {
                    IntervalEndpoint::Date(d)
                    | IntervalEndpoint::OnOrBefore(d)
                    | IntervalEndpoint::OnOrAfter(d) => qualify_date(d, q),
                    IntervalEndpoint::Open | IntervalEndpoint::Unknown => {}
                }
            }
            // Qualification never moves a bound, so the order still holds.
            *iv = Interval::new(start, end).ok()?;
            c.notes.push(Note::QualifierDistributed);
        }
        Expr::Part { mask, lo: _, hi: _ } => {
            let mut d = (*mask)?;
            qualify_date(&mut d, q);
            c.expr = Expr::Date(d);
            for n in &mut c.notes {
                if *n == Note::CenturyPart {
                    *n = Note::CenturyMask;
                }
            }
            c.notes.push(Note::ModifierDropped);
        }
        Expr::OneOf(elements) => {
            for e in elements {
                match e {
                    SetElement::Date(d) | SetElement::OnOrBefore(d) | SetElement::OnOrAfter(d) => {
                        qualify_date(d, q);
                    }
                    SetElement::Range(..) => return None,
                }
            }
        }
        Expr::Value(_) => return None,
        Expr::Impossible => {}
    }
    Some(())
}

/// The value a reading denotes and its canonical text, or `None` for the
/// impossible reading. Every value was built through core's constructors,
/// so it is valid EDTF at a known level by construction.
fn render(expr: Expr) -> Option<(Edtf, String)> {
    let one_member = |elements| Set::new(SetKind::OneMember, elements).ok().map(Edtf::Set);
    let value = match expr {
        Expr::Date(d) => Edtf::Date(d),
        Expr::Interval(iv) => Edtf::Interval(iv),
        Expr::Part { lo, hi, mask: _ } => {
            one_member(vec![SetElement::Range(SetRange::new(lo, hi).ok()?)])?
        }
        Expr::OneOf(elements) => one_member(elements)?,
        Expr::Value(v) => v,
        Expr::Impossible => return None,
    };
    let s = value.to_string();
    Some((value, s))
}

/// An outcome that matched nothing, with the reason.
const fn no_match(reason: NoMatchReason) -> Outcome {
    Outcome::NoMatch { reason }
}

/// Build the public outcome from a reading.
fn outcome_from(reading: Reading) -> Outcome {
    // Fail closed: a reading that dies at the calendar check (February 30,
    // a reversed masked range) is a prose error, and silently promoting the
    // survivor would fake certainty the input never had (N14).
    let total = reading.len();
    let mut interps: Vec<Interpretation> = reading
        .into_iter()
        .filter_map(|c| {
            let (value, edtf) = render(c.expr)?;
            Some(Interpretation {
                edtf,
                value,
                reading: c.reading,
                notes: c.notes,
            })
        })
        .collect();
    if interps.len() < total {
        return no_match(NoMatchReason::ImpossibleDate);
    }
    // Two readings that render the same are one reading: both of N17's
    // readings of "before winter 1941-42" start in one winter.
    let mut seen: Vec<String> = Vec::new();
    interps.retain(|i| {
        let fresh = !seen.contains(&i.edtf);
        if fresh {
            seen.push(i.edtf.clone());
        }
        fresh
    });
    // A reading is never empty, so at least one interpretation survives.
    if interps.len() == 1 {
        let i = interps.remove(0);
        Outcome::Normalized(Normalized {
            edtf: i.edtf,
            value: i.value,
            notes: i.notes,
        })
    } else {
        Outcome::Ambiguous(Ambiguous {
            interpretations: interps,
        })
    }
}

// ---------------------------------------------------------------------------
// Entry point

/// Normalise one input under `opts`; the crate's whole entry point.
///
/// A whole input is not special: it is preprocessed, stripped of its
/// whole-expression qualifiers and read like any "or" alternative, so a
/// one-token input that is EDTF reads through the atom reader (N18).
pub(crate) fn run(input: &str, opts: Options) -> Outcome {
    let lang = lang_for(opts.language);
    let pre = preprocess(input, opts, lang);
    let (rest, q) = strip_qualifiers(&pre, lang);
    if rest.is_empty() {
        return no_match(NoMatchReason::OutOfGrammar);
    }
    if lang.explicit_no_date.contains(&rest.as_str()) {
        // The form decides what "unknown" means; the reason lets it (N12).
        return no_match(NoMatchReason::ExplicitNoDate);
    }
    let toks: Vec<&str> = rest.split(' ').collect();
    // An "or" list takes the whole input's qualifier on each alternative,
    // before its set is built; any other reading takes it here.
    let (mut reading, qualified) = match parse_reading(&rest, opts, lang) {
        Some(reading) => (reading, false),
        None => match parse_or(&toks, opts, lang, q) {
            Some(reading) => (reading, true),
            None => return no_match(NoMatchReason::OutOfGrammar),
        },
    };
    // Already-valid EDTF is noted only where the EDTF token is the whole
    // input, with nothing stripped from around it (N18).
    let whole = rest == pre;
    for c in &mut reading {
        if !whole {
            c.notes.retain(|n| *n != Note::AlreadyValidEdtf);
        }
        if !qualified && qualify_candidate(c, q).is_none() {
            return no_match(NoMatchReason::OutOfGrammar);
        }
    }
    outcome_from(reading)
}
