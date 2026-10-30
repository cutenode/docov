//! `require-returns`: a function or method whose own body returns a value (`return x;`, or an arrow
//! function's expression body) has `@returns` (or `@return`). One whose return type is `void`,
//! `undefined` or `Promise<void>` needs none.

use crate::check::{Check, Entry, Finding, NoOptions, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<RequireReturns>()
}

/// The check.
pub struct RequireReturns;

impl Check for RequireReturns {
    type Options = NoOptions;
    const ABOUT: &'static str =
        "A function or method that returns a value has @returns, unless its return type is void.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        if subject.shape.returns && !subject.shape.void && !subject.has(&["returns", "return"]) {
            subject.missing("@returns".into(), out);
        }
    }
}
