//! `text`, the default: each file's percentage and its undocumented items, then the total.

use crate::report::{Context, Format, Text};
use crate::{FileReport, Totals};

/// The format.
pub const FORMAT: Format =
    Format { name: "text", help: "Each file's percentage, its undocumented items, and the total (the default)", render };

/// One line per file with something to document, then its undocumented items as
/// `path:line:column`, which terminals and editors open at the spot. `--omit-covered` leaves out
/// fully documented files, and `--quiet` all of them.
fn render(context: &Context) -> String {
    let Text { omit_covered, quiet } = context.text;
    let shown = |file: &&FileReport| {
        let covered = file.items.iter().all(|i| i.documented);
        !file.items.is_empty() && !quiet && !(omit_covered && covered)
    };
    let mut out = String::new();
    for file in context.files.iter().filter(shown) {
        let totals = file.totals();
        out += &format!("{:>6.1}%  {}/{}  {}\n", totals.percent(), totals.documented, totals.total, file.path.display());
        for item in file.items.iter().filter(|i| !i.documented) {
            let (path, kind) = (file.path.display(), item.kind.as_str());
            out += &format!("         {path}:{}:{}  {kind} {}\n", item.line, item.column, item.name);
        }
    }
    let totals = Totals::of(context.files);
    out + &format!("{:>6.1}%  {}/{}  total\n", totals.percent(), totals.documented, totals.total)
}
