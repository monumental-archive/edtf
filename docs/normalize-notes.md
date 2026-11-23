# edtf-normalize — decisions and scope

The authority document for `crates/edtf-normalize` (issue #20): a bounded,
deterministic pattern grammar over date prose. Companion to
`docs/spec-notes.md`, which governs what valid EDTF *is*; this document
governs how prose becomes EDTF. Every judgement call below has a numbered
**N-decision** id; `Note` values returned by the crate cite these ids, test
files reference them in doc comments, and the oracle fixture
(`tests/fixtures/natlang/python-edtf-natlang.json`) cites them per entry.
Disagree with one? Each is a single table row or match arm — file an issue
against the N-number.

## 1. Scope and architecture

- The grammar is **whole-input**: the entire (noise-stripped) input must match
  one pattern. There is no substring extraction (N11) — pulling dates out of
  running prose is the job of the caller (a human, or an LLM triage step for
  bulk imports whose no-match rows escalate to a human).
- Values are built through `edtf-core`'s validating constructors (D31) and
  rendered by core's canonical `Display`. A constructor's refusal is the
  calendar check: a February 31, a February 29 a stated year lacks, or a
  span that ends before it starts becomes an impossible reading, and the
  whole input fails closed (N14). A value core built is valid EDTF at a
  known level, so nothing is re-parsed.
- The return type is honest: `Normalized { edtf, value, notes }`,
  `Ambiguous { interpretations }`, or `NoMatch { reason }`. Nothing ever
  guesses silently; every non-obvious mapping carries a `Note` citing its
  N-decision, and `NoMatchReason` discriminates out-of-grammar (N11),
  explicitly-undated (N12), and impossible-date (N14) failures so a
  bulk-import triage step can route them differently.
- Pattern tables are per-language (`Language::English`, `Language::Russian`).
  The grammar in `engine.rs` is language-neutral; a locale is one `Lang`
  const in `tables.rs` (word tables, era phrases, modifier phrases, range
  words, numeric-order convention). Contributions add tables, not code.
- Already-valid EDTF passes through untouched (canonicalized only, noted) —
  with one deliberate exception, N13.

## 2. Decisions

- **N1 — early/mid/late and halves of centuries:** ISO 8601-2 is silent on
  prose; python-edtf silently *discards* these modifiers ("1857-mid 1860s" →
  `1857/186X` in its own tests). We map standalone part-of-century phrases to
  decade-rounded spans of years: for the century starting `s` (N2),
  **early** = `s..s+29`, **mid** = `s+30..s+69`, **late** = `s+70..s+99`,
  **first half** = `s..s+49`, **second half** = `s+50..s+99`. The phrase
  names one year somewhere in the span, so the output is ISO's one-of set
  over a range (ISO 8601-2 6.2, 6.3 c), not a period: "early 19th century"
  → `[1800..1829]` (`CenturyPart`). An interval `1800/1829` would say
  something began sometime in 1800 and ended sometime in 1829 (A.4.4); the
  repository already reads "19th century" as "some year during" (`18XX`,
  9.2.2), and a part reads the same way. Thirds and halves each partition
  the century exactly (property-tested). BC centuries take their parts
  chronologically: "late 2nd century BC" → `[-0129..-0100]`. Scope limits,
  each recorded with `Note::ModifierDropped`: on **decades** ("early
  1930s") the modifier is dropped — sub-decade thirds would be false
  precision; inside **range endpoints** ("конец XIX - начало XX века") a
  century modifier collapses to the whole century, because a range end is
  a "sometime in" date (A.4.4) and a part's edge would state a year the
  prose never gave; and a **qualified part** ("circa early 19th century")
  falls back to the century's mask, `18XX~`, true but looser, because a
  range inside a set cannot carry a qualifier (D27). A qualified BC century
  or BC century part has no mask (D21) and is refused.
- **N2 — century arithmetic:** the *n*th century runs
  `(n-1)·100 ..= (n-1)·100+99`. ISO 8601-1 3.1.2.23 defines a century as
  beginning with a year divisible by 100 and gives the 19th century as 1800
  through 1899, so it masks as `(n-1)XX`, which says exactly that: "19th
  century" → `18XX`, "1st century" → `00XX` (years 0 to 99). **BC
  centuries cannot be masked**: edtf-core rejects unspecified digits in
  negative years (D21; Annex A's L1 mask shapes are positive-only), and the
  1st century BC would need `-0000`. They are counted from 1 BC backwards
  as ISO 8601-2 4.4.1.8 does, and emitted as the one-of set of their
  astronomical years (6.2, 6.3 c): "2nd century BC" → `[-0199..-0100]`,
  "1st century BC" → `[-0099..0000]`, carrying `BcCentury` (never
  `CenturyMask`, whose masking claim would be false for them). python-edtf
  emits `-01XX`, which our core rejects — recorded as an ism, not adopted.
- **N3 — astronomical BC years:** EDTF/ISO 8601-2 uses astronomical
  numbering: year 0 exists and is 1 BC. Year *b* BC = `-(b-1)`: "500 BC" →
  `-0499`, "64 BCE" → `-0063`. Year markers may sit *before* the era phrase
  ("44 г. до н. э.", the standard Russian textbook form) — trailing noise is
  stripped again after the era splits off. python-edtf applies the offset to
  centuries but *not* to years (its tests demand `c64 BCE` → `-0064~` yet
  `2nd century bc` → `-01XX` = −199..−100) — the internal inconsistency is
  recorded as an ism and its year outputs are not adopted.
- **N4 — elided end years:** in a hyphen range whose right side is two
  digits, the end year inherits the start's century: "1914-18" → `1914/1918`,
  "1851-52" → `1851/1852`. The elided value must exceed the start year;
  otherwise no range reading exists. Two-digit ends 01–12 are never ranges
  (they read as EDTF months) and 21–41 collide with sub-year codes (N13) —
  and these limits apply identically to the spaced and range-word forms
  ("1914 - 21", "from 1914 to 21"), so spacing cannot smuggle a reading past
  them. A **BC start refuses elision entirely** ("500 BC to 18" → `NoMatch`):
  no deterministic reading exists — the CE reading, the BC-ordinal reading,
  and truncating century arithmetic all disagree — so per the contract the
  normalizer refuses rather than fabricates. One implementation reads every
  form: the unspaced token, the spaced hyphen and the range words all go
  through the same endpoint pairing, so they cannot disagree. A range may
  start and end in the same year (`1914-1914` → `1914/1914`); only a
  reversed range is refused. Year 0 is 1 BC, so it refuses elision too.
- **N5 — all-numeric dates:** "12/04/1985" is DD/MM or MM/DD; without context
  the order is *unknowable* and the result is `Ambiguous` with both readings
  — never python-edtf's silent `dayfirst=False` guess. Resolution, in
  precedence order, each with its own note: a field > 12 proves the order;
  equal fields make it irrelevant; `Options::numeric_order` (the form's
  locale) resolves it; the *language's* convention resolves it (Russian
  `12.04.1985` is day-first by national convention). Year-first
  (`1985/04/12`) is inherently unambiguous. Bare two-digit years
  ("12/04/85", "90") are not adopted: the century is unknowable.
- **N6 — decades:** "1980s"/"1980-е" → `198X`. Two forms are inherently
  ambiguous and reported as such: "**1900s**" is the 1900–1909 decade
  (`190X`) *or* the century (`19XX`); a **bare decade** ("the 80s", "80-е")
  could sit in any century — we offer the 1800s and 1900s readings, and
  `Options::default_century` (e.g. `Some(1900)`, domain `0..=9999`) resolves
  it for forms whose material has a known era. A decade tied to an explicit
  century is not ambiguous at all: "60-е годы XIX века" → `186X`
  (`DecadeOfCentury`), the idiom behind «шестидесятники». python-edtf is
  internally inconsistent here ("1800s" → `18XX` but "c1900s" → `190X~`) —
  recorded, not adopted.
- **N7 — seasons:** season names map to ISO 8601-2 §4.8 sub-year grouping
  codes 21–24 (spring=21, summer=22, autumn/fall=23, winter=24; Russian
  весна/лето/осень/зима likewise), which 4.8.1 lists as independent of
  location. Codes 25–41 (hemisphere-specific, quarters, semesters) are
  **never emitted** — no deterministic English or Russian prose form maps
  to them. A season has no dates (4.8.1), so it has no predecessor or
  successor: strictly before or after one has no form (N8).
- **N8 — before and after take ISO's forms:** one table of phrases, each
  row carrying its meaning, matched longest phrase first:

  | Meaning | English | Russian | Output |
  | --- | --- | --- | --- |
  | strictly before | `earlier than`, `before` | `ранее`, `до` | `[..pred(lo)]` |
  | on or before | `no later than`, `by` | `не позднее` | `[..hi]` |
  | period ending | `until` | none | `../X` |
  | strictly after | `later than`, `after` | `после` | `[succ(hi)..]` |
  | on or after | `no earlier than` | `не ранее` | `[lo..]` |
  | period starting | `since` | `начиная с`, `с` | `X/..` |

  "before 1917" names one year before 1917, so it is the one-of set
  `[..1916]`: ISO 8601-2 6.3 a reads `..1984` as 1984 or a year before it,
  which is the claim "on or before" makes, and "strictly before" ends a
  unit earlier. "until" and "since" state a period, so they stay open
  intervals with the current-spec `..` (never python-edtf's old-draft bare
  slash `/1928`). Standing alone, Russian `до` is exclusive (`по` is the
  inclusive end); inside a range `с … до …` and `от … до …` stay range
  words. `lo` and `hi` are the operand's earliest and latest day
  (`bounds()`), cut to the operand's own precision (year, month or day);
  `pred` and `succ` step one unit at that precision, through core's
  `BoundDate::previous_day` and `next_day`, so the leap rule lives in one
  place: "before 1 March 1900" → `[..1900-02-28]`, "before the 1980s" →
  `[..1979]`, "no later than the 1980s" → `[..1989]`. An unmasked date is
  its own `lo` and `hi`, as written ("no later than Y17E7" → `[..Y17E7]`);
  a stepped or cut unit takes the one year form core accepts for its
  value, four digits within ±9999 and `Y`-prefixed beyond (4.7.2):
  "before Y10000" → `[..9999]`, "after 9999" → `[Y10000..]`. The unit
  carries the operand's qualifier ("before circa 1917" → `[..1916~]`). A
  span — a
  one-of range, or an interval with two dated ends — gives the `lo` of its
  start and the `hi` of its end: "before 1914-1918" → `[..1913]`, "after
  early 19th century" → `[1830..]`. For the two period rows `X` is the
  operand itself when it is one date ("until the 1980s" → `../198X`) and
  its `hi` or `lo` when it is a span. A season has no predecessor or
  successor (N7), so strictly before or after one is refused; the other
  rows use the season date itself ("no later than spring 1920" →
  `[..1920-21]`). A **year-less operand** — a year with no specified
  digit, as a standalone month reads under N9 ("June" is `XXXX-06`) or
  `xxxx` — is refused for the four set rows: its units would be read off
  core's 0000–9999 bounds (`[..0000-05]` for "before June"), stating a
  year the prose never gave. "until" and "since" take it as written
  ("until June" → `../XXXX-06`). A partly masked year still reads
  ("before 19XX" → `[..1899]`). A set of several elements, a datetime and
  an interval with an open or unknown end are refused as operands. An operand with
  several readings gives one output per reading, and a reading with no
  form refuses the input (N14). Recorded so it is not re-proposed: the
  open interval `../1917` was chosen first for every row, as "a single
  event somewhere in an open span", and overruled by 6.3, under which
  `../1917` includes the year "before" excludes and one output served
  three meanings.
- **N9 — missing years are masked:** a *standalone* month with no year
  ("January", "January 12", "апрель") gets year `XXXX`: `XXXX-01`,
  `XXXX-01-12`. Never an assumed current year — the normalizer has no clock
  on purpose (determinism: same input, same output, forever). Inside a range
  the year is elided, not missing — N16 governs there, and `XXXX` never
  leaks into a range that states its year. A year-less date is no
  operand for before, after, on or before, or on or after (N8): only
  "until" and "since" take it. python-edtf's broader masked-precision
  prose ("day in Spring 1849" → `1849-21-XX`) is not
  adopted: "a day in" is annotation, not a date.
- **N10 — whole-expression qualifiers distribute:** a qualifier scoping the
  whole input applies to every component ("circa June 1940" → `1940-06~`,
  rendered in core's canonical trailing form) and, on intervals, to every
  dated endpoint: "circa 1914-1918" → `1914~/1918~`, "1868-1871?" →
  `1868?/1871?`. The symbols `?`, `~` and `%` scope alike, and like a
  trailing qualifier word: a trailing symbol on a range or an "or" list
  scopes all of it ("1868-1871~" → `1868~/1871~`, "1914 or 1985~" →
  `[1914~,1985~]`). Text that is valid EDTF as it stands carries its
  own qualifiers and passes through untouched ("1984?/2004~"). python-edtf
  attaches the trailing qualifier to the end year only (`1868/1871?`);
  prose uncertainty scopes the whole range, so we diverge. Endpoint-local
  qualifiers stay local: "1856-ca. 1865" → `1856/1865~`.
- **N11 — no substring extraction:** inputs with trailing annotations
  ("1861-67 (later print)", "1863, printed 1870", "birthday in 1872") are
  `NoMatch`, full stop. python-edtf extracts a date from all of these; that
  behavior is the single largest recorded ism family. The boundary is
  deliberate: deterministic keystroke normalization here, freeform
  extraction in the caller's triage layer.
- **N12 — "unknown" is the form's problem:** "unknown", "undated", "n.d.",
  "без даты", "не датировано" → `NoMatch` with
  `NoMatchReason::ExplicitNoDate` — distinguishable from out-of-grammar
  prose (`OutOfGrammar`, N11) so a triage step can route "explicitly
  undated" rows without re-reading the input. Whether that means an EDTF
  `XXXX`, an empty column, or a required-field error is form policy, not
  normalizer policy.
- **N13 — the sub-year-code / elided-range collision:** `NNNN-NN` with NN in
  21–41 is *valid EDTF* (a sub-year code: "1914-21" is spring 1914) **and**
  a plausible elided range (1914–1921). This is the one place already-valid
  EDTF does not pass through silently: when the elided reading is
  chronologically possible, both readings are reported. Elided ends 13–20
  and 42–99 are not valid codes, so only the range reading exists
  ("1914-18" → `1914/1918`). Inside a phrase the same holds: "no later
  than 1914-21" has two readings, `[..1914-21]` and `[..1921]`, while
  "before 1914-21" is refused, because strictly before a season has no
  form (N7, N8) and a reading with no form refuses the input (N14). Where
  no elided reading exists the token is simply the sub-year date ("no
  later than 1985-21" → `[..1985-21]`). As a range endpoint a colliding
  token is refused.
- **N14 — alternatives are one set, and dead readings poison:**
  "1863 or 1864" ("1863 или 1864") → `[1863,1864]`, ISO's one-of set
  (ISO 8601-2 6.2), which says exactly what was written: one of these.
  python-edtf silently keeps the first. When every alternative is a date or
  a one-of set (whose elements are taken in place), the result is one set
  in written order, `Normalized`, with `OrAlternatives`: "1863 or before
  1850" → `[1863,..1849]`, "1863 or early 19th century" →
  `[1863,1800..1829]`, "1914 or [1667,1668]" → `[1914,1667,1668]`. Equal
  elements are written once, and a set left with one plain date is that
  date ("1914 or 1914" → `1914`). Year inheritance (N16) and qualifiers
  apply before the set is built ("circa 1863 or 1864" → `[1863~,1864~]`).
  An alternative with several readings gives one set per combination
  ("1863 or 1900s" → `[1863,190X]` or `[1863,19XX]`). When any alternative
  is a period (an interval), a datetime or an all-members set, no single
  expression exists and each alternative is reported as a reading:
  "from 1914 to 1918 or 1920" is `1914/1918` or `1920`. "or" always
  separates whole alternatives and never binds inside a range, and any
  number of alternatives is read. When any reading of an input dies at the
  calendar check ("31 June 1940 or 1 July 1940", a reversed masked range
  like "1910s to 1900s"), or has no form, the whole input fails closed —
  promoting the survivor would fake certainty the prose never had.
  Recorded so it is not re-proposed: the set was first rejected here as "a
  different claim" from two plain readings; ISO 8601-2 6.2 and 6.3 make it
  the claim.
- **N15 — Roman-numeral centuries (Russian):** "XIX век", "XIX в.",
  "XVII-XIX вв." parse as centuries, including with Cyrillic lookalike
  letters (Х/х for X, etc.), which real Russian sources routinely contain,
  and including numerals that *start* with a subtractive pair ("IV век",
  "IX-X вв." — the accumulator is signed for exactly this reason). A *bare*
  Roman numeral is accepted as a century only when it is at least two
  letters ("I" alone is too weak a signal; "I век" is fine). Roman
  provenance is cited on results as `Note::RomanCentury`.
- **N16 — range endpoints inherit the stated year:** "June-July 1940",
  "с июня по июль 1940", "12 January to 15 March 1940" — when exactly one
  endpoint lacks a year and the other states one, the year scopes both: the
  year-less endpoint inherits it (`EndpointYearDistributed`), symmetric to
  N4's century inheritance and direction-aware ("from January 1940 to June"
  fills rightward). Emitting `XXXX` there would assert the year is unknown
  when the prose gives it. Reversed month ranges ("July-June 1940") fail
  closed at the calendar check. Both-endpoints-year-less ranges
  ("May-June") keep their masks: no year exists anywhere in the input. The
  same holds between alternatives: "June or July 1940" is
  `[1940-06,1940-07]`. When the alternatives that state a year disagree,
  or state one that is not fully specified, the input is refused.
- **N17 — cross-year winters are ambiguous:** "winter 1941-42" (also
  "-1942", "зима 1941-1942 гг.") names either ONE winter spanning the year
  boundary — EDTF's code 24 wraps, so `1941-24` already covers Dec 1941 –
  Feb 1942 — or winter 1941 through the whole of 1942. Both readings are
  reported (`CrossYearSeason`), N13-style. A non-adjacent end year
  ("winter 1941-45") has only the range reading. Winter is the only emitted
  season that wraps; the others compose as plain ranges.
- **N18 — EDTF letters in either case:** EDTF notation is read in either
  letter case, as a whole input and as a token inside a phrase ("198x" →
  `198X`; "until 198X" → `../198X`; "198X or 1990" → `[198X,1990]`).
  A whole input and a one-token part of a phrase alike are tried as EDTF
  before they are read as prose, so Russian "xxxx" is the masked year
  `XXXX`, not a century, and "до xxxx" is refused, as any year-less
  operand of "before" is (N8); forty as a Roman numeral is XL ("до xxxx
  века", before the 40th century, is `[..3899]`). The one exception is a
  colliding sub-year token, which stays N13's question. Any EDTF value
  may be an alternative ("1914 or 1985/1990", "1914 or [1667,1668]"); an
  interval with two dated ends is a span, so it is a before/after operand
  by its start or its end (N8); a set of several elements, a datetime or
  an interval with an open or unknown end is no operand, and none of
  these is a range end. The note `EdtfCaseNormalized` is attached exactly
  when the fold changed the text, and `AlreadyValidEdtf` only when the
  EDTF token is the whole input.

## 3. Test ceremony

- `tests/traps.rs` / `tests/traps_ru.rs` — the acceptance trap lists; every
  expected output is re-parsed in core and its level asserted.
- `tests/props.rs` — constructive property tests: for every century 2 to
  20, century parts partition ISO's century exactly as one-of ranges; BC
  arithmetic; and the umbrella invariant that anything emitted parses in
  core and equals its returned `value`.
- `tests/oracle.rs` over
  `tests/fixtures/natlang/python-edtf-natlang.json` — the python-edtf
  corpus, bucketed: `agreements` (enforced equal), `divergences` (enforced
  to OUR documented output, each citing an N-decision, each recording their
  ism), `not_adopted` (their extractions/guesses, enforced `NoMatch`).
  python-edtf is an **oracle, not an authority**: agreement is evidence,
  divergence is documented policy, and this crate takes precedence.

## 4. Non-goals

Freeform NLP, sentence extraction, multilingual guessing without tables,
wall-clock-relative dates ("last summer"), and any output that does not
parse in `edtf-core`.
