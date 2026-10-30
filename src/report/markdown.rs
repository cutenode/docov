//! `markdown`: a summary for `$GITHUB_STEP_SUMMARY` or a pull request comment.

use crate::Totals;
use crate::report::{Context, Format, escape, slash};

/// The format.
pub const FORMAT: Format =
    Format { name: "markdown", help: "A summary for $GITHUB_STEP_SUMMARY or a pull request comment", render };

/// The total and any failed gates, a table of the files, and each file's undocumented items,
/// folded away.
fn render(context: &Context) -> String {
    let totals = Totals::of(context.files);
    let (percent, documented, total) = (totals.percent(), totals.documented, totals.total);
    let mut out = format!("## docov\n\n**{percent:.1}%** documented ({documented}/{total})\n");
    for failure in context.failures {
        out += &format!("\n**Failed:** {}\n", plain(failure));
    }
    let files: Vec<_> = context.files.iter().filter(|f| !f.items.is_empty()).collect();
    if !files.is_empty() {
        out += "\n| Documented | Items | File |\n| ---: | ---: | :--- |\n";
    }
    for file in &files {
        let totals = file.totals();
        // GitHub splits table cells at `|`, even in code.
        let path = code(&slash(&file.path)).replace('|', "\\|");
        out += &format!("| {:.1}% | {}/{} | {path} |\n", totals.percent(), totals.documented, totals.total);
    }
    for file in files.iter().filter(|f| f.items.iter().any(|i| !i.documented)) {
        let totals = file.totals();
        // Markdown isn't read in an HTML block, which runs to the blank line.
        let path = escape(&slash(&file.path));
        out += &format!("\n<details>\n<summary><code>{path}</code>: {} undocumented</summary>\n\n", totals.total - totals.documented);
        for item in file.items.iter().filter(|i| !i.documented) {
            out += &format!("- {}:{} {} {}\n", item.line, item.column, item.kind.as_str(), code(&item.name));
        }
        out += "\n</details>\n";
    }
    out
}

/// `text` as a code span: fenced by more backticks than it has in a row, and padded where it
/// starts or ends with a backtick or space, which the padding keeps. A code span reads line breaks
/// as spaces anyway, and a table row can't have them.
fn code(text: &str) -> String {
    let text = text.replace(['\r', '\n'], " ");
    let fence = "`".repeat(text.split(|c| c != '`').map(str::len).max().unwrap_or(0) + 1);
    let edge = [' ', '`'];
    let pad = if text.starts_with(edge) || text.ends_with(edge) { " " } else { "" };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// `text` with what Markdown would read as formatting escaped, on one line.
fn plain(text: &str) -> String {
    text.chars().fold(String::new(), |mut out, c| {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~' | '&' | '#') {
            out.push('\\');
        }
        out.push(if matches!(c, '\r' | '\n') { ' ' } else { c });
        out
    })
}
