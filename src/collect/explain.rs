//! `--explain`: for each item in a file, whether it counts and is documented, and why, and what
//! the scan passed over, and why.

use std::path::Path;

use rustc_hash::FxHashSet;

use super::{Collected, Error, Found, Rules, collect, doc, read};
use crate::feature::inherit::Inherit;

/// A line for each item in the file at `path` and each declaration or block the scan passed over,
/// in the file's order, saying what it is and why it counts, doesn't, or was passed over:
/// `path:line:column  kind name  status`.
pub fn explain(path: &Path, rules: &Rules) -> Result<Vec<String>, Error> {
    let (source, source_type) = read(path)?;
    let reached = rules.public.as_ref().and_then(|api| api.reach(path));
    let lines = collect(&source, source_type, rules, Some(path), true, |c| c.explain(rules, reached));
    let lines = lines.map_err(|e| e.at(path, &source))?;
    Ok(lines.into_iter().map(|(line, column, text)| format!("{}:{line}:{column}  {text}", path.display())).collect())
}

impl Collected<'_> {
    /// Each item's and each skip's line, column and explanation, in the file's order.
    fn explain(self, rules: &Rules, reached: Option<&FxHashSet<String>>) -> Vec<(usize, usize, String)> {
        let items = self
            .found
            .iter()
            .map(|f| (f.at, format!("{} {}  {}", f.kind.as_str(), f.name, self.status(f, rules, reached))));
        let skips = self.skips.iter().map(|s| (s.at, format!("skipped {}: {}", s.what, s.why)));
        let mut all: Vec<_> = items.chain(skips).collect();
        all.sort_by_key(|(at, _)| *at);
        all.into_iter()
            .map(|(at, text)| {
                let (line, column) = self.lines.position(at as usize);
                (line, column, text)
            })
            .collect()
    }

    /// Whether `found` counts and is documented, and why.
    fn status(&self, found: &Found, rules: &Rules, reached: Option<&FxHashSet<String>>) -> String {
        if let Some(why) = rules.left_out(found, &self.exported, reached) {
            return format!("left out: {why}");
        }
        let block = |doc: &oxc_jsdoc::JSDoc| {
            // The block's span starts past its `/**`.
            let (line, column) = self.lines.position(doc.span.start as usize - 3);
            format!("{line}:{column}")
        };
        if !rules.documents(found) {
            let line = self.lines.position(found.at as usize).0;
            if let Some(ignore) = self.ignores.covering(found.kind, line) {
                let comment = if ignore.file { "docov-ignore-file" } else { "docov-ignore-next-line" };
                let reason = ignore.reason.map(|reason| format!(" ({reason})")).unwrap_or_default();
                return format!("ignored by the {comment} at {}:{}{reason}", ignore.line, ignore.column);
            }
            return match &found.doc {
                Some(doc) => format!("undocumented: the block at {} doesn't describe it", block(doc)),
                None => "undocumented".into(),
            };
        }
        let describes = |doc: &&oxc_jsdoc::JSDoc| {
            found.documented == Some(true) || !rules.require_description || doc::describes(doc, found.kind)
        };
        match found.doc.as_ref().filter(describes) {
            Some(doc) => format!("documented by the block at {}", block(doc)),
            None if rules.inherit == Inherit::Override && found.overrides => {
                "documented: it overrides, and inherits its docs".into()
            }
            None => "documented by a matching @property in its parent's block".into(),
        }
    }
}
