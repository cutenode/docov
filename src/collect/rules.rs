//! Which items count, and when one is documented: the scan's settings, which features fill in.

use regex::Regex;
use rustc_hash::FxHashSet;

use super::{Found, Via, doc};
use crate::{
    Kind, Scope,
    feature::{access::Access, exports::Exports, file_docs::FileDocs, inherit::Inherit, members::Members},
};

/// Which items count, and when one is documented. The default is docov's out of the box.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    /// Which declarations count.
    pub scope: Scope,
    /// Whether a JSDoc block documents its item only when it describes it, not with tags alone.
    pub require_description: bool,
    /// The JSDoc tags, without their `@`, that take an item out of the total.
    pub exclude_tags: Vec<String>,
    /// The kinds of item that count, or every kind.
    pub kinds: Option<Vec<Kind>>,
    /// Regexes that take an item out of the total when its name, or a member's own name, matches.
    pub ignore_names: Vec<Regex>,
    /// The access levels that count, or `None` to look at no access tags.
    pub access: Option<Vec<Access>>,
    /// What documents an item by inheriting its docs.
    pub inherit: Inherit,
    /// Whether items whose JSDoc has `@deprecated` are left out.
    pub ignore_deprecated: bool,
    /// Whether a file whose first comment has `@generated` has nothing to count.
    pub skip_generated: bool,
    /// Which ways of exporting count.
    pub exports: Exports,
    /// Whether each file is an item, documented by a JSDoc block about it.
    pub file_docs: FileDocs,
    /// Whether a JavaScript file's named `@typedef` and `@callback` blocks are items.
    pub virtual_types: bool,
    /// Which members count.
    pub members: Members,
}

impl From<Scope> for Rules {
    fn from(scope: Scope) -> Self {
        Self { scope, ..Self::default() }
    }
}

impl Rules {
    /// Whether `found` counts: it's in scope (`exported` holds the names exported apart from their
    /// declarations, and how), and no filter leaves it out.
    pub(super) fn counts(&self, found: &Found, exported: &FxHashSet<(&str, Via)>) -> bool {
        self.in_scope(found, exported) && !self.leaves_out(found)
    }

    fn in_scope(&self, found: &Found, exported: &FxHashSet<(&str, Via)>) -> bool {
        // A value assigned to a global is an item only when globals count.
        if found.via == Some(Via::Global) && !self.exports.globals {
            return false;
        }
        self.scope == Scope::All || self.exported(found, exported)
    }

    /// Whether `found` is exported by a way of exporting that counts, where it's declared or apart
    /// from that. An item that counts under any scope is.
    pub(super) fn exported(&self, found: &Found, exported: &FxHashSet<(&str, Via)>) -> bool {
        let counts = |via: Via| match via {
            Via::Esm => self.exports.esm,
            Via::Cjs => self.exports.cjs,
            Via::Global => self.exports.globals,
        };
        let apart = |binding| {
            [Via::Esm, Via::Cjs, Via::Global].into_iter().any(|via| counts(via) && exported.contains(&(binding, via)))
        };
        found.standalone || found.via.is_some_and(counts) || found.binding.is_some_and(apart)
    }

    /// Whether its kind, name, access level or tags leave `found` out.
    fn leaves_out(&self, found: &Found) -> bool {
        let kind = self.kinds.as_ref().is_some_and(|kinds| !kinds.contains(&found.kind));
        let member = matches!(found.kind, Kind::Method | Kind::Accessor | Kind::Property);
        let own_name = found.name.split_once('.').filter(|_| member).map(|(_, name)| name);
        let named = |re: &Regex| re.is_match(&found.name) || own_name.is_some_and(|name| re.is_match(name));
        let level = found.doc.as_ref().and_then(doc::access);
        let level = level.unwrap_or(if found.protected { Access::Protected } else { Access::Public });
        let access = self.access.as_ref().is_some_and(|levels| !levels.contains(&level));
        let tagged = |tag: &String| found.doc.as_ref().is_some_and(|doc| doc::has_tag(doc, tag));
        let deprecated =
            self.ignore_deprecated && found.doc.as_ref().is_some_and(|doc| doc::has_tag(doc, "deprecated"));
        kind || access || deprecated || self.exclude_tags.iter().any(tagged) || self.ignore_names.iter().any(named)
    }

    /// Whether `found` is documented.
    pub(super) fn documents(&self, found: &Found) -> bool {
        if let Some(documented) = found.documented {
            return documented;
        }
        let inherits = self.inherit == Inherit::Override && found.overrides;
        inherits
            || found.listed
            || found.doc.as_ref().is_some_and(|doc| !self.require_description || doc::describes(doc, found.kind))
    }
}
