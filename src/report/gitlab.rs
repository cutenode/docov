//! `gitlab`: a GitLab Code Quality report, for merge request widgets.

use std::hash::Hasher;

use serde_json::json;

use crate::report::{Context, Format, RULE, json, message, slash, undocumented};

/// The format.
pub const FORMAT: Format = Format { name: "gitlab", help: "A GitLab Code Quality report", render };

/// An issue at each undocumented item:
/// https://docs.gitlab.com/ci/testing/code_quality/#code-quality-report-format
fn render(context: &Context) -> String {
    let issues: Vec<_> = undocumented(context.files)
        .map(|(file, item)| {
            let path = slash(&file.path);
            json!({
                "description": message(item),
                "check_name": RULE,
                "fingerprint": fingerprint(&[&path, item.kind.as_str(), &item.name]),
                "severity": "minor",
                "location": { "path": path, "lines": { "begin": item.line } },
            })
        })
        .collect();
    json(issues.into())
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
