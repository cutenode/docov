//! `github`: GitHub Actions annotations, a warning on each undocumented item, one at its level on
//! each doc check finding, and an error for each failed gate.

use crate::report::{Context, Format, diagnostics, message, severity, slash, undocumented};

/// The format.
pub const FORMAT: Format =
    Format { name: "github", help: "GitHub Actions annotations: undocumented items, findings and failed gates", render };

/// Workflow commands, which the runner reads from the step's output:
/// <https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands>
fn render(context: &Context) -> String {
    let mut out = String::new();
    for (file, item) in undocumented(context.files) {
        let (path, message) = (property(&slash(&file.path)), data(&message(item)));
        out += &format!("::warning file={path},line={},col={},title=docov::{message}\n", item.line, item.column);
    }
    for (file, d) in diagnostics(context.files) {
        let (level, path) = (severity(d), property(&slash(&file.path)));
        let (line, column, rule, message) = (d.line, d.column, property(d.rule), data(&d.message));
        out += &format!("::{level} file={path},line={line},col={column},title=docov/{rule}::{message}\n");
    }
    for failure in context.failures {
        out += &format!("::error title=docov::{}\n", data(failure));
    }
    out
}

/// A command's message, escaped the way the runner unescapes it.
fn data(text: &str) -> String {
    text.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

/// A command's property value, which also can't hold the `:` and `,` that separate properties.
fn property(text: &str) -> String {
    data(text).replace(':', "%3A").replace(',', "%2C")
}
