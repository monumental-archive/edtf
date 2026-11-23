// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pinned hand-worked relation cases (issue #22's test ceremony) across
//! masks, qualifiers, seasons, open/unknown ends, Y-years, sets and
//! datetimes. Property-level invariants live in `tests/props.rs`.

#![expect(
    clippy::min_ident_chars,
    reason = "a and b are the two operands of the interval relation algebra and ab/ba its two directions; that is the notation the invariants are written in"
)]
#![expect(clippy::panic, reason = "a panic in a test IS the failure signal")]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use edtf_core::{Date, Edtf, Modality, Qualifier, Relation, Relations, Year, YearKind};

fn rel(a: &str, b: &str) -> Relations {
    Edtf::parse(a)
        .unwrap_or_else(|e| panic!("{a}: {e}"))
        .relation(&Edtf::parse(b).unwrap_or_else(|e| panic!("{b}: {e}")))
}

/// Assert the exact possible-set (canonical order) and the definite one.
fn assert_rel(a: &str, b: &str, possible: &[Relation], definite: Option<Relation>) {
    let r = rel(a, b);
    let got: Vec<Relation> = r.possible().collect();
    assert_eq!(got, possible, "possible set for {a} vs {b} (got: {r})");
    assert_eq!(r.definite(), definite, "definite for {a} vs {b}");
}

use Relation::{After, Before, Contains, Equal, Overlaps, Within};

const ALL: [Relation; 6] = Relation::ALL;

// ------------------------------------------------------------ definite

#[test]
fn issue_example_definitely_before() {
    // latest(1985~) = 1985-12-31 < earliest(199X) = 1990-01-01; the `~`
    // does not move bounds (ISO 8601-2 §8.4.2 NOTE).
    assert_rel("1985~", "199X", &[Before], Some(Before));
    assert_rel("199X", "1985~", &[After], Some(After));
}

#[test]
fn concrete_days() {
    assert_rel("1985-04-12", "1985-04-12", &[Equal], Some(Equal));
    assert_rel("1985-04-12", "1985-04-13", &[Before], Some(Before));
    assert_rel("1985-04-13", "1985-04-12", &[After], Some(After));
    // Qualification is invisible to bounds: still definitely equal.
    assert_rel("1985-04-12?", "1985-04-12~", &[Equal], Some(Equal));
}

#[test]
fn adjacent_years_and_days() {
    assert_rel("1985", "1986", &[Before], Some(Before));
    assert_rel("1985-12-31", "1986-01-01", &[Before], Some(Before));
}

#[test]
fn y_years() {
    assert_rel("Y-20000", "Y20000", &[Before], Some(Before));
    assert_rel("Y20000", "1985", &[After], Some(After));
    // Exponential year with significant digits: 171000000..171999999.
    assert_rel("Y171010000S3", "Y17E7", &[After], Some(After));
}

#[test]
fn seasons_order() {
    // Northern spring (Mar-May) vs northern autumn (Sep-Nov), same year.
    assert_rel("2001-21", "2001-23", &[Before], Some(Before));
    // Northern winter 2001 wraps into 2002 (D12): Dec 2001 - Feb 2002,
    // strictly after spring 2001.
    assert_rel("2001-24", "2001-21", &[After], Some(After));
}

#[test]
fn open_ends_can_still_be_definite() {
    // ..1980 ends by 1980-12-31; 1985/.. starts at 1985-01-01.
    assert_rel("../1980", "1985/..", &[Before], Some(Before));
}

#[test]
fn sets_relate_by_their_span() {
    assert_rel("{1667,1668}", "1670", &[Before], Some(Before));
    assert_rel("[1985-04,1985-06]", "1986", &[Before], Some(Before));
}

#[test]
fn same_day_datetimes_are_equal_at_day_granularity() {
    // Bounds are day-granular; time of day refines within the day.
    assert_rel(
        "1985-04-12T10:00:00Z",
        "1985-04-12T23:59:59Z",
        &[Equal],
        Some(Equal),
    );
}

// ------------------------------------------------------------ possible

#[test]
fn issue_example_mask_vs_year() {
    // 198X completions run 1980..1989: before, after and on 1985 are all
    // live, and under region semantics so are containment and overlap —
    // all reported, none asserted.
    assert_rel("198X", "1985", &ALL, None);
}

#[test]
fn year_vs_its_own_month_is_not_definite_containment() {
    // The documented honesty caveat: bounds-interval semantics read `1985`
    // as "sometime during 1985", so containing June is possible, never
    // definite (D23).
    assert_rel("1985", "1985-06", &ALL, None);
}

#[test]
fn year_vs_its_first_day() {
    // A completion of 1985 cannot end before 1985-01-01, so Before is
    // impossible; Within is impossible because the single day has no room.
    assert_rel("1985", "1985-01-01", &[After, Contains, Equal], None);
    assert_rel("1985-01-01", "1985", &[Before, Within, Equal], None);
}

#[test]
fn two_day_region_admits_no_partial_overlap() {
    // Partial overlap needs three distinct days (a1 < p < b2); a two-day
    // interval against itself cannot supply them.
    assert_rel(
        "2004-06-01/2004-06-02",
        "2004-06-01/2004-06-02",
        &[Before, After, Contains, Within, Equal],
        None,
    );
    // A year against itself has plenty of room: everything is possible.
    assert_rel("1985", "1985", &ALL, None);
}

#[test]
fn interval_endpoint_linkage_is_coarsened() {
    // Every true completion of 2004/2005 straddles June 2004, but region
    // semantics only see [2004-01-01, 2005-12-31] ⊇ June (D23).
    assert_rel("2004/2005", "2004-06", &ALL, None);
}

#[test]
fn opposed_open_ends_leave_everything_possible() {
    assert_rel("1985/..", "../1990", &ALL, None);
    assert_rel("XXXX", "1985", &ALL, None);
}

// ------------------------------------------------------------ unknown

#[test]
fn unknown_bound_is_possible_everything() {
    // 1985/ has an unknown end. Its start alone would place it after
    // ../1980, but Unknown propagates as possible-everything, never
    // definite (documented conservatism, D23).
    assert_rel("1985/", "../1980", &ALL, None);
    assert_rel("1985/", "1990", &ALL, None);
    assert_rel("/1985", "1990", &ALL, None);
    // A year the library cannot compute with bounds to Unknown as well. The
    // parser refuses such years (D30), so it is built by hand: 9E18S1's
    // sweep tops out past i64.
    let huge = Edtf::Date(Date {
        year: Year {
            kind: YearKind::Exponential {
                significand: 9,
                exponent: 18,
            },
            significant_digits: Some(1),
            qualifier: Qualifier::default(),
        },
        month: None,
        day: None,
    });
    let r = huge.relation(&Edtf::parse("1985").unwrap_or_else(|e| panic!("1985: {e}")));
    assert_eq!(r.possible().collect::<Vec<_>>(), ALL, "got: {r}");
    assert_eq!(r.definite(), None);
}

// ------------------------------------------------------------ API shape

#[test]
fn modalities_and_display() {
    let r = rel("1985~", "199X");
    assert_eq!(r.modality(Before), Modality::Definite);
    assert_eq!(r.modality(After), Modality::Impossible);
    assert!(r.is_definite(Before));
    assert!(r.is_possible(Before));
    assert!(r.is_impossible(Equal));
    assert_eq!(r.to_string(), "definitely before");

    let all = rel("198X", "1985");
    assert_eq!(
        all.to_string(),
        "possibly before, possibly after, possibly overlaps, \
            possibly contains, possibly within, possibly equal"
    );
}

#[test]
fn converse_pairs() {
    assert_eq!(Before.converse(), After);
    assert_eq!(After.converse(), Before);
    assert_eq!(Contains.converse(), Within);
    assert_eq!(Within.converse(), Contains);
    assert_eq!(Overlaps.converse(), Overlaps);
    assert_eq!(Equal.converse(), Equal);
}

#[test]
fn modality_names_are_lower_case() {
    assert_eq!(Modality::Impossible.as_str(), "impossible");
    assert_eq!(Modality::Possible.as_str(), "possible");
    assert_eq!(Modality::Definite.as_str(), "definite");
}

#[test]
fn a_definite_relation_is_not_impossible() {
    let r = rel("1985~", "199X");
    assert!(!r.is_impossible(Before));
    assert!(r.is_impossible(After));
}

/// Overlap is asked in both directions and either one suffices.
///
/// Here only one partial overlap is feasible — the earlier region may end on
/// the later one's first day, but the later one cannot start first — and the
/// coarsened `Overlaps` still reports it, whichever operand comes first.
#[test]
fn overlap_feasible_in_one_direction_only() {
    let earlier = "1980-01-01/1985-01-01";
    let later = "1985-01-01/1990-12-31";
    assert!(rel(earlier, later).is_possible(Overlaps));
    assert!(rel(later, earlier).is_possible(Overlaps));
}
