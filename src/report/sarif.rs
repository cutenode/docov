//! `sarif`: SARIF 2.1.0, for GitHub code scanning and other static analysis viewers.

use std::path::Path;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

use crate::{
    check,
    report::{Context, Format, RULE, diagnostics, json, message, rule_id, severity, slash, undocumented},
};

/// The format.
pub const FORMAT: Format = Format { name: "sarif", help: "SARIF 2.1.0, for GitHub code scanning", render };

/// What a URI's path keeps as it is: the unreserved characters and `/`.
const PATH: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~').remove(b'/');

/// One run, with a result at each undocumented item and each doc check finding, and a rule for
/// undocumented items and for each doc check with a finding.
fn render(context: &Context) -> String {
    let location = |path: &Path, line: usize, column: usize| {
        let uri = utf8_percent_encode(&slash(path), PATH).to_string();
        json!([{
            "physicalLocation": {
                "artifactLocation": { "uri": uri },
                "region": { "startLine": line, "startColumn": column },
            },
        }])
    };
    let mut results: Vec<_> = undocumented(context.files)
        .map(|(file, item)| {
            json!({
                "ruleId": RULE,
                "ruleIndex": 0,
                "level": "warning",
                "message": { "text": message(item) },
                "locations": location(&file.path, item.line, item.column),
            })
        })
        .collect();
    let found: Vec<_> = diagnostics(context.files).collect();
    let checks: Vec<_> = check::catalog().filter(|(id, _)| found.iter().any(|(_, d)| d.rule == *id)).collect();
    results.extend(found.iter().map(|(file, d)| {
        json!({
            "ruleId": rule_id(d.rule),
            "ruleIndex": 1 + checks.iter().position(|(id, _)| *id == d.rule).expect("each finding's rule is listed"),
            "level": severity(d),
            "message": { "text": d.message },
            "locations": location(&file.path, d.line, d.column),
        })
    }));
    let rule = json!({
        "id": RULE,
        "name": "Undocumented",
        "shortDescription": { "text": "A declaration without JSDoc" },
        "defaultConfiguration": { "level": "warning" },
    });
    let rules = std::iter::once(rule)
        .chain(checks.iter().map(|(id, about)| json!({ "id": rule_id(id), "shortDescription": { "text": about } })));
    let driver = json!({
        "name": "docov",
        "version": env!("CARGO_PKG_VERSION"),
        "informationUri": env!("CARGO_PKG_REPOSITORY"),
        "rules": rules.collect::<Vec<_>>(),
    });
    json(json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{ "tool": { "driver": driver }, "results": results }],
    }))
}
