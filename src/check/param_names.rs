//! `param-names`: each `@param` names one of its function's parameters, so none is left over from a
//! rename. A destructured parameter takes the name of the `@param` at its place, and a dotted name
//! (`@param options.a`) belongs to its root.

use rustc_hash::FxHashSet;

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    collect::doc,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<ParamNames>()
}

/// The check.
pub struct ParamNames;

impl Check for ParamNames {
    type Options = NoOptions;
    const ABOUT: &'static str = "Each @param names a parameter of its function or method.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        let Some(params) = subject.params else { return };
        let tags: Vec<_> = doc::param_names(subject.doc).collect();
        let tops = tags.iter().map(|(name, _)| *name).filter(|name| !name.contains('.'));
        let destructured = tops.enumerate().filter(|(i, _)| params.get(*i) == Some(&None)).map(|(_, name)| name);
        let roots: FxHashSet<&str> = params.iter().flatten().copied().chain(destructured).collect();
        for (name, at) in tags {
            let root = name.split('.').next().unwrap_or_default();
            if !roots.contains(root) {
                out.push(Finding { at, message: format!("@param `{name}` doesn't match a parameter") });
            }
        }
    }
}
