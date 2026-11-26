// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage-guided string rules: every input is held to the text rule
//! and, when the parser accepts it, to the value, letters, whitespace and
//! (for a date) qualification rules.
//!
//! The rules are `edtf_core::properties`, each stated there once; this
//! target only feeds them parsed values, including the years and masks a
//! generator does not reach.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    use core::str::from_utf8;

    use edtf_core::{Edtf, properties as rules};

    let Ok(input) = from_utf8(data) else {
        return;
    };
    let text = rules::observe_text(input);
    rules::check_text(&text);
    let Ok(value) = text.parsed.as_ref() else {
        return;
    };
    rules::check_value(&rules::observe_value(value, VALUES_CHECKED));
    rules::check_letters(&rules::observe_letters(input));
    rules::check_whitespace(&rules::observe_whitespace(input));
    if let &Edtf::Date(date) = value {
        rules::check_qualification(&rules::observe_qualification(date));
    }
});

/// How much of an enumeration each input checks.
const VALUES_CHECKED: usize = 64;
