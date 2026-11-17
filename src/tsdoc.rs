//! TSDoc: the tags a tsdoc.json defines, and a parser of TSDoc's grammar that finds what
//! `@microsoft/tsdoc` does, by its message ids, so docov's findings line up with
//! `eslint-plugin-tsdoc`'s. It's a port of that package's parser, which would otherwise take Node
//! to run; nothing on crates.io parses TSDoc's grammar or reads tsdoc.json.

pub mod config;
mod parse;
#[cfg(test)]
mod tests;
mod tokens;

use std::sync::LazyLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use self::parse::{Message, Parsed, parse};

/// The parts of TSDoc's grammar, each of which a doc check reports the messages of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Tags: undefined, unsupported, of the wrong kind or repeated, and what a tag needs.
    Tags,
    /// `@param` and `@typeParam` blocks.
    Params,
    /// How a tag is written.
    InlineTags,
    /// `{`, `}`, `>`, `@` and backslashes in text.
    Escapes,
    /// Code spans and fences.
    Code,
    /// HTML elements.
    Html,
    /// `{@link}` and the declaration references in it.
    Links,
}

impl Message {
    /// The part of the grammar it's about.
    pub fn group(&self) -> Group {
        let id = self.id;
        match id {
            "tsdoc-undefined-tag"
            | "tsdoc-unsupported-tag"
            | "tsdoc-tag-should-not-have-braces"
            | "tsdoc-inline-tag-missing-braces"
            | "tsdoc-missing-deprecation-message"
            | "tsdoc-extra-inheritdoc-tag"
            | "tsdoc-inheritdoc-incompatible-tag"
            | "tsdoc-inheritdoc-incompatible-summary"
            | "docov-repeated-tag"
            | "docov-modifier-tag-text" => Group::Tags,
            "tsdoc-escape-right-brace"
            | "tsdoc-escape-greater-than"
            | "tsdoc-unnecessary-backslash"
            | "tsdoc-at-sign-in-word"
            | "tsdoc-at-sign-without-tag-name"
            | "tsdoc-malformed-inline-tag" => Group::Escapes,
            "tsdoc-malformed-tag-name"
            | "tsdoc-characters-after-block-tag"
            | "tsdoc-characters-after-inline-tag"
            | "tsdoc-inline-tag-missing-right-brace"
            | "tsdoc-inline-tag-unescaped-brace"
            | "tsdoc-inheritdoc-tag-syntax" => Group::InlineTags,
            "tsdoc-text-after-html-string" | "tsdoc-malformed-html-name" | "tsdoc-unsupported-html-name" => Group::Html,
            _ if id.starts_with("tsdoc-param-") => Group::Params,
            _ if id.starts_with("tsdoc-code-") => Group::Code,
            _ if id.starts_with("tsdoc-html-") => Group::Html,
            _ => Group::Links,
        }
    }
}

/// How a tag is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SyntaxKind {
    /// In braces, inside other text: `{@link}`.
    Inline,
    /// Starting a section of its own: `@remarks`.
    Block,
    /// By itself, marking the item: `@internal`.
    Modifier,
}

/// A tag TSDoc knows.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct TagDefinition {
    /// Its name, with its `@`.
    pub name: String,
    /// How it's written.
    pub kind: SyntaxKind,
    /// Whether a comment may have it more than once.
    pub allow_multiple: bool,
}

/// TSDoc's standard tags, by name, kind and whether a comment may have more than one.
const STANDARD: [(&str, SyntaxKind, bool); 29] = {
    use SyntaxKind::{Block, Inline, Modifier};
    [
        ("@alpha", Modifier, false),
        ("@beta", Modifier, false),
        ("@decorator", Block, true),
        ("@defaultValue", Block, false),
        ("@deprecated", Block, false),
        ("@eventProperty", Modifier, false),
        ("@example", Block, true),
        ("@experimental", Modifier, false),
        ("@inheritDoc", Inline, false),
        ("@internal", Modifier, false),
        ("@label", Inline, false),
        ("@link", Inline, true),
        ("@override", Modifier, false),
        ("@packageDocumentation", Modifier, false),
        ("@param", Block, true),
        ("@privateRemarks", Block, false),
        ("@public", Modifier, false),
        ("@readonly", Modifier, false),
        ("@remarks", Block, false),
        ("@returns", Block, false),
        ("@sealed", Modifier, false),
        ("@see", Block, false),
        ("@throws", Block, true),
        ("@typeParam", Block, true),
        ("@virtual", Modifier, false),
        ("@jsx", Block, false),
        ("@jsxRuntime", Block, false),
        ("@jsxFrag", Block, false),
        ("@jsxImportSource", Block, false),
    ]
};

/// What TSDoc's parser is set up with: the tags it knows, which of them the tool supports, and
/// which HTML elements it allows. The default is `@microsoft/tsdoc`'s: every standard tag, and no
/// check of support or HTML elements.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Configuration {
    /// The tags it knows.
    pub tags: Vec<TagDefinition>,
    /// The names of the tags it supports, upper-cased, when it reports the others.
    pub supported: Option<Vec<String>>,
    /// The HTML element names it allows.
    pub html: Vec<String>,
    /// Whether it reports the HTML elements `html` doesn't name.
    pub report_unsupported_html: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Self::standard(true)
    }
}

/// The default configuration, shared.
pub static DEFAULT: LazyLock<Configuration> = LazyLock::new(Configuration::default);

impl Configuration {
    /// A configuration with TSDoc's standard tags if `standard`, or none.
    pub fn standard(standard: bool) -> Self {
        let tags = STANDARD.iter().filter(|_| standard);
        let tags = tags.map(|&(name, kind, allow_multiple)| TagDefinition { name: name.into(), kind, allow_multiple });
        Self { tags: tags.collect(), supported: None, html: Vec::new(), report_unsupported_html: false }
    }

    /// The tag named `name` (with its `@`), in any case.
    pub fn tag(&self, name: &str) -> Option<&TagDefinition> {
        self.tags.iter().find(|tag| tag.name.eq_ignore_ascii_case(name))
    }

    /// Whether a tag it knows is unsupported, when it says which are.
    fn unsupported(&self, tag: &TagDefinition) -> bool {
        let upper = tag.name.to_ascii_uppercase();
        self.supported.as_ref().is_some_and(|supported| !supported.contains(&upper))
    }

    /// Whether it reports the HTML element `name`.
    fn reports_html(&self, name: &str) -> bool {
        self.report_unsupported_html && !self.html.iter().any(|allowed| allowed == name)
    }
}
