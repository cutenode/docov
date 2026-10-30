//! `docov-ignore` comments: `// docov-ignore-next-line` takes the items on the next line out of
//! the total, and `/* docov-ignore-file */` the file's, unless they're documented.

use oxc_ast::ast::Program;
use oxc_span::GetSpan;
use rustc_hash::FxHashMap;
use serde::Serialize;

use super::Lines;

/// A `docov-ignore` comment that covers an item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ignore {
    /// What follows the `:`, if anything does.
    pub reason: Option<String>,
    /// Where the comment starts. 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
    /// Whether it's a `docov-ignore-file`, rather than a `docov-ignore-next-line`.
    pub file: bool,
}

/// A file's `docov-ignore` comments.
pub(super) struct Ignores {
    /// Each `docov-ignore-next-line`, by the line it covers.
    next_line: FxHashMap<usize, Ignore>,
    /// The last `docov-ignore-file` outside every statement.
    file: Option<Ignore>,
}

impl Ignores {
    /// The ones in `program`.
    pub(super) fn find(program: &Program, lines: &Lines) -> Self {
        let mut ignores = Self { next_line: FxHashMap::default(), file: None };
        for comment in &program.comments {
            let text = comment.content_span().source_text(program.source_text);
            let Some((directive, reason)) = directive(text) else { continue };
            let (line, column) = lines.position(comment.span.start as usize);
            let ignore = Ignore { reason, line, column, file: directive == Directive::File };
            let nested =
                |s: &oxc_ast::ast::Statement| s.span().start < comment.span.start && comment.span.end <= s.span().end;
            if directive == Directive::NextLine {
                ignores.next_line.insert(lines.position(comment.span.end as usize).0 + 1, ignore);
            } else if !program.body.iter().any(nested) {
                ignores.file = Some(ignore);
            }
        }
        ignores
    }

    /// The one that covers the items on `line`, if one does.
    pub(super) fn covering(&self, line: usize) -> Option<Ignore> {
        self.next_line.get(&line).or(self.file.as_ref()).cloned()
    }
}

/// What a comment asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Directive {
    /// `docov-ignore-next-line`.
    NextLine,
    /// `docov-ignore-file`.
    File,
}

/// The directive a comment's text (without its `//` or `/* */`) is, and its reason: the text after a
/// `:`, if there's any.
fn directive(text: &str) -> Option<(Directive, Option<String>)> {
    let text = text.trim();
    let (directive, rest) = match text.strip_prefix("docov-ignore-next-line") {
        Some(rest) => (Directive::NextLine, rest),
        None => (Directive::File, text.strip_prefix("docov-ignore-file")?),
    };
    if rest.is_empty() {
        return Some((directive, None));
    }
    let reason = rest.trim_start().strip_prefix(':')?.trim();
    Some((directive, Some(reason.to_owned()).filter(|r| !r.is_empty())))
}
