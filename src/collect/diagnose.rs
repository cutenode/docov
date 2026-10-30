//! Runs the doc checks that are on over a file's counted items with a block.

use std::path::Path;

use rustc_hash::FxHashSet;

use super::{Collected, Rules};
use crate::check::Subject;

impl Collected<'_> {
    /// Runs `rules`' checks, if it has any, over the items that count (`reached` holds the local
    /// names the public scope's entry points reach in the file at `path`, if they reach it).
    pub(super) fn diagnose(&self, rules: &Rules, reached: Option<&FxHashSet<String>>, path: &Path) {
        let Some(checks) = &rules.checks else { return };
        for found in self.found.iter().filter(|f| rules.counts(f, &self.exported, reached)) {
            let Some(doc) = &found.doc else { continue };
            let subject = Subject { doc, params: found.params.as_deref() };
            checks.run(path, &subject, |at| self.lines.position(at as usize));
        }
    }
}
