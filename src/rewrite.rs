//! Adds lines to source files and leaves every other line as it was, line endings included.

use std::{fs, io, path::Path};

/// `source` with each insert's text (a line, or several) put above its 1-based line, indented like
/// that line and ended like it. `inserts` go in line order, and a line's own in the order given.
pub fn insert_above(source: &str, inserts: &[(usize, &str)]) -> String {
    let ending_of = |line: &str| line.strip_suffix('\n').map(|l| if l.ends_with('\r') { "\r\n" } else { "\n" });
    // A last line with no line ending takes the file's first one.
    let file_ending = source.split_inclusive('\n').next().and_then(ending_of).unwrap_or("\n");
    let mut inserts = inserts.iter().peekable();
    let mut out = String::with_capacity(source.len());
    for (i, line) in source.split_inclusive('\n').enumerate() {
        let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
        let ending = ending_of(line).unwrap_or(file_ending);
        while let Some((_, text)) = inserts.next_if(|(at, _)| *at == i + 1) {
            for text_line in text.lines() {
                out.extend([indent, text_line, ending]);
            }
        }
        out.push_str(line);
    }
    out
}

/// Puts each insert's text above its line in the file at `path`, as [`insert_above`] does.
pub fn insert_above_in(path: &Path, inserts: &[(usize, &str)]) -> io::Result<()> {
    let source = fs::read_to_string(path)?;
    fs::write(path, insert_above(&source, inserts))
}
