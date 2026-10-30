//! Which items count, and when one is documented: the scan's settings, which features fill in.

use regex::Regex;
use rustc_hash::FxHashSet;

use super::{Found, doc};
use crate::{
    Kind, Scope,
    feature::{access::Access, inherit::Inherit},
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
        let kind = self.kinds.as_ref().is_none_or(|kinds| kinds.contains(&found.kind));
        let member = matches!(found.kind, Kind::Method | Kind::Accessor | Kind::Property);
        let own_name = found.name.split_once('.').filter(|_| member).map(|(_, name)| name);
        let named = |re: &Regex| re.is_match(&found.name) || own_name.is_some_and(|name| re.is_match(name));
        let level = found.doc.as_ref().and_then(doc::access);
        let level = level.unwrap_or(if found.protected { Access::Protected } else { Access::Public });
        let access = self.access.as_ref().is_none_or(|levels| levels.contains(&level));
        in_scope && kind && access && !self.exclude_tags.iter().any(tagged) && !self.ignore_names.iter().any(named)
    }

    /// Whether `found` is documented.
    pub(super) fn documents(&self, found: &Found) -> bool {
        let inherits = self.inherit == Inherit::Override && found.overrides;
        inherits || found.doc.as_ref().is_some_and(|doc| !self.require_description || doc::describes(doc, found.kind))
    }
}
