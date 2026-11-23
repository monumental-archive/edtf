// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Postgres extension exposing [`edtf_core`] in SQL — the same validator the
//! application runs via WebAssembly, so the two layers can never diverge.
//!
//! SQL surface:
//! - `edtf_valid(text) → boolean`
//! - `edtf_level(text) → integer` (0/1/2, NULL if invalid)
//! - `edtf_canonical(text) → text` (spec-preferred form, NULL if invalid)
//! - `edtf_min(text) → date` / `edtf_max(text) → date` — earliest/latest
//!   calendar day, converted as a day count (the bound's Julian Day Number),
//!   so astronomical year 0 is 1 BC. Open ends map to `-infinity`/`infinity`;
//!   a day before Postgres's floor (4714-11-24 BC) saturates to `-infinity`
//!   and one past its ceiling (5874897-12-31) to `infinity`; only an unknown
//!   end is NULL.
//! - `edtf_relation(text, text) → text[]` — three-valued temporal relation: one
//!   entry per non-impossible relation, `definitely_<r>` when it holds for
//!   every completion, `possibly_<r>` otherwise.

// Every suppression in this crate is an item-level `#[expect]`, never an
// inner one at the top of a module: an inner attribute does not survive
// `#[pg_schema]`, because syn re-emits it as an outer attribute and the
// build dies, while plain rustc accepts the same source (edtf#185).

#[cfg(test)]
pub mod pg_test;

use edtf_core::{Bound, Edtf, Modality};
use pgrx::{datetime::Date, pg_extern, pg_sys};

::pgrx::pg_module_magic!(name, version);

/// True if the input is valid EDTF (ISO 8601-2:2019 Annex A, levels 0-2).
#[pg_extern(immutable, parallel_safe, strict)]
fn edtf_valid(input: &str) -> bool {
    edtf_core::is_valid(input)
}

/// Minimum EDTF conformance level (0, 1 or 2); NULL if invalid.
#[pg_extern(immutable, parallel_safe, strict)]
fn edtf_level(input: &str) -> Option<i32> {
    edtf_core::level(input).map(i32::from)
}

/// Canonical (spec-preferred) rendering; NULL if invalid.
#[pg_extern(immutable, parallel_safe, strict)]
fn edtf_canonical(input: &str) -> Option<String> {
    Some(Edtf::parse(input).ok()?.to_string())
}

/// A bound as a Postgres `date`, converted day to day.
///
/// A Postgres `date` is a count of days from 2000-01-01, and an edtf-core
/// bound is a proleptic-Gregorian day with a Julian Day Number, so the
/// conversion is a subtraction, with no detour through year/month/day
/// notation and its BC numbering (astronomical year 0 is 1 BC, `-4712` is
/// 4713 BC).
///
/// Postgres holds 4714-11-24 BC (Julian day 0) to 5874897-12-31. A day
/// before that floor saturates to `-infinity` and one past that ceiling to
/// `infinity`, so the bound still compares, contains and overlaps correctly
/// against every date Postgres can hold. NULL means one thing: the bound is
/// unknown.
fn to_pg_date(bound: Bound) -> Option<Date> {
    match bound {
        Bound::NegativeInfinity => Some(Date::negative_infinity()),
        Bound::PositiveInfinity => Some(Date::positive_infinity()),
        Bound::Unknown => None,
        Bound::Date(date) => {
            let jdn = date.julian_day_number();
            if jdn < i128::from(pg_sys::DATETIME_MIN_JULIAN) {
                Some(Date::negative_infinity())
            } else if jdn >= i128::from(pg_sys::DATE_END_JULIAN) {
                Some(Date::positive_infinity())
            } else {
                let days = jdn.checked_sub(i128::from(pg_sys::POSTGRES_EPOCH_JDATE))?;
                Date::try_from(i32::try_from(days).ok()?).ok()
            }
        }
    }
}

/// Earliest calendar day the expression touches; `-infinity` for open
/// starts and for a day before the Postgres date range (4714-11-24 BC);
/// NULL when invalid or unknown.
#[pg_extern(immutable, parallel_safe, strict)]
fn edtf_min(input: &str) -> Option<Date> {
    to_pg_date(Edtf::parse(input).ok()?.bounds().earliest)
}

/// Latest calendar day the expression touches; `infinity` for open ends
/// and for a day past the Postgres date range (5874897-12-31); NULL when
/// invalid or unknown.
#[pg_extern(immutable, parallel_safe, strict)]
fn edtf_max(input: &str) -> Option<Date> {
    to_pg_date(Edtf::parse(input).ok()?.bounds().latest)
}

/// Three-valued temporal relation between two EDTF expressions.
///
/// Semantics: `docs/spec-notes.md` D23. One entry per relation that at least
/// some completion pair satisfies, in canonical order (before, after,
/// overlaps, contains, within, equal): `definitely_<r>` when every completion
/// pair satisfies it, else `possibly_<r>`. Unknown bounds yield all six as
/// `possibly_`. NULL if either input is invalid. A consistency rule reads:
/// `NOT ('definitely_after' = ANY(edtf_relation(born, died)))`.
#[pg_extern(immutable, parallel_safe, strict)]
#[expect(
    clippy::min_ident_chars,
    reason = "pgrx derives the SQL argument names from these idents: `edtf_relation(\"a\" TEXT, \"b\" TEXT)` is the published surface (schema.snapshot.sql), so renaming them breaks named-notation callers"
)]
fn edtf_relation(a: &str, b: &str) -> Option<Vec<String>> {
    let relations = Edtf::parse(a).ok()?.relation(&Edtf::parse(b).ok()?);
    Some(
        relations
            .possible()
            .map(|relation| {
                let adverb = match relations.modality(relation) {
                    Modality::Definite => "definitely",
                    // `possible()` yields only non-impossible relations, so
                    // this arm is reached for `Possible` alone; `Impossible`
                    // is named rather than wildcarded so a new variant is a
                    // compile error here.
                    Modality::Possible | Modality::Impossible => "possibly",
                };
                format!("{adverb}_{}", relation.as_str())
            })
            .collect(),
    )
}

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::{Spi, pg_test};

    /// The shared conformance corpus (`tests/corpus.sql`).
    ///
    /// The prebuilt-tarball smoke test also runs it with `psql -f` against a
    /// real install. Running it from here too is what keeps the binary held
    /// to the same standard as the source: one set of assertions, two
    /// callers, nothing to drift. Each check is a plpgsql `ASSERT`, so a
    /// failure surfaces as an SPI error carrying the message.
    ///
    /// A new SQL assertion goes in `corpus.sql`, never in a new pgrx test.
    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn"
    )]
    #[expect(
        clippy::expect_used,
        reason = "test code: a panic here is the failure signal, not a crash path"
    )]
    fn shared_corpus() {
        Spi::run(include_str!("../tests/corpus.sql")).expect("shared corpus");
    }
}
