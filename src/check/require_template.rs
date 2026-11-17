//! `require-template`: a declaration with type parameters has a `@template`, or TSDoc's
//! `@typeParam`, for each, as in `@template T`, `@template T, U`, `@template {string} K` or
//! `@typeParam T - The key.` A finding names the tag the config's `syntax` writes.

use crate::{
    check::{Check, Entry, Finding, NoOptions, Subject},
    feature::syntax::Syntax,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<RequireTemplate>()
}

/// The check.
pub struct RequireTemplate;

impl Check for RequireTemplate {
    type Options = NoOptions;
    const ABOUT: &'static str = "Each type parameter of a declaration has a @template or @typeParam.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        let templates = subject.doc.tags().iter().filter(|tag| matches!(tag.kind.parsed(), "template" | "typeParam"));
        let named: Vec<String> = templates.flat_map(|tag| names(&tag.type_comment().1.parsed())).collect();
        let tag = if subject.syntax == Syntax::Tsdoc { "@typeParam" } else { "@template" };
        for param in subject.shape.type_params.iter().filter(|param| !named.iter().any(|name| name == *param)) {
            subject.missing(format!("{tag} for `{param}`"), out);
        }
    }
}

/// The names at the start of a `@template` or `@typeParam` tag's text, past its type: `T`, `T, U`
/// or `[T=string]`, before its description (`- The key.` in TSDoc).
fn names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for token in text.split_whitespace() {
        let parts = token.split(',').filter(|part| !part.is_empty());
        names.extend(parts.map(|part| part.trim_matches(['[', ']']).split('=').next().unwrap_or_default().to_owned()));
        if !token.ends_with(',') {
            break;
        }
    }
    names
}
