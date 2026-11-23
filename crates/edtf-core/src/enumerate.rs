// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Enumeration of the concrete calendar values an expression denotes.
//!
//! ISO 8601-2 grounds exactly three value-set constructions, and
//! [`Edtf::values`] enumerates those and nothing else:
//!
//! - set range elements expand inclusively at their own precision (§6.3 c, §6.4
//!   Example 1: `{1667,1668,1670..1672}` *is* `{1667,1668,1670,1671, 1672}`);
//! - unspecified digits denote their valid completions (§9.2.2 Example 6:
//!   `1560-X2` is "either February or December");
//! - significant-digit years denote a swept year range (§4.4.3: `1950S2` is
//!   some year 1900–1999).
//!
//! Everything else is a singleton (a concrete date, datetime, season or
//! `Y`-year yields itself, verbatim) or [`Unenumerable`]: intervals denote
//! one continuous extent — Clause 10 has no enumeration language — and the
//! `..`-prefixed/suffixed set elements of §6.3 a–b are "indication", never
//! "expansion" (§6.4 Example 2 pointedly leaves `1760-12..` unexpanded).
//!
//! Values are yielded lazily in written element order, ascending within
//! each element, with qualifiers copied through unchanged (qualification
//! never moves the value set, §8.4.2 NOTE). Decisions D24–D29 in
//! docs/spec-notes.md.

#![expect(
    clippy::min_ident_chars,
    reason = "y, m and d are the universal notation for a date's components, and this crate is about little else"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "every flagged operation is bounded where it stands: slice indices by a length guard on the line above, digit values to 0-9 by the match arm that binds them, and the JDN forms by being computed in i128 so any i64 year fits. The operations that genuinely could leave range already use checked_/saturating_ and return an error rather than wrapping"
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
    clippy::as_conversions,
    reason = "the operands are proven in range by the guard or type immediately above each cast, and try_from at these sites would add an unreachable error path"
)]
#![expect(
    clippy::absolute_paths,
    reason = "a one-use std path written in full at the call site is clearer than an import that only appears once"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "each index is preceded by the bounds check that justifies it, or indexes a fixed-size array whose length is in its type"
)]
#![expect(
    clippy::missing_inline_in_public_items,
    reason = "inlining is the compiler's call across a crate boundary, and the release profile enables fat LTO — annotating every public item would assert a decision this crate has not measured"
)]
#![expect(
    clippy::missing_trait_methods,
    reason = "the default implementations are what this type wants; overriding them to satisfy a lint would be code with no reason to exist"
)]
#![expect(
    clippy::unreachable,
    reason = "an unreachable! whose comment names the caller-side check that makes it unreachable — a deliberate assertion of an invariant, not an unhandled case"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "these enums enumerate what ISO 8601-2 defines; the spec fixes the variants, so non_exhaustive would promise additions the format cannot make"
)]
#![expect(
    clippy::too_long_first_doc_paragraph,
    reason = "the items are crate-private, so these paragraphs render in no rustdoc summary; they are written to be read next to the code they describe"
)]

use alloc::vec::Vec;

use crate::{
    bounds::{date_span, day_candidates_of, is_leap, last_day, month_candidates_of},
    types::{Date, DateField, Edtf, Qualifier, SetElement, SetRange, ValueError, Year, YearKind},
};

/// Why [`Edtf::values`] cannot enumerate an expression (decisions D24–D25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unenumerable {
    /// Intervals denote a single continuous extent, not a collection of
    /// values (ISO 8601-2 Clause 10 defines no enumeration for them).
    Interval,
    /// The set contains a `..date` or `date..` element, whose value set is
    /// unbounded (§6.3 a–b give "indication", not "expansion").
    UnboundedSetElement,
}

impl core::fmt::Display for Unenumerable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Interval => "intervals denote an extent, not enumerable values",
            Self::UnboundedSetElement => "'..'-open set elements denote unbounded value sets",
        })
    }
}

impl core::error::Error for Unenumerable {}

impl Edtf {
    /// Enumerate the concrete calendar values this expression denotes, each
    /// at its own precision (decisions D24–D29 in docs/spec-notes.md).
    ///
    /// The iterator is lazy — state is proportional to the number of set
    /// elements, never to the number of values — so pathological
    /// cardinalities (`XXXX-XX-XX` denotes ~3.65 million days) stream
    /// without allocation. Errors are structural and complete at
    /// construction: iteration itself cannot fail.
    ///
    /// ```
    /// use edtf_core::Edtf;
    ///
    /// // ISO 8601-2 §6.4 Example 1, expanded exactly as the spec does:
    /// let set = Edtf::parse("{1667,1668,1670..1672}").unwrap();
    /// let years: Vec<String> = set.values().unwrap().map(|v| v.to_string()).collect();
    /// assert_eq!(years, ["1667", "1668", "1670", "1671", "1672"]);
    ///
    /// // Unspecified digits enumerate their valid completions (§9.2.2):
    /// let masked = Edtf::parse("1985-0X-31").unwrap();
    /// let months: Vec<String> = masked.values().unwrap().map(|v| v.to_string()).collect();
    /// assert_eq!(
    ///     months,
    ///     [
    ///         "1985-01-31",
    ///         "1985-03-31",
    ///         "1985-05-31",
    ///         "1985-07-31",
    ///         "1985-08-31"
    ///     ]
    /// );
    ///
    /// // Intervals denote one extent, not a collection:
    /// assert!(Edtf::parse("2004/2005").unwrap().values().is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// [`Unenumerable`] when the value denotes no finite collection: an
    /// interval, or a set with an unbounded `..date`/`date..` element. Every
    /// year sweep fits the numeric range this library computes with
    /// (decision D30).
    pub fn values(&self) -> Result<Values, Unenumerable> {
        let state = match self {
            Self::Interval(_) => return Err(Unenumerable::Interval),
            Self::DateTime(dt) => State::Singleton(Some(Self::DateTime(*dt))),
            Self::Date(d) => State::Date(DateValues::new(d)),
            Self::Set(s) => {
                let mut queue = Vec::with_capacity(s.elements().len());
                for e in s.elements() {
                    queue.push(match e {
                        SetElement::Date(d) => ElementValues::Date(DateValues::new(d)),
                        SetElement::Range(r) => ElementValues::Range(RangeWalk::new(r)),
                        SetElement::OnOrBefore(_) | SetElement::OnOrAfter(_) => {
                            return Err(Unenumerable::UnboundedSetElement);
                        }
                    });
                }
                State::Set { queue, idx: 0 }
            }
        };
        Ok(Values { state })
    }
}

/// Lazy iterator over the values an expression denotes; see [`Edtf::values`].
#[derive(Debug, Clone)]
pub struct Values {
    /// What the iterator is currently walking.
    state: State,
}

#[derive(Debug, Clone)]
/// The shape of value the expression enumerates.
enum State {
    /// A concrete expression: itself, once.
    Singleton(Option<Edtf>),
    /// A single date with masked or ranged fields.
    Date(DateValues),
    /// A set: each element walked in written order.
    Set {
        /// Element walkers still to be drained, in written order.
        queue: Vec<ElementValues>,
        /// Index of the element currently being drained.
        idx: usize,
    },
}

#[derive(Debug, Clone)]
/// How one set element enumerates.
enum ElementValues {
    /// A date element, masked or concrete.
    Date(DateValues),
    /// A range element, walked endpoint to endpoint.
    Range(RangeWalk),
}

impl ElementValues {
    /// The next date this element yields, or `None` when drained.
    fn next(&mut self) -> Option<Date> {
        match self {
            Self::Date(dv) => dv.next(),
            Self::Range(rw) => rw.next(),
        }
    }
}

impl Iterator for Values {
    type Item = Edtf;

    fn next(&mut self) -> Option<Edtf> {
        match &mut self.state {
            State::Singleton(v) => v.take(),
            State::Date(dv) => dv.next().map(Edtf::Date),
            State::Set { queue, idx } => loop {
                let cur = queue.get_mut(*idx)?;
                if let Some(d) = cur.next() {
                    return Some(Edtf::Date(d));
                }
                *idx += 1;
            },
        }
    }
}

/// A value enumeration builds from a valid one. Every value it yields is a
/// completion or a step of a valid value, so each constructor accepts it.
fn built<T>(value: Result<T, ValueError>) -> T {
    value.unwrap_or_else(|e| unreachable!("enumeration builds only valid values: {e}"))
}

/// Rebuild a concrete year value in its canonical syntactic form: four
/// digits within ±9999 (D2: `-0000` cannot arise — the value would be 0),
/// `Y`-prefixed beyond (D1).
fn concrete_year(v: i64, qualifier: Qualifier) -> Year {
    built(Year::from_value(v)).with_qualifier(qualifier)
}

/// A concrete value as a `DateField`, carrying the field's qualifier.
fn concrete_field(v: u8, qualifier: Qualifier) -> DateField {
    built(DateField::from_value(v)).with_qualifier(qualifier)
}

/// A concrete date of these components.
fn concrete_date(year: Year, month: Option<DateField>, day: Option<DateField>) -> Date {
    built(Date::new(year, month, day))
}

/// Values of a single (possibly masked or significant-digit) date.
#[derive(Debug, Clone)]
enum DateValues {
    /// Fully specified: yields itself, verbatim, once.
    Singleton(Option<Date>),
    /// `S`-suffix year sweep (§4.4.3), ascending, year precision.
    Sweep {
        /// The year currently being yielded.
        cur: i64,
        /// The last year in the walk, inclusive.
        hi: i64,
        /// The qualifier every yielded date carries.
        qualifier: Qualifier,
        /// Set once the walk has passed `hi`.
        done: bool,
    },
    /// Unspecified digits: valid completions in ascending calendar order.
    Masked(Masked),
}

impl DateValues {
    /// Build a walker for one date, or say why it cannot be enumerated.
    fn new(d: &Date) -> Self {
        let year = d.year();
        if year.significant_digits().is_some() {
            let (lo, hi) = date_span(d);
            return Self::Sweep {
                cur: lo.year(),
                hi: hi.year(),
                qualifier: year.qualifier(),
                done: false,
            };
        }
        if !d.has_unspecified() {
            return Self::Singleton(Some(*d));
        }
        Self::Masked(Masked::new(d))
    }

    /// The next value, if any.
    fn next(&mut self) -> Option<Date> {
        match self {
            Self::Singleton(d) => d.take(),
            Self::Sweep {
                cur,
                hi,
                qualifier,
                done,
            } => {
                if *done {
                    return None;
                }
                let v = *cur;
                if v == *hi {
                    *done = true;
                } else {
                    *cur += 1;
                }
                Some(concrete_date(concrete_year(v, *qualifier), None, None))
            }
            Self::Masked(m) => m.next(),
        }
    }
}

/// Odometer over the valid completions of a masked date: years ascend
/// (least-significant masked digit fastest, so consecutive counters yield
/// consecutive matching years), months and days ascend within each year,
/// and calendar-invalid day combinations are skipped. Validation (D11)
/// guarantees at least one completion overall; masked years are
/// non-negative (D21) and masked months draw from 01–12 only (D14).
#[derive(Debug, Clone)]
enum MaskedYear {
    /// Concrete (possibly negative) year value.
    Fixed(i64),
    /// Masked digit pattern to sweep (non-negative, D21).
    Pattern([Option<u8>; 4]),
}

#[derive(Debug, Clone)]
/// Odometer over a masked date's free digits, most significant first.
struct Masked {
    /// The masked year and the completions it admits.
    year: MaskedYear,
    /// How many year completions the mask has.
    year_count: u32,
    /// Month candidates, when the month itself is masked.
    months: Option<Vec<u8>>,
    /// Day candidates, when the day itself is masked.
    days: Option<Vec<u8>>,
    /// Qualifier carried by every yielded year.
    yq: Qualifier,
    /// Qualifier carried by every yielded month.
    mq: Qualifier,
    /// Qualifier carried by every yielded day.
    dq: Qualifier,
    /// Position in the year odometer.
    yi: u32,
    /// Position in the month candidates.
    mi: usize,
    /// Position in the day candidates.
    di: usize,
}

impl Masked {
    /// Build the odometer for a masked date.
    fn new(d: &Date) -> Self {
        // Masks only occur on four-digit years; any other year is fixed, and
        // a fixed year is its own earliest year.
        let (year, year_count) = match (d.year().value(), d.year().kind()) {
            (
                None,
                YearKind::Standard {
                    digits,
                    negative: _,
                },
            ) => {
                // At most four maskable digit positions.
                let n = u32::try_from(digits.iter().filter(|x| x.is_none()).count()).unwrap_or(4);
                (MaskedYear::Pattern(digits), 10_u32.pow(n))
            }
            _ => (MaskedYear::Fixed(date_span(d).0.year()), 1),
        };
        Self {
            year,
            year_count,
            months: d.month().map(month_candidates_of),
            days: d.day().map(day_candidates_of),
            yq: d.year().qualifier(),
            mq: d.month().map(DateField::qualifier).unwrap_or_default(),
            dq: d.day().map(DateField::qualifier).unwrap_or_default(),
            yi: 0,
            mi: 0,
            di: 0,
        }
    }

    /// The year the odometer's `counter`th position denotes.
    fn year_at(&self, counter: u32) -> i64 {
        match self.year {
            MaskedYear::Fixed(v) => v,
            MaskedYear::Pattern(digits) => {
                let mut rem = counter;
                let mut filled = digits.map(|d| d.unwrap_or(0));
                for pos in (0..4).rev() {
                    if digits[pos].is_none() {
                        filled[pos] = (rem % 10) as u8;
                        rem /= 10;
                    }
                }
                filled.iter().fold(0, |acc, d| acc * 10 + i64::from(*d))
            }
        }
    }

    /// The next completion, or `None` once every position is spent.
    fn next(&mut self) -> Option<Date> {
        loop {
            if self.yi >= self.year_count {
                return None;
            }
            let y = self.year_at(self.yi);
            let Some(months) = &self.months else {
                self.yi += 1;
                return Some(concrete_date(concrete_year(y, self.yq), None, None));
            };
            while self.mi < months.len() {
                let m = months[self.mi];
                let Some(days) = &self.days else {
                    self.mi += 1;
                    // `m` may be a season code 21–41 (only when written
                    // concretely — masked slots draw from 01–12, D14).
                    return Some(concrete_date(
                        concrete_year(y, self.yq),
                        Some(concrete_field(m, self.mq)),
                        None,
                    ));
                };
                while self.di < days.len() {
                    let dd = days[self.di];
                    self.di += 1;
                    if dd <= last_day(m, is_leap(y)) {
                        return Some(concrete_date(
                            concrete_year(y, self.yq),
                            Some(concrete_field(m, self.mq)),
                            Some(concrete_field(dd, self.dq)),
                        ));
                    }
                }
                self.di = 0;
                self.mi += 1;
            }
            self.mi = 0;
            self.yi += 1;
        }
    }
}

/// Inclusive expansion of a set range element `a..b` (§6.3 c, §6.4) by
/// calendar successor at the endpoints' shared precision. A [`SetRange`]
/// holds concrete, unqualified, non-season, same-precision dates in order
/// (D27, D18), so the walk terminates by reaching `b` exactly.
#[derive(Debug, Clone)]
struct RangeWalk {
    /// Current position; slots beyond the precision stay 0 so plain
    /// tuple equality detects the end.
    cur: (i64, u8, u8),
    /// The last (year, month, day) in the walk, inclusive.
    end: (i64, u8, u8),
    /// The unit the walk steps by.
    step: Step,
    /// Set once the walk has passed `end`.
    done: bool,
}

/// The unit a range walks by: its endpoints' precision. A range endpoint is
/// never a season (D27), so a month is a calendar month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// One year.
    Year,
    /// One calendar month.
    Month,
    /// One calendar day.
    Day,
}

impl RangeWalk {
    /// Build a walker between a range's two endpoints.
    fn new(r: &SetRange) -> Self {
        let (a, b) = (r.start(), r.end());
        let step = if a.day().is_some() {
            Step::Day
        } else if a.month().is_some() {
            Step::Month
        } else {
            Step::Year
        };
        // An unmasked date's earliest day carries its year, month and day.
        let position = |d: &Date| {
            let first = date_span(d).0;
            (
                first.year(),
                if step == Step::Year { 0 } else { first.month() },
                if step == Step::Day { first.day() } else { 0 },
            )
        };
        Self {
            cur: position(&a),
            end: position(&b),
            step,
            done: false,
        }
    }

    /// The next value in the walk, if any.
    fn next(&mut self) -> Option<Date> {
        if self.done {
            return None;
        }
        let (y, m, d) = self.cur;
        if self.cur == self.end {
            self.done = true;
        } else {
            self.cur = match self.step {
                Step::Year => (y + 1, 0, 0),
                Step::Month => {
                    if m == 12 {
                        (y + 1, 1, 0)
                    } else {
                        (y, m + 1, 0)
                    }
                }
                Step::Day => {
                    if d < last_day(m, is_leap(y)) {
                        (y, m, d + 1)
                    } else if m == 12 {
                        (y + 1, 1, 1)
                    } else {
                        (y, m + 1, 1)
                    }
                }
            };
        }
        let q = Qualifier::default();
        Some(concrete_date(
            concrete_year(y, q),
            (self.step != Step::Year).then(|| concrete_field(m, q)),
            (self.step == Step::Day).then(|| concrete_field(d, q)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enumeration rebuilds values from valid ones, so a constructor's
    /// refusal is a broken invariant: it panics, naming the rule, rather
    /// than yield a wrong value or stop early.
    #[test]
    #[should_panic(expected = "enumeration builds only valid values: a day requires a month")]
    fn a_refused_rebuild_panics_naming_the_rule() {
        let refused: Result<Date, ValueError> = Err(ValueError::new(ValueError::DAY_WITHOUT_MONTH));
        let _refused = built(refused);
    }
}
