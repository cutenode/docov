//! HTML tags: `<a href="…">`, `<br/>` and `</a>`.

use super::{Failure, Parser, Reader};
use crate::tsdoc::tokens::Kind;

impl Parser<'_, '_> {
    /// An HTML start tag, or a self-closing one.
    pub(super) fn html_start_tag(&mut self, reader: &mut Reader) {
        let marker = reader.marker();
        reader.read();
        reader.extract();
        if let Err(failure) = self.html_name(reader) {
            return self.error_for_failure(reader, marker, "Invalid HTML element: ", failure);
        }
        self.spacing(reader);
        while reader.peek() == Kind::AsciiWord {
            if let Err(failure) = self.html_attribute(reader) {
                return self.error_for_failure(reader, marker, "The HTML element has an invalid attribute: ", failure);
            }
        }
        if reader.peek() == Kind::Slash {
            reader.read();
        }
        if reader.peek() != Kind::GreaterThan {
            let text = "Expecting an attribute or \">\" or \"/>\"";
            let failure = Failure::new("tsdoc-html-tag-missing-greater-than", text.into());
            return self.error_for_failure(reader, marker, "The HTML tag has invalid syntax: ", failure);
        }
        reader.read();
        reader.extract();
    }

    /// An attribute, `name="value"`.
    fn html_attribute(&mut self, reader: &mut Reader) -> Result<(), Failure> {
        self.html_name(reader)?;
        self.spacing(reader);
        if reader.peek() != Kind::Equals {
            let text = "Expecting \"=\" after HTML attribute name";
            return Err(Failure::new("tsdoc-html-tag-missing-equals", text.into()));
        }
        reader.read();
        reader.extract();
        self.spacing(reader);
        self.html_string(reader)?;
        reader.extract();
        self.spacing(reader);
        Ok(())
    }

    /// An attribute's value, in single or double quotes.
    fn html_string(&mut self, reader: &mut Reader) -> Result<(), Failure> {
        let quote = reader.peek();
        if !matches!(quote, Kind::DoubleQuote | Kind::SingleQuote) {
            let text = "Expecting an HTML string starting with a single-quote or double-quote character";
            return Err(Failure::new("tsdoc-html-tag-missing-string", text.into()));
        }
        reader.read();
        while reader.peek() != quote {
            if matches!(reader.peek(), Kind::EndOfInput | Kind::Newline) {
                let text = "The HTML string is missing its closing quote";
                return Err(Failure::new("tsdoc-html-string-missing-quote", text.into()));
            }
            reader.read();
        }
        reader.read();
        if reader.peek() == Kind::AsciiWord {
            let text = "The next character after a closing quote must be spacing or punctuation";
            return Err(Failure::new("tsdoc-text-after-html-string", text.into()));
        }
        Ok(())
    }

    /// An HTML end tag, `</name>`.
    pub(super) fn html_end_tag(&mut self, reader: &mut Reader) {
        let marker = reader.marker();
        reader.read();
        reader.read();
        reader.extract();
        if let Err(failure) = self.html_name(reader) {
            return self.error_for_failure(reader, marker, "Expecting an HTML element name: ", failure);
        }
        self.spacing(reader);
        if reader.peek() != Kind::GreaterThan {
            let text = "Expecting a closing \">\" for the HTML tag";
            let failure = Failure::new("tsdoc-html-tag-missing-greater-than", text.into());
            return self.error_for_failure(reader, marker, "", failure);
        }
        reader.read();
        reader.extract();
    }

    /// An element's or attribute's name, which has to be one the configuration allows when it
    /// says which it does.
    fn html_name(&mut self, reader: &mut Reader) -> Result<(), Failure> {
        if reader.peek() == Kind::Spacing {
            return Err(Failure::new("tsdoc-malformed-html-name", "A space is not allowed here".into()));
        }
        while matches!(reader.peek(), Kind::Hyphen | Kind::Period | Kind::AsciiWord) {
            reader.read();
        }
        let Some(name) = reader.try_extract() else {
            return Err(Failure::new("tsdoc-malformed-html-name", "Expecting an HTML name".into()));
        };
        let name = self.text(name);
        let mut chars = name.chars();
        let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !valid {
            let text = "An HTML name must be an ASCII letter followed by zero or more letters, digits, or hyphens";
            return Err(Failure::new("tsdoc-malformed-html-name", text.into()));
        }
        if self.config.reports_html(name) {
            let quoted = serde_json::to_string(name).expect("a string serializes");
            let text = format!("The HTML element name {quoted} is not defined by your TSDoc configuration");
            return Err(Failure::new("tsdoc-unsupported-html-name", text));
        }
        Ok(())
    }
}
