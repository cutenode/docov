use std::io::{self, Write};

use crate::FileReport;

/// Each counted item as a line record (`DA:<line>,<hits>`), hit once when it's documented. Items that
/// share a line are one record, hit only when all of them are documented. Items come in source
/// order, so those sharing a line are next to each other.
pub fn write(files: &[FileReport], out: &mut impl Write) -> io::Result<()> {
    for file in files {
        writeln!(out, "SF:{}", file.path.display())?;
        let mut lines: Vec<(usize, bool)> = Vec::new();
        for item in file.counted() {
            match lines.last_mut() {
                Some((line, documented)) if *line == item.line => *documented &= item.documented,
                _ => lines.push((item.line, item.documented)),
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
