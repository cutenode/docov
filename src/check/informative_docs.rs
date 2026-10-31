//! `informative-docs`: a description says more than its item's name, as eslint-plugin-jsdoc's
//! informative-docs has it: `/** The user name. */` on `userName` restates it. Words are compared
//! without case, split at camelCase and separators, and past filler words.

use rustc_hash::FxHashSet;

use crate::check::{Check, Entry, Finding, NoOptions, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<InformativeDocs>()
}

/// The check.
pub struct InformativeDocs;

/// Words that say nothing about an item.
const FILLER: [&str; 7] = ["a", "an", "the", "this", "gets", "sets", "returns"];

impl Check for InformativeDocs {
    type Options = NoOptions;
    const ABOUT: &'static str = "A description says more than its item's name.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        let Some(at) = subject.description() else { return };
        let description = subject.doc.comment().parsed();
        let own = subject.name.rsplit('.').next().unwrap_or_default();
        let named: FxHashSet<String> = words(own).collect();
        if words(&description).all(|word| FILLER.contains(&word.as_str()) || named.contains(&word)) {
            let message = format!("the description only restates the name `{own}`");
            out.push(Finding { at, message });
        }
    }
}

/// The words in `text`, lowercased: runs of letters and digits, split between letters and digits,
/// where a lowercase letter meets an uppercase one, and before the last capital of a run of them
/// (`HTTPServer` is `http` and `server`).
fn words(text: &str) -> impl Iterator<Item = String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            words.extend(Some(std::mem::take(&mut word)).filter(|w| !w.is_empty()));
            continue;
        }
        let previous = i.checked_sub(1).map(|i| chars[i]).filter(|p| p.is_alphanumeric());
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        let boundary = previous.is_some_and(|p| {
            p.is_numeric() != c.is_numeric() || (c.is_uppercase() && (!p.is_uppercase() || next_lower))
        });
        if boundary {
            words.push(std::mem::take(&mut word));
        }
        word.extend(c.to_lowercase());
    }
    words.extend(Some(word).filter(|w| !w.is_empty()));
    words.into_iter()
}
