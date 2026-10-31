//! `rdjson`: reviewdog's diagnostic format, for review comments.

use serde_json::json;

use crate::report::{Context, Format, RULE, diagnostics, json, message, rule_id, severity, slash, undocumented};

/// The format.
pub const FORMAT: Format = Format { name: "rdjson", help: "reviewdog's diagnostic format (rdjson)", render };

/// A diagnostic at each undocumented item, then one at each doc check finding:
/// https://github.com/reviewdog/reviewdog/tree/master/proto/rdf
fn render(context: &Context) -> String {
    let diagnostic = |path, line: usize, column: usize, message: &str, severity: &str, code: &str| {
        json!({
            "message": message,
            "location": { "path": path, "range": { "start": { "line": line, "column": column } } },
            "severity": severity,
            "code": { "value": code },
        })
    };
    let items = undocumented(context.files)
        .map(|(file, item)| diagnostic(slash(&file.path), item.line, item.column, &message(item), "WARNING", RULE));
    let findings = diagnostics(context.files).map(|(file, d)| {
        let severity = severity(d).to_ascii_uppercase();
        diagnostic(slash(&file.path), d.line, d.column, &d.message, &severity, &rule_id(d.rule))
    });
    let source = json!({ "name": "docov", "url": env!("CARGO_PKG_REPOSITORY") });
    let diagnostics: Vec<_> = items.chain(findings).collect();
    json(json!({ "source": source, "severity": "WARNING", "diagnostics": diagnostics }))
}
