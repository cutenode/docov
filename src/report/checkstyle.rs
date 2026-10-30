//! `checkstyle`: Checkstyle XML, which many CI tools and editors read.

use crate::report::{Context, Format, RULE, escape, message, slash};

/// The format.
pub const FORMAT: Format = Format { name: "checkstyle", help: "Checkstyle XML: a warning at each undocumented item", render };

/// Each file, with a warning at each undocumented item.
fn render(context: &Context) -> String {
    let mut out = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<checkstyle version=\"4.3\">\n".to_owned();
    for file in context.files {
        out += &format!("  <file name=\"{}\">\n", escape(&slash(&file.path)));
        for item in file.counted().filter(|i| !i.documented) {
            let (line, column, message) = (item.line, item.column, escape(&message(item)));
            out += &format!("    <error line=\"{line}\" column=\"{column}\" severity=\"warning\" message=\"{message}\" source=\"{RULE}\"/>\n");
        }
        out += "  </file>\n";
    }
    out + "</checkstyle>\n"
}
