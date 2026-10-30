//! Which items count, and when one is documented: the scan's settings, which features fill in.

use rustc_hash::FxHashSet;

use super::{Found, doc};
use crate::Scope;

/// Which items count, and when one is documented. The default is docov's out of the box.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    /// Which declarations count.
    pub scope: Scope,
    /// Whether a JSDoc block documents its item only when it describes it, not with tags alone.
    pub require_description: bool,
    /// The JSDoc tags, without their `@`, that take an item out of the total.
    pub exclude_tags: Vec<String>,
}

impl From<Scope> for Rules {
    fn from(scope: Scope) -> Self {
        Self { scope, ..Self::default() }
    }
}

impl Rules {
    /// Whether `found` counts: it's in scope (`exported` holds the names exported apart from their
    /// declarations), and nothing leaves it out.
    pub(super) fn counts(&self, found: &Found, exported: &FxHashSet<&str>) -> bool {
        let in_scope =
            self.scope == Scope::All || found.exported || found.binding.is_some_and(|b| exported.contains(b));
        let tagged = |tag: &String| found.doc.as_ref().is_some_and(|doc| doc::has_tag(doc, tag));
        in_scope && !self.exclude_tags.iter().any(tagged)
    }

    /// Whether `found` is documented.
    pub(super) fn documents(&self, found: &Found) -> bool {
        found.doc.as_ref().is_some_and(|doc| !self.require_description || doc::describes(doc, found.kind))
    }
}
