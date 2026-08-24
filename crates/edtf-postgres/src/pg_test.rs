// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Standard pgrx test harness plumbing.
//!
//! `cargo pgrx test` generates a runner that calls `crate::pg_test::setup`
//! and `crate::pg_test::postgresql_conf_options` by path, so the module name
//! is pgrx's contract. Its being a *file* is not: `clippy::inline_modules`
//! wants it out of `lib.rs`, and moving it here satisfies the lint without
//! touching the harness contract.

/// No per-test setup needed.
#[inline]
pub fn setup(_options: Vec<&str>) {}

/// No extra postgresql.conf settings needed.
#[inline]
#[must_use]
pub fn postgresql_conf_options() -> Vec<&'static str> {
    Vec::new()
}
