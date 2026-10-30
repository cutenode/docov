//! What an item's JSDoc says past whether it documents the item: the JSON report has these, and the
//! metrics and orders beyond coverage use them.

use serde::{Deserialize, Serialize};

use super::{Found, doc};

/// What an item's JSDoc says, and what about the item the reports order by. The JSON report leaves
/// out the skipped fields, which the cache keeps apart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    /// Whether its JSDoc describes it: text before its first tag, or a tag that stands in for that.
    pub has_description: bool,
    /// Whether its JSDoc has an `@example`.
    pub has_example: bool,
    /// For a function or method, whether each of its parameters has a matching `@param`.
    pub params_documented: Option<bool>,
    /// Whether its JSDoc has `@deprecated`.
    pub deprecated: bool,
    /// The text of its JSDoc's `@since`, if it has one with text.
    pub since: Option<String>,
    /// How many parameters it has, for a function or method.
    #[serde(skip)]
    pub params: usize,
    /// Whether it's exported, by the ways of exporting that count, whatever the scope.
    #[serde(skip)]
    pub exported: bool,
    /// Whether its JSDoc has exactly one release tag: `@public`, `@beta`, `@alpha` or `@internal`.
    #[serde(skip)]
    pub release_tagged: bool,
    /// For a function or method in JavaScript, whether its JSDoc gives each `@param` a type, and its
    /// `@returns` one when it has one.
    #[serde(skip)]
    pub typed: Option<bool>,
}

impl Facts {
    /// The facts about `found`, which is `exported` or not, in JavaScript or not.
    pub(super) fn of(found: &Found, exported: bool, javascript: bool) -> Self {
        let doc = found.doc.as_ref();
        let tagged = |name| doc.is_some_and(|doc| doc::has_tag(doc, name));
        Self {
            has_description: doc.is_some_and(|doc| doc::describes(doc, found.kind)),
            has_example: tagged("example"),
            params_documented: found.params.as_ref().map(|params| doc::documents(doc, params)),
            deprecated: tagged("deprecated"),
            since: doc.and_then(|doc| doc::text(doc, "since")),
            params: found.params.as_ref().map_or(0, Vec::len),
            exported,
            release_tagged: doc.is_some_and(|doc| doc::release_tags(doc) == 1),
            typed: found.params.as_ref().filter(|_| javascript).map(|_| doc.is_some_and(doc::typed)),
        }
    }
}
