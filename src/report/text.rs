//! `text`: each file's percentage and its undocumented items, then the total. It's the default.
//!
//! `--omit-covered` leaves out fully documented files, and `--quiet` leaves out every file, so only
//! the total and the totals after it are left. Failed gates go to stderr either way.
//! `--sort priority` lists each file's undocumented items by what to document first: exported
//! functions and methods (more parameters first), then classes, then the rest.
//!
//! On a terminal, a percentage under the global threshold is red and the undocumented items are
//! dim. `--color always` or `--color never` says otherwise, and `NO_COLOR` turns the colors off
//! unless `--color always` is given. Written to a file, the report has no colors.

use crate::report::{Context, Format, Text};
use crate::{FileReport, Item, Totals, metric::EXAMPLES};

/// The format.
pub const FORMAT: Format =
    Format { name: "text", help: "Each file's percentage, its undocumented items, and the total (the default)", render };

/// One line per file with something to document, then its undocumented items as
/// `path:line:column`, which terminals and editors open at the spot, in the order `--sort` puts
/// them. `--omit-covered` leaves out fully documented files, `--quiet` all of them, `--examples`
/// adds how many of each line's items have an `@example`, each metric adds a line after the total,
/// `--by-package` adds each package's total, and `--by-kind` the totals by kind.
fn render(context: &Context) -> String {
    let Text { omit_covered, quiet, by_kind } = context.text;
    let shown = |file: &&FileReport| {
        let covered = file.counted().all(|i| i.documented);
        file.counted().next().is_some() && !quiet && !(omit_covered && covered)
    };
    let line = |totals: Totals, examples: Totals, name: &dyn std::fmt::Display| {
        let (percent, documented, total) = (totals.percent(), totals.documented, totals.total);
        let figure =
            if context.examples { format!("examples {}/{}  ", examples.documented, examples.total) } else { String::new() };
        format!("{percent:>6.1}%  {documented}/{total}  {figure}{name}\n")
    };
    let mut out = String::new();
    for file in context.files.iter().filter(shown) {
        out += &line(file.totals(), EXAMPLES.file(file), &file.path.display());
        let mut undocumented: Vec<&Item> = file.counted().filter(|i| !i.documented).collect();
        context.sort.order(&mut undocumented);
        for item in undocumented {
            let (path, kind) = (file.path.display(), item.kind.as_str());
            out += &format!("         {path}:{}:{}  {kind} {}\n", item.line, item.column, item.name);
        }
    }
    out += &line(Totals::of(context.files), EXAMPLES.total(context.files), &"total");
    for metric in context.metrics {
        out += &metric.line(context.files);
    }
    for package in context.packages.into_iter().flatten() {
        let Totals { documented, total } = package.totals;
        out += &format!("{:>6.1}%  {documented}/{total}  {} total\n", package.percent, package.path);
    }
    if !by_kind {
        return out;
    }
    let kinds: Vec<String> = Totals::by_kind(context.files)
        .into_iter()
        .map(|(kind, totals)| format!("{} {}/{}", kind.as_str(), totals.documented, totals.total))
        .collect();
    out + &format!("by kind: {}\n", kinds.join(", "))
}
