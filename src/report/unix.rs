//! `unix`: a line per undocumented item and per doc check finding, the way compilers write
//! warnings, for editors and grep.

use crate::report::{Context, Format, diagnostics, undocumented};

/// The format.
pub const FORMAT: Format = Format {
    name: "unix",
    help: "path:line:column: what's wrong, per undocumented item and doc check finding",
    render,
};

fn render(context: &Context) -> String {
    let items = undocumented(context.files).map(|(file, item)| {
        let (path, kind) = (file.path.display(), item.kind.as_str());
        format!("{path}:{}:{}: {kind} {} undocumented [docov]\n", item.line, item.column, item.name)
    });
    items.chain(diagnostics(context.files).map(|(file, d)| format!("{}:{d}\n", file.path.display()))).collect()
}
