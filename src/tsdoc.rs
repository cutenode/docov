//! TSDoc: the `tsdoc-*` checks read each block with tsdocrs, a port of `@microsoft/tsdoc`'s
//! parser, so their findings carry its message ids and texts and line up with
//! `eslint-plugin-tsdoc`'s. This module sorts its messages by the part of the grammar each check
//! covers, and adds the two docov checks TSDoc has no message for: a tag repeated without
//! `allowMultiple`, and text after a modifier tag on its line.

pub mod config;
pub mod jsdoc_types;

use std::sync::LazyLock;

use rustc_hash::FxHashSet;
pub use tsdocrs::TSDocConfiguration as Configuration;
use tsdocrs::{ParserContext, TSDocParser, TSDocTagSyntaxKind, parser::TokenKind};

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

/// Something wrong in a comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// TSDoc's message id, as `eslint-plugin-tsdoc` reports it, or docov's for what TSDoc doesn't
    /// check (`docov-repeated-tag`, `docov-modifier-tag-text`).
    pub id: &'static str,
    /// What's wrong, in TSDoc's words.
    pub text: String,
    /// Where, as a byte offset into the comment.
    pub at: u32,
}

/// What a comment's parse found.
#[derive(Debug, Default)]
pub struct Parsed {
    /// What's wrong, in the order the parser found it, then docov's own findings.
    pub messages: Vec<Message>,
}

/// The default configuration, shared.
pub static DEFAULT: LazyLock<Configuration> = LazyLock::new(Configuration::new);

/// Parses `comment`, a whole `/** */` comment, with the tags and HTML elements `config` defines.
/// Where TSDoc's parser throws (a selector cut off by the end of a tag, `{@link (Foo:}`), this
/// keeps the messages it logged before that.
pub fn parse(comment: &str, config: &Configuration) -> Parsed {
    let message = |m: &tsdocrs::ParserMessage| Message {
        id: m.message_id.as_str(),
        text: m.unformatted_text.clone(),
        at: m.text_range.pos as u32,
    };
    match TSDocParser::new(config).parse_string(comment) {
        Ok(context) => {
            let mut messages: Vec<Message> = context.log.iter().map(message).collect();
            repeated_tags(&context, config, &mut messages);
            modifier_tag_text(&context, config, &mut messages);
            Parsed { messages }
        }
        Err(error) => Parsed { messages: error.log.iter().map(message).collect() },
    }
}

/// Reports each use of a tag after its first, in any case, unless its definition allows more:
/// `docov-repeated-tag`. TSDoc keeps the last block of a kind and the first of a modifier, and
/// says nothing. A second `{@inheritDoc}` has a message of its own.
fn repeated_tags(context: &ParserContext, config: &Configuration, out: &mut Vec<Message>) {
    let mut used = FxHashSet::default();
    for tag in &context.tags {
        let Some(definition) = config.try_get_tag_definition(&tag.tag_name) else { continue };
        let inline = definition.syntax_kind == TSDocTagSyntaxKind::InlineTag;
        let counted = inline == tag.inline && definition.tag_name_with_upper_case != "@INHERITDOC";
        if !counted || definition.allow_multiple || used.insert(&definition.tag_name_with_upper_case) {
            continue;
        }
        out.push(Message {
            id: "docov-repeated-tag",
            text: format!("The TSDoc tag \"{}\" may only appear once in a doc comment", tag.tag_name),
            at: context.tokens[tag.token_sequence.start_index].range().pos as u32,
        });
    }
}

/// Reports a modifier tag with text after it on its line, which TSDoc puts in the section before
/// the tag: `docov-modifier-tag-text`. Another tag after it is fine.
fn modifier_tag_text(context: &ParserContext, config: &Configuration, out: &mut Vec<Message>) {
    let block_tag_at =
        |index: usize| context.tags.iter().any(|tag| !tag.inline && tag.token_sequence.start_index == index);
    for tag in context.tags.iter().filter(|tag| !tag.inline) {
        let modifier = config.try_get_tag_definition(&tag.tag_name).map(|definition| definition.syntax_kind);
        if modifier != Some(TSDocTagSyntaxKind::ModifierTag) {
            continue;
        }
        let after =
            context.tokens[tag.token_sequence.end_index..].iter().position(|token| token.kind != TokenKind::Spacing);
        let next = after.map(|after| tag.token_sequence.end_index + after);
        let ends_line = next.is_none_or(|next| {
            matches!(context.tokens[next].kind, TokenKind::Newline | TokenKind::EndOfInput) || block_tag_at(next)
        });
        if !ends_line {
            out.push(Message {
                id: "docov-modifier-tag-text",
                text: format!(
                    "The TSDoc tag \"{}\" is a modifier tag; text after it on its line belongs to the section before it",
                    tag.tag_name
                ),
                at: context.tokens[tag.token_sequence.start_index].range().pos as u32,
            });
        }
    }
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
