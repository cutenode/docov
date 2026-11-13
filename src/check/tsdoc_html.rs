//! `tsdoc-html`: HTML tags are well formed, as `@microsoft/tsdoc` reads them, and when the file's
//! tsdoc.json has `supportedHtmlElements` (or `reportUnsupportedHtmlElements`), name only the
//! elements it lists. Like TSDoc, it holds attribute names to that list too.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    tsdoc::Group,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocHtml>()
}

/// The check.
pub struct TsdocHtml;

impl Check for TsdocHtml {
    type Options = NoOptions;
    const ABOUT: &'static str =
        "Each HTML tag in a TSDoc block is well formed, and an element the file's tsdoc.json supports.";
    const TSDOC: bool = true;

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        subject.tsdoc(Group::Html, out);
    }
}
