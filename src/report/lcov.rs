//! `lcov`: a line record for each line with items, for coverage services and editor gutters.

use crate::report::{Context, Format, lines};

/// The format.
pub const FORMAT: Format = Format {
    name: "lcov",
    help: "A line record for each line with items, for coverage services and editor gutters",
    render,
};

/// Each item as a function (`FN`), hit once (`FNDA`) when it's documented, and each line with
/// items as a line record (`DA:<line>,<hits>`), hit when all of them are documented.
fn render(context: &Context) -> String {
    let mut out = String::new();
    for file in context.files {
        out += &format!("TN:docov\nSF:{}\n", file.path.display());
        for item in &file.items {
            out += &format!("FN:{},{}\n", item.line, item.name);
        }
        for item in &file.items {
            out += &format!("FNDA:{},{}\n", u8::from(item.documented), item.name);
        }
        let totals = file.totals();
        out += &format!("FNF:{}\nFNH:{}\n", totals.total, totals.documented);
        let lines = lines(file);
        for (line, documented) in &lines {
            out += &format!("DA:{line},{}\n", u8::from(*documented));
        }
        let hit = lines.iter().filter(|(_, documented)| *documented).count();
        out += &format!("LH:{hit}\nLF:{}\nend_of_record\n", lines.len());
    }
    out
}
