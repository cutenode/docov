use std::io::{self, Write};

use super::{Options, metric::EXAMPLES};
use crate::{FileReport, Item, Totals};

/// One line per file with something to document, then its undocumented items as
/// `path:line:column`, which terminals and editors open at the spot, in the order `options` sorts.
/// With `options.examples`, each line says how many of its items have an `@example`.
pub fn write(files: &[FileReport], options: &Options, out: &mut impl Write) -> io::Result<()> {
    let line = |out: &mut dyn Write, totals: Totals, examples: Totals, name: &dyn std::fmt::Display| {
        let (percent, documented, total) = (totals.percent(), totals.documented, totals.total);
        let figure = if options.examples {
            format!("examples {}/{}  ", examples.documented, examples.total)
        } else {
            String::new()
        };
        writeln!(out, "{percent:>6.1}%  {documented}/{total}  {figure}{name}")
    };
    for file in files.iter().filter(|f| f.counted().next().is_some()) {
        line(out, file.totals(), EXAMPLES.file(file), &file.path.display())?;
        let mut undocumented: Vec<&Item> = file.counted().filter(|i| !i.documented).collect();
        options.sort.order(&mut undocumented);
        for item in undocumented {
            let (path, kind) = (file.path.display(), item.kind.as_str());
            writeln!(out, "         {path}:{}:{}  {kind} {}", item.line, item.column, item.name)?;
        }
    }
    line(out, Totals::of(files), EXAMPLES.total(files), &"total")
}
