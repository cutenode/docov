//! `docov-ignore` comments: `// docov-ignore-next-line` takes the items on the next line out of
//! the total (and the file's own item, before the file's code), and `/* docov-ignore-file */` the
//! file's, unless they're documented.

use oxc_ast::ast::Program;
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};
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

impl Ignore {
    /// Which comment it is: `docov-ignore-file` or `docov-ignore-next-line`.
    pub(super) fn directive(&self) -> &'static str {
        if self.file { "docov-ignore-file" } else { "docov-ignore-next-line" }
    }
}

/// A file's `docov-ignore` comments.
#[derive(Default)]
pub(super) struct Ignores {
    /// Each one, in the file's order, with what it reaches.
    comments: Vec<(Ignore, Reach)>,
    /// What they cover, all together.
    cover: Cover,
}

/// What a `docov-ignore` comment can cover.
enum Reach {
    /// A `docov-ignore-next-line`: the items on this line, and the file's own item too when it's
    /// before the file's code and the first such comment there.
    Line { line: usize, head: bool },
    /// A `docov-ignore-file` outside every statement: the file's items.
    File,
    /// A `docov-ignore-file` inside a statement: nothing.
    Nothing,
}

/// Which comment covers what, by each one's index.
#[derive(Default)]
struct Cover {
    /// The last `docov-ignore-next-line` before each line.
    next_line: FxHashMap<usize, usize>,
    /// The first `docov-ignore-next-line` before the file's code, which also covers the file's own
    /// item: that's at line 1, where no comment above can be.
    module: Option<usize>,
    /// The last `docov-ignore-file` outside every statement.
    file: Option<usize>,
}

impl Cover {
    /// What the comments `reaches` holds, by index, cover.
    fn of<'a>(reaches: impl Iterator<Item = (usize, &'a Reach)>) -> Self {
        let mut cover = Self::default();
        for (i, reach) in reaches {
            match *reach {
                Reach::Line { line, head } => {
                    if head {
                        cover.module.get_or_insert(i);
                    }
                    cover.next_line.insert(line, i);
                }
                Reach::File => cover.file = Some(i),
                Reach::Nothing => {}
            }
        }
        cover
    }

    /// The index of the comment that covers an item of `kind` on `line`, if one does.
    fn covering(&self, kind: Kind, line: usize) -> Option<usize> {
        let next_line = if kind == Kind::Module { self.module } else { self.next_line.get(&line).copied() };
        next_line.or(self.file)
    }
}

impl Ignores {
    /// The ones in `program`, whose code starts at `code`.
    pub(super) fn find(program: &Program, lines: &Lines, code: u32) -> Self {
        let mut comments = Vec::new();
        for comment in &program.comments {
            let text = comment.content_span().source_text(program.source_text);
            let Some((directive, reason)) = directive(text) else { continue };
            let (line, column) = lines.position(comment.span.start as usize);
            let ignore = Ignore { reason, line, column, file: directive == Directive::File };
            let nested =
                |s: &oxc_ast::ast::Statement| s.span().start < comment.span.start && comment.span.end <= s.span().end;
            let reach = if directive == Directive::NextLine {
                Reach::Line { line: lines.position(comment.span.end as usize).0 + 1, head: comment.span.end <= code }
            } else if program.body.iter().any(nested) {
                Reach::Nothing
            } else {
                Reach::File
            };
            comments.push((ignore, reach));
        }
        let cover = Cover::of(comments.iter().map(|(_, reach)| reach).enumerate());
        Self { comments, cover }
    }

    /// The one that covers an item of `kind` on `line`, if one does.
    pub(super) fn covering(&self, kind: Kind, line: usize) -> Option<Ignore> {
        self.cover.covering(kind, line).map(|i| self.comments[i].0.clone())
    }

    /// The ones that change nothing, in a file whose undocumented items that count are `items`, by
    /// kind and line: removing all of them leaves each item ignored or not as it was, and removing
    /// any one left then would change one. They go one at a time, each unless the others left
    /// still cover every ignored item: `docov-ignore-next-line` comments before
    /// `docov-ignore-file` ones, which cover what they do, and in each, those no item is ignored by
    /// first, which keeps the comments `--explain` names where it can.
    pub(super) fn unused(&self, items: impl Iterator<Item = (Kind, usize)>) -> Vec<Ignore> {
        let ignored: Vec<_> =
            items.filter_map(|(kind, line)| Some((kind, line, self.cover.covering(kind, line)?))).collect();
        let named: FxHashSet<usize> = ignored.iter().map(|&(.., i)| i).collect();
        let mut order: Vec<usize> = (0..self.comments.len()).collect();
        order.sort_by_key(|&i| (self.comments[i].0.file, named.contains(&i)));
        let mut kept = vec![true; self.comments.len()];
        for i in order {
            kept[i] = false;
            let left = self.comments.iter().enumerate().filter(|&(j, _)| kept[j]);
            let cover = Cover::of(left.map(|(j, (_, reach))| (j, reach)));
            kept[i] = !ignored.iter().all(|&(kind, line, _)| cover.covering(kind, line).is_some());
        }
        self.comments.iter().zip(kept).filter(|(_, kept)| !kept).map(|((ignore, _), _)| ignore.clone()).collect()
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
