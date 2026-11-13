//! `tsdoc-code`: code spans and code fences are closed and well formed, as `@microsoft/tsdoc`
//! reads them: a code span has something between its backticks and ends on its line, and a code
//! fence's backticks start their lines, unindented, with no backtick in its language and nothing
//! after the closing ones.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    tsdoc::Group,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocCode>()
}

/// The check.
pub struct TsdocCode;

impl Check for TsdocCode {
    type Options = NoOptions;
    const ABOUT: &'static str = "Each TSDoc code span and code fence is closed and well formed.";
    const TSDOC: bool = true;

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        subject.tsdoc(Group::Code, out);
    }
}
