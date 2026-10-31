//! `doc-markdown`: a description's code is in backticks, and its Markdown lists have no lazy
//! continuation lines, as clippy's doc_markdown and doc_lazy_continuation have it. A word outside
//! backticks, inline tags and code blocks that looks like code (`snake_case`, `a::b`, `camelCase`
//! or `PascalCase`) is a finding unless it's in the allowlist, which adds to a default one of words
//! like `JavaScript` and `GitHub`.

use std::sync::LazyLock;

use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::check::{self, Check, Entry, Finding, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<DocMarkdown>()
}

/// The check.
pub struct DocMarkdown;

/// doc-markdown's options.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "DocMarkdownOptions")]
pub struct Options {
    /// Words that look like code and aren't, on top of the default ones.
    pub allowlist: Vec<String>,
}

/// Words that look like code and aren't.
const ALLOWED: &[&str] = &[
    "CommonJS", "DevTools", "ECMAScript", "GitHub", "GitLab", "GraphQL", "IPv4", "IPv6", "JavaScript", "JSDoc",
    "LaTeX", "MongoDB", "MySQL", "NaN", "NaNs", "NodeJS", "OAuth", "OpenAPI", "OpenSSL", "PostgreSQL", "TSDoc",
    "TypeScript", "WebAssembly", "WebGL", "WebGPU", "WebRTC", "WebSocket", "WebSockets", "YouTube", "iOS", "iPad",
    "iPhone", "macOS",
];

/// A list item's marker and the space after it.
static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ *([-*+]|\d+[.)]) +").expect("a valid regex"));

impl Check for DocMarkdown {
    type Options = Options;
    const ABOUT: &'static str = "A description's code is in backticks, and its lists have no lazy continuation lines.";

    fn check(options: &Options, subject: &Subject, out: &mut Vec<Finding>) {
        let allowed = |word: &str| ALLOWED.contains(&word) || options.allowlist.iter().any(|a| a == word);
        // The indentation of the text of the list item the line before is in, if it's in one.
        let mut item: Option<usize> = None;
        let mut fenced = false;
        for (at, line) in check::lines(subject.source, subject.doc.comment().span) {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            if line.is_empty() {
                item = None;
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            match ITEM.find(line) {
                Some(marker) => item = Some(marker.end()),
                None if item.is_some_and(|text| indent < text) => {
                    out.push(Finding { at, message: "a lazy continuation line in a list item: indent it".into() });
                }
                None => {}
            }
            for (start, word) in words(line) {
                if looks_like_code(word) && !allowed(word) {
                    let at = at + start as u32;
                    out.push(Finding { at, message: format!("`{word}` is code, so put it in backticks") });
                }
            }
        }
    }
}

/// The words of `line` outside code spans and inline tags (`{@link …}`), without the punctuation
/// around them, and where each starts.
fn words(line: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut spans = Vec::new();
    let (mut code, mut tag, mut start) = (false, false, None);
    for (i, c) in line.char_indices() {
        let breaks = match c {
            '`' => {
                code = !code;
                true
            }
            '{' if !code && line[i..].starts_with("{@") => {
                tag = true;
                true
            }
            '}' if tag => {
                tag = false;
                false
            }
            c => c.is_whitespace(),
        };
        if breaks {
            spans.extend(start.take().map(|s| (s, i)));
        } else if !code && !tag && c != '}' {
            start.get_or_insert(i);
        }
    }
    spans.extend(start.map(|s| (s, line.len())));
    spans.into_iter().filter_map(|(s, e)| {
        let word = &line[s..e];
        let trimmed = word.trim_start_matches(['(', '[', '"', '\'', '*', '_']);
        let s = s + word.len() - trimmed.len();
        let trimmed = trimmed.trim_end_matches([')', ']', '"', '\'', '.', ',', ';', ':', '!', '?', '*', '_']);
        Some((s, trimmed)).filter(|(_, w)| !w.is_empty())
    })
}

/// Whether `word` looks like code: it has `::`, a `_` between letters or digits, or a lowercase
/// letter right before an uppercase one. URLs, paths and addresses don't.
fn looks_like_code(word: &str) -> bool {
    if word.contains(['/', '@']) {
        return false;
    }
    let chars: Vec<char> = word.chars().collect();
    let snake = chars.windows(3).any(|w| w[1] == '_' && w[0].is_alphanumeric() && w[2].is_alphanumeric());
    snake || word.contains("::") || chars.windows(2).any(|w| w[0].is_lowercase() && w[1].is_uppercase())
}
