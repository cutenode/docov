//! `tsdoc-params`: each `@param` and `@typeParam` names a parameter with a valid identifier and
//! puts ` - ` before its description (`tsdoc-param-tag-missing-hyphen`), with no JSDoc-style
//! `{type}` or `[optional]` name, as `@microsoft/tsdoc` reads them.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    tsdoc::Group,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocParams>()
}

/// The check.
pub struct TsdocParams;

impl Check for TsdocParams {
    type Options = NoOptions;
    const ABOUT: &'static str =
        "Each TSDoc @param and @typeParam has a valid name and a hyphen before its description, without a {type}.";
    const TSDOC: bool = true;

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        subject.tsdoc(Group::Params, out);
    }
}
