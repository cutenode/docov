//! `ignoreDeprecated`: items whose JSDoc has `@deprecated` don't count.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<IgnoreDeprecated>()
}

/// Whether items whose JSDoc has `@deprecated` are left out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IgnoreDeprecated(pub bool);

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Leave out items whose JSDoc has @deprecated.
    ignore_deprecated: bool,
}

impl Feature for IgnoreDeprecated {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        Self(keys.ignore_deprecated)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.ignore_deprecated = self.0;
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }
}
