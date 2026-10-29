use std::io::{self, Write};

use crate::{FileReport, Totals};

/// One line per file with something to document, then its undocumented items as
/// `path:line:column`, which terminals and editors open at the spot.
pub fn write(files: &[FileReport], out: &mut impl Write) -> io::Result<()> {
    for file in files.iter().filter(|f| !f.items.is_empty()) {
        let totals = file.totals();
        writeln!(out, "{:>6.1}%  {}/{}  {}", totals.percent(), totals.documented, totals.total, file.path.display())?;
        for item in file.items.iter().filter(|i| !i.documented) {
            let (path, kind) = (file.path.display(), item.kind.as_str());
            writeln!(out, "         {path}:{}:{}  {kind} {}", item.line, item.column, item.name)?;
        }
    }
    let totals = Totals::of(files);
    writeln!(out, "{:>6.1}%  {}/{}  total", totals.percent(), totals.documented, totals.total)
}
