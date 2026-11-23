// SPDX-FileCopyrightText: Copyright (c) the edtf contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The summary of a parsed value: the words every surface prints for it.
//!
//! The CLI's `info` command and the wasm `parse` export both print this one
//! [`Summary`], so the words for a kind, a precision and a bound are
//! defined here, once, beside the type — the same way [`Modality::as_str`]
//! owns the words for a relation's modality.
//!
//! [`Modality::as_str`]: crate::Modality::as_str

use alloc::string::{String, ToString as _};

use crate::{Bound, Date, Edtf, Precision};

/// What a surface prints for one valid expression: its canonical form,
/// level, kind, precision, bounds and qualification flags.
///
/// Built by [`Edtf::summary`]. Under the `serde` feature it serialises to
/// the JSON object the CLI's `info` command and the wasm `parse` export
/// print, with these field names.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct Summary {
    /// The canonical (spec-preferred) rendering of the value.
    pub canonical: String,
    /// Minimum EDTF conformance level (0, 1 or 2).
    pub level: u8,
    /// `"date" | "datetime" | "interval" | "set"`.
    pub kind: &'static str,
    /// `"year" | "month" | "season" | "day" | null` (dates/datetimes only).
    pub precision: Option<&'static str>,
    /// Earliest calendar day: `"YYYY-MM-DD"`, `"-infinity"`, or null (unknown).
    pub earliest: Option<String>,
    /// Latest calendar day: `"YYYY-MM-DD"`, `"infinity"`, or null (unknown).
    pub latest: Option<String>,
    /// Any component marked uncertain (`?` or `%`).
    pub uncertain: bool,
    /// Any component marked approximate (`~` or `%`).
    pub approximate: bool,
    /// Any component with unspecified digits (`X`).
    pub unspecified: bool,
}

impl Edtf {
    /// The [`Summary`] of this value: the words a surface prints for it.
    #[must_use]
    pub fn summary(&self) -> Summary {
        let (kind, precision) = match *self {
            Self::Date(d) => ("date", Some(precision_word(&d))),
            Self::DateTime(dt) => ("datetime", Some(precision_word(&dt.date))),
            Self::Interval(_) => ("interval", None),
            Self::Set(_) => ("set", None),
        };
        let bounds = self.bounds();
        Summary {
            canonical: self.to_string(),
            level: self.level(),
            kind,
            precision,
            earliest: bound_word(bounds.earliest),
            latest: bound_word(bounds.latest),
            uncertain: self.is_uncertain(),
            approximate: self.is_approximate(),
            unspecified: self.has_unspecified(),
        }
    }
}

/// The precision of a date as the lowercase word a surface prints.
fn precision_word(d: &Date) -> &'static str {
    match d.precision() {
        Precision::Year => "year",
        Precision::Month => "month",
        Precision::Season => "season",
        Precision::Day => "day",
    }
}

/// A bound as the word a surface prints: its `YYYY-MM-DD` day,
/// `-infinity` or `infinity`, or `None` for an unknown bound.
fn bound_word(b: Bound) -> Option<String> {
    match b {
        Bound::Date(d) => Some(d.to_string()),
        Bound::NegativeInfinity => Some(String::from("-infinity")),
        Bound::PositiveInfinity => Some(String::from("infinity")),
        Bound::Unknown => None,
    }
}
