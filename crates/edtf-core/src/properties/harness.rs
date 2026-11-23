// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The deterministic string engine: the parser must never panic on any
//! input, and every input is held to the rules in `properties`.
//!
//! Uses a fixed-seed xorshift generator so failures are reproducible; no
//! nightly toolchain or external fuzzer needed, so this guards every CI run.
//! Iteration counts scale up under `--release` (and via `EDTF_FUZZ_ITERS`).

#![expect(
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::cast_possible_truncation,
    reason = "generator and oracle ranges are bounded by construction"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "indexing a fixture the test itself constructed; an out-of-range index is a failing test, not a crash path"
)]
#![expect(
    clippy::as_conversions,
    reason = "casting fixture values the test itself bounded"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test arithmetic is over literal fixtures and generator-bounded values; an overflow here would fail the test, which is the signal"
)]
#![expect(
    clippy::integer_division_remainder_used,
    reason = "same integer calendar arithmetic as the code under test"
)]
#![expect(
    clippy::absolute_paths,
    reason = "a one-use std path written in full at the call site"
)]
#![expect(
    clippy::default_numeric_fallback,
    reason = "literal fixtures whose type the assertion already fixes"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching through a reference without restating & at every level"
)]

extern crate std;

use alloc::vec::Vec;

use super::{
    check_letters, check_qualification, check_text, check_value, check_whitespace, observe_letters,
    observe_qualification, observe_text, observe_value, observe_whitespace,
};
use crate::Edtf;

struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        // xorshift64* — plenty for fuzzing, fully deterministic.
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    const fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn iterations(default: usize) -> usize {
    if let Ok(v) = std::env::var("EDTF_FUZZ_ITERS") {
        if let Ok(n) = v.parse() {
            return n;
        }
    }
    if cfg!(debug_assertions) {
        default
    } else {
        default * 10
    }
}

/// Characters that actually appear in the EDTF grammar, so random inputs
/// exercise deep parser paths instead of failing on byte one.
const ALPHABET: &[u8] = b"0123456789XYESTZ?~%-+:./,{}[]";

/// Hold one input string to every rule a string can be held to: the text
/// rule, and, when it is accepted, the value, letters, whitespace and (for
/// a date) qualification rules.
fn check(input: &str) {
    let text = observe_text(input);
    check_text(&text);
    if let Ok(value) = &text.parsed {
        check_value(&observe_value(value, VALUES_CHECKED));
        check_letters(&observe_letters(input));
        check_whitespace(&observe_whitespace(input));
        if let Edtf::Date(date) = value {
            check_qualification(&observe_qualification(*date));
        }
    }
}

/// How much of an enumeration each accepted input checks.
const VALUES_CHECKED: usize = 64;

#[test]
fn random_grammar_soup_never_panics() {
    let mut rng = Rng(0x0EDF_0EDF_0EDF_0EDF);
    let mut buf = Vec::new();
    for _ in 0..iterations(200_000) {
        buf.clear();
        let len = rng.below(24) + 1;
        for _ in 0..len {
            buf.push(ALPHABET[rng.below(ALPHABET.len())]);
        }
        check(core::str::from_utf8(&buf).expect("alphabet is ASCII"));
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng(0xBA_D5EE_DBAD_5EED);
    let mut buf = Vec::new();
    for _ in 0..iterations(50_000) {
        buf.clear();
        let len = rng.below(32);
        for _ in 0..len {
            buf.push((rng.next() & 0xFF) as u8);
        }
        if let Ok(s) = core::str::from_utf8(&buf) {
            check(s);
        }
    }
}

/// The edges of D30's one supported year range, through the harness's check.
///
/// Whatever side of the line each lands on, an accepted one keeps the bound
/// promise. Before D30, `Y1E19` was accepted with unknown bounds.
#[test]
fn year_range_edges_keep_the_bound_promise() {
    for s in [
        "Y999999999999999999",
        "Y-999999999999999999",
        "Y9E17",
        "Y9E17S1",
        "Y1E18",
        "Y1E19",
        "Y9E18S1",
        "Y1E30/1985",
        "[Y1E19,1985]",
        "{Y1E19..Y1E19}",
        "Y9E17/..",
        "/Y9E17",
    ] {
        check(s);
    }
}

#[test]
fn mutated_valid_inputs_never_panic() {
    let seeds: &[&str] = &[
        "1985-04-12",
        "2004-06~-11",
        "?2004-06-~11",
        "Y3388E2S3",
        "156X-12-25",
        "1985-04-12T23:20:30+04:30",
        "..2004-06-01/2004-06-20..",
        "{..1983-12-31,1984-10-10..1984-11-01,1984-11-05..}",
        "[1667,1760-12]",
        "1985-04-12~/",
        "2001-24",
        "X*", // near-miss of explicit-form syntax
    ];
    let mut rng = Rng(0xC0FF_EE00_C0FF_EE00);
    for _ in 0..iterations(20_000) {
        let seed = seeds[rng.below(seeds.len())];
        let mut bytes = seed.as_bytes().to_vec();
        for _ in 0..=rng.below(3) {
            match rng.below(3) {
                0 if !bytes.is_empty() => {
                    // replace
                    let i = rng.below(bytes.len());
                    bytes[i] = ALPHABET[rng.below(ALPHABET.len())];
                }
                1 => {
                    // insert
                    let i = rng.below(bytes.len() + 1);
                    bytes.insert(i, ALPHABET[rng.below(ALPHABET.len())]);
                }
                _ if !bytes.is_empty() => {
                    // delete
                    let i = rng.below(bytes.len());
                    bytes.remove(i);
                }
                _ => {}
            }
        }
        if let Ok(s) = core::str::from_utf8(&bytes) {
            check(s);
        }
    }
}
