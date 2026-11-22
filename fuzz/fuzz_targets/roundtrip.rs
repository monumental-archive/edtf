// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage-guided round-trip property: anything the parser accepts must
//! render to a canonical form that reparses to the same value.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else {
        return;
    };
    if let Ok(parsed) = edtf_core::Edtf::parse(s) {
        let rendered = parsed.to_string();
        let reparsed = edtf_core::Edtf::parse(&rendered).unwrap_or_else(|e| {
            panic!("accepted {s:?}, but canonical form {rendered:?} fails to reparse: {e}")
        });
        assert_eq!(
            parsed, reparsed,
            "round trip changed meaning for {s:?} (canonical {rendered:?})"
        );
        // Everything accepted can be bounded (decision D30): a bound is
        // Unknown only on the side of an interval whose endpoint there is
        // Unknown. A date, a datetime or a set never has an unknown bound.
        let bounds = parsed.bounds();
        let (start_unknown, end_unknown) = match parsed {
            edtf_core::Edtf::Interval(iv) => (
                matches!(iv.start, edtf_core::IntervalEndpoint::Unknown),
                matches!(iv.end, edtf_core::IntervalEndpoint::Unknown),
            ),
            edtf_core::Edtf::Date(_) | edtf_core::Edtf::DateTime(_) | edtf_core::Edtf::Set(_) => {
                (false, false)
            }
        };
        assert_eq!(
            matches!(bounds.earliest, edtf_core::Bound::Unknown),
            start_unknown,
            "accepted {s:?}, but its earliest bound is {:?}",
            bounds.earliest
        );
        assert_eq!(
            matches!(bounds.latest, edtf_core::Bound::Unknown),
            end_unknown,
            "accepted {s:?}, but its latest bound is {:?}",
            bounds.latest
        );
        // Every value an accepted expression enumerates (D24-D29) is
        // concrete, round-trips, and lies within the expression's bounds.
        // The stream can be astronomically long, so a prefix is checked.
        if let Ok(values) = parsed.values() {
            for item in values.take(VALUES_CHECKED) {
                let shown = item.to_string();
                assert_eq!(
                    edtf_core::Edtf::parse(&shown).as_ref(),
                    Ok(&item),
                    "{s:?} enumerated {shown:?}, which does not round-trip"
                );
                assert!(
                    !item.has_unspecified(),
                    "{s:?} enumerated {shown:?}, which is still masked"
                );
                let inner = item.bounds();
                if let (edtf_core::Bound::Date(outer), edtf_core::Bound::Date(first)) =
                    (bounds.earliest, inner.earliest)
                {
                    assert!(outer <= first, "{s:?} enumerated {shown:?} before its earliest bound");
                }
                if let (edtf_core::Bound::Date(outer), edtf_core::Bound::Date(last)) =
                    (bounds.latest, inner.latest)
                {
                    assert!(last <= outer, "{s:?} enumerated {shown:?} after its latest bound");
                }
            }
        }
    }
});

/// How much of an enumeration each input checks.
const VALUES_CHECKED: usize = 64;
