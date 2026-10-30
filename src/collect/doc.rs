//! What an item's JSDoc block says, read with oxc_jsdoc.

use oxc_jsdoc::JSDoc;

use super::Kind;
use crate::feature::access::Access;

/// Whether `doc` describes its item: it has text before its first tag, or a tag that stands in for
/// that (`@description`, `@desc`, `@summary`, a class's `@classdesc`, each with text, or
/// `@inheritdoc` in any case).
pub(super) fn describes(doc: &JSDoc, kind: Kind) -> bool {
    !doc.comment().parsed().is_empty()
        || doc.tags().iter().any(|tag| {
            let name = tag.kind.parsed();
            let describing =
                matches!(name, "description" | "desc" | "summary") || (name == "classdesc" && kind == Kind::Class);
            name.eq_ignore_ascii_case("inheritdoc") || (describing && !tag.comment().parsed().is_empty())
        })
}

/// The tags that make a block about its file.
const FILE_TAGS: [&str; 5] = ["file", "fileoverview", "overview", "module", "packageDocumentation"];

/// Whether `doc` is about its file, and if it is, whether it says something: a description, or
/// text on one of its file tags.
pub(super) fn file(doc: &JSDoc) -> Option<bool> {
    let tags: Vec<_> = doc.tags().iter().filter(|tag| FILE_TAGS.contains(&tag.kind.parsed())).collect();
    let text = !doc.comment().parsed().is_empty() || tags.iter().any(|tag| !tag.comment().parsed().is_empty());
    (!tags.is_empty()).then_some(text)
}

/// The type `doc` declares with its first `@typedef` or `@callback`, if that names one, and
/// whether it's described: a description, or text after the name.
pub(super) fn declared_type(doc: &JSDoc) -> Option<(String, bool)> {
    let tag = doc.tags().iter().find(|tag| matches!(tag.kind.parsed(), "typedef" | "callback"))?;
    let (_, name, text) = tag.type_name_comment();
    Some((name?.parsed().to_owned(), !doc.comment().parsed().is_empty() || !text.parsed().is_empty()))
}

/// Whether `doc` has a `@property` (or `@prop`) named `name`.
pub(super) fn lists(doc: &JSDoc, name: &str) -> bool {
    let properties = doc.tags().iter().filter(|tag| matches!(tag.kind.parsed(), "property" | "prop"));
    properties.filter_map(|tag| tag.type_name_comment().1).any(|named| named.parsed() == name)
}

/// How many of the release tags `@public`, `@beta`, `@alpha` and `@internal` `doc` has.
pub(super) fn release_tags(doc: &JSDoc) -> usize {
    ["public", "beta", "alpha", "internal"].iter().filter(|tag| has_tag(doc, tag)).count()
}

/// Whether each of `doc`'s `@param` tags (or `@arg`, or `@argument`) has a `{type}`, and so do
/// its `@returns` (or `@return`).
pub(super) fn typed(doc: &JSDoc) -> bool {
    let typing = doc
        .tags()
        .iter()
        .filter(|tag| matches!(tag.kind.parsed(), "param" | "arg" | "argument" | "returns" | "return"));
    typing.into_iter().all(|tag| tag.r#type().is_some())
}

/// Whether `doc` has the tag `name` (without its `@`).
pub(super) fn has_tag(doc: &JSDoc, name: &str) -> bool {
    doc.tags().iter().any(|tag| tag.kind.parsed() == name)
}

/// The text of `doc`'s first `name` tag (without its `@`), if it has one with text.
pub(super) fn text(doc: &JSDoc, name: &str) -> Option<String> {
    let tag = doc.tags().iter().find(|tag| tag.kind.parsed() == name)?;
    Some(tag.comment().parsed()).filter(|text| !text.is_empty())
}

/// Whether each of `params` (a parameter's name, or `None` for a destructured one) has a matching
/// `@param` in `doc`: one with its name, or for a destructured one, one at its place. A dotted name
/// (`@param options.a`) documents a part of a parameter, not one of its own.
pub(super) fn documents(doc: Option<&JSDoc>, params: &[Option<&str>]) -> bool {
    let tags = doc.map_or(&[][..], |doc| doc.tags().as_slice());
    let names: Vec<&str> = tags
        .iter()
        .filter(|tag| matches!(tag.kind.parsed(), "param" | "arg" | "argument"))
        .filter_map(|tag| tag.type_name_comment().1.map(|name| name.parsed()))
        .filter(|name| !name.contains('.'))
        .collect();
    params.iter().enumerate().all(|(i, param)| param.map_or(i < names.len(), |param| names.contains(&param)))
}

/// The access level `doc`'s first access tag gives: `@public`, `@protected`, `@package`,
/// `@private`, or `@access` with one of those.
pub(super) fn access(doc: &JSDoc) -> Option<Access> {
    doc.tags().iter().find_map(|tag| match tag.kind.parsed() {
        "access" => level(tag.comment().parsed().split_whitespace().next().unwrap_or_default()),
        name => level(name),
    })
}

fn level(name: &str) -> Option<Access> {
    match name {
        "public" => Some(Access::Public),
        "protected" => Some(Access::Protected),
        "package" => Some(Access::Package),
        "private" => Some(Access::Private),
        _ => None,
    }
}
