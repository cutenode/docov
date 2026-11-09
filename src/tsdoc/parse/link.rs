//! `{@link}` and `{@inheritDoc}`, and the declaration references they name:
//! `@scope/pkg/path#Class.(member:static)`.

use super::{Inline, Parser, Reader, Sequence, invalid_identifier};
use crate::tsdoc::tokens::Kind;

/// TSDoc's system selectors, which an unquoted identifier can't be.
const SYSTEM_SELECTORS: [&str; 10] =
    ["instance", "static", "constructor", "class", "enum", "function", "interface", "namespace", "type", "variable"];

impl Parser<'_, '_> {
    /// The content of `{@inheritDoc}`, whose name starts at token `name`: nothing, or a
    /// declaration reference.
    pub(super) fn inherit_doc_tag(&mut self, content: Option<Sequence>, name: usize) -> Inline {
        let Some(content) = content else { return Inline::InheritDoc };
        let mut reader = Reader::new(self.tokens, content);
        if self.declaration_reference(&mut reader, name).is_none() {
            return Inline::Error;
        }
        if reader.peek() != Kind::EndOfInput {
            let token = reader.read();
            let text = "Unexpected character after declaration reference";
            self.log("tsdoc-inheritdoc-tag-syntax", text.into(), token.start);
            return Inline::Error;
        }
        Inline::InheritDoc
    }

    /// The content of `{@link}`, whose name starts at token `name`: a URL or a declaration
    /// reference, and text after a `|`.
    pub(super) fn link_tag(&mut self, content: Option<Sequence>, name: usize) -> Inline {
        let Some(content) = content else {
            self.log("tsdoc-link-tag-empty", "The @link tag content is missing".into(), self.at(name));
            return Inline::Error;
        };
        let mut reader = Reader::new(self.tokens, content);
        let slashes = |reader: &Reader| reader.peek() == Kind::Slash && reader.peek_after() == Kind::Slash;
        let mut url = slashes(&reader);
        let marker = reader.marker();
        while !url {
            match reader.peek() {
                Kind::AsciiWord | Kind::Period | Kind::Hyphen | Kind::Plus => _ = reader.read(),
                Kind::Colon => {
                    reader.read();
                    url = slashes(&reader);
                    break;
                }
                _ => break,
            }
        }
        reader.backtrack(marker);
        let destination = if url { self.url(&mut reader) } else { self.declaration_reference(&mut reader, name) };
        if destination.is_none() {
            return Inline::Error;
        }
        match reader.peek() {
            Kind::Pipe => {
                reader.read();
                reader.extract();
                self.spacing(&mut reader);
                let mut text = false;
                loop {
                    match reader.peek() {
                        Kind::EndOfInput => return Inline::Link(text),
                        Kind::Pipe | Kind::LeftCurly => {
                            let token = reader.read();
                            let c = &self.comment[token.start as usize..token.end as usize];
                            let text =
                                format!("The \"{c}\" character may not be used in the link text without escaping it");
                            // At the link's text, as far as it goes.
                            let at = self.at(reader.extract().start);
                            self.log("tsdoc-link-tag-unescaped-text", text, at);
                            return Inline::Error;
                        }
                        Kind::Spacing | Kind::Newline => _ = reader.read(),
                        _ => {
                            text = true;
                            reader.read();
                        }
                    }
                }
            }
            Kind::EndOfInput => Inline::Link(false),
            _ => {
                let token = reader.read();
                let text = "Unexpected character after link destination";
                self.log("tsdoc-link-tag-destination-syntax", text.into(), token.start);
                Inline::Error
            }
        }
    }

    /// A URL, which has to start with a scheme and `://`, and the spacing after it.
    fn url(&mut self, reader: &mut Reader) -> Option<()> {
        while !matches!(reader.peek(), Kind::Spacing | Kind::Newline | Kind::EndOfInput | Kind::Pipe | Kind::RightCurly)
        {
            reader.read();
        }
        let sequence = reader.extract();
        let at = self.at(sequence.start);
        let url = self.text(sequence);
        let scheme = url.split_once("://").map(|(scheme, _)| scheme).filter(|scheme| {
            let mut chars = scheme.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric())
        });
        let why = match scheme {
            None => {
                "An @link URL must begin with a scheme comprised only of letters and numbers followed by \"://\". (For general URLs, use an HTML \"<a>\" tag instead.)"
            }
            Some(scheme) if url.len() == scheme.len() + 3 => {
                "An @link URL must have at least one character after \"://\""
            }
            Some(_) => {
                self.spacing(reader);
                return Some(());
            }
        };
        self.log("tsdoc-link-tag-invalid-url", why.into(), at);
        None
    }

    /// A declaration reference: a package name and import path, then `#`, then member references,
    /// either part optional. A mistake in it is reported at the token `context` when it has no
    /// place of its own.
    fn declaration_reference(&mut self, reader: &mut Reader, context: usize) -> Option<()> {
        let marker = reader.marker();
        let (mut hash, mut import_characters, mut looking) = (false, false, true);
        loop {
            match reader.peek() {
                Kind::Pound => {
                    hash = true;
                    break;
                }
                Kind::Slash | Kind::AtSign => {
                    import_characters |= looking;
                    reader.read();
                }
                Kind::AsciiWord | Kind::Period | Kind::Hyphen => _ = reader.read(),
                Kind::DoubleQuote
                | Kind::EndOfInput
                | Kind::LeftCurly
                | Kind::LeftParen
                | Kind::LeftSquare
                | Kind::Newline
                | Kind::Pipe
                | Kind::RightCurly
                | Kind::RightParen
                | Kind::RightSquare
                | Kind::SingleQuote
                | Kind::Spacing => break,
                _ => {
                    looking = false;
                    reader.read();
                }
            }
        }
        if !hash && import_characters {
            let start = reader.extract().start;
            let text = "The declaration reference appears to contain a package name or import path, but it is missing the \"#\" delimiter";
            self.log("tsdoc-reference-missing-hash", text.into(), self.at(start));
            return None;
        }
        reader.backtrack(marker);
        let (mut package, mut import) = (false, false);
        if hash {
            if reader.peek() != Kind::Period {
                let scoped = reader.peek() == Kind::AtSign;
                let mut finished_scope = false;
                loop {
                    match reader.peek() {
                        Kind::Slash if scoped && !finished_scope => {
                            reader.read();
                            finished_scope = true;
                        }
                        Kind::Slash | Kind::Pound => break,
                        _ => _ = reader.read(),
                    }
                }
                if let Some(sequence) = reader.try_extract() {
                    package = true;
                    let name = self.text(sequence.clone());
                    if !valid_package_name(name) {
                        let quoted = serde_json::to_string(name).expect("a string serializes");
                        let text = format!("The package name {quoted} is not a valid package name");
                        self.log("tsdoc-reference-malformed-package-name", text, self.at(sequence.start));
                        return None;
                    }
                }
            }
            while reader.peek() != Kind::Pound {
                reader.read();
            }
            if let Some(sequence) = reader.try_extract() {
                import = true;
                let path = self.text(sequence.clone());
                let why = if path.contains("//") {
                    Some("An import path must not contain \"//\"")
                } else if path.ends_with('/') {
                    Some("An import path must not end with \"/\"")
                } else if !package && path.starts_with('/') {
                    Some("An import path must not start with \"/\" unless prefixed by a package name")
                } else {
                    None
                };
                if let Some(why) = why {
                    self.log("tsdoc-reference-malformed-import-path", why.into(), self.at(sequence.start));
                    return None;
                }
            }
            reader.read();
            let hash = reader.extract().start;
            self.spacing(reader);
            if !package && !import {
                let text = "The hash character must be preceded by a package name or import path";
                self.log("tsdoc-reference-hash-syntax", text.into(), self.at(hash));
                return None;
            }
        }
        let mut members = 0;
        while matches!(
            reader.peek(),
            Kind::Period | Kind::LeftParen | Kind::AsciiWord | Kind::Colon | Kind::LeftSquare | Kind::DoubleQuote
        ) {
            self.member_reference(reader, members > 0, context)?;
            members += 1;
        }
        if !package && !import && members == 0 {
            self.log("tsdoc-missing-reference", "Expecting a declaration reference".into(), self.at(context));
            return None;
        }
        Some(())
    }

    /// One member reference: `.name`, `.(name:selector)` or `[symbol]`.
    fn member_reference(&mut self, reader: &mut Reader, dot: bool, context: usize) -> Option<()> {
        if dot {
            if reader.peek() != Kind::Period {
                let text = "Expecting a period before the next component of a declaration reference";
                self.log("tsdoc-reference-missing-dot", text.into(), self.at(context));
                return None;
            }
            reader.read();
            reader.extract();
            self.spacing(reader);
        }
        let paren = (reader.peek() == Kind::LeftParen).then(|| {
            reader.read();
            reader.extract().start
        });
        if paren.is_some() {
            self.spacing(reader);
        }
        if reader.peek() == Kind::LeftSquare {
            self.member_symbol(reader)?;
        } else {
            self.member_identifier(reader, context)?;
        }
        self.spacing(reader);
        if reader.peek() == Kind::Colon {
            reader.read();
            let colon = reader.extract().start;
            self.spacing(reader);
            if paren.is_none() {
                let text = "Syntax error in declaration reference: the member selector must be enclosed in parentheses";
                self.log("tsdoc-reference-selector-missing-parens", text.into(), self.at(colon));
                return None;
            }
            self.member_selector(reader, colon)?;
            self.spacing(reader);
        } else if let Some(paren) = paren {
            let text = "Expecting a colon after the identifier because the expression is in parentheses";
            self.log("tsdoc-reference-missing-colon", text.into(), self.at(paren));
            return None;
        }
        if let Some(paren) = paren {
            if reader.peek() != Kind::RightParen {
                self.log(
                    "tsdoc-reference-missing-right-paren",
                    "Expecting a matching right parenthesis".into(),
                    self.at(paren),
                );
                return None;
            }
            reader.read();
            reader.extract();
            self.spacing(reader);
        }
        Some(())
    }

    /// A symbol, `[declaration reference]`.
    fn member_symbol(&mut self, reader: &mut Reader) -> Option<()> {
        reader.read();
        let bracket = reader.extract().start;
        self.spacing(reader);
        if self.declaration_reference(reader, bracket).is_none() {
            let text = "Missing declaration reference in symbol reference";
            self.log("tsdoc-reference-symbol-syntax", text.into(), self.at(bracket));
            return None;
        }
        if reader.peek() != Kind::RightSquare {
            let text = "Missing closing square bracket for symbol reference";
            self.log("tsdoc-reference-missing-right-bracket", text.into(), self.at(bracket));
            return None;
        }
        reader.read();
        reader.extract();
        Some(())
    }

    /// A member's name, plain or in double quotes.
    fn member_identifier(&mut self, reader: &mut Reader, context: usize) -> Option<()> {
        if reader.peek() == Kind::DoubleQuote {
            reader.read();
            let quote = reader.extract().start;
            while reader.peek() != Kind::DoubleQuote {
                if reader.peek() == Kind::EndOfInput {
                    let text = "Unexpected end of input inside quoted member identifier";
                    self.log("tsdoc-reference-missing-quote", text.into(), self.at(quote));
                    return None;
                }
                reader.read();
            }
            if reader.is_empty() {
                self.log(
                    "tsdoc-reference-empty-identifier",
                    "The quoted identifier cannot be empty".into(),
                    self.at(quote),
                );
                return None;
            }
            reader.extract();
            reader.read();
            reader.extract();
            return Some(());
        }
        while matches!(reader.peek(), Kind::AsciiWord | Kind::Dollar) {
            reader.read();
        }
        let Some(sequence) = reader.try_extract() else {
            let text = "Syntax error in declaration reference: expecting a member identifier";
            self.log("tsdoc-reference-missing-identifier", text.into(), self.at(context));
            return None;
        };
        let identifier = self.text(sequence.clone());
        let why = match invalid_identifier(identifier) {
            Some(why) => why.to_owned(),
            None if SYSTEM_SELECTORS.contains(&identifier) => {
                format!("The identifier \"{identifier}\" must be quoted because it is a TSDoc system selector name")
            }
            None => return Some(()),
        };
        self.log("tsdoc-reference-unquoted-identifier", why, self.at(sequence.start));
        None
    }

    /// A selector after a member's `:`, at token `colon`: a system selector, a label or an index.
    fn member_selector(&mut self, reader: &mut Reader, colon: usize) -> Option<()> {
        if reader.peek() != Kind::AsciiWord {
            self.log(
                "tsdoc-reference-missing-label",
                "Expecting a selector label after the colon".into(),
                self.at(colon),
            );
            // `@microsoft/tsdoc` reads the next token as the selector whatever it is, and throws at
            // the end of the tag's content; this stops there.
            if reader.peek() == Kind::EndOfInput {
                return None;
            }
        }
        reader.read();
        let sequence = reader.extract();
        let selector = self.text(sequence.clone());
        let Some(why) = selector_error(selector) else { return Some(()) };
        self.log("tsdoc-reference-selector-syntax", why, self.at(sequence.start));
        None
    }
}

/// Whether `name` is a valid npm package name, scoped or not.
fn valid_package_name(name: &str) -> bool {
    let part = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    match name.strip_prefix('@').map(|scoped| scoped.split_once('/')) {
        Some(Some((scope, name))) => part(scope) && part(name),
        Some(None) => false,
        None => part(name),
    }
}

/// What's wrong with `selector`, if something is: a selector is a system selector (`static`), a
/// label in upper case (`LABEL`), or an index (`2`).
fn selector_error(selector: &str) -> Option<String> {
    // A selector is a token, so it has a character.
    let first = selector.chars().next().unwrap_or_default();
    let rest = &selector[first.len_utf8()..];
    if first.is_ascii_digit() {
        let index = first != '0' && rest.chars().all(|c| c.is_ascii_digit());
        return (!index).then(|| "If the selector begins with a number, it must be a positive integer value".into());
    }
    if first.is_ascii_uppercase() || first == '_' {
        let label = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        let text = "A label selector must be comprised of upper case letters, numbers, and underscores and must not start with a number";
        return (!label).then(|| text.into());
    }
    if SYSTEM_SELECTORS.contains(&selector) {
        return None;
    }
    if selector.chars().all(|c| c.is_ascii_lowercase()) {
        let quoted = serde_json::to_string(selector).expect("a string serializes");
        return Some(format!("The selector {quoted} is not a recognized TSDoc system selector name"));
    }
    Some("Invalid syntax for selector".into())
}
