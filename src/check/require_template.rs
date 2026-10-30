//! `require-template`: a declaration with type parameters has a `@template` for each, as in
//! `@template T`, `@template T, U` or `@template {string} K`.

use crate::check::{Check, Entry, Finding, NoOptions, Subject};

pub(crate) fn entry() -> Entry {
    Entry::of::<RequireTemplate>()
}

/// The check.
pub struct RequireTemplate;

impl Check for RequireTemplate {
    type Options = NoOptions;
    const ABOUT: &'static str = "Each type parameter of a declaration has a @template.";

    fn check(_: &NoOptions, subject: &Subject, out: &mut Vec<Finding>) {
        let templates = subject.doc.tags().iter().filter(|tag| tag.kind.parsed() == "template");
        let named: Vec<String> = templates.flat_map(|tag| names(&tag.type_comment().1.parsed())).collect();
        for param in subject.shape.type_params.iter().filter(|param| !named.iter().any(|name| name == *param)) {
            subject.missing(format!("@template for `{param}`"), out);
        }
    }
}

/// The names at the start of a `@template` tag's text, past its type: `T`, `T, U` or `[T=string]`,
/// before its description.
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
