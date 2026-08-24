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
//!   calendar day. Open ends map to `-infinity`/`infinity`; unknown ends and
//!   years outside the Postgres date range map to NULL.
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
use pgrx::{datetime::Date, pg_extern};

::pgrx::pg_module_magic!(name, version);

/// True if the input is valid EDTF (ISO 8601-2:2019 Annex A, levels 0-2).
#[pg_extern(immutable, parallel_safe, strict)]
#[expect(
    clippy::single_call_fn,
    reason = "pgrx generates the `_wrapper` Postgres calls, and it is this function's only caller"
)]
fn edtf_valid(input: &str) -> bool {
    edtf_core::is_valid(input)
}

/// Minimum EDTF conformance level (0, 1 or 2); NULL if invalid.
#[pg_extern(immutable, parallel_safe, strict)]
#[expect(
    clippy::single_call_fn,
    reason = "pgrx generates the `_wrapper` Postgres calls, and it is this function's only caller"
)]
fn edtf_level(input: &str) -> Option<i32> {
    edtf_core::level(input).map(i32::from)
}

/// Canonical (spec-preferred) rendering; NULL if invalid.
#[pg_extern(immutable, parallel_safe, strict)]
#[expect(
    clippy::single_call_fn,
    reason = "pgrx generates the `_wrapper` Postgres calls, and it is this function's only caller"
)]
fn edtf_canonical(input: &str) -> Option<String> {
    Some(Edtf::parse(input).ok()?.to_string())
}

/// Postgres `date` range is 4713-01-01 BC .. 5874897-12-31 AD. In the
/// astronomical numbering edtf-core uses (year 0 exists), 4713 BC is -4712.
fn to_pg_date(bound: Bound) -> Option<Date> {
    match bound {
        Bound::NegativeInfinity => Some(Date::negative_infinity()),
        Bound::PositiveInfinity => Some(Date::positive_infinity()),
        Bound::Unknown => None,
        Bound::Date(date) => {
            if date.year < -4712 || date.year > 5_874_897 {
                return None;
            }
            let year = i32::try_from(date.year).ok()?;
            Date::new(year, date.month, date.day).ok()
        }
    }
}

/// Earliest calendar day the expression touches; `-infinity` for open
/// starts; NULL when invalid, unknown, or before the Postgres date range.
#[pg_extern(immutable, parallel_safe, strict)]
#[expect(
    clippy::single_call_fn,
    reason = "pgrx generates the `_wrapper` Postgres calls, and it is this function's only caller"
)]
fn edtf_min(input: &str) -> Option<Date> {
    to_pg_date(Edtf::parse(input).ok()?.bounds().earliest)
}

/// Latest calendar day the expression touches; `infinity` for open ends;
/// NULL when invalid, unknown, or beyond the Postgres date range.
#[pg_extern(immutable, parallel_safe, strict)]
#[expect(
    clippy::single_call_fn,
    reason = "pgrx generates the `_wrapper` Postgres calls, and it is this function's only caller"
)]
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
    clippy::single_call_fn,
    reason = "pgrx generates the `_wrapper` Postgres calls, and it is this function's only caller"
)]
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

    /// Runs `sql` and returns its single non-null `boolean` column.
    ///
    /// # Panics
    ///
    /// If the query errors or returns NULL. Both are the failure signal a
    /// caller in this module wants: the assertion never gets to run.
    #[expect(
        clippy::expect_used,
        reason = "test code: a panic here is the failure signal, not a crash path"
    )]
    fn q_bool(sql: &str) -> bool {
        Spi::get_one(sql).expect("SPI").expect("non-null")
    }

    /// Runs `sql` and returns its single `text` column, NULL included.
    ///
    /// # Panics
    ///
    /// If the query errors. That is the failure signal a caller in this
    /// module wants: the assertion never gets to run.
    #[expect(
        clippy::expect_used,
        reason = "test code: a panic here is the failure signal, not a crash path"
    )]
    fn q_text(sql: &str) -> Option<String> {
        Spi::get_one(sql).expect("SPI")
    }

    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        clippy::single_call_fn,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn, and pgrx generates the harness entry point that is its only caller"
    )]
    fn valid_and_invalid() {
        assert!(
            q_bool("SELECT edtf_valid('1985-04-12')"),
            "a complete level 0 date is valid"
        );
        assert!(
            q_bool("SELECT edtf_valid('2004-06~-11')"),
            "a level 2 qualified date is valid"
        );
        assert!(
            q_bool("SELECT edtf_valid('{1667,1668,1670..1672}')"),
            "a level 2 set representation is valid"
        );
        assert!(
            !q_bool("SELECT edtf_valid('1985-02-30')"),
            "1985-02-30 is not a calendar day"
        );
        assert!(
            !q_bool("SELECT edtf_valid('19850412')"),
            "EDTF requires the extended (hyphenated) format"
        );
        assert!(
            !q_bool("SELECT edtf_valid('2004/2003')"),
            "an interval may not end before it starts"
        );
    }

    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        clippy::single_call_fn,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn, and pgrx generates the harness entry point that is its only caller"
    )]
    #[expect(
        clippy::unwrap_used,
        reason = "test code: a panic here is the failure signal, not a crash path"
    )]
    fn levels() {
        assert_eq!(
            Spi::get_one::<i32>("SELECT edtf_level('1985-04-12')").unwrap(),
            Some(0_i32),
            "a complete date is level 0"
        );
        assert_eq!(
            Spi::get_one::<i32>("SELECT edtf_level('1985~')").unwrap(),
            Some(1_i32),
            "an approximate year is level 1"
        );
        assert_eq!(
            Spi::get_one::<i32>("SELECT edtf_level('156X-12-25')").unwrap(),
            Some(2_i32),
            "an unspecified digit in the year is level 2"
        );
        assert_eq!(
            Spi::get_one::<i32>("SELECT edtf_level('junk')").unwrap(),
            None,
            "an invalid expression has no level"
        );
    }

    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        clippy::single_call_fn,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn, and pgrx generates the harness entry point that is its only caller"
    )]
    fn canonicalization() {
        assert_eq!(
            q_text("SELECT edtf_canonical('?2004-?06-?11')").as_deref(),
            Some("2004-06-11?"),
            "per-component uncertainty collapses to the whole-date qualifier"
        );
    }

    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        clippy::single_call_fn,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn, and pgrx generates the harness entry point that is its only caller"
    )]
    fn bounds_as_dates() {
        assert_eq!(
            q_text("SELECT edtf_min('1985')::text").as_deref(),
            Some("1985-01-01"),
            "a year's earliest day is its first"
        );
        assert_eq!(
            q_text("SELECT edtf_max('1985')::text").as_deref(),
            Some("1985-12-31"),
            "a year's latest day is its last"
        );
        assert_eq!(
            q_text("SELECT edtf_max('2003-24')::text").as_deref(),
            Some("2004-02-29"),
            "northern winter wraps into the next year, here a leap year"
        );
        assert_eq!(
            q_text("SELECT edtf_max('1985-04-12/..')::text").as_deref(),
            Some("infinity"),
            "an open end maps to Postgres `infinity`"
        );
        assert_eq!(
            q_text("SELECT edtf_min('../1985')::text").as_deref(),
            Some("-infinity"),
            "an open start maps to Postgres `-infinity`"
        );
        // Unknown end and out-of-range years are NULL. Both ends of the
        // range guard in `to_pg_date`, not just the upper one: the lower
        // side is the only arm of this crate that a suite testing only
        // `Y17E7` leaves untaken.
        assert_eq!(
            q_text("SELECT edtf_max('1986-04/')::text"),
            None,
            "an unknown end is NULL, not infinity"
        );
        assert_eq!(
            q_text("SELECT edtf_min('Y17E7')::text"),
            None,
            "a year beyond the Postgres date range is NULL"
        );
        assert_eq!(
            q_text("SELECT edtf_min('Y-17E7')::text"),
            None,
            "a year before the Postgres date range is NULL"
        );
    }

    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        clippy::single_call_fn,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn, and pgrx generates the harness entry point that is its only caller"
    )]
    fn relations() {
        assert!(
            q_bool("SELECT edtf_relation('1985~', '199X') = ARRAY['definitely_before']"),
            "disjoint ranges relate definitely, and only one way"
        );
        assert!(
            q_bool(
                "SELECT edtf_relation('198X', '1985') = ARRAY[\
                'possibly_before','possibly_after','possibly_overlaps',\
                'possibly_contains','possibly_within','possibly_equal']"
            ),
            "a decade against a year inside it leaves every relation possible"
        );
        // Unknown interval end: possible-everything, never definite.
        assert!(
            q_bool("SELECT NOT ('definitely_after' = ANY(edtf_relation('1985/', '../1980')))"),
            "an unknown end can never make a relation definite"
        );
        // The issue's consistency-rule shape: born must not be after died.
        assert!(
            q_bool("SELECT NOT ('definitely_after' = ANY(edtf_relation('1890~', '1976-01-12')))"),
            "the documented consistency rule holds for a plausible birth and death"
        );
        assert!(
            q_bool("SELECT edtf_relation('junk', '1985') IS NULL"),
            "an invalid operand yields NULL, not an empty array"
        );
    }

    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        clippy::single_call_fn,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn, and pgrx generates the harness entry point that is its only caller"
    )]
    fn range_query_shape() {
        // The intended usage pattern: index-friendly range overlap.
        assert!(
            q_bool(concat!(
                "SELECT daterange(edtf_min('156X'), edtf_max('156X'), '[]') ",
                "@> DATE '1965-06-15' IS FALSE",
            )),
            "the 1560s do not contain a day in 1965"
        );
        assert!(
            q_bool(
                "SELECT daterange(edtf_min('196X'), edtf_max('196X'), '[]') @> DATE '1965-06-15'"
            ),
            "the 1960s do contain a day in 1965"
        );
    }

    /// The shared conformance corpus (`tests/corpus.sql`).
    ///
    /// The prebuilt-tarball smoke test also runs it with `psql -f` against a
    /// real install. Running it from here too is what keeps the binary held
    /// to the same standard as the source: one set of assertions, two
    /// callers, nothing to drift. Each check is a plpgsql `ASSERT`, so a
    /// failure surfaces as an SPI error carrying the message.
    #[pg_test]
    #[expect(
        clippy::missing_panics_doc,
        clippy::single_call_fn,
        reason = "a pgrx test: it asserts by panicking, so there is no caller to warn, and pgrx generates the harness entry point that is its only caller"
    )]
    #[expect(
        clippy::expect_used,
        reason = "test code: a panic here is the failure signal, not a crash path"
    )]
    fn shared_corpus() {
        Spi::run(include_str!("../tests/corpus.sql")).expect("shared corpus");
    }
}
