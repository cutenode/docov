//! `members`: which members count, past those of classes and interfaces.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Members>()
}

/// Which members count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Members {
    /// The public members of classes and interfaces.
    #[default]
    Classes,
    /// Those, and enum members, the members of a type alias's object type, and the properties of
    /// an object literal tagged `@enum`, `@namespace` or `@lends`. A matching `@property` in the
    /// parent's block documents one of these.
    All,
}

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Which members count: those of classes and interfaces, or all of them, with enum members, a type alias's object members and the properties of an object literal tagged @enum, @namespace or @lends.
    members: Members,
}

impl Feature for Members {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        keys.members
    }

    fn rules(&self, rules: &mut Rules) {
        rules.members = *self;
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!(self))
    }
}
