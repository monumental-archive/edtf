// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `edtf` — command-line EDTF (ISO 8601-2:2019 Annex A) validator.
//!
//! ```text
//! edtf validate <EXPR>...     exit 0 if all valid; prints one line per input
//! edtf canonical <EXPR>...    print the spec-preferred form of each input
//! edtf level <EXPR>...        print the conformance level (0/1/2) of each input
//! edtf info <EXPR>...         print a JSON summary of each input
//! edtf relation <A> <B>       three-valued temporal relation between A and B
//! edtf from-julian <DATE>...  Julian (O.S.) date to proleptic-Gregorian EDTF
//! ```
//!
//! Pass `-` (or pipe with no arguments) to read newline-separated
//! expressions from stdin — handy for validating whole data files:
//! `cut -f3 dates.tsv | edtf validate -`.
//!
//! A leading `--` after the command is skipped; inputs that begin with
//! `-` need no `--`.

#![expect(
    clippy::absolute_paths,
    reason = "a one-use std path written in full at the call site is clearer than an import that only appears once"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching through a reference without restating & at every level is the idiomatic form the rest of this crate uses"
)]

use core::fmt::Arguments;
use std::{
    io::{BufRead, Write},
    process::ExitCode,
};

use edtf_core::Edtf;

/// The `--help` text, and the usage line every argument error prints.
const USAGE: &str = "\
edtf \u{2014} EDTF (ISO 8601-2:2019 Annex A) validator, levels 0-2

USAGE:
    edtf <COMMAND> [EXPR]...      operate on arguments
    edtf <COMMAND> -              read newline-separated inputs from stdin
    edtf <COMMAND>                (with piped stdin) same as '-'
    edtf relation <A> <B>         compare exactly two expressions

    a leading '--' after the command is skipped; inputs that begin with
    '-' need no '--'.

COMMANDS:
    validate     check inputs; exit 0 only if every input is valid EDTF
    canonical    print the spec-preferred form of each valid input
    level        print the minimum conformance level (0, 1 or 2)
    info         print a JSON summary (kind, precision, bounds, flags)
    relation     three-valued temporal relation between two expressions
        (e.g. 'definitely before', 'possibly overlaps, ...')
    from-julian  convert proleptic Julian (Old Style) dates to Gregorian
        EDTF; input Y, Y-MM or Y-MM-DD (astronomical years). Year/month
        precision prints an interval, never a bare 'converted' year \u{2014}
        Julian 1917 is 1917-01-14/1918-01-13. A result outside the
        Gregorian years -9999 to 9999 has no EDTF spelling and is refused

OPTIONS:
    -h, --help       show this help
    -V, --version    show version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(
        &args,
        std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

/// The whole program: the arguments after the program name, a reader for
/// stdin and writers for stdout and stderr, to the exit code.
///
/// `main` hands it the process's own; the tests hand it their own.
fn run(
    args: &[String],
    stdin: impl BufRead,
    out: &mut impl Write,
    err: &mut impl Write,
) -> ExitCode {
    let ok = match Command::parse(args) {
        Command::Help => write!(out, "{USAGE}").is_ok(),
        Command::Version => writeln!(out, "edtf {}", env!("CARGO_PKG_VERSION")).is_ok(),
        Command::Unknown(other) => refuse(
            err,
            format_args!("edtf: unknown command {other:?}\n\n{USAGE}"),
        ),
        Command::Relation(rest) => relation(rest, out, err),
        Command::Each(each, Inputs::Arguments(inputs)) => {
            let mut ok = true;
            for input in inputs {
                ok &= each.apply(input, out, err);
            }
            ok
        }
        Command::Each(each, Inputs::Stdin) => each_line(each, stdin, out, err),
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The command line, parsed once: what to do, and what it acts on.
enum Command<'args> {
    /// `-h`, `--help`, `help`, or no command at all.
    Help,
    /// `-V` or `--version`.
    Version,
    /// A command applied to each input in turn.
    Each(Each, Inputs<'args>),
    /// `relation`, with every argument after the command.
    Relation(&'args [String]),
    /// Anything else in the command's place.
    Unknown(&'args str),
}

/// Where a per-input command finds its inputs.
enum Inputs<'args> {
    /// Newline-separated lines of stdin; an empty line is skipped.
    Stdin,
    /// The arguments, each an input verbatim; an empty one is an error.
    Arguments(&'args [String]),
}

/// The commands that handle each input on its own.
#[derive(Clone, Copy)]
enum Each {
    /// `validate`: report each input's level, or refuse it.
    Validate,
    /// `canonical`: print each input's spec-preferred form.
    Canonical,
    /// `level`: print each input's minimum conformance level.
    Level,
    /// `info`: print each input's JSON summary.
    Info,
    /// `from-julian`: convert each Julian date to Gregorian EDTF.
    FromJulian,
}

impl<'args> Command<'args> {
    /// Parse the arguments after the program name.
    fn parse(args: &'args [String]) -> Self {
        let Some((command, after)) = args.split_first() else {
            return Self::Help;
        };
        // Everything after the command is an input, verbatim, except one
        // leading `--` end-of-options marker, which is dropped (#329). No
        // other argument is an option: `-0500` and `-` (stdin) stay inputs.
        let rest = match after {
            [marker, tail @ ..] if marker == "--" => tail,
            _ => after,
        };
        let each = match command.as_str() {
            "-h" | "--help" | "help" => return Self::Help,
            "-V" | "--version" => return Self::Version,
            "relation" => return Self::Relation(rest),
            "validate" => Each::Validate,
            "canonical" => Each::Canonical,
            "level" => Each::Level,
            "info" => Each::Info,
            "from-julian" => Each::FromJulian,
            other => return Self::Unknown(other),
        };
        let inputs = match rest {
            [] => Inputs::Stdin,
            [dash] if dash == "-" => Inputs::Stdin,
            _ => Inputs::Arguments(rest),
        };
        Self::Each(each, inputs)
    }
}

impl Each {
    /// Handle one input: its line to `out`, or its diagnostic to `err`.
    /// Returns false if the input was refused or its line not written.
    fn apply(self, input: &str, out: &mut impl Write, err: &mut impl Write) -> bool {
        let parse = || Edtf::parse(input).map_err(|error| error.to_string());
        let result = match self {
            // A Julian date, not an EDTF expression: `edtf-calendars` reads it.
            Self::FromJulian => edtf_calendars::parse_julian(input)
                .and_then(|(year, month, day)| edtf_calendars::convert(year, month, day))
                .map(|value| value.to_string())
                .map_err(|error| error.to_string()),
            Self::Validate => {
                parse().map(|parsed| format!("{input}: ok (level {})", parsed.level()))
            }
            Self::Canonical => parse().map(|parsed| parsed.to_string()),
            Self::Level => parse().map(|parsed| parsed.level().to_string()),
            Self::Info => parse().map(|parsed| info_json(input, &parsed)),
        };
        match result {
            Ok(line) => writeln!(out, "{line}").is_ok(),
            Err(error) => refuse(err, format_args!("{input}: {error}")),
        }
    }
}

/// Apply `each` to every non-empty line of `stdin`. A read error (bytes
/// that are not UTF-8, say) stops the run and fails it.
fn each_line(each: Each, stdin: impl BufRead, out: &mut impl Write, err: &mut impl Write) -> bool {
    let mut ok = true;
    for read in stdin.lines() {
        match read {
            Ok(line) if line.is_empty() => {}
            Ok(line) => ok &= each.apply(&line, out, err),
            Err(error) => return refuse(err, format_args!("edtf: stdin: {error}")),
        }
    }
    ok
}

/// `edtf relation <A> <B>`: print the three-valued temporal relation.
fn relation(rest: &[String], out: &mut impl Write, err: &mut impl Write) -> bool {
    let [first, second] = rest else {
        return refuse(
            err,
            format_args!("edtf: relation takes exactly two expressions\n\n{USAGE}"),
        );
    };
    let mut parse = |text: &str| match Edtf::parse(text) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            refuse(err, format_args!("{text}: {error}"));
            None
        }
    };
    let (Some(earlier), Some(later)) = (parse(first), parse(second)) else {
        return false;
    };
    writeln!(out, "{}", earlier.relation(&later)).is_ok()
}

/// Write one diagnostic line to `err`, and fail.
///
/// Every diagnostic accompanies a failure, so one that cannot be written
/// changes nothing: the run fails either way.
fn refuse(err: &mut impl Write, message: Arguments<'_>) -> bool {
    let _unwritten = writeln!(err, "{message}");
    false
}

/// The `info` command's JSON object for one parsed expression: core's
/// [`edtf_core::Summary`] with the input added.
fn info_json(input: &str, parsed: &Edtf) -> String {
    let mut json = serde_json::json!(parsed.summary());
    if let Some(object) = json.as_object_mut() {
        object.insert("input".into(), input.into());
    }
    json.to_string()
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::missing_panics_doc,
        reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
    )]
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a panic here is the failure signal, not a crash path"
    )]

    use core::iter::repeat_n;

    use proptest::prelude::*;

    use super::*;

    /// What one run of the program did.
    #[derive(Debug, PartialEq)]
    struct Outcome {
        success: bool,
        out: String,
        err: String,
    }

    /// Run the program on `args` with `stdin`, capturing both writers.
    fn edtf(args: &[&str], stdin: &[u8]) -> Outcome {
        let owned: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&owned, stdin, &mut out, &mut err);
        Outcome {
            success: code == ExitCode::SUCCESS,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    /// `cmd` followed by `before`, then `inputs`, as one argument list.
    fn argv<'args>(
        cmd: &'args str,
        before: &[&'args str],
        inputs: &'args [String],
    ) -> Vec<&'args str> {
        let mut args = vec![cmd];
        args.extend_from_slice(before);
        args.extend(inputs.iter().map(String::as_str));
        args
    }

    /// The same inputs as stdin lines.
    fn lines(inputs: &[String]) -> Vec<u8> {
        inputs
            .iter()
            .flat_map(|input| format!("{input}\n").into_bytes())
            .collect()
    }

    /// The commands that take each input on its own.
    const EACH: [&str; 5] = ["validate", "canonical", "level", "info", "from-julian"];

    /// Inputs that exercise every command's success path, and a near miss.
    const SAMPLES: [&str; 14] = [
        "1985",
        "1985-04-12",
        "-0500",
        "1985-04-12/..",
        "2004-06~",
        "156X-12-25",
        "2001-21",
        "?2004-?06-?11",
        "[1667,1668]",
        "Y-170000002",
        "1917-10-25",
        "-0044-03-15",
        "+1917-10-25",
        "1985-02-30",
    ];

    fn command() -> impl Strategy<Value = &'static str> {
        prop::sample::select(&EACH[..])
    }

    /// One non-empty input that can also be a stdin line: valid and
    /// near-valid EDTF and Julian text, option-shaped strings, and
    /// anything else without a line break.
    fn input() -> impl Strategy<Value = String> {
        prop_oneof![
            prop::sample::select(&SAMPLES[..]).prop_map(str::to_owned),
            "-?[0-9X]{1,5}(-[0-9]{1,2}){0,2}[?~%]?",
            Just("--".to_owned()),
            Just("-".to_owned()),
            "-{1,2}[^\n\r]{0,8}",
            "[^\n\r]{1,12}",
        ]
    }

    /// Inputs that may follow a `--` marker: anything but a lone `-`,
    /// which asks for stdin.
    fn marked_inputs() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(input(), 1..6)
            .prop_filter("a lone `-` reads stdin", |inputs| inputs != &["-"])
    }

    /// Inputs that may be the arguments as they stand: also not led by
    /// `--`, which is the marker.
    fn inputs() -> impl Strategy<Value = Vec<String>> {
        marked_inputs().prop_filter("a leading `--` is the marker", |inputs| {
            inputs.first().is_some_and(|first| first != "--")
        })
    }

    /// Julian text, well and badly formed.
    fn julian_input() -> impl Strategy<Value = String> {
        prop_oneof!["[-+]?[0-9]{1,6}(-[0-9]{1,2}){0,2}", "[^\n\r]{1,12}",]
    }

    /// An argument list of any shape: command words, markers and any
    /// text at all.
    fn any_args() -> impl Strategy<Value = Vec<String>> {
        let word = prop_oneof![
            prop::sample::select(
                &[
                    "validate",
                    "canonical",
                    "level",
                    "info",
                    "from-julian",
                    "relation",
                    "help",
                    "-h",
                    "--help",
                    "-V",
                    "--version",
                    "--",
                    "-",
                ][..]
            )
            .prop_map(str::to_owned),
            input(),
            any::<String>(),
        ];
        prop::collection::vec(word, 0..6)
    }

    /// What `edtf-calendars` returns for one Julian date's text.
    ///
    /// # Errors
    ///
    /// Whatever `parse_julian` or `convert` refuses the text with.
    fn julian(input: &str) -> Result<Edtf, edtf_calendars::CalendarError> {
        let (year, month, day) = edtf_calendars::parse_julian(input)?;
        edtf_calendars::convert(year, month, day)
    }

    proptest! {
        /// It returns, for any arguments and any stdin bytes.
        #[test]
        fn returns_for_any_arguments_and_stdin(
            args in any_args(),
            stdin in prop_oneof![
                prop::collection::vec(any::<u8>(), 0..64),
                inputs().prop_map(|inputs| lines(&inputs)),
            ],
        ) {
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            edtf(&borrowed, &stdin);
        }

        /// `validate` exits success exactly when every input is valid EDTF.
        #[test]
        fn validate_succeeds_exactly_when_every_input_is_valid(inputs in inputs()) {
            let run = edtf(&argv("validate", &[], &inputs), b"");
            prop_assert_eq!(
                run.success,
                inputs.iter().all(|input| edtf_core::is_valid(input))
            );
        }

        /// The same non-empty inputs as arguments and as stdin lines (after
        /// `-`, or after the bare command) print the same and exit alike.
        #[test]
        fn arguments_and_stdin_lines_agree(
            cmd in command(),
            inputs in inputs(),
            dash in any::<bool>(),
        ) {
            let from_args = edtf(&argv(cmd, &[], &inputs), b"");
            let stdin_args: &[&str] = if dash { &[cmd, "-"] } else { &[cmd] };
            let from_stdin = edtf(stdin_args, &lines(&inputs));
            prop_assert_eq!(from_args.out, from_stdin.out);
            prop_assert_eq!(from_args.success, from_stdin.success);
        }

        /// One leading `--` after the command is dropped; a second is an
        /// input, and so is any input beginning with `-`: after the marker
        /// the arguments do exactly what the same stdin lines do.
        #[test]
        fn one_leading_double_dash_is_dropped(cmd in command(), inputs in marked_inputs()) {
            let marked = edtf(&argv(cmd, &["--"], &inputs), b"");
            let from_stdin = edtf(&[cmd], &lines(&inputs));
            prop_assert_eq!(marked, from_stdin);
        }

        /// `relation` skips the same marker before its two expressions.
        #[test]
        fn relation_drops_one_leading_double_dash(first in input(), second in input()) {
            prop_assume!(first != "--");
            prop_assert_eq!(
                edtf(&["relation", "--", &first, &second], b""),
                edtf(&["relation", &first, &second], b"")
            );
        }

        /// `info` prints one JSON object per valid input, equal to core's
        /// summary with the input added, and exits success exactly when
        /// every input is valid.
        #[test]
        fn info_prints_the_core_summary_of_each_valid_input(inputs in inputs()) {
            let run = edtf(&argv("info", &[], &inputs), b"");
            let printed: Vec<serde_json::Value> = run
                .out
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let expected: Vec<serde_json::Value> = inputs
                .iter()
                .filter_map(|input| Edtf::parse(input).ok().map(|parsed| (input, parsed)))
                .map(|(input, parsed)| {
                    let mut json = serde_json::json!(parsed.summary());
                    json.as_object_mut()
                        .unwrap()
                        .insert("input".into(), input.as_str().into());
                    json
                })
                .collect();
            prop_assert_eq!(run.success, expected.len() == inputs.len());
            prop_assert_eq!(printed, expected);
        }

        /// `from-julian` prints what `edtf-calendars` returns for each
        /// input, and exits success exactly when it returns a value for all.
        #[test]
        fn from_julian_prints_what_calendars_returns(
            inputs in prop::collection::vec(julian_input(), 1..6)
                .prop_filter("a lone `-` reads stdin", |inputs| inputs != &["-"])
                .prop_filter("a leading `--` is the marker", |inputs| {
                    inputs.first().is_some_and(|first| first != "--")
                }),
        ) {
            let run = edtf(&argv("from-julian", &[], &inputs), b"");
            let converted: Vec<_> = inputs.iter().map(|input| julian(input)).collect();
            let expected: Vec<String> =
                converted.iter().flatten().map(ToString::to_string).collect();
            let printed: Vec<String> = run.out.lines().map(str::to_owned).collect();
            prop_assert_eq!(run.success, converted.iter().all(Result::is_ok));
            prop_assert_eq!(printed, expected);
        }

        /// An empty stdin line is skipped: blank lines anywhere change
        /// nothing.
        #[test]
        fn empty_stdin_lines_are_skipped(
            cmd in command(),
            inputs in inputs(),
            blanks in prop::collection::vec(0_usize..3, 1..7),
        ) {
            let mut padded = Vec::new();
            for (index, blank) in blanks.iter().enumerate() {
                padded.extend(repeat_n(b'\n', *blank));
                if let Some(input) = inputs.get(index) {
                    padded.extend_from_slice(input.as_bytes());
                    padded.push(b'\n');
                }
            }
            for input in inputs.iter().skip(blanks.len()) {
                padded.extend_from_slice(input.as_bytes());
                padded.push(b'\n');
            }
            prop_assert_eq!(edtf(&[cmd], &padded), edtf(&[cmd], &lines(&inputs)));
        }

        /// An empty argument is an error, wherever it stands.
        #[test]
        fn an_empty_argument_is_an_error(
            cmd in command(),
            inputs in inputs(),
            at in any::<prop::sample::Index>(),
        ) {
            let mut with_empty = inputs;
            with_empty.insert(at.index(with_empty.len().saturating_add(1)), String::new());
            let run = edtf(&argv(cmd, &[], &with_empty), b"");
            prop_assert!(!run.success);
            prop_assert!(run.err.lines().any(|line| line.starts_with(": ")), "{}", run.err);
        }
    }

    /// `tests/cli.rs` pins the same outputs through the real binary; these
    /// go through the same function with writers of our own, which is what
    /// pins that it writes where it is told rather than to the process's.
    #[test]
    fn every_command_writes_to_the_supplied_writers() {
        let ok = |out: &str| Outcome {
            success: true,
            out: out.to_owned(),
            err: String::new(),
        };
        assert_eq!(
            edtf(&["validate", "1985-04-12"], b""),
            ok("1985-04-12: ok (level 0)\n")
        );
        assert_eq!(
            edtf(&["canonical", "?2004-?06-?11"], b""),
            ok("2004-06-11?\n")
        );
        assert_eq!(edtf(&["level", "156X-12-25"], b""), ok("2\n"));
        assert_eq!(
            edtf(&["from-julian", "1917-10-25"], b""),
            ok("1917-11-07\n")
        );
        assert_eq!(
            edtf(&["relation", "1985~", "199X"], b""),
            ok("definitely before\n")
        );
        let info = edtf(&["info", "1985"], b"");
        assert!(info.success);
        let json: serde_json::Value = serde_json::from_str(info.out.trim()).unwrap();
        assert_eq!(
            json.get("kind").and_then(serde_json::Value::as_str),
            Some("date")
        );
        let invalid = edtf(&["validate", "1985-02-30"], b"");
        assert!(!invalid.success);
        assert_eq!(invalid.out, "");
        assert!(invalid.err.starts_with("1985-02-30: "), "{}", invalid.err);
    }
}
