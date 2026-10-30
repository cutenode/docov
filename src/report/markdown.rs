//! `markdown`: a summary for `$GITHUB_STEP_SUMMARY` or a pull request comment.

use std::path::Path;

use rustc_hash::FxHashMap;

use crate::Totals;
use crate::report::{Context, Format, escape, slash};

/// The format.
pub const FORMAT: Format =
    Format { name: "markdown", help: "A summary for $GITHUB_STEP_SUMMARY or a pull request comment", render };

/// The total and any failed gates, a table of the files, and each file's undocumented items,
/// folded away. With `--compare`, the total and each file's row show the change from the earlier
/// report too, and files it had that are gone get a row of their own.
fn render(context: &Context) -> String {
    let totals = Totals::of(context.files);
    let (percent, documented, total) = (totals.percent(), totals.documented, totals.total);
    let mut out = format!("## docov\n\n**{percent:.1}%** documented ({documented}/{total})");
    if let Some(base) = context.base {
        out += &format!(", {} from {:.1}%", change(base.totals, totals), base.totals.percent());
    }
    out += "\n";
    for failure in context.failures {
        out += &format!("\n**Failed:** {}\n", plain(failure));
    }
    let files: Vec<_> = context.files.iter().filter(|f| f.counted().next().is_some()).collect();
    // A file without counted items has no row, so one the base had without them doesn't either.
    let base = context.base.iter().flat_map(|base| &base.files).filter(|f| f.totals.total > 0);
    let mut before: FxHashMap<&str, Totals> = base.map(|f| (&*f.path, f.totals)).collect();
    let compared = context.base.is_some();
    if !files.is_empty() || !before.is_empty() {
        out += if compared { COMPARED } else { HEADER };
    }
    for file in &files {
        // The base has paths as `json` writes them.
        let old = before.remove(&*file.path.display().to_string());
        out += &row(Some(file.totals()), old, compared, &file.path);
    }
    let mut gone: Vec<_> = before.into_iter().collect();
    gone.sort_unstable_by_key(|&(path, _)| path);
    for (path, old) in gone {
        out += &row(None, Some(old), compared, Path::new(path));
    }
    for file in files.iter().filter(|f| f.counted().any(|i| !i.documented)) {
        let totals = file.totals();
        // Markdown isn't read in an HTML block, which runs to the blank line.
        let path = escape(&slash(&file.path));
        out += &format!("\n<details>\n<summary><code>{path}</code>: {} undocumented</summary>\n\n", totals.total - totals.documented);
        for item in file.counted().filter(|i| !i.documented) {
            out += &format!("- {}:{} {} {}\n", item.line, item.column, item.kind.as_str(), code(&item.name));
        }
        out += "\n</details>\n";
    }
    out
}

/// The table's header.
const HEADER: &str = "\n| Documented | Items | File |\n| ---: | ---: | :--- |\n";

/// The table's header with `--compare`.
const COMPARED: &str =
    "\n| Documented | Before | Change | Items | File |\n| ---: | ---: | ---: | ---: | :--- |\n";

/// A table row: a file's totals `now`, or none when it's gone, and `before`, from the base when
/// `compared`, or none when the file is new.
fn row(now: Option<Totals>, before: Option<Totals>, compared: bool, path: &Path) -> String {
    let percent = |totals: Totals| format!("{:.1}%", totals.percent());
    let mut cells = vec![now.map_or_else(String::new, percent)];
    if compared {
        cells.push(before.map_or_else(String::new, percent));
        cells.push(match (now, before) {
            (Some(now), Some(before)) => change(before, now),
            (Some(_), None) => "new".into(),
            (None, _) => "gone".into(),
        });
    }
    cells.push(now.map_or_else(String::new, |totals| format!("{}/{}", totals.documented, totals.total)));
    // GitHub splits table cells at `|`, even in code.
    cells.push(code(&slash(path)).replace('|', "\\|"));
    format!("| {} |\n", cells.join(" | "))
}

/// The change in percentage points from `before` to `now`, as `+6.7`. It's the difference of the
/// percentages as shown, so it's what a reader gets subtracting them.
fn change(before: Totals, now: Totals) -> String {
    let shown = |totals: Totals| format!("{:.1}", totals.percent()).parse::<f64>().expect("a formatted number parses");
    format!("{:+.1}", shown(now) - shown(before))
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
