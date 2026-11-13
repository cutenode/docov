//! `tsdoc-links`: each `{@link}` has a URL with a scheme (`https://…`) or a declaration reference
//! in TSDoc's grammar (`pkg#Class.(member:static)`), then nothing but its text after a `|`, and
//! each `{@inheritDoc}` reference follows that grammar too, as `@microsoft/tsdoc` reads them.
//! `valid-link` checks JSDoc's links, and looks up their names.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    tsdoc::Group,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocLinks>()
}

/// The check.
pub struct TsdocLinks;

impl Check for TsdocLinks {
    type Options = NoOptions;
    const ABOUT: &'static str =
        "Each TSDoc {@link} has a URL or a declaration reference in TSDoc's grammar, and text only after a |.";
    const TSDOC: bool = true;

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        subject.tsdoc(Group::Links, out);
    }
}
