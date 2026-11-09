//! `tsdoc-inline-tags`: tags are well formed, as `@microsoft/tsdoc` reads them. A tag's name is a
//! letter then letters and digits, with a space after a block tag's and an inline tag's, and an
//! inline tag `{@name …}` is closed, with any `{` inside escaped. `{@inheritDoc}` has nothing past
//! its declaration reference.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    tsdoc::Group,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocInlineTags>()
}

/// The check.
pub struct TsdocInlineTags;

impl Check for TsdocInlineTags {
    type Options = NoOptions;
    const ABOUT: &'static str =
        "Each TSDoc tag has a valid name with a space after it, and each inline tag is closed with no unescaped { inside.";
    const TSDOC: bool = true;

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        subject.tsdoc(Group::InlineTags, out);
    }
}
