# edtf-calendars

Proleptic **Julian (Old Style) ↔ Gregorian** conversion at the EDTF ingest
boundary. Deterministic day-number arithmetic, `#![no_std]`, no third-party
dependencies (only `edtf-core`).

- `julian_to_gregorian` / `gregorian_to_julian` — complete dates, both
  directions, each validated under its own calendar's leap rule (Julian
  `1900-02-29` exists and converts; Gregorian `1900-02-29` never existed).
- `convert` — year / year-month / complete-date parts as they arrive from
  archival records, producing an EDTF value built through `edtf-core`'s
  constructors: a date for a complete date, an interval from the first to
  the last Gregorian day for a Julian year or month. A result outside the
  Gregorian years −9999 to 9999 has no EDTF spelling and is refused with
  `CalendarError::OutOfRange`.
- `parse_julian` — the text forms `Y`, `Y-MM` and `Y-MM-DD` (astronomical
  year, optional `-`) read into the parts `convert` takes.

```rust
use edtf_calendars::{convert, parse_julian};

let (year, month, day) = parse_julian("1917").unwrap();
assert_eq!(
    convert(year, month, day).unwrap().to_string(),
    "1917-01-14/1918-01-13"
);
```

Part of the [`edtf`](https://github.com/monumental-archive/edtf) crate family
(`edtf-core`, `edtf-normalize`, `edtf-calendars`, `edtf-cli`, `edtf-wasm`,
`edtf-postgres`).
