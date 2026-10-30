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

/// Whether `doc` has the tag `name` (without its `@`).
pub(super) fn has_tag(doc: &JSDoc, name: &str) -> bool {
    doc.tags().iter().any(|tag| tag.kind.parsed() == name)
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
