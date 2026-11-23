// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage-guided normaliser properties: `normalize_with` must return, not
//! crash, on arbitrary UTF-8 under every option it reads, and everything it
//! emits must be canonical EDTF that the core parser reads back as the value
//! it reports. Input the core parser already accepts must come back as that
//! same value (the already-valid path the N-decisions put first), or, for
//! the N13 sub-year-code collision, among the readings reported.

#![no_main]

use core::str::from_utf8;

use edtf_core::Edtf;
use edtf_normalize::{Century, Language, NumericOrder, Options, Outcome, normalize_with};
use libfuzzer_sys::fuzz_target;

/// Every reading the normaliser emits is canonical and reparses to the value
/// it was reported with.
///
/// # Panics
///
/// When either half fails: a panic is how a fuzz target reports a finding.
fn assert_emitted(input: &str, options: Options, edtf: &str, value: &Edtf) {
    assert_eq!(
        Edtf::parse(edtf).as_ref(),
        Ok(value),
        "{input:?} under {options:?}: emitted {edtf:?} does not reparse to its value"
    );
    assert_eq!(
        value.to_string(),
        edtf,
        "{input:?} under {options:?}: emitted {edtf:?} is not canonical"
    );
}

fuzz_target!(|data: &[u8]| {
    let Ok(input) = from_utf8(data) else {
        return;
    };
    let already_valid = Edtf::parse(input).ok();
    for language in [Language::English, Language::Russian] {
        for numeric_order in [
            None,
            Some(NumericOrder::DayFirst),
            Some(NumericOrder::MonthFirst),
        ] {
            for default_century in [None, Century::new(1900)] {
                let options = Options {
                    language,
                    numeric_order,
                    default_century,
                };
                match normalize_with(input, options) {
                    Outcome::Normalized(normalized) => {
                        assert_emitted(input, options, &normalized.edtf, &normalized.value);
                        if let Some(valid) = already_valid.as_ref() {
                            assert_eq!(
                                &normalized.value, valid,
                                "{input:?} under {options:?}: valid EDTF was read as another value"
                            );
                        }
                    }
                    Outcome::Ambiguous(amb) => {
                        // N13: a sub-year code that is also a plausible
                        // elided range is reported both ways, but its own
                        // reading is never lost.
                        if let Some(valid) = already_valid.as_ref() {
                            assert!(
                                amb.interpretations
                                    .iter()
                                    .any(|reading| &reading.value == valid),
                                "{input:?} under {options:?}: valid EDTF was reported ambiguous without its own reading"
                            );
                        }
                        assert!(
                            amb.interpretations.len() >= 2,
                            "{input:?} under {options:?}: ambiguous with fewer than two readings"
                        );
                        for (index, reading) in amb.interpretations.iter().enumerate() {
                            assert_emitted(input, options, &reading.edtf, &reading.value);
                            assert!(
                                amb.interpretations
                                    .iter()
                                    .take(index)
                                    .all(|earlier| earlier.value != reading.value),
                                "{input:?} under {options:?}: reading {:?} reported twice",
                                reading.edtf
                            );
                        }
                    }
                    Outcome::NoMatch { reason } => assert!(
                        already_valid.is_none(),
                        "{input:?} under {options:?}: valid EDTF was not matched ({reason:?})"
                    ),
                }
            }
        }
    }
});
