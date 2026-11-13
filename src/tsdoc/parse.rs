//! TSDoc's grammar: a port of `@microsoft/tsdoc`'s NodeParser that keeps what it reports, by
//! message id and where, and leaves out the tree it builds. Each construct is read the way that
//! parser reads it, so a mistake is reported once, where `@microsoft/tsdoc` reports it, and the
//! text after it reads the same.

mod code;
mod html;
mod link;

use std::ops::Range;

use super::{
    Configuration, SyntaxKind, TagDefinition,
    tokens::{Kind, Token, tokens},
};

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
    /// What's wrong, in the order the parser found it.
    pub messages: Vec<Message>,
}

/// Parses `comment`, a whole `/** */` comment, with the tags and HTML elements `config` defines.
pub fn parse(comment: &str, config: &Configuration) -> Parsed {
    let tokens = tokens(comment);
    let mut parser = Parser {
        comment,
        tokens: &tokens,
        config,
        messages: Vec::new(),
        sections: vec![false],
        section: 0,
        deprecated: None,
        remarks: None,
        inherit_doc: false,
        modifier: None,
        used: Vec::new(),
    };
    parser.run();
    Parsed { messages: parser.messages }
}

/// A token sequence, by token indices.
type Sequence = Range<usize>;

/// Why a construct couldn't be read, before it's reported: TSDoc's message id and text.
struct Failure {
    id: &'static str,
    text: String,
}

impl Failure {
    fn new(id: &'static str, text: String) -> Self {
        Self { id, text }
    }
}

/// A reader over the tokens, or the tokens of an inline tag's content: the token it's at, and the
/// first of those it read since it last took them as a sequence.
struct Reader<'t> {
    tokens: &'t [Token],
    end: usize,
    current: usize,
    accumulated: usize,
}

impl<'t> Reader<'t> {
    fn new(tokens: &'t [Token], range: Sequence) -> Self {
        Self { tokens, end: range.end, current: range.start, accumulated: range.start }
    }

    fn kind(&self, at: usize) -> Kind {
        if at < self.end { self.tokens[at].kind } else { Kind::EndOfInput }
    }

    fn peek(&self) -> Kind {
        self.kind(self.current)
    }

    fn peek_after(&self) -> Kind {
        self.kind(self.current + 1)
    }

    fn peek_after_after(&self) -> Kind {
        self.kind(self.current + 2)
    }

    /// The kind of the token before this one, in the whole comment.
    fn previous(&self) -> Kind {
        self.current.checked_sub(1).map_or(Kind::EndOfInput, |at| self.tokens[at].kind)
    }

    fn read(&mut self) -> Token {
        self.current += 1;
        self.tokens[self.current - 1]
    }

    fn marker(&self) -> usize {
        self.current
    }

    fn backtrack(&mut self, marker: usize) {
        self.current = marker;
        self.accumulated = self.accumulated.min(marker);
    }

    fn is_empty(&self) -> bool {
        self.accumulated == self.current
    }

    /// The tokens read since the last sequence taken.
    fn extract(&mut self) -> Sequence {
        let sequence = self.accumulated..self.current;
        self.accumulated = self.current;
        sequence
    }

    fn try_extract(&mut self) -> Option<Sequence> {
        (!self.is_empty()).then(|| self.extract())
    }
}

/// The parse of one comment.
struct Parser<'c, 't> {
    comment: &'c str,
    tokens: &'t [Token],
    config: &'c Configuration,
    messages: Vec<Message>,
    /// Whether each section has text, the summary first.
    sections: Vec<bool>,
    /// The section text goes into now.
    section: usize,
    /// The last `@deprecated` block's section, its tag's name as written, and where it is.
    deprecated: Option<(usize, String, u32)>,
    /// The last `@remarks` block's tag, as written, and where it is.
    remarks: Option<(String, u32)>,
    /// Whether the comment has an `{@inheritDoc}`.
    inherit_doc: bool,
    /// The modifier tag on this line, before any text after it.
    modifier: Option<(String, u32)>,
    /// The tags used so far that a comment may have only one of, upper-cased.
    used: Vec<String>,
}

/// What an inline tag turned out to be.
enum Inline {
    /// A mistake.
    Error,
    /// `{@inheritDoc}`, read without a mistake.
    InheritDoc,
    /// `{@link}`, with whether its text has more than spaces.
    Link(bool),
    /// Another tag.
    Tag,
}

impl<'c> Parser<'c, '_> {
    fn run(&mut self) {
        let mut reader = Reader::new(self.tokens, 0..self.tokens.len());
        loop {
            match reader.peek() {
                Kind::EndOfInput => break,
                Kind::Newline => {
                    self.flush(&mut reader);
                    reader.read();
                    reader.extract();
                    self.modifier = None;
                }
                Kind::Backslash => {
                    self.before_markup(&mut reader);
                    self.backslash(&mut reader);
                }
                Kind::AtSign => {
                    self.flush(&mut reader);
                    self.block(&mut reader);
                }
                Kind::LeftCurly => {
                    self.before_markup(&mut reader);
                    let marker = reader.marker();
                    match self.inline_tag(&mut reader) {
                        Inline::InheritDoc if self.inherit_doc => {
                            let end = reader.marker() - 1;
                            let text = "A doc comment cannot have more than one @inheritDoc tag";
                            self.error_range(&mut reader, marker, end, "tsdoc-extra-inheritdoc-tag", text.into());
                        }
                        Inline::InheritDoc => self.inherit_doc = true,
                        Inline::Link(text) => self.sections[self.section] |= text,
                        Inline::Error | Inline::Tag => {}
                    }
                }
                Kind::RightCurly => {
                    self.before_markup(&mut reader);
                    let text = "The \"}\" character should be escaped using a backslash to avoid confusion with a TSDoc inline tag";
                    self.error(&mut reader, "tsdoc-escape-right-brace", text.into());
                }
                Kind::LessThan => {
                    self.before_markup(&mut reader);
                    if reader.peek_after() == Kind::Slash {
                        self.html_end_tag(&mut reader);
                    } else {
                        self.html_start_tag(&mut reader);
                    }
                }
                Kind::GreaterThan => {
                    self.before_markup(&mut reader);
                    let text =
                        "The \">\" character should be escaped using a backslash to avoid confusion with an HTML tag";
                    self.error(&mut reader, "tsdoc-escape-greater-than", text.into());
                }
                Kind::Backtick => {
                    self.before_markup(&mut reader);
                    if reader.peek_after() == Kind::Backtick && reader.peek_after_after() == Kind::Backtick {
                        self.fenced_code(&mut reader);
                    } else {
                        self.code_span(&mut reader);
                    }
                }
                _ => {
                    reader.read();
                }
            }
        }
        self.flush(&mut reader);
        self.validate();
    }

    /// The checks of the whole comment, once it's read.
    fn validate(&mut self) {
        if let Some((section, name, at)) = self.deprecated.take()
            && !self.sections[section]
        {
            let text = format!(
                "The {name} block must include a deprecation message, e.g. describing the recommended alternative"
            );
            self.log("tsdoc-missing-deprecation-message", text, at);
        }
        if !self.inherit_doc {
            return;
        }
        if let Some((name, at)) = self.remarks.take() {
            let text =
                format!("A \"{name}\" block must not be used, because that content is provided by the @inheritDoc tag");
            self.log("tsdoc-inheritdoc-incompatible-tag", text, at);
        }
        if self.sections[0] {
            let text = "The summary section must not have any content, because that content is provided by the @inheritDoc tag";
            self.log("tsdoc-inheritdoc-incompatible-summary", text.into(), 0);
        }
    }

    /// The text of the tokens in `sequence`.
    fn text(&self, sequence: Sequence) -> &'c str {
        match sequence.len() {
            0 => "",
            _ => &self.comment[self.tokens[sequence.start].start as usize..self.tokens[sequence.end - 1].end as usize],
        }
    }

    /// Where the token at `index` starts.
    fn at(&self, index: usize) -> u32 {
        self.tokens[index].start
    }

    fn log(&mut self, id: &'static str, text: String, at: u32) {
        self.messages.push(Message { id, text, at });
    }

    /// Takes the plain text read so far, into the current section.
    fn flush(&mut self, reader: &mut Reader) {
        let Some(text) = reader.try_extract() else { return };
        if self.tokens[text].iter().any(|token| token.kind != Kind::Spacing) {
            self.sections[self.section] = true;
            self.after_modifier();
        }
    }

    /// Takes the plain text read so far, before markup, which is text after a modifier tag too.
    fn before_markup(&mut self, reader: &mut Reader) {
        self.flush(reader);
        self.after_modifier();
    }

    /// Reports text after a modifier tag on its line, which TSDoc puts in the section before it.
    fn after_modifier(&mut self) {
        if let Some((name, at)) = self.modifier.take() {
            let text = format!(
                "The TSDoc tag \"{name}\" is a modifier tag; text after it on its line belongs to the section before it"
            );
            self.log("docov-modifier-tag-text", text, at);
        }
    }

    /// Reports `definition`'s second use in the comment, if it may have only one.
    fn used(&mut self, definition: &TagDefinition, name: &str, at: u32) {
        if definition.allow_multiple {
            return;
        }
        let upper = definition.name.to_ascii_uppercase();
        if self.used.contains(&upper) {
            self.log(
                "docov-repeated-tag",
                format!("The TSDoc tag \"{name}\" may only appear once in a doc comment"),
                at,
            );
        } else {
            self.used.push(upper);
        }
    }

    /// Reports a tag that isn't defined, is used as the wrong kind, or isn't supported.
    fn validate_tag(&mut self, definition: Option<&TagDefinition>, name: &str, inline: bool, at: u32) {
        let Some(definition) = definition else {
            let text = format!("The TSDoc tag \"{name}\" is not defined in this configuration");
            return self.log("tsdoc-undefined-tag", text, at);
        };
        match (definition.kind == SyntaxKind::Inline, inline) {
            (false, true) => {
                let text = format!(
                    "The TSDoc tag \"{name}\" is not an inline tag; it must not be enclosed in \"{{ }}\" braces"
                );
                self.log("tsdoc-tag-should-not-have-braces", text, at);
            }
            (true, false) => {
                let text =
                    format!("The TSDoc tag \"{name}\" is an inline tag; it must be enclosed in \"{{ }}\" braces");
                self.log("tsdoc-inline-tag-missing-braces", text, at);
            }
            _ if self.config.unsupported(definition) => {
                let text = format!("The TSDoc tag \"{name}\" is not supported by this tool");
                self.log("tsdoc-unsupported-tag", text, at);
            }
            _ => {}
        }
    }

    /// A block or modifier tag, `@name`.
    fn block(&mut self, reader: &mut Reader) {
        let Some((name, sequence)) = self.block_tag(reader) else {
            return self.after_modifier();
        };
        self.modifier = None;
        let at = self.at(sequence.start);
        let definition = self.config.tag(name);
        self.validate_tag(definition, name, false, at);
        let Some(definition) = definition.filter(|definition| definition.kind != SyntaxKind::Inline) else { return };
        self.used(definition, name, at);
        if definition.kind == SyntaxKind::Modifier {
            self.modifier = Some((name.to_owned(), at));
            return;
        }
        let param = ["@param", "@typeParam"].into_iter().find(|param| param.eq_ignore_ascii_case(name));
        if let Some(param) = param {
            self.param_block(reader, param, at);
        }
        self.sections.push(false);
        self.section = self.sections.len() - 1;
        if name.eq_ignore_ascii_case("@deprecated") {
            self.deprecated = Some((self.section, name.to_owned(), at));
        } else if name.eq_ignore_ascii_case("@remarks") {
            self.remarks = Some((name.to_owned(), at));
        }
    }

    /// A block tag's `@name`, or none when it's a mistake, which it reports.
    fn block_tag(&mut self, reader: &mut Reader) -> Option<(&'c str, Sequence)> {
        let marker = reader.marker();
        if !matches!(reader.previous(), Kind::EndOfInput | Kind::Spacing | Kind::Newline) {
            let text = "The \"@\" character looks like part of a TSDoc tag; use a backslash to escape it";
            self.backtrack_error(reader, marker, "tsdoc-at-sign-in-word", text.into());
            return None;
        }
        reader.read();
        if reader.peek() != Kind::AsciiWord {
            let text =
                "Expecting a TSDoc tag name after \"@\"; if it is not a tag, use a backslash to escape this character";
            self.backtrack_error(reader, marker, "tsdoc-at-sign-without-tag-name", text.into());
            return None;
        }
        reader.read();
        let name = self.text(marker..reader.marker());
        if !matches!(reader.peek(), Kind::Spacing | Kind::Newline | Kind::EndOfInput) {
            let bad = self.first_char(reader.marker());
            let text = format!(
                "The token \"{name}\" looks like a TSDoc tag but contains an invalid character {bad}; if it is not a tag, use a backslash to escape the \"@\""
            );
            self.backtrack_error(reader, marker, "tsdoc-characters-after-block-tag", text);
            return None;
        }
        if !valid_tag_name(name) {
            let failure = Failure::new("tsdoc-malformed-tag-name", TAG_NAME.into());
            self.error_for_failure(reader, marker, "", failure);
            return None;
        }
        Some((name, reader.extract()))
    }

    /// The first character of the token at `index`, quoted as JSON.
    fn first_char(&self, index: usize) -> String {
        let c = self.comment[self.at(index) as usize..].chars().next().unwrap_or_default();
        serde_json::to_string(&c).expect("a char serializes")
    }

    /// A `@param` or `@typeParam` block's name and hyphen.
    fn param_block(&mut self, reader: &mut Reader, tag: &str, at: u32) {
        let start = reader.marker();
        self.spacing(reader);
        self.jsdoc_type(reader, tag);
        let bracket = (reader.peek() == Kind::LeftSquare).then(|| {
            reader.read();
            reader.extract().start
        });
        let name_start = reader.marker();
        while matches!(reader.peek(), Kind::AsciiWord | Kind::Period | Kind::Dollar) {
            reader.read();
        }
        let name = self.text(name_start..reader.marker());
        if let Some(why) = invalid_identifier(name) {
            reader.backtrack(start);
            let text = match name {
                "" => format!("The {tag} block should be followed by a parameter name"),
                _ => format!("The {tag} block should be followed by a valid parameter name: {why}"),
            };
            return self.log("tsdoc-param-tag-with-invalid-name", text, at);
        }
        reader.extract();
        if let Some(bracket) = bracket {
            // A newline follows the name at least, so this reads to the `]` or goes back.
            let marker = reader.marker();
            self.value_rest(reader, Kind::LeftSquare, Kind::RightSquare, marker);
            let text = format!(
                "The {tag} should not include a JSDoc-style optional name; it must not be enclosed in '[ ]' brackets."
            );
            self.log("tsdoc-param-tag-with-invalid-optional-name", text, self.at(bracket));
        }
        self.spacing(reader);
        self.jsdoc_type(reader, tag);
        if reader.peek() != Kind::Hyphen {
            let text = format!("The {tag} block should be followed by a parameter name and then a hyphen");
            return self.log("tsdoc-param-tag-missing-hyphen", text, at);
        }
        reader.read();
        reader.extract();
        self.spacing(reader);
        self.jsdoc_type(reader, tag);
    }

    /// Reports a JSDoc-style `{type}` in a `@param` block, which TSDoc leaves to TypeScript.
    fn jsdoc_type(&mut self, reader: &mut Reader, tag: &str) {
        if reader.peek() != Kind::LeftCurly || reader.peek_after() == Kind::AtSign {
            return;
        }
        let start = reader.marker();
        reader.read();
        if let Some(sequence) = self.value_rest(reader, Kind::LeftCurly, Kind::RightCurly, start) {
            let text = format!("The {tag} block should not include a JSDoc-style '{{type}}'");
            self.log("tsdoc-param-tag-with-invalid-type", text, self.at(sequence.start));
            self.spacing(reader);
        }
    }

    /// The rest of a JSDoc type or default value after its opening `open`, up to the `close` that
    /// matches it, past nested pairs and quoted strings, or none when it isn't closed.
    fn value_rest(&mut self, reader: &mut Reader, open: Kind, close: Kind, start: usize) -> Option<Sequence> {
        let (mut quote, mut depth) = (None, 1);
        while depth > 0 {
            let mut kind = reader.peek();
            match kind {
                _ if kind == open => depth += usize::from(quote.is_none()),
                _ if kind == close => depth -= usize::from(quote.is_none()),
                Kind::Backslash if quote.is_some() => {
                    reader.read();
                    kind = reader.peek();
                }
                Kind::DoubleQuote | Kind::SingleQuote | Kind::Backtick => {
                    quote = match quote {
                        None => Some(kind),
                        Some(quoted) if quoted == kind => None,
                        quoted => quoted,
                    };
                }
                _ => {}
            }
            if kind == Kind::EndOfInput {
                reader.backtrack(start);
                return None;
            }
            reader.read();
        }
        reader.try_extract()
    }

    /// A backslash escape.
    fn backslash(&mut self, reader: &mut Reader) {
        let marker = reader.marker();
        reader.read();
        // Every line ends with a newline, so something follows a backslash.
        if !reader.read().kind.punctuation() {
            let text = "A backslash can only be used to escape a punctuation character";
            return self.backtrack_error(reader, marker, "tsdoc-unnecessary-backslash", text.into());
        }
        reader.extract();
        self.sections[self.section] = true;
    }

    /// An inline tag, `{@name ...}`.
    fn inline_tag(&mut self, reader: &mut Reader) -> Inline {
        let marker = reader.marker();
        reader.read();
        reader.extract();
        let at_sign = reader.marker();
        if reader.peek() != Kind::AtSign {
            let text = "Expecting a TSDoc tag starting with \"{@\"";
            self.backtrack_error(reader, marker, "tsdoc-malformed-inline-tag", text.into());
            return Inline::Error;
        }
        reader.read();
        while reader.peek() == Kind::AsciiWord {
            reader.read();
        }
        let name = self.text(at_sign..reader.marker());
        if name == "@" {
            let text = "Expecting a TSDoc inline tag name after the \"{@\" characters";
            let failure = Failure::new("tsdoc-malformed-inline-tag", text.into());
            return self.error_range_for_failure(reader, marker, at_sign, "", failure);
        }
        if !valid_tag_name(name) {
            let failure = Failure::new("tsdoc-malformed-tag-name", TAG_NAME.into());
            return self.error_range_for_failure(reader, marker, at_sign, "", failure);
        }
        let name_sequence = reader.extract();
        if self.spacing(reader).is_none() && reader.peek() != Kind::RightCurly {
            let bad = self.first_char(reader.marker());
            let text = format!("The character {bad} cannot appear after the TSDoc tag name; expecting a space");
            let failure = Failure::new("tsdoc-characters-after-inline-tag", text);
            return self.error_range_for_failure(reader, marker, at_sign, "", failure);
        }
        loop {
            match reader.peek() {
                Kind::EndOfInput => {
                    let text = "The TSDoc inline tag name is missing its closing \"}\"";
                    self.error_range(reader, marker, at_sign, "tsdoc-inline-tag-missing-right-brace", text.into());
                    return Inline::Error;
                }
                Kind::Backslash => {
                    reader.read();
                    if !reader.peek().punctuation() {
                        let text = "A backslash can only be used to escape a punctuation character";
                        let failure = Failure::new("tsdoc-unnecessary-backslash", text.into());
                        let prefix = "Error reading inline TSDoc tag: ";
                        return self.error_range_for_failure(reader, marker, at_sign, prefix, failure);
                    }
                    reader.read();
                }
                Kind::LeftCurly => {
                    let text =
                        "The \"{\" character must be escaped with a backslash when used inside a TSDoc inline tag";
                    let failure = Failure::new("tsdoc-inline-tag-unescaped-brace", text.into());
                    return self.error_range_for_failure(reader, marker, at_sign, "", failure);
                }
                Kind::RightCurly => break,
                _ => {
                    reader.read();
                }
            }
        }
        let content = reader.try_extract();
        reader.read();
        reader.extract();
        let tag = if name.eq_ignore_ascii_case("@inheritDoc") {
            self.inherit_doc_tag(content, name_sequence.start)
        } else if name.eq_ignore_ascii_case("@link") {
            self.link_tag(content, name_sequence.start)
        } else {
            Inline::Tag
        };
        let at = self.at(name_sequence.start);
        let definition = self.config.tag(name);
        self.validate_tag(definition, name, true, at);
        // A second `{@inheritDoc}` has a message of its own.
        let inline = definition.filter(|d| d.kind == SyntaxKind::Inline && !name.eq_ignore_ascii_case("@inheritDoc"));
        if let Some(definition) = inline {
            self.used(definition, name, at);
        }
        tag
    }

    /// Reads spaces and line breaks, if there are any.
    fn spacing(&mut self, reader: &mut Reader) -> Option<Sequence> {
        while matches!(reader.peek(), Kind::Spacing | Kind::Newline) {
            reader.read();
        }
        reader.try_extract()
    }

    /// Reports the token it's at as a mistake, and reads past it.
    fn error(&mut self, reader: &mut Reader, id: &'static str, text: String) {
        self.backtrack_error(reader, reader.marker(), id, text);
    }

    /// Goes back to `marker`, and reports the token there as a mistake.
    fn backtrack_error(&mut self, reader: &mut Reader, marker: usize, id: &'static str, text: String) {
        self.error_range(reader, marker, marker, id, text);
    }

    /// Goes back to `start` and reads past `end`, reporting a mistake.
    fn error_range(&mut self, reader: &mut Reader, start: usize, end: usize, id: &'static str, text: String) {
        self.error_range_for_failure(reader, start, end, "", Failure::new(id, text));
    }

    /// Goes back to `start` and reads past `end`, reporting `failure` there, where
    /// `@microsoft/tsdoc` reports it: at the start of the text it skips.
    fn error_range_for_failure(
        &mut self,
        reader: &mut Reader,
        start: usize,
        end: usize,
        prefix: &str,
        failure: Failure,
    ) -> Inline {
        // Every caller's `end` is a token it has seen, so never the end of the input.
        reader.backtrack(start);
        while reader.marker() <= end {
            reader.read();
        }
        reader.extract();
        self.log(failure.id, format!("{prefix}{}", failure.text), self.at(start));
        Inline::Error
    }

    /// Goes back to `marker` and reads one token, reporting `failure`.
    fn error_for_failure(&mut self, reader: &mut Reader, marker: usize, prefix: &str, failure: Failure) {
        self.error_range_for_failure(reader, marker, marker, prefix, failure);
    }
}

/// What a tag name that isn't valid is told.
const TAG_NAME: &str = "A TSDoc tag name must start with a letter and contain only letters and numbers";

/// Whether `name` is a valid tag name: `@`, a letter, then letters and digits.
pub(super) fn valid_tag_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next() == Some('@')
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric())
}

/// Why `identifier` isn't a valid unquoted identifier, if it isn't.
fn invalid_identifier(identifier: &str) -> Option<&'static str> {
    if identifier.is_empty() {
        Some("The identifier cannot be an empty string")
    } else if identifier.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')) {
        Some("The identifier cannot non-word characters")
    } else if identifier.starts_with(|c: char| c.is_ascii_digit()) {
        Some("The identifier must not start with a number")
    } else {
        None
    }
}
