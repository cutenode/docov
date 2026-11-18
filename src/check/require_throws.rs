//! `require-throws`: an exported function or method whose own body has a `throw` (outside a `try`
//! with a `catch`, and not in a function inside it) has `@throws` (or `@exception`).
//!
//! Async functions, async generators among them, are left out, as [eslint-plugin-jsdoc's
//! `require-throws`](https://github.com/gajus/eslint-plugin-jsdoc/blob/main/docs/rules/require-throws.md)
//! leaves them out: a `throw` in one rejects the promise it returns instead of throwing to its
//! caller. A sync generator still counts, since its `throw` reaches whoever iterates it.

use crate::check::{Check, Entry, Finding, NoOptions, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<RequireThrows>()
}

/// The check.
pub struct RequireThrows;

impl Check for RequireThrows {
    type Options = NoOptions;
    const ABOUT: &'static str = "An exported function or method that throws has @throws.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        if subject.exported && subject.shape.throws && !subject.has(&["throws", "exception"]) {
            subject.missing("@throws".into(), out);
        }
    }
}
