//! `lcov`: a line record for each line with items, for coverage services and editor gutters.

use crate::report::{Context, Format};

/// The format.
pub const FORMAT: Format = Format {
    name: "lcov",
    help: "A line record for each line with items, for coverage services and editor gutters",
    render,
};

/// Each item as a line record (`DA:<line>,<hits>`), hit once when it's documented. Items that
/// share a line are one record, hit only when all of them are documented. Items come in source
/// order, so those sharing a line are next to each other.
fn render(context: &Context) -> String {
    let mut out = String::new();
    for file in context.files {
        out += &format!("SF:{}\n", file.path.display());
        let mut lines: Vec<(usize, bool)> = Vec::new();
        for item in &file.items {
            match lines.last_mut() {
                Some((line, documented)) if *line == item.line => *documented &= item.documented,
                _ => lines.push((item.line, item.documented)),
            }
        }
        for (line, documented) in &lines {
            out += &format!("DA:{line},{}\n", u8::from(*documented));
        }
        let hit = lines.iter().filter(|(_, documented)| *documented).count();
        out += &format!("LH:{hit}\nLF:{}\nend_of_record\n", lines.len());
    }
    out
}
