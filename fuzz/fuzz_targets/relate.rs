// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage-guided relation properties over two values the parser accepts.
//!
//! The input is two EDTF strings joined by a newline, which no EDTF string
//! contains. Without one the input is related to itself, so the shared
//! single-string seed corpus already exercises this target and the fuzzer
//! splices pairs out of it. `tests/props.rs` holds the same invariants over
//! generated values; this target holds them over parsed ones, including the
//! years and masks a generator does not reach.

#![no_main]

use edtf_core::{Bound, Edtf, Modality, Relation};
use libfuzzer_sys::fuzz_target;

fn any_unknown(v: &Edtf) -> bool {
    let b = v.bounds();
    b.earliest == Bound::Unknown || b.latest == Bound::Unknown
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else {
        return;
    };
    let (left, right) = s.split_once('\n').unwrap_or((s, s));
    let (Ok(a), Ok(b)) = (Edtf::parse(left), Edtf::parse(right)) else {
        return;
    };
    let ab = a.relation(&b);
    let ba = b.relation(&a);

    // Converse symmetry: swapping the operands maps every relation to its
    // converse with the same modality.
    for r in Relation::ALL {
        assert_eq!(
            ab.modality(r),
            ba.modality(r.converse()),
            "{a} vs {b}: {r:?} and its converse disagree"
        );
    }

    // Soundness: something is possible, at most one relation is definite,
    // and a definite one is the only possibility and is Before, After or
    // Equal (containment and overlap are never forced).
    let possible: Vec<Relation> = ab.possible().collect();
    assert!(!possible.is_empty(), "{a} vs {b}: no relation is possible");
    let definite: Vec<Relation> = Relation::ALL
        .into_iter()
        .filter(|r| ab.is_definite(*r))
        .collect();
    assert!(definite.len() <= 1, "{a} vs {b}: two definite relations");
    assert_eq!(ab.definite(), definite.first().copied(), "{a} vs {b}");
    if let Some(d) = ab.definite() {
        assert_eq!(
            possible,
            [d],
            "{a} vs {b}: a definite relation must be sole"
        );
        assert!(
            matches!(d, Relation::Before | Relation::After | Relation::Equal),
            "{a} vs {b}: {d:?} can never be definite"
        );
    }

    // An Unknown bound on either side leaves every relation possible.
    let unknown = any_unknown(&a) || any_unknown(&b);
    if unknown {
        for r in Relation::ALL {
            assert_eq!(
                ab.modality(r),
                Modality::Possible,
                "{a} vs {b}: an Unknown bound must leave {r:?} possible"
            );
        }
    }

    // Definite Before holds exactly when A's latest day precedes B's
    // earliest day and no bound anywhere is Unknown.
    let expected_before = !unknown
        && matches!(
            (a.bounds().latest, b.bounds().earliest),
            (Bound::Date(hi), Bound::Date(lo)) if hi < lo
        );
    assert_eq!(
        ab.is_definite(Relation::Before),
        expected_before,
        "{a} vs {b}: definite Before disagrees with the bounds"
    );

    // Impossible is exactly not-possible, every relation and modality has a
    // name, and the description of the relation is never empty.
    for r in Relation::ALL {
        assert_eq!(ab.is_impossible(r), !ab.is_possible(r), "{a} vs {b}: {r:?}");
        assert!(!r.as_str().is_empty() && !ab.modality(r).as_str().is_empty());
    }
    assert!(!ab.to_string().is_empty(), "{a} vs {b}: empty description");

    // A value can always equal itself.
    if a == b {
        assert!(
            ab.is_possible(Relation::Equal),
            "{a} vs itself: Equal is ruled out"
        );
    }
});
