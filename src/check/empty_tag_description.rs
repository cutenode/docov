//! `empty-tag-description`: each `@param`, `@returns`, `@throws` and `@yields` tag (or `@arg`,
//! `@argument`, `@return`, `@exception` or `@yield`) says something past its type and name. A
//! `@returns` of a function that returns nothing (by its return type or the tag's: `void`,
//! `undefined` or `Promise<void>`) may say nothing.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    collect::doc,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<EmptyTagDescription>()
}

/// The check.
pub struct EmptyTagDescription;

impl Check for EmptyTagDescription {
    type Options = NoOptions;
    const ABOUT: &'static str = "Each @param, @returns, @throws and @yields tag has a description.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        for tag in subject.doc.tags() {
            let kind = tag.kind.parsed();
            let (name, text) = match kind {
                "param" | "arg" | "argument" => match doc::name(tag) {
                    Some(name) => (Some(name.parsed()), tag.type_name_comment().2),
                    None => (None, tag.type_comment().1),
                },
                "returns" | "return" | "throws" | "exception" | "yields" | "yield" => {
                    let (type_part, text) = tag.type_comment();
                    let nothing = type_part.is_some_and(|t| ["void", "undefined", "Promise<void>"].contains(&t.parsed()));
                    if matches!(kind, "returns" | "return") && (subject.shape.void || nothing) {
                        continue;
                    }
                    (None, text)
                }
                _ => continue,
            };
            if text.parsed().is_empty() {
                let name = name.map(|name| format!(" `{name}`")).unwrap_or_default();
                out.push(Finding { at: tag.kind.span.start, message: format!("@{kind}{name} has no description") });
            }
        }
    }
}
