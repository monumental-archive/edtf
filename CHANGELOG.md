# Changelog

All notable changes to this project are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
over the public APIs of the crates this repository publishes.

## [2.0.0](https://github.com/monumental-archive/edtf/compare/v1.3.5...v2.0.0) - 2026-11-23

### Breaking

- the `pg14` feature of `edtf-postgres` is removed;
1.3.5 is the last release that has it.
🤖 Generated with [Claude Code](https://claude.com/claude-code) ([#316](https://github.com/monumental-archive/edtf/pull/316))
- Y-prefixed years of 19 or more digits (Y1E18 and
larger, in either spelling) no longer parse, and an oversized
exponent reports "year out of supported range" instead of "exponent
out of supported range".
--------- ([#332](https://github.com/monumental-archive/edtf/pull/332))
- `edtf_min` / `edtf_max` return `-infinity` / `infinity`
instead of NULL for a day outside the Postgres date range (before
4714-11-24 BC or after 5874897-12-31). Bounds in years ≤ 0 also change,
which is the fix.
--------- ([#334](https://github.com/monumental-archive/edtf/pull/334))
- `Note` gains the variant `EdtfCaseNormalized`, so an
exhaustive match on `Note` must handle it. Russian "xxxx" now reads as
the masked year `XXXX`, not `39XX`.
--------- ([#340](https://github.com/monumental-archive/edtf/pull/340))
- edtf-wasm's Rust API no longer has its own
`edtf_wasm::Summary`; `summarize` returns `edtf_core::Summary`, with
the same fields and the same JSON. The JS surface is unchanged. ([#373](https://github.com/monumental-archive/edtf/pull/373))
- `1868-1871~` reads as `1868~/1871~` (was
`1868/1871~`); `1914 or 1985~` reads as `1914~ | 1985~` (was
`1914 | 1985~`); a range ambiguous on both sides reports every
combination instead of refusing. ([#374](https://github.com/monumental-archive/edtf/pull/374))
- before/after, century parts, BC centuries and "or"
lists now emit ISO's one-of sets (`[..1916]`, `[1800..1829]`,
`[-0199..-0100]`, `[1863,1864]`), and centuries count from the year
divisible by 100. `Note::OpenInterval` is now `Note::BeforeAfter`,
`Note::CenturyPartInterval` is `Note::CenturyPart` and
`Note::BcCenturyInterval` is `Note::BcCentury`.
--------- ([#378](https://github.com/monumental-archive/edtf/pull/378))
- `Options::default_century` is `Option<Century>`,
built with `Century::new(first_year)`; a value that is not a
century's first year no longer constructs, and the JS
`defaultCentury` option rejects it. ([#382](https://github.com/monumental-archive/edtf/pull/382))
- `Note::EdtfCaseNormalized` is renamed
`Note::EdtfLettersNormalized`, with the message "EDTF letters read in
either case or script; written as upper-case Latin". ([#381](https://github.com/monumental-archive/edtf/pull/381))
- every public type of edtf-core with a rule has private
fields and validating constructors; `SetElement::Range` holds a
`SetRange`; `TimeShift` is a struct; `Unenumerable::YearRangeOverflow`
is removed; under `serde`, `Edtf` is its canonical string and no other
model type deserialises; edtf-calendars' `Converted` no longer
deserialises and `gregorian_to_julian` no longer returns `InvalidDate`.
--------- ([#385](https://github.com/monumental-archive/edtf/pull/385))
- `convert` returns `edtf_core::Edtf` and `Converted`
is removed; `CalendarError` gains `Malformed`; `convert` refuses a
result EDTF cannot spell with `OutOfRange`. ([#391](https://github.com/monumental-archive/edtf/pull/391))

### Added

- drop Postgres 14 at its end of life ([#289](https://github.com/monumental-archive/edtf/pull/289))
- build for Postgres 19 ([#353](https://github.com/monumental-archive/edtf/pull/353))

### Changed

- convert inner lint allows to expects ([#295](https://github.com/monumental-archive/edtf/pull/295))
- drop date display's uniform-qualifier shortcut ([#337](https://github.com/monumental-archive/edtf/pull/337))

### Fixed

- move edtf-postgres to pgrx 0.20.0 at rust 1.99.1 ([#293](https://github.com/monumental-archive/edtf/pull/293))
- print the year i64::MIN without overflow ([#333](https://github.com/monumental-archive/edtf/pull/333))
- skip a leading -- end-of-options marker ([#336](https://github.com/monumental-archive/edtf/pull/336))
- read ranges and "or" sides through one grammar ([#335](https://github.com/monumental-archive/edtf/pull/335))
- let before/after take a span, read sub-year codes ([#339](https://github.com/monumental-archive/edtf/pull/339))
- update pgrx to 0.20.1 ([#344](https://github.com/monumental-archive/edtf/pull/344))
- refuse century 0 as a bare left range endpoint ([#346](https://github.com/monumental-archive/edtf/pull/346))
- hold the fuzz crate to msrv, clippy and deny policy ([#354](https://github.com/monumental-archive/edtf/pull/354))
- lock file maintenance ([#352](https://github.com/monumental-archive/edtf/pull/352))
- distribute a stated year to a typed xxxx year ([#399](https://github.com/monumental-archive/edtf/pull/399))
- never split a range inside an iso date ([#403](https://github.com/monumental-archive/edtf/pull/403))
- refuse a span by the end its row reads ([#406](https://github.com/monumental-archive/edtf/pull/406))
- end no date unit at a hyphen number suffix ([#409](https://github.com/monumental-archive/edtf/pull/409))

### Documentation

- record the concept doi in the citation file ([#264](https://github.com/monumental-archive/edtf/pull/264))
- re-point carlallenn/edtf links to monumental-archive/edtf ([#287](https://github.com/monumental-archive/edtf/pull/287))
- correct the postgres matrix and three wrong doc comments ([#375](https://github.com/monumental-archive/edtf/pull/375))

### Testing

- adopt the canon's rust mutation leg ([#275](https://github.com/monumental-archive/edtf/pull/275))
- pin the to_pg_date range guard's edges ([#306](https://github.com/monumental-archive/edtf/pull/306))
- kill the parse_julian_parts sign-check mutant ([#307](https://github.com/monumental-archive/edtf/pull/307))
- assert year 0 displays unsigned ([#305](https://github.com/monumental-archive/edtf/pull/305))
- kill the 3 edtf-wasm mutation survivors ([#308](https://github.com/monumental-archive/edtf/pull/308))
- kill or explain the edtf-core mutation survivors ([#309](https://github.com/monumental-archive/edtf/pull/309))
- kill or explain the edtf-normalize mutation survivors ([#311](https://github.com/monumental-archive/edtf/pull/311))
- commit the seed corpus and hold it to the fixtures ([#314](https://github.com/monumental-archive/edtf/pull/314))
- add inputs that reach new code ([#322](https://github.com/monumental-archive/edtf/pull/322))
- hold sql assertions in the corpus alone ([#338](https://github.com/monumental-archive/edtf/pull/338))
- assert a february julian day number ([#343](https://github.com/monumental-archive/edtf/pull/343))
- fuzz the normaliser and the relation algebra ([#347](https://github.com/monumental-archive/edtf/pull/347))
- seed every reachable line and branch, measured per target ([#350](https://github.com/monumental-archive/edtf/pull/350))
- record equivalent mutants at their sites ([#359](https://github.com/monumental-archive/edtf/pull/359))
- sweep the bound conversion over the date range ([#376](https://github.com/monumental-archive/edtf/pull/376))
- add inputs that reach new code ([#377](https://github.com/monumental-archive/edtf/pull/377))
- search the arithmetic over every i64 year ([#390](https://github.com/monumental-archive/edtf/pull/390))
- make the cli one function held by properties ([#394](https://github.com/monumental-archive/edtf/pull/394))
- assert each general rule once, in every engine ([#392](https://github.com/monumental-archive/edtf/pull/392))
- retire the parse target into roundtrip ([#405](https://github.com/monumental-archive/edtf/pull/405))
- assert the composition rules once ([#393](https://github.com/monumental-archive/edtf/pull/393))

### CI

- adopt the canon's rust branch-coverage floor ([#273](https://github.com/monumental-archive/edtf/pull/273))
- remove codecov.yml for the org's global yaml ([#277](https://github.com/monumental-archive/edtf/pull/277))
- update canon to v1.72.0 and swap in the subject stubs ([#317](https://github.com/monumental-archive/edtf/pull/317))
- update canon to v1.73.0 and adopt its fuzz floor lines ([#320](https://github.com/monumental-archive/edtf/pull/320))
- update canon to v1.75.0 and drop the advisory ignore ([#321](https://github.com/monumental-archive/edtf/pull/321))

### Dependencies

- update node to 26.13.1 ([#267](https://github.com/monumental-archive/edtf/pull/267))
- pin cargo-auditable in its prebuilt github: form ([#268](https://github.com/monumental-archive/edtf/pull/268))
- update postgresql-binaries to 18.7.0 ([#279](https://github.com/monumental-archive/edtf/pull/279))
- update postgresql-binaries ([#283](https://github.com/monumental-archive/edtf/pull/283))
- update rust to 1.100.0 ([#290](https://github.com/monumental-archive/edtf/pull/290))

## [1.3.5](https://github.com/monumental-archive/edtf/compare/v1.3.4...v1.3.5) - 2026-11-16

### Fixed

- update canon to v1.68.1 ([#256](https://github.com/monumental-archive/edtf/pull/256))
- lock file maintenance ([#258](https://github.com/monumental-archive/edtf/pull/258))

### Documentation

- correct the install recipes to what v1.3.4 ships ([#248](https://github.com/monumental-archive/edtf/pull/248))

### CI

- adopt the scheduled security release ([#249](https://github.com/monumental-archive/edtf/pull/249))

## [1.3.4](https://github.com/monumental-archive/edtf/compare/v1.3.3...v1.3.4) - 2026-11-14

### Fixed

- update proptest to =1.12.0 ([#224](https://github.com/monumental-archive/edtf/pull/224))

## [1.3.3](https://github.com/monumental-archive/edtf/compare/v1.3.2...v1.3.3) - 2026-11-14

### Fixed

- conform the extension crate to the org clippy ladder ([#200](https://github.com/monumental-archive/edtf/pull/200))
- lock file maintenance ([#205](https://github.com/monumental-archive/edtf/pull/205))
- lock file maintenance ([#207](https://github.com/monumental-archive/edtf/pull/207))
- lock file maintenance ([#208](https://github.com/monumental-archive/edtf/pull/208))
- lock file maintenance ([#210](https://github.com/monumental-archive/edtf/pull/210))
- lock file maintenance ([#216](https://github.com/monumental-archive/edtf/pull/216))
- update pgrx to 0.19.3 ([#218](https://github.com/monumental-archive/edtf/pull/218))
- lock file maintenance ([#219](https://github.com/monumental-archive/edtf/pull/219))
- lock file maintenance ([#221](https://github.com/monumental-archive/edtf/pull/221))
- lock file maintenance ([#225](https://github.com/monumental-archive/edtf/pull/225))
- lock file maintenance ([#230](https://github.com/monumental-archive/edtf/pull/230))

### Documentation

- show the reuse badge now the org path is registered ([#239](https://github.com/monumental-archive/edtf/pull/239))

### CI

- run the gate for the merge queue ([#233](https://github.com/monumental-archive/edtf/pull/233))

### Dependencies

- update github/codeql-action to v4.37.8 ([#204](https://github.com/monumental-archive/edtf/pull/204))
- update github/codeql-action to v4.37.9 ([#206](https://github.com/monumental-archive/edtf/pull/206))
- update github/codeql-action to v4.38.0 ([#209](https://github.com/monumental-archive/edtf/pull/209))
- update node to 26.9.0 ([#212](https://github.com/monumental-archive/edtf/pull/212))
- update cargo:cargo-auditable to 0.7.6 ([#211](https://github.com/monumental-archive/edtf/pull/211))
- update github/codeql-action to v4.38.1 ([#214](https://github.com/monumental-archive/edtf/pull/214))
- update node to 26.10.0 ([#215](https://github.com/monumental-archive/edtf/pull/215))
- update github/codeql-action to v4.38.2 ([#217](https://github.com/monumental-archive/edtf/pull/217))
- update cargo:cargo-auditable to 0.7.7 ([#220](https://github.com/monumental-archive/edtf/pull/220))
- update node to 26.11.1 ([#222](https://github.com/monumental-archive/edtf/pull/222))
- update github/codeql-action to v4.38.3 ([#223](https://github.com/monumental-archive/edtf/pull/223))
- update ossf/scorecard-action to v2.5.0 ([#226](https://github.com/monumental-archive/edtf/pull/226))
- update node to 26.12.0 ([#228](https://github.com/monumental-archive/edtf/pull/228))
- update github/codeql-action to v4.39.0 ([#229](https://github.com/monumental-archive/edtf/pull/229))
- update node to 26.12.1 ([#231](https://github.com/monumental-archive/edtf/pull/231))
- update github/codeql-action to v4.39.1 ([#232](https://github.com/monumental-archive/edtf/pull/232))
- update github/codeql-action to v4.39.3 ([#235](https://github.com/monumental-archive/edtf/pull/235))
- update aqua:rustwasm/wasm-pack to 0.15.1 ([#234](https://github.com/monumental-archive/edtf/pull/234))
- update node to 26.13.0 ([#236](https://github.com/monumental-archive/edtf/pull/236))
- update ubuntu to 26.04 ([#227](https://github.com/monumental-archive/edtf/pull/227))

## [1.3.2](https://github.com/monumental-archive/edtf/compare/v1.3.1...v1.3.2) - 2026-08-24

### Fixed

- lock file maintenance ([#195](https://github.com/monumental-archive/edtf/pull/195))

## [1.3.1](https://github.com/monumental-archive/edtf/compare/v1.3.0...v1.3.1) - 2026-08-21

### Fixed

- repair the two defects that burned v1.3.0 ([#185](https://github.com/monumental-archive/edtf/pull/185))

## [1.3.0](https://github.com/monumental-archive/edtf/compare/v1.2.3...v1.3.0) - 2026-08-21

### Added

- publish the coverage number to Codecov, without a threshold gate ([#143](https://github.com/monumental-archive/edtf/pull/143))
- make the project citable — CITATION.cff, and the Zenodo path to a DOI ([#145](https://github.com/monumental-archive/edtf/pull/145))
- gate CITATION.cff against the CFF schema, not just YAML syntax ([#150](https://github.com/monumental-archive/edtf/pull/150))
- fold edtf-postgres into the coverage metric ([#155](https://github.com/monumental-archive/edtf/pull/155))
- publish a FROM scratch artifact variant ([#156](https://github.com/monumental-archive/edtf/pull/156))

### Fixed

- exclude the Apache licence's own http:// URLs from lychee ([#138](https://github.com/monumental-archive/edtf/pull/138))
- pin the OCI image base and record it in provenance ([#140](https://github.com/monumental-archive/edtf/pull/140))
- allow the host Codecov actually uploads to, and silence its telemetry ([#146](https://github.com/monumental-archive/edtf/pull/146))
- add the root CHANGELOG the release path writes into ([#173](https://github.com/monumental-archive/edtf/pull/173))
- keep a version number out of the CHANGELOG's headings ([#174](https://github.com/monumental-archive/edtf/pull/174))
- derive the released versions instead of listing them ([#180](https://github.com/monumental-archive/edtf/pull/180))

### Documentation

- correct the 1.2.x post-mortem with the actual root cause ([#137](https://github.com/monumental-archive/edtf/pull/137))
- let licence detection land on the real licence texts ([#139](https://github.com/monumental-archive/edtf/pull/139))
- storage.googleapis.com is proven, not assumed ([#148](https://github.com/monumental-archive/edtf/pull/148))
- the runnable image is a convenience, not an artifact ([#157](https://github.com/monumental-archive/edtf/pull/157))
- tell COPY --from consumers to move to -artifact (#161) ([#165](https://github.com/monumental-archive/edtf/pull/165))
- document verifying an image attestation (#159) ([#166](https://github.com/monumental-archive/edtf/pull/166))

### Testing

- cover every remaining defensive guard ([#152](https://github.com/monumental-archive/edtf/pull/152))
- prove the CloudNativePG ImageVolume claim (#158) ([#162](https://github.com/monumental-archive/edtf/pull/162))

### CI

- gate the image path on pull requests (#160) ([#164](https://github.com/monumental-archive/edtf/pull/164))

### Dependencies

- update dependency jdx/mise to v2026.8.0 ([#167](https://github.com/monumental-archive/edtf/pull/167))
- update jdx/mise-action action to v4.2.4 ([#169](https://github.com/monumental-archive/edtf/pull/169))

## Earlier history lives per crate

Until the 2026-08-21 import into `monumental-archive` this repository
released each crate on its own version line, and release-plz wrote a
changelog per crate. Those files are kept exactly as they were:

- [`crates/edtf-core/CHANGELOG.md`](crates/edtf-core/CHANGELOG.md)
- [`crates/edtf-calendars/CHANGELOG.md`](crates/edtf-calendars/CHANGELOG.md)
- [`crates/edtf-normalize/CHANGELOG.md`](crates/edtf-normalize/CHANGELOG.md)
- [`crates/edtf-cli/CHANGELOG.md`](crates/edtf-cli/CHANGELOG.md)
- [`crates/edtf-wasm/CHANGELOG.md`](crates/edtf-wasm/CHANGELOG.md)
- [`crates/edtf-postgres/CHANGELOG.md`](crates/edtf-postgres/CHANGELOG.md)

From the first unified release the repository has one version, inherited
by every member, and
this file is the changelog. It is written by the release machinery from the
commits in each release's range; edit the commits, not this file.
