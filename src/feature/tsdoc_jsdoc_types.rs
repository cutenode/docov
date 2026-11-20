//! `tsdocJsdocTypes`: the `tsdoc-*` checks read blocks typed in JSDoc, as a JavaScript project
//! whose types come from its JSDoc writes them, leaving out what TSDoc's grammar has no place for.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocJsdocTypes>()
}

/// Whether the `tsdoc-*` checks skip JSDoc types and read JSDoc's `@param` names.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TsdocJsdocTypes(pub bool);

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// For JavaScript typed in JSDoc: the tsdoc-* checks skip the `{type}` after `@param`, `@returns`, `@type`, `@typedef`, `@property`, `@template`, `@throws` and the other tags that take one, and read `[name]`, `[name=default]` and `name.sub` as plain names. Everything else is checked as before, and tsdoc-params still wants ` - ` before a `@param`'s description.
    tsdoc_jsdoc_types: bool,
}

impl Feature for TsdocJsdocTypes {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        Self(keys.tsdoc_jsdoc_types)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.tsdoc_jsdoc_types = self.0;
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }
}
