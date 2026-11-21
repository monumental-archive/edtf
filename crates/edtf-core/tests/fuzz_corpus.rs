// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The committed fuzz corpus holds every seed the fixtures define (#302).
//!
//! The seed set is every `edtf` string in the legacy conformance corpus,
//! every key of the golden snapshot, and every example and invalid case of
//! the `LoC` interop corpus: the inputs the rest of this suite already
//! knows are interesting. Each target in [`TARGETS`] must carry each seed
//! as the exact content of some file in `fuzz/corpus/<target>/`; files
//! beyond the seed set (crash regressions the fuzzer found) are allowed.
//!
//! Run with `EDTF_WRITE_FUZZ_CORPUS=1` and this test is also the writer:
//! it adds each missing seed, named by the FNV-1a 64 hash of its bytes, and
//! never overwrites a file holding other bytes.

#![expect(
    clippy::expect_used,
    reason = "test/bench code: a panic here is the failure signal, not a crash path"
)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test under tests/ is compiled as its own crate whose every item is test support, so there is no non-test code for a mod tests to separate it from"
)]
#![expect(
    clippy::missing_panics_doc,
    reason = "a test asserts by panicking; that is the failure signal, so there is no caller to warn"
)]
#![expect(clippy::panic, reason = "a panic in a test IS the failure signal")]
#![expect(clippy::std_instead_of_alloc, reason = "this target links std")]
#![expect(
    clippy::single_call_fn,
    reason = "one named helper per step is what makes the test readable"
)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde_json::Value;

/// The fuzz targets under `fuzz/fuzz_targets/`. Every one takes the same
/// seeds; a target added later gets its corpus directory by joining this
/// list.
const TARGETS: [&str; 2] = ["parse", "roundtrip"];

/// Held while the committed corpus is read or written, so the scratch
/// tests never copy a corpus the writer is halfway through filling.
static COMMITTED: Mutex<()> = Mutex::new(());

/// The command that rewrites missing seeds, quoted in every failure.
const REMEDY: &str = "EDTF_WRITE_FUZZ_CORPUS=1 cargo test -p edtf-core --test fuzz_corpus";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/edtf-core sits two levels below the repository root")
        .to_path_buf()
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read fixture {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("parse fixture {}: {err}", path.display()))
}

/// The seed set, collected from the three fixture files.
fn seeds() -> BTreeSet<String> {
    let fixtures = repo_root().join("tests/fixtures");
    let mut seeds = BTreeSet::new();

    let conformance = read_json(&fixtures.join("legacy/edtf-conformance-corpus.json"));
    let sections = conformance
        .as_object()
        .expect("conformance corpus is an object");
    // Non-array values are metadata keys, not case lists.
    for case in sections.values().filter_map(Value::as_array).flatten() {
        if let Some(edtf) = case.get("edtf").and_then(Value::as_str) {
            seeds.insert(edtf.to_owned());
        }
    }

    let golden = read_json(&fixtures.join("legacy/edtf-golden.json"));
    seeds.extend(
        golden
            .as_object()
            .expect("golden snapshot is an object")
            .keys()
            .cloned(),
    );

    let loc = read_json(&fixtures.join("loc/loc-edtf-examples.json"));
    for list in ["examples", "invalid"] {
        let cases = loc
            .get(list)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("LoC corpus has no {list} array"));
        for case in cases {
            let edtf = case
                .get("edtf")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("LoC {list} case without an edtf string"));
            seeds.insert(edtf.to_owned());
        }
    }

    seeds
}

/// FNV-1a, 64-bit: the corpus file name of a seed's bytes.
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// The contents of every file in `dir`; empty when `dir` does not exist.
fn contents(dir: &Path) -> BTreeSet<Vec<u8>> {
    let Ok(entries) = fs::read_dir(dir) else {
        return BTreeSet::new();
    };
    entries
        .map(|entry| {
            let path = entry.expect("read corpus entry").path();
            fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        })
        .collect()
}

/// Hold `corpus` (a `fuzz/corpus` directory) to `seeds`. With `write`, a
/// missing seed is written instead of reported; a name already taken by
/// other bytes is an error either way.
///
/// # Errors
///
/// A report naming every missing seed and its directory (or every name
/// taken by other bytes), ending in the command that writes them.
#[expect(
    clippy::panic_in_result_fn,
    reason = "an I/O failure on the corpus is a broken test environment, not a corpus verdict; panicking keeps it out of the report"
)]
fn check(corpus: &Path, seeds: &BTreeSet<String>, write: bool) -> Result<(), String> {
    let mut errors = Vec::new();
    for target in TARGETS {
        let dir = corpus.join(target);
        let present = contents(&dir);
        for seed in seeds
            .iter()
            .filter(|seed| !present.contains(seed.as_bytes()))
        {
            if !write {
                errors.push(format!("seed {seed:?} is missing from {}", dir.display()));
                continue;
            }
            let path = dir.join(format!("{:016x}", fnv1a64(seed.as_bytes())));
            if let Ok(held) = fs::read(&path) {
                errors.push(format!(
                    "{} holds {:?}, not seed {seed:?}; refusing to overwrite",
                    path.display(),
                    String::from_utf8_lossy(&held),
                ));
                continue;
            }
            fs::create_dir_all(&dir)
                .unwrap_or_else(|err| panic!("create {}: {err}", dir.display()));
            fs::write(&path, seed).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{}\nremedy: {REMEDY}", errors.join("\n")))
    }
}

/// A fresh copy of the committed corpus under the target's scratch space.
fn scratch_copy(name: &str) -> PathBuf {
    let _guard = COMMITTED.lock().unwrap_or_else(PoisonError::into_inner);
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if root.exists() {
        fs::remove_dir_all(&root).expect("clear scratch corpus");
    }
    for target in TARGETS {
        let from = repo_root().join("fuzz/corpus").join(target);
        let to = root.join(target);
        fs::create_dir_all(&to).expect("create scratch corpus");
        for listed in fs::read_dir(&from).expect("read committed corpus") {
            let entry = listed.expect("read committed corpus entry");
            fs::copy(entry.path(), to.join(entry.file_name())).expect("copy corpus file");
        }
    }
    root
}

#[test]
fn committed_corpus_holds_every_seed() {
    let write = env::var_os("EDTF_WRITE_FUZZ_CORPUS").is_some_and(|value| value == "1");
    let corpus = repo_root().join("fuzz/corpus");
    let _guard = COMMITTED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Err(report) = check(&corpus, &seeds(), write) {
        panic!("{report}");
    }
}

#[test]
fn a_deleted_seed_is_named_then_rewritten() {
    let seeds = seeds();
    let corpus = scratch_copy("fuzz-corpus-deleted");
    let seed = seeds.first().expect("the fixtures define seeds");
    let dir = corpus.join(TARGETS[0]);
    let path = dir.join(format!("{:016x}", fnv1a64(seed.as_bytes())));
    fs::remove_file(&path).expect("delete one seed file");

    let report = check(&corpus, &seeds, false).expect_err("a missing seed fails");
    assert!(report.contains(&format!("{seed:?}")), "{report}");
    assert!(report.contains(&dir.display().to_string()), "{report}");
    assert!(report.contains(REMEDY), "{report}");

    check(&corpus, &seeds, true).expect("the writer restores the seed");
    assert_eq!(fs::read(&path).expect("rewritten seed"), seed.as_bytes());
    check(&corpus, &seeds, false).expect("the restored corpus passes");
}

#[test]
fn a_taken_name_is_never_overwritten() {
    let seeds = seeds();
    let corpus = scratch_copy("fuzz-corpus-taken");
    let seed = seeds.first().expect("the fixtures define seeds");
    let path = corpus
        .join(TARGETS[0])
        .join(format!("{:016x}", fnv1a64(seed.as_bytes())));
    fs::write(&path, "not the seed").expect("plant other bytes");

    let report = check(&corpus, &seeds, true).expect_err("a taken name fails");
    assert!(report.contains(&path.display().to_string()), "{report}");
    assert!(report.contains("not the seed"), "{report}");
    assert!(report.contains(&format!("{seed:?}")), "{report}");
    assert_eq!(fs::read(&path).expect("planted file"), b"not the seed");
}

#[test]
fn fnv1a64_matches_the_published_vectors() {
    assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
}
