// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage-guided round-trip property: anything the parser accepts must
//! render to a canonical form that reparses to the same value.

#![no_main]

use core::str::from_utf8;

use edtf_core::{Bound, Edtf, IntervalEndpoint, level as level_of};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = from_utf8(data) else {
        return;
    };
    let result = Edtf::parse(input);
    assert_eq!(
        input.parse::<Edtf>(),
        result,
        "FromStr and Edtf::parse disagree on {input:?}"
    );
    if let Err(refusal) = result.as_ref() {
        assert!(
            !refusal.to_string().is_empty(),
            "{input:?} is refused with an empty message"
        );
    }
    if let Ok(parsed) = result {
        let rendered = parsed.to_string();
        // Compared as results, so a canonical form that fails to reparse
        // fails this assertion with its refusal in the message.
        let reparsed = Edtf::parse(&rendered);
        assert_eq!(
            reparsed.as_ref(),
            Ok(&parsed),
            "round trip changed meaning for {input:?} (canonical {rendered:?})"
        );
        // The conformance level is 0, 1 or 2, the free function agrees with
        // the method, and the canonical form keeps it.
        let level = parsed.level();
        assert!(level <= 2, "{input:?} classified at level {level}");
        assert_eq!(level_of(input), Some(level), "level({input:?}) disagrees");
        assert_eq!(
            reparsed.as_ref().map(Edtf::level),
            Ok(level),
            "{input:?} changed level as {rendered:?}"
        );
        assert_eq!(
            reparsed
                .as_ref()
                .map(|value| (value.is_uncertain(), value.is_approximate())),
            Ok((parsed.is_uncertain(), parsed.is_approximate())),
            "{input:?} changed qualification as {rendered:?}"
        );
        // Everything accepted can be bounded (decision D30): a bound is
        // Unknown only on the side of an interval whose endpoint there is
        // Unknown. A date, a datetime or a set never has an unknown bound.
        let bounds = parsed.bounds();
        let (start_unknown, end_unknown) = match parsed {
            Edtf::Interval(iv) => (
                matches!(iv.start(), IntervalEndpoint::Unknown),
                matches!(iv.end(), IntervalEndpoint::Unknown),
            ),
            Edtf::Date(_) | Edtf::DateTime(_) | Edtf::Set(_) => (false, false),
        };
        assert_eq!(
            matches!(bounds.earliest, Bound::Unknown),
            start_unknown,
            "accepted {input:?}, but its earliest bound is {:?}",
            bounds.earliest
        );
        assert_eq!(
            matches!(bounds.latest, Bound::Unknown),
            end_unknown,
            "accepted {input:?}, but its latest bound is {:?}",
            bounds.latest
        );
        // Day order and Julian Day Number order agree, so a range query on
        // either answers the same; both bounds display.
        if let (Bound::Date(lo), Bound::Date(hi)) = (bounds.earliest, bounds.latest) {
            assert_eq!(
                lo <= hi,
                lo.julian_day_number() <= hi.julian_day_number(),
                "{input:?}: day order and JDN order disagree for {lo} and {hi}"
            );
            assert!(
                !lo.to_string().is_empty() && !hi.to_string().is_empty(),
                "{input:?}: a bound of {lo:?}..{hi:?} displays empty"
            );
        }
        // Every value an accepted expression enumerates (D24-D29) is
        // concrete, round-trips, and lies within the expression's bounds.
        // The stream can be astronomically long, so a prefix is checked.
        let values = match parsed.values() {
            Ok(values) => values,
            Err(why) => {
                assert!(
                    !why.to_string().is_empty(),
                    "{input:?}: unenumerable without a reason"
                );
                return;
            }
        };
        for item in values.take(VALUES_CHECKED) {
            let shown = item.to_string();
            assert_eq!(
                Edtf::parse(&shown).as_ref(),
                Ok(&item),
                "{input:?} enumerated {shown:?}, which does not round-trip"
            );
            assert!(
                !item.has_unspecified(),
                "{input:?} enumerated {shown:?}, which is still masked"
            );
            let inner = item.bounds();
            if let (Bound::Date(outer), Bound::Date(first)) = (bounds.earliest, inner.earliest) {
                assert!(
                    outer <= first,
                    "{input:?} enumerated {shown:?} before its earliest bound"
                );
            }
            if let (Bound::Date(outer), Bound::Date(last)) = (bounds.latest, inner.latest) {
                assert!(
                    last <= outer,
                    "{input:?} enumerated {shown:?} after its latest bound"
                );
            }
        }
    }
});

/// How much of an enumeration each input checks.
const VALUES_CHECKED: usize = 64;
