//! Which items count, and when one is documented: the scan's settings, which features fill in.

use std::{fmt, sync::Arc};

use regex::Regex;
use rustc_hash::FxHashSet;

use super::{Found, Via, doc};
use crate::{
    Kind, Scope,
    feature::{access::Access, exports::Exports, file_docs::FileDocs, inherit::Inherit, members::Members},
    public::Api,
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
    /// Whether `X.prototype.m = …` and `X.m = …` add members, and `this.a = …` in a CommonJS file
    /// exports.
    pub commonjs_members: bool,
    /// What the package's entry points reach, for the public scope.
    pub public: Option<Arc<Api>>,
}

impl From<Scope> for Rules {
    fn from(scope: Scope) -> Self {
        Self { scope, ..Self::default() }
    }
}

impl Rules {
    /// Whether `found` counts: it's in scope (`exported` holds the names exported apart from their
    /// declarations, and how, and `reached` the local names the package's entry points reach in
    /// its file, if they reach it), and no filter leaves it out.
    pub(super) fn counts(
        &self,
        found: &Found,
        exported: &FxHashSet<(&str, Via)>,
        reached: Option<&FxHashSet<String>>,
    ) -> bool {
        self.out_of_scope(found, exported, reached).is_none() && self.filter(found).is_none()
    }

    /// Why `found` doesn't count, if it doesn't, as [`counts`](Self::counts) decides.
    pub(super) fn left_out(
        &self,
        found: &Found,
        exported: &FxHashSet<(&str, Via)>,
        reached: Option<&FxHashSet<String>>,
    ) -> Option<String> {
        let out_of_scope = self.out_of_scope(found, exported, reached).map(str::to_owned);
        out_of_scope.or_else(|| self.filter(found).map(|filter| filter.to_string()))
    }

    /// Why `found` is out of scope, if it is.
    fn out_of_scope(
        &self,
        found: &Found,
        exported: &FxHashSet<(&str, Via)>,
        reached: Option<&FxHashSet<String>>,
    ) -> Option<&'static str> {
        // A value assigned to a global is an item only when globals count.
        if found.via == Some(Via::Global) && !self.exports.globals {
            return Some("a global, and globals don't count");
        }
        let (in_scope, why) = match self.scope {
            Scope::All => (true, ""),
            Scope::Exports => (self.exported(found, exported), "not exported by a way of exporting that counts"),
            // Reached by its module-level name, or, for what has no name the graph follows (a
            // CommonJS export, a file's own item), by its file.
            Scope::Public => {
                let commonjs =
                    found.via == Some(Via::Cjs) || found.binding.is_some_and(|b| exported.contains(&(b, Via::Cjs)));
                let local =
                    found.binding.unwrap_or_else(|| found.name.split_once('.').map_or(&found.name, |(owner, _)| owner));
                let reached = reached.is_some_and(|names| found.standalone || commonjs || names.contains(local));
                (self.exported(found, exported) && reached, "not reached from the package's entry points")
            }
        };
        (!in_scope).then_some(why)
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

    /// The filter that leaves `found` out, by its kind, access level, tags or name, if one does.
    fn filter<'r>(&'r self, found: &Found) -> Option<Filter<'r>> {
        if self.kinds.as_ref().is_some_and(|kinds| !kinds.contains(&found.kind)) {
            return Some(Filter::Kind(found.kind));
        }
        let level = found.doc.as_ref().and_then(doc::access);
        let level = level.unwrap_or(if found.protected { Access::Protected } else { Access::Public });
        if self.access.as_ref().is_some_and(|levels| !levels.contains(&level)) {
            return Some(Filter::Access(level));
        }
        let tagged = |tag: &str| found.doc.as_ref().is_some_and(|doc| doc::has_tag(doc, tag));
        if self.ignore_deprecated && tagged("deprecated") {
            return Some(Filter::Tag("deprecated"));
        }
        if let Some(tag) = self.exclude_tags.iter().find(|tag| tagged(tag)) {
            return Some(Filter::Tag(tag));
        }
        let own_name = found.name.split_once('.').filter(|_| found.kind.is_member()).map(|(_, name)| name);
        let named = |re: &&Regex| re.is_match(&found.name) || own_name.is_some_and(|name| re.is_match(name));
        self.ignore_names.iter().find(named).map(Filter::Name)
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

/// What leaves an item out of the total.
enum Filter<'r> {
    /// `--kind`, without its kind.
    Kind(Kind),
    /// `--access`, without its access level.
    Access(Access),
    /// `--exclude-tag` or `ignoreDeprecated`, with its tag.
    Tag(&'r str),
    /// `--ignore-name`, with the pattern its name matches.
    Name(&'r Regex),
}

impl fmt::Display for Filter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Kind(kind) => write!(f, "its kind, {}, doesn't count", kind.as_str()),
            Self::Access(level) => {
                write!(f, "its access level, {}, doesn't count", format!("{level:?}").to_lowercase())
            }
            Self::Tag(tag) => write!(f, "its JSDoc has @{tag}"),
            Self::Name(re) => write!(f, "its name matches {re}"),
        }
    }
}
