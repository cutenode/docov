//! `rdjson`: reviewdog's diagnostic format, for review comments.

use serde_json::json;

use crate::report::{Context, Format, RULE, json, message, slash, undocumented};

/// The format.
pub const FORMAT: Format = Format { name: "rdjson", help: "reviewdog's diagnostic format (rdjson)", render };

/// A diagnostic at each undocumented item:
/// https://github.com/reviewdog/reviewdog/tree/master/proto/rdf
fn render(context: &Context) -> String {
    let diagnostics: Vec<_> = undocumented(context.files)
        .map(|(file, item)| {
            json!({
                "message": message(item),
                "location": {
                    "path": slash(&file.path),
                    "range": { "start": { "line": item.line, "column": item.column } },
                },
                "severity": "WARNING",
                "code": { "value": RULE },
            })
        })
        .collect();
    let source = json!({ "name": "docov", "url": env!("CARGO_PKG_REPOSITORY") });
    json(&json!({ "source": source, "severity": "WARNING", "diagnostics": diagnostics }))
}
