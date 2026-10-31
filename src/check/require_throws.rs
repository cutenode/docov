//! `require-throws`: an exported function or method whose own body has a `throw` (outside a `try`
//! with a `catch`, and not in a function inside it) has `@throws` (or `@exception`).

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
