use std::io::{self, Write};

use crate::FileReport;

/// Each item as a line record (`DA:<line>,<hits>`), hit once when it's documented. Items that
/// share a line are one record, hit only when all of them are documented.
pub fn write(files: &[FileReport], out: &mut impl Write) -> io::Result<()> {
    for file in files {
        writeln!(out, "SF:{}", file.path.display())?;
        let mut lines: Vec<(usize, bool)> = Vec::new();
        for item in &file.items {
            match lines.iter_mut().find(|(line, _)| *line == item.line) {
                Some((_, documented)) => *documented &= item.documented,
                None => lines.push((item.line, item.documented)),
            }
        }
        for (line, documented) in &lines {
            writeln!(out, "DA:{line},{}", u8::from(*documented))?;
        }
        let hit = lines.iter().filter(|(_, documented)| *documented).count();
        writeln!(out, "LH:{hit}\nLF:{}\nend_of_record", lines.len())?;
    }
    Ok(())
}
