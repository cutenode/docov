//! Code spans (`` `code` ``) and code fences.

use super::{Failure, Parser, Reader};
use crate::tsdoc::tokens::Kind;

impl Parser<'_, '_> {
    /// A code fence: a line of three backticks and a language, code, and a line of three
    /// backticks.
    pub(super) fn fenced_code(&mut self, reader: &mut Reader) {
        let start = reader.marker();
        let opening = start + 2;
        if !matches!(reader.previous(), Kind::Newline | Kind::EndOfInput) {
            let text = "The opening backtick for a code fence must appear at the start of the line";
            return self.error_range(reader, start, opening, "tsdoc-code-fence-opening-indent", text.into());
        }
        for _ in 0..3 {
            reader.read();
        }
        reader.extract();
        // The language, to the end of the line. Every line ends with a newline before the end of
        // the input, here and after the closing backticks.
        loop {
            match reader.peek() {
                Kind::Newline => {
                    reader.read();
                    break;
                }
                Kind::Backtick => {
                    let text = "The language specifier cannot contain backtick characters";
                    let failure = Failure::new("tsdoc-code-fence-specifier-syntax", text.into());
                    self.error_range_for_failure(reader, start, opening, "Error parsing code fence: ", failure);
                    return;
                }
                _ => _ = reader.read(),
            }
        }
        reader.extract();
        let code = reader.marker();
        let (before, code_end) = loop {
            match reader.peek() {
                Kind::EndOfInput => {
                    let failure =
                        Failure::new("tsdoc-code-fence-missing-delimiter", "Missing closing delimiter".into());
                    self.error_range_for_failure(reader, start, opening, "Error parsing code fence: ", failure);
                    return;
                }
                Kind::Newline => {
                    let mut before = reader.read();
                    let code_end = reader.marker();
                    while reader.peek() == Kind::Spacing {
                        before = reader.read();
                    }
                    let mut backticks = 0;
                    while backticks < 3 && reader.peek() == Kind::Backtick {
                        reader.read();
                        backticks += 1;
                    }
                    if backticks == 3 {
                        break (before, code_end);
                    }
                }
                _ => _ = reader.read(),
            }
        };
        if before.kind != Kind::Newline {
            let text = "The closing delimiter for a code fence must not be indented";
            self.log("tsdoc-code-fence-closing-indent", text.into(), before.start);
        }
        reader.extract();
        self.sections[self.section] |=
            self.tokens[code..code_end].iter().any(|token| !matches!(token.kind, Kind::Spacing | Kind::Newline));
        // The rest of the closing line.
        loop {
            match reader.peek() {
                Kind::Spacing => _ = reader.read(),
                Kind::Newline => {
                    reader.read();
                    break;
                }
                _ => {
                    let text = "Unexpected characters after closing delimiter for code fence";
                    self.log("tsdoc-code-fence-closing-syntax", text.into(), self.at(reader.marker()));
                    break;
                }
            }
        }
        reader.try_extract();
    }

    /// A code span.
    pub(super) fn code_span(&mut self, reader: &mut Reader) {
        let marker = reader.marker();
        reader.read();
        reader.extract();
        loop {
            match reader.peek() {
                Kind::Backtick if reader.is_empty() => {
                    let text = "A code span must contain at least one character between the backticks";
                    return self.error_range(reader, marker, marker + 1, "tsdoc-code-span-empty", text.into());
                }
                Kind::Backtick => {
                    let code = reader.extract();
                    self.sections[self.section] |= self.tokens[code].iter().any(|token| token.kind != Kind::Spacing);
                    reader.read();
                    reader.extract();
                    return;
                }
                Kind::EndOfInput | Kind::Newline => {
                    let text = "The code span is missing its closing backtick";
                    return self.backtrack_error(reader, marker, "tsdoc-code-span-missing-delimiter", text.into());
                }
                _ => _ = reader.read(),
            }
        }
    }
}
