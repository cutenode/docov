//! `sarif`: SARIF 2.1.0, for GitHub code scanning and other static analysis viewers.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

use crate::report::{Context, Format, RULE, json, message, slash, undocumented};

/// The format.
pub const FORMAT: Format = Format { name: "sarif", help: "SARIF 2.1.0, for GitHub code scanning", render };

/// What a URI's path keeps as it is: the unreserved characters and `/`.
const PATH: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~').remove(b'/');

/// One run with one rule, and a result at each undocumented item.
fn render(context: &Context) -> String {
    let results: Vec<_> = undocumented(context.files)
        .map(|(file, item)| {
            let uri = utf8_percent_encode(&slash(&file.path), PATH).to_string();
            json!({
                "ruleId": RULE,
                "ruleIndex": 0,
                "level": "warning",
                "message": { "text": message(item) },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": uri },
                        "region": { "startLine": item.line, "startColumn": item.column },
                    },
                }],
            })
        })
        .collect();
    let rule = json!({
        "id": RULE,
        "name": "Undocumented",
        "shortDescription": { "text": "A declaration without JSDoc" },
        "defaultConfiguration": { "level": "warning" },
    });
    let driver = json!({
        "name": "docov",
        "version": env!("CARGO_PKG_VERSION"),
        "informationUri": env!("CARGO_PKG_REPOSITORY"),
        "rules": [rule],
    });
    json(json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{ "tool": { "driver": driver }, "results": results }],
    }))
}
