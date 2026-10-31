//! `summary-sentence`: a description's first sentence, its summary, is at most `maxLength`
//! characters, and with `period` on, ends with `.`, `!` or `?`. The summary ends at the first of
//! those followed by a space, or else with the first paragraph.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::check::{Check, Entry, Finding, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<SummarySentence>()
}

/// The check.
pub struct SummarySentence;

/// summary-sentence's options.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[schemars(rename = "SummarySentenceOptions")]
pub struct Options {
    /// The most characters the summary may have.
    pub max_length: usize,
    /// Whether the summary ends with `.`, `!` or `?`.
    pub period: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { max_length: 120, period: true }
    }
}

impl Check for SummarySentence {
    type Options = Options;
    const ABOUT: &'static str = "A description's first sentence is short and ends with a period.";

    fn check(options: &Options, subject: &Subject, out: &mut Vec<Finding>) {
        let Some(at) = subject.description() else { return };
        let description = subject.doc.comment().parsed_preserving_whitespace();
        let paragraph = description.split("\n\n").find(|p| !p.trim().is_empty()).unwrap_or_default();
        let paragraph = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut ends = paragraph.match_indices(['.', '!', '?']).map(|(i, _)| i + 1);
        let end = ends.find(|&i| paragraph[i..].starts_with(' ')).unwrap_or(paragraph.len());
        let summary = &paragraph[..end];
        let length = summary.chars().count();
        if length > options.max_length {
            let message = format!("the summary is {length} characters, over {}", options.max_length);
            out.push(Finding { at, message });
        }
        if options.period && !summary.ends_with(['.', '!', '?']) {
            out.push(Finding { at, message: "the summary doesn't end with a period".into() });
        }
    }
}
