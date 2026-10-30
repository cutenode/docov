//! `unix`: a line per undocumented item, the way compilers write warnings, for editors and grep.

use crate::report::{Context, Format, undocumented};

/// The format.
pub const FORMAT: Format =
    Format { name: "unix", help: "path:line:column: kind name undocumented [docov], per undocumented item", render };

fn render(context: &Context) -> String {
    undocumented(context.files)
        .map(|(file, item)| {
            let (path, kind) = (file.path.display(), item.kind.as_str());
            format!("{path}:{}:{}: {kind} {} undocumented [docov]\n", item.line, item.column, item.name)
        })
        .collect()
}
