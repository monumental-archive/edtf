// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage-guided drive of the normaliser's properties
//! (`edtf_normalize::properties`, built under `cfg(fuzzing)`).
//!
//! Arbitrary UTF-8 is read under every option, and its readings must be
//! honest (rule 9). The input is also split at its first newline into A and
//! B, and in every language the composition rules run on them: range forms
//! agree (1), "A or B" composes (2), before and after compose (3), and every
//! emitted string re-reads as itself (5). The rules live in the crate; this
//! target only feeds them. The module exists only under the `fuzzing` cfg
//! cargo-fuzz builds with, so an ordinary build of this crate (as
//! `lint:rust` runs it) compiles the target empty.

#![no_main]

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    use edtf_normalize::properties::{
        LANGUAGES, check_honest, check_idempotent, check_open_composes, check_or_composes,
        check_range_forms_agree, every_options, observe_honest, observe_open, observe_or,
        observe_range_forms, observe_reread, options,
    };

    let Ok(input) = core::str::from_utf8(data) else {
        return;
    };
    for opts in every_options() {
        check_honest(&observe_honest(input, opts));
    }
    let Some((left, right)) = input.split_once('\n') else {
        return;
    };
    for language in LANGUAGES {
        if let Some(forms) = observe_range_forms(left, right, language) {
            check_range_forms_agree(&forms);
        }
        if let Some(observation) = observe_or(left, right, language) {
            check_or_composes(&observation);
        }
        for side in [left, right] {
            if let Some(observation) = observe_open(side, language) {
                check_open_composes(&observation);
            }
            check_idempotent(&observe_reread(side, options(language)));
        }
    }
});
