// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end tests of the `edtf` binary via `std::process`.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::min_ident_chars,
    reason = "the test bodies use the same y/m/d date-component names as the code they exercise"
)]
#![expect(
    clippy::shadow_unrelated,
    reason = "short-lived rebinding inside one assertion chain"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "indexing a fixture the test itself constructed; an out-of-range index is a failing test, not a crash path"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]

use std::{
    io::Write as _,
    process::{Command, Stdio},
};

fn edtf(args: &[&str]) -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_edtf"))
        .args(args)
        .output()
        .expect("binary runs");
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.success(),
    )
}

#[test]
fn validate_mixed_inputs() {
    let (stdout, stderr, ok) = edtf(&["validate", "1985-04-12", "1985-02-30"]);
    assert!(!ok, "one invalid input must fail the run");
    assert!(stdout.contains("1985-04-12: ok (level 0)"));
    assert!(stderr.contains("1985-02-30"));
    assert!(stderr.contains("offset 8"));
}

#[test]
fn canonicalizes() {
    let (stdout, _, ok) = edtf(&["canonical", "?2004-?06-?11"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "2004-06-11?");
}

#[test]
fn levels() {
    let (stdout, _, ok) = edtf(&["level", "156X-12-25"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "2");
}

#[test]
fn info_is_json() {
    let (stdout, _, ok) = edtf(&["info", "1985-04-12/.."]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["kind"], "interval");
    assert_eq!(v["earliest"], "1985-04-12");
    assert_eq!(v["latest"], "infinity");
}

#[test]
fn reads_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_edtf"))
        .args(["validate", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"1985\n2001-21\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("1985: ok (level 0)"));
    assert!(stdout.contains("2001-21: ok (level 1)"));
}

#[test]
fn help_and_version() {
    let (stdout, _, ok) = edtf(&["--help"]);
    assert!(ok);
    assert!(stdout.contains("USAGE"));
    let (stdout, _, ok) = edtf(&["--version"]);
    assert!(ok);
    assert!(stdout.starts_with("edtf "));
}

#[test]
fn relation_definite() {
    let (stdout, _, ok) = edtf(&["relation", "1985~", "199X"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "definitely before");
}

#[test]
fn relation_possible() {
    // D23 "sometime during": a day within June 2004 may fall before,
    // on or after a day elsewhere within 2004 — nothing is impossible.
    let (stdout, _, ok) = edtf(&["relation", "2004-06", "2004"]);
    assert!(ok);
    assert_eq!(
        stdout.trim(),
        "possibly before, possibly after, possibly overlaps, possibly contains, \
            possibly within, possibly equal"
    );
    // A single day can never contain or partially overlap a region.
    let (stdout, _, ok) = edtf(&["relation", "2004-06-11", "2004-06"]);
    assert!(ok);
    assert_eq!(
        stdout.trim(),
        "possibly before, possibly after, possibly within, possibly equal"
    );
}

#[test]
fn relation_arity_and_parse_errors() {
    let (_, stderr, ok) = edtf(&["relation", "1985"]);
    assert!(!ok);
    assert!(stderr.contains("exactly two"));
    let (_, stderr, ok) = edtf(&["relation", "1985", "not-a-date"]);
    assert!(!ok);
    assert!(stderr.contains("not-a-date"));
}

#[test]
fn from_julian_day() {
    // The October Revolution: 25 October 1917 O.S. = 7 November 1917 N.S.
    let (stdout, _, ok) = edtf(&["from-julian", "1917-10-25"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "1917-11-07");
}

#[test]
fn from_julian_year_is_a_span() {
    let (stdout, _, ok) = edtf(&["from-julian", "1917"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "1917-01-14/1918-01-13");
}

#[test]
fn from_julian_month_and_negative_year() {
    let (stdout, _, ok) = edtf(&["from-julian", "1900-02", "-0044-03-15"]);
    assert!(ok);
    let lines: Vec<&str> = stdout.lines().collect();
    // Julian Feb 1900 exists through the 29th; offset shifts inside it.
    assert_eq!(lines[0], "1900-02-13/1900-03-13");
    // Proleptic before the calendars coincide: Gregorian runs 2 days
    // behind Julian this far back (astronomical year -44 = 45 BC).
    assert_eq!(lines[1], "-0044-03-13");
}

/// `from-julian` prints EDTF or nothing (#365).
///
/// A Gregorian day outside the years -9999 to 9999 needs the `Y` form,
/// which carries no month or day, so the conversion is refused. Before #365 the first four printed
/// `10000-03-14`, `10000-03-13`, `9999-03-15/10000-03-13` and
/// `10000-03-14/10001-03-14` and exited 0; the last reported a shape
/// error for a year of the right shape.
#[test]
fn from_julian_refuses_results_without_an_edtf_spelling() {
    for input in [
        "10000-01-01",
        "9999-12-31",
        "9999",
        "10000",
        "-9223372036854775808",
    ] {
        let (stdout, stderr, ok) = edtf(&["from-julian", input]);
        assert!(!ok, "{input} must be refused");
        assert!(stdout.is_empty(), "{input} printed {stdout:?}");
        assert_eq!(
            stderr.trim(),
            format!("{input}: converted year out of range")
        );
    }
    // A year too large for `i64` is out of range too, not a shape error.
    let (_, stderr, ok) = edtf(&["from-julian", "99999999999999999999"]);
    assert!(!ok);
    assert!(
        stderr.contains("converted year out of range"),
        "got {stderr:?}"
    );
}

#[test]
fn from_julian_rejects_garbage() {
    for bad in [
        "1917-13",
        "1917-02-30",
        "1917-2-3",
        "191A",
        "1917-10-25-01",
        "",
        "1917-aa",
    ] {
        let (_, stderr, ok) = edtf(&["from-julian", bad]);
        assert!(!ok, "{bad} must be rejected");
        assert!(stderr.contains(bad));
    }
}

#[test]
fn unknown_command() {
    let (_, stderr, ok) = edtf(&["frobnicate"]);
    assert!(!ok);
    assert!(stderr.contains("unknown command \"frobnicate\""));
    assert!(stderr.contains("USAGE"));
}

#[test]
fn stdin_read_errors_fail_the_run() {
    // Invalid UTF-8 makes `lines()` return an Err — the run must stop with
    // a diagnostic rather than skipping silently.
    let mut child = Command::new(env!("CARGO_BIN_EXE_edtf"))
        .args(["validate", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\xff\xfe\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8(out.stderr).unwrap().contains("stdin:"));
}

#[test]
fn info_covers_every_kind_and_bound_shape() {
    // Open start: earliest is -infinity.
    let (stdout, _, ok) = edtf(&["info", "../1985"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["earliest"], "-infinity");
    // Unknown end (empty endpoint): latest is null.
    let (stdout, _, ok) = edtf(&["info", "1985/"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["latest"], serde_json::Value::Null);
    // Datetime and set kinds.
    let (stdout, _, ok) = edtf(&["info", "1985-04-12T10:00:00Z"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["kind"], "datetime");
    assert_eq!(v["precision"], "day");
    let (stdout, _, ok) = edtf(&["info", "[1985,1987]"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["kind"], "set");
    // Year, month and season precisions on plain dates.
    for (input, precision) in [
        ("1985", "year"),
        ("1985-04", "month"),
        ("2001-21", "season"),
    ] {
        let (stdout, _, ok) = edtf(&["info", input]);
        assert!(ok);
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["precision"], precision, "{input}");
    }
}

#[test]
fn no_args_reads_empty_stdin() {
    // `.output()` wires stdin to null (immediate EOF): nothing to validate
    // is a successful, silent run.
    let (stdout, stderr, ok) = edtf(&["validate"]);
    assert!(ok);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");
}

#[test]
fn blank_stdin_lines_are_skipped() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_edtf"))
        .args(["level", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\n1985\n\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "0\n");
}

#[test]
fn double_dash_ends_options() {
    let (stdout, stderr, ok) = edtf(&["validate", "--", "1985"]);
    assert!(
        ok,
        "a leading -- is the end-of-options marker, not an input"
    );
    assert_eq!(stdout.trim(), "1985: ok (level 0)");
    assert!(stderr.is_empty(), "nothing on stderr, got {stderr:?}");

    let (stdout, _, ok) = edtf(&["canonical", "--", "-0500"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "-0500");

    let (stdout, _, ok) = edtf(&["from-julian", "--", "1917"]);
    assert!(ok);
    assert_eq!(stdout.trim(), "1917-01-14/1918-01-13");

    let (_, _, ok) = edtf(&["relation", "--", "1985", "1990"]);
    assert!(ok, "relation takes the two expressions after the marker");

    // Only the first marker is dropped: a second `--` is an input.
    let (_, stderr, ok) = edtf(&["validate", "--", "--"]);
    assert!(!ok);
    assert!(stderr.contains("--: invalid EDTF"), "got {stderr:?}");

    // A value beginning with `-` needs no marker, as before.
    let (_, _, ok) = edtf(&["validate", "-0500"]);
    assert!(ok);
}
