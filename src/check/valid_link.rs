//! `valid-link`: each `{@link}`, `{@linkcode}` and `{@linkplain}` is closed, has no unescaped `{`
//! inside, and has a target, and each `@see` has text. A target that's a plain name, or a namepath
//! (`A#b`, `A.b`, `A~b`) by its first part, names something the file declares or imports, a type a
//! block there declares, or a JavaScript, browser or Node.js global. URLs, `module:` paths and other
//! targets aren't looked up, nor are names in other files.

use std::sync::LazyLock;

use regex::Regex;

use crate::check::{Check, Entry, Finding, NoOptions, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<ValidLink>()
}

/// The check.
pub struct ValidLink;

/// An inline link tag's start.
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{@link(code|plain)?\b").expect("a valid regex"));

/// A plain name or namepath, which the file can declare.
static NAMEPATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}_$][\p{L}\p{N}_$]*([.#~][\p{L}\p{N}_$]+)*(\(\))?$").expect("a valid regex"));

impl Check for ValidLink {
    type Options = NoOptions;
    const ABOUT: &'static str =
        "Each {@link} is well formed, and a target that's a name is declared or imported in its file.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        // Each link is in the description or a tag's text, and closes there.
        let tags = subject.doc.tags();
        let parts = std::iter::once(subject.doc.comment().span).chain(tags.iter().map(|tag| tag.comment().span));
        for part in parts {
            let text = part.source_text(subject.source);
            for link in LINK.find_iter(text) {
                let line = &text[text[..link.start()].rfind('\n').map_or(0, |i| i + 1)..link.start()];
                // Escaped, or in a code span, it's an example of the syntax.
                if line.ends_with('\\') || line.matches('`').count() % 2 == 1 {
                    continue;
                }
                let (at, tag, rest) = (part.start + link.start() as u32, &link.as_str()[1..], &text[link.end()..]);
                let Some(end) = unescaped(rest, '}') else {
                    out.push(Finding { at, message: format!("{{{tag}}} isn't closed") });
                    continue;
                };
                if let Some(open) = unescaped(&rest[..end], '{') {
                    let at = at + (link.len() + open) as u32;
                    out.push(Finding { at, message: format!("an unescaped `{{` in {{{tag}}}") });
                    continue;
                }
                match rest[..end].split(['|', ' ', '\t', '\r', '\n']).find(|part| !part.is_empty()) {
                    None => out.push(Finding { at, message: format!("{{{tag}}} has no target") }),
                    Some(target) => resolve(subject, target, at, out),
                }
            }
        }
        for tag in tags.iter().filter(|tag| tag.kind.parsed() == "see") {
            let text = tag.comment().parsed();
            let at = tag.kind.span.start;
            match text.split_whitespace().collect::<Vec<_>>()[..] {
                [] => out.push(Finding { at, message: "@see has no text".into() }),
                [target] => resolve(subject, target, at, out),
                _ => {}
            }
        }
    }
}

/// Where `text` first has `c` without a `\` before it.
fn unescaped(text: &str, c: char) -> Option<usize> {
    text.match_indices(c).map(|(i, _)| i).find(|&i| !text[..i].ends_with('\\'))
}

/// Adds a finding at `at` when `target` is a plain name or namepath whose first part `subject`'s
/// file doesn't have.
fn resolve(subject: &Subject, target: &str, at: u32, out: &mut Vec<Finding>) {
    if !NAMEPATH.is_match(target) {
        return;
    }
    let first = target.split(['.', '#', '~', '(']).next().unwrap_or_default();
    let global = ["builtin", "browser", "node"]
        .iter()
        .any(|env| javascript_globals::GLOBALS.get(env).is_some_and(|globals| globals.contains_key(first)));
    if !global && !subject.names.contains(first) {
        out.push(Finding { at, message: format!("`{target}` isn't declared or imported in this file") });
    }
}
