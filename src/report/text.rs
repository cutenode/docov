use std::io::{self, Write};

use super::Options;
use crate::{FileReport, Item, Totals};

/// One line per file with something to document, then its undocumented items as
/// `path:line:column`, which terminals and editors open at the spot, in the order `options` sorts.
pub fn write(files: &[FileReport], options: &Options, out: &mut impl Write) -> io::Result<()> {
    for file in files.iter().filter(|f| f.counted().next().is_some()) {
        let totals = file.totals();
        writeln!(out, "{:>6.1}%  {}/{}  {}", totals.percent(), totals.documented, totals.total, file.path.display())?;
        let mut undocumented: Vec<&Item> = file.counted().filter(|i| !i.documented).collect();
        options.sort.order(&mut undocumented);
        for item in undocumented {
            let (path, kind) = (file.path.display(), item.kind.as_str());
            writeln!(out, "         {path}:{}:{}  {kind} {}", item.line, item.column, item.name)?;
        }
    }
    let totals = Totals::of(files);
    writeln!(out, "{:>6.1}%  {}/{}  total", totals.percent(), totals.documented, totals.total)
}
