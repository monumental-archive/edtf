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

use core::str::from_utf8;

use edtf_core::{Bound, Edtf, Modality, Relation};
use libfuzzer_sys::fuzz_target;

/// Whether either of `value`'s bounds is Unknown.
fn any_unknown(value: &Edtf) -> bool {
    let bounds = value.bounds();
    bounds.earliest == Bound::Unknown || bounds.latest == Bound::Unknown
}

fuzz_target!(|data: &[u8]| {
    let Ok(input) = from_utf8(data) else {
        return;
    };
    let (left_text, right_text) = input.split_once('\n').unwrap_or((input, input));
    let (Ok(left), Ok(right)) = (Edtf::parse(left_text), Edtf::parse(right_text)) else {
        return;
    };
    let forward = left.relation(&right);
    let backward = right.relation(&left);

    // Converse symmetry: swapping the operands maps every relation to its
    // converse with the same modality.
    for relation in Relation::ALL {
        assert_eq!(
            forward.modality(relation),
            backward.modality(relation.converse()),
            "{left} vs {right}: {relation:?} and its converse disagree"
        );
    }

    // Soundness: something is possible, at most one relation is definite,
    // and a definite one is the only possibility and is Before, After or
    // Equal (containment and overlap are never forced).
    let possible: Vec<Relation> = forward.possible().collect();
    assert!(
        !possible.is_empty(),
        "{left} vs {right}: no relation is possible"
    );
    let definite: Vec<Relation> = Relation::ALL
        .into_iter()
        .filter(|relation| forward.is_definite(*relation))
        .collect();
    assert!(
        definite.len() <= 1,
        "{left} vs {right}: two definite relations"
    );
    assert_eq!(
        forward.definite(),
        definite.first().copied(),
        "{left} vs {right}"
    );
    if let Some(sole) = forward.definite() {
        assert_eq!(
            possible,
            [sole],
            "{left} vs {right}: a definite relation must be sole"
        );
        assert!(
            matches!(sole, Relation::Before | Relation::After | Relation::Equal),
            "{left} vs {right}: {sole:?} can never be definite"
        );
    }

    // An Unknown bound on either side leaves every relation possible.
    let unknown = any_unknown(&left) || any_unknown(&right);
    if unknown {
        for relation in Relation::ALL {
            assert_eq!(
                forward.modality(relation),
                Modality::Possible,
                "{left} vs {right}: an Unknown bound must leave {relation:?} possible"
            );
        }
    }

    // Definite Before holds exactly when A's latest day precedes B's
    // earliest day and no bound anywhere is Unknown.
    let expected_before = !unknown
        && matches!(
            (left.bounds().latest, right.bounds().earliest),
            (Bound::Date(hi), Bound::Date(lo)) if hi < lo
        );
    assert_eq!(
        forward.is_definite(Relation::Before),
        expected_before,
        "{left} vs {right}: definite Before disagrees with the bounds"
    );

    // Impossible is exactly not-possible, every relation and modality has a
    // name, and the description of the relation is never empty.
    for relation in Relation::ALL {
        assert_eq!(
            forward.is_impossible(relation),
            !forward.is_possible(relation),
            "{left} vs {right}: {relation:?}"
        );
        assert!(
            !relation.as_str().is_empty() && !forward.modality(relation).as_str().is_empty(),
            "{left} vs {right}: {relation:?} or its modality has no name"
        );
    }
    assert!(
        !forward.to_string().is_empty(),
        "{left} vs {right}: empty description"
    );

    // A value can always equal itself.
    if left == right {
        assert!(
            forward.is_possible(Relation::Equal),
            "{left} vs itself: Equal is ruled out"
        );
    }
});
