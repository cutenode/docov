//! Adds lines to source files and leaves every other line as it was, line endings included.

use std::{fs, io, path::Path};

use crate::FileReport;

/// `source` with each insert's text (a line, or several) put above its 1-based line, indented like
/// that line and ended like it. `inserts` go in line order, and a line's own in the order given.
/// Those for lines past the last go at the end, unindented, after a line ending if the last line
/// has none. Those for line 1 go below a shebang, which has to stay the first line.
pub fn insert_above(source: &str, inserts: &[(usize, &str)]) -> String {
    let ending_of = |line: &str| line.strip_suffix('\n').map(|l| if l.ends_with('\r') { "\r\n" } else { "\n" });
    // A last line with no line ending takes the file's first one.
    let file_ending = source.split_inclusive('\n').next().and_then(ending_of).unwrap_or("\n");
    let first = 1 + usize::from(source.starts_with("#!"));
    let mut inserts = inserts.iter().map(|&(at, text)| (at.max(first), text)).peekable();
    let mut out = String::with_capacity(source.len());
    let put = |out: &mut String, text: &str, indent: &str, ending: &str| {
        for text_line in text.lines() {
            out.extend([indent, text_line, ending]);
        }
    };
    for (i, line) in source.split_inclusive('\n').enumerate() {
        let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
        let ending = ending_of(line).unwrap_or(file_ending);
        while let Some((_, text)) = inserts.next_if(|(at, _)| *at == i + 1) {
            put(&mut out, text, indent, ending);
        }
        out.push_str(line);
    }
    for (_, text) in inserts {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(file_ending);
        }
        put(&mut out, text, "", file_ending);
    }
    out
}

/// Puts each insert's text above its line in the file at `path`, as [`insert_above`] does.
pub fn insert_above_in(path: &Path, inserts: &[(usize, &str)]) -> io::Result<()> {
    let source = fs::read_to_string(path)?;
    fs::write(path, insert_above(&source, inserts))
}

/// Puts the inserts `of` gives for each of `reports` above their lines in its file, as
/// [`insert_above`] does, skipping the files with none, and says how many it put, or which file it
/// couldn't change and why.
pub fn insert_into<'a>(
    reports: &'a [FileReport],
    of: impl Fn(&'a FileReport) -> Vec<(usize, &'a str)>,
) -> Result<usize, String> {
    let mut added = 0;
    for report in reports {
        let inserts = of(report);
        if inserts.is_empty() {
            continue;
        }
        insert_above_in(&report.path, &inserts).map_err(|e| format!("{}: {e}", report.path.display()))?;
        added += inserts.len();
    }
    Ok(added)
}
