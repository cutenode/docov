//! `docov-ignore` comments: `// docov-ignore-next-line` takes the items on the next line out of
//! the total (and the file's own item, before the file's code), and `/* docov-ignore-file */` the
//! file's, unless they're documented.

use oxc_ast::ast::Program;
use oxc_span::GetSpan;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::{Kind, Lines};

/// A `docov-ignore` comment that covers an item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The first `docov-ignore-next-line` before the file's code, which also covers the file's own
    /// item: that's at line 1, where no comment above can be.
    module: Option<Ignore>,
    /// The last `docov-ignore-file` outside every statement.
    file: Option<Ignore>,
}

impl Ignores {
    /// The ones in `program`.
    pub(super) fn find(program: &Program, lines: &Lines) -> Self {
        let mut ignores = Self { next_line: FxHashMap::default(), module: None, file: None };
        let code = program.directives.first().map(|d| d.span.start);
        let code = code.or_else(|| program.body.first().map(|s| s.span().start)).unwrap_or(u32::MAX);
        for comment in &program.comments {
            let text = comment.content_span().source_text(program.source_text);
            let Some((directive, reason)) = directive(text) else { continue };
            let (line, column) = lines.position(comment.span.start as usize);
            let ignore = Ignore { reason, line, column, file: directive == Directive::File };
            let nested =
                |s: &oxc_ast::ast::Statement| s.span().start < comment.span.start && comment.span.end <= s.span().end;
            if directive == Directive::NextLine {
                if comment.span.end <= code && ignores.module.is_none() {
                    ignores.module = Some(ignore.clone());
                }
                ignores.next_line.insert(lines.position(comment.span.end as usize).0 + 1, ignore);
            } else if !program.body.iter().any(nested) {
                ignores.file = Some(ignore);
            }
        }
        ignores
    }

    /// The one that covers an item of `kind` on `line`, if one does.
    pub(super) fn covering(&self, kind: Kind, line: usize) -> Option<Ignore> {
        let next_line = if kind == Kind::Module { self.module.as_ref() } else { self.next_line.get(&line) };
        next_line.or(self.file.as_ref()).cloned()
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
