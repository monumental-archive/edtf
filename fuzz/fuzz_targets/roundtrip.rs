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
    let result = edtf_core::Edtf::parse(s);
    assert_eq!(
        s.parse::<edtf_core::Edtf>(),
        result,
        "FromStr and Edtf::parse disagree on {s:?}"
    );
    if let Err(e) = &result {
        assert!(
            !e.to_string().is_empty(),
            "{s:?} is refused with an empty message"
        );
    }
    if let Ok(parsed) = result {
        let rendered = parsed.to_string();
        let reparsed = edtf_core::Edtf::parse(&rendered).unwrap_or_else(|e| {
            panic!("accepted {s:?}, but canonical form {rendered:?} fails to reparse: {e}")
        });
        assert_eq!(
            parsed, reparsed,
            "round trip changed meaning for {s:?} (canonical {rendered:?})"
        );
        // The conformance level is 0, 1 or 2, the free function agrees with
        // the method, and the canonical form keeps it.
        let level = parsed.level();
        assert!(level <= 2, "{s:?} classified at level {level}");
        assert_eq!(edtf_core::level(s), Some(level), "level({s:?}) disagrees");
        assert_eq!(
            reparsed.level(),
            level,
            "{s:?} changed level as {rendered:?}"
        );
        assert_eq!(
            (reparsed.is_uncertain(), reparsed.is_approximate()),
            (parsed.is_uncertain(), parsed.is_approximate()),
            "{s:?} changed qualification as {rendered:?}"
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
        // Day order and Julian Day Number order agree, so a range query on
        // either answers the same; both bounds display.
        if let (edtf_core::Bound::Date(lo), edtf_core::Bound::Date(hi)) =
            (bounds.earliest, bounds.latest)
        {
            assert_eq!(
                lo <= hi,
                lo.julian_day_number() <= hi.julian_day_number(),
                "{s:?}: day order and JDN order disagree for {lo} and {hi}"
            );
            assert!(!lo.to_string().is_empty() && !hi.to_string().is_empty());
        }
        // Every value an accepted expression enumerates (D24-D29) is
        // concrete, round-trips, and lies within the expression's bounds.
        // The stream can be astronomically long, so a prefix is checked.
        let values = match parsed.values() {
            Ok(values) => values,
            Err(why) => {
                assert!(
                    !why.to_string().is_empty(),
                    "{s:?}: unenumerable without a reason"
                );
                return;
            }
        };
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
                assert!(
                    outer <= first,
                    "{s:?} enumerated {shown:?} before its earliest bound"
                );
            }
            if let (edtf_core::Bound::Date(outer), edtf_core::Bound::Date(last)) =
                (bounds.latest, inner.latest)
            {
                assert!(
                    last <= outer,
                    "{s:?} enumerated {shown:?} after its latest bound"
                );
            }
        }
    }
});

/// How much of an enumeration each input checks.
const VALUES_CHECKED: usize = 64;
