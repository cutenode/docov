//! `require-param`: each of a function's or method's parameters has a matching `@param`, by name,
//! or for a destructured one, by place, as `paramsDocumented` counts them.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    collect::doc,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<RequireParam>()
}

/// The check.
pub struct RequireParam;

impl Check for RequireParam {
    type Options = NoOptions;
    const ABOUT: &'static str = "Each parameter of a function or method has a matching @param.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        let Some(params) = subject.params else { return };
        for (i, name) in doc::undocumented(Some(subject.doc), params) {
            let what = name.map_or_else(|| format!("parameter {}", i + 1), |name| format!("`{name}`"));
            subject.missing(format!("@param for {what}"), out);
        }
    }
}
