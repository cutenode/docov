//! `tsdoc-tags`: each tag is one the file's tsdoc.json defines (`tsdoc-undefined-tag`) and
//! supports, when its `supportForTags` says which it does (`tsdoc-unsupported-tag`), and is used as
//! the kind it's defined as: an inline tag in braces, a block or modifier tag not
//! (`tsdoc-tag-should-not-have-braces`, `tsdoc-inline-tag-missing-braces`). A tag without
//! `allowMultiple` is used once (`docov-repeated-tag`), and a modifier tag has nothing after it on
//! its line (`docov-modifier-tag-text`), which TSDoc would put in the section before it; TSDoc
//! itself checks neither, so their ids are docov's. It also has TSDoc's checks of particular
//! tags: `@deprecated` needs a message, and `{@inheritDoc}` comes once, with no summary or
//! `@remarks`.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    tsdoc::Group,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocTags>()
}

/// The check.
pub struct TsdocTags;

impl Check for TsdocTags {
    type Options = NoOptions;
    const ABOUT: &'static str = "Each TSDoc tag is defined and supported by the file's tsdoc.json, used as its kind, and repeated only with allowMultiple.";
    const TSDOC: bool = true;

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        subject.tsdoc(Group::Tags, out);
    }
}
