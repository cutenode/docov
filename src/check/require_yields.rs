//! `require-yields`: a generator whose own body yields a value has `@yields` (or `@yield`).

use crate::check::{Check, Entry, Finding, NoOptions, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<RequireYields>()
}

/// The check.
pub struct RequireYields;

impl Check for RequireYields {
    type Options = NoOptions;
    const ABOUT: &'static str = "A generator that yields a value has @yields.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        if subject.shape.yields && !subject.has(&["yields", "yield"]) {
            subject.missing("@yields".into(), out);
        }
    }
}
