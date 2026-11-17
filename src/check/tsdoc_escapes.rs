//! `tsdoc-escapes`: text escapes what TSDoc would read as markup, as `@microsoft/tsdoc` reads it:
//! a `{` that doesn't start an inline tag, a `}` and a `>` outside one, and an `@` in a word or
//! without a tag name. A backslash escapes only punctuation.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    tsdoc::Group,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocEscapes>()
}

/// The check.
pub struct TsdocEscapes;

impl Check for TsdocEscapes {
    type Options = NoOptions;
    const ABOUT: &'static str =
        "TSDoc text escapes a {, } or > that isn't markup and an @ that isn't a tag, and a backslash escapes only punctuation.";
    const TSDOC: bool = true;

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        subject.tsdoc(Group::Escapes, out);
    }
}
