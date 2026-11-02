//! `gitlab`: a GitLab Code Quality report, for merge request widgets.

use std::hash::Hasher;

use serde_json::json;

use crate::{
    check::Level,
    report::{Context, Format, RULE, diagnostics, json, message, rule_id, slash, undocumented},
};

/// The format.
pub const FORMAT: Format =
    Format { name: "gitlab", help: "A GitLab Code Quality report: undocumented items and findings", render };

/// An issue at each undocumented item, then one at each doc check finding:
/// <https://docs.gitlab.com/ci/testing/code_quality/#code-quality-report-format>
fn render(context: &Context) -> String {
    let issue = |path: String, line: usize, description: &str, check: &str, severity: &str, fingerprint: String| {
        json!({
            "description": description,
            "check_name": check,
            "fingerprint": fingerprint,
            "severity": severity,
            "location": { "path": path, "lines": { "begin": line } },
        })
    };
    let items = undocumented(context.files).map(|(file, item)| {
        let path = slash(&file.path);
        let fingerprint = fingerprint(&[&path, item.kind.as_str(), &item.name]);
        issue(path, item.line, &message(item), RULE, "minor", fingerprint)
    });
    // A finding has no name to keep it by, so its line and message stand in.
    let findings = diagnostics(context.files).map(|(file, d)| {
        let path = slash(&file.path);
        let fingerprint = fingerprint(&[&path, &d.line.to_string(), d.rule, &d.message]);
        let severity = if d.severity == Level::Error { "major" } else { "minor" };
        issue(path, d.line, &d.message, &rule_id(d.rule), severity, fingerprint)
    });
    json(items.chain(findings).collect::<Vec<_>>().into())
}

/// A hash of an issue's parts that stays the same between runs and docov versions, which GitLab
/// tells new issues from old ones by: 64-bit FNV-1a, with each part ended by a NUL so no two lists
/// of parts run together the same.
fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = fnv::FnvHasher::default();
    for part in parts {
        hasher.write(part.as_bytes());
        hasher.write(&[0]);
    }
    format!("{:016x}", hasher.finish())
}
