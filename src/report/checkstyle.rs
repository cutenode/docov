//! `checkstyle`: Checkstyle XML, which many CI tools and editors read.

use crate::report::{Context, Format, RULE, escape, findings, message, severity, slash};

/// The format.
pub const FORMAT: Format = Format {
    name: "checkstyle",
    help: "Checkstyle XML: undocumented items and doc check findings",
    render,
};

/// Each file, with a warning at each undocumented item, then an error or warning at each doc check
/// finding, its source the check's rule id as ESLint's formatter writes one: `docov.require-param`.
fn render(context: &Context) -> String {
    let mut out = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<checkstyle version=\"4.3\">\n".to_owned();
    let error = |line, column, severity, message: &str, source: &str| {
        let message = escape(message);
        format!("    <error line=\"{line}\" column=\"{column}\" severity=\"{severity}\" message=\"{message}\" source=\"{source}\"/>\n")
    };
    for file in context.files {
        out += &format!("  <file name=\"{}\">\n", escape(&slash(&file.path)));
        for item in file.counted().filter(|i| !i.documented) {
            out += &error(item.line, item.column, "warning", &message(item), RULE);
        }
        for d in findings(file) {
            out += &error(d.line, d.column, severity(d), &d.message, &format!("docov.{}", d.rule));
        }
        out += "  </file>\n";
    }
    out + "</checkstyle>\n"
}
