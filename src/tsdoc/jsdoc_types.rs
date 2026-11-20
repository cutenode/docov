//! What `tsdocJsdocTypes` hides from TSDoc's parser: the `{type}` JSDoc writes after a tag, and
//! the brackets, default and dots of a JSDoc name, which TSDoc's grammar has no place for. Each is
//! blanked byte for byte, so the offsets TSDoc reports still point into the block as written.

use oxc_jsdoc::JSDoc;

/// The tags, without their `@`, that JSDoc, Closure or TypeScript's JSDoc read a `{type}` after,
/// with their synonyms: eslint-plugin-jsdoc's tags with a type position, and TypeScript's
/// `@satisfies`, `@template` and `@extends`.
const TYPED: [&str; 37] = [
    "arg",
    "argument",
    "augments",
    "class",
    "const",
    "constant",
    "constructor",
    "define",
    "enum",
    "exception",
    "export",
    "extends",
    "implements",
    "member",
    "modifies",
    "module",
    "namespace",
    "next",
    "package",
    "param",
    "private",
    "prop",
    "property",
    "protected",
    "public",
    "return",
    "returns",
    "satisfies",
    "static",
    "template",
    "this",
    "throws",
    "type",
    "typedef",
    "var",
    "yield",
    "yields",
];

/// The tags whose name may be JSDoc's `[name]`, `[name=default]` or `name.sub`.
const NAMED: [&str; 6] = ["arg", "argument", "param", "prop", "property", "template"];

/// `comment`, the block `doc` was parsed from, which starts at byte `at` of its file, with each
/// JSDoc type blanked out and each JSDoc name left as a plain identifier in its place: `[name]`
/// and `[name=default]` as `name`, and `name.sub` as `name_sub`.
pub fn blank(comment: &str, at: u32, doc: &JSDoc) -> String {
    let mut text = comment.as_bytes().to_vec();
    for tag in doc.tags() {
        let kind = tag.kind.parsed();
        if !TYPED.contains(&kind) {
            continue;
        }
        let (ty, name, _) = tag.type_name_comment();
        // An inline tag where a type would go is TSDoc's.
        if let Some(ty) = ty.filter(|ty| !ty.raw().starts_with("{@")) {
            spaces(&mut text[(ty.span.start - at) as usize..(ty.span.end - at) as usize]);
        }
        let Some(name) = name.filter(|_| NAMED.contains(&kind)) else { continue };
        let written = &mut text[(name.span.start - at) as usize..(name.span.end - at) as usize];
        spaces(written);
        let plain = name.parsed();
        let from = name.raw().find(plain).expect("a name is part of what's written");
        written[from..from + plain.len()].copy_from_slice(plain.replace('.', "_").as_bytes());
    }
    String::from_utf8(text).expect("blanking swaps whole characters for ASCII")
}

/// Swaps every byte of `text` but its line breaks for a space.
fn spaces(text: &mut [u8]) {
    text.iter_mut().filter(|byte| !matches!(byte, b'\n' | b'\r')).for_each(|byte| *byte = b' ');
}
