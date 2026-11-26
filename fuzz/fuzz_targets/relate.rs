// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage-guided relation rules over two values the parser accepts.
//!
//! The input is two EDTF strings joined by a newline, which no EDTF string
//! contains. Without one the input is related to itself, so the shared
//! single-string seed corpus already exercises this target and the fuzzer
//! splices pairs out of it.
//!
//! The rules are `edtf_core::properties`, each stated there once and driven
//! over generated values by its proptest engine; this target drives the
//! same checks over parsed ones, including the years and masks a
//! generator does not reach.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    use core::str::from_utf8;

    use edtf_core::{Edtf, properties as rules};

    let Ok(input) = from_utf8(data) else {
        return;
    };
    let (left_text, right_text) = input.split_once('\n').unwrap_or((input, input));
    let (Ok(left), Ok(right)) = (Edtf::parse(left_text), Edtf::parse(right_text)) else {
        return;
    };
    rules::check_pair(&rules::observe_pair(&left, &right));
    if let (Edtf::Date(start), Edtf::Date(end)) = (left, right) {
        rules::check_order(&rules::observe_order(start, end));
    }
});
