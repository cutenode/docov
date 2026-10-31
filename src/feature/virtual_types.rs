//! `virtual`: in JavaScript, `@typedef` and `@callback` blocks declare types of their own, which
//! count as items.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<VirtualTypes>()
}

/// Whether a JavaScript file's named `@typedef` and `@callback` blocks are items.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VirtualTypes(pub bool);

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Count a JavaScript file's named @typedef and @callback blocks as items of kind `type`, documented when they have a description. They then document nothing else.
    r#virtual: bool,
}

impl Feature for VirtualTypes {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        Self(keys.r#virtual)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.virtual_types = self.0;
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }
}
