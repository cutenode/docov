//! `inherit`: what documents an item by inheriting its docs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Inherit>()
}

/// What documents an item by inheriting its docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Inherit {
    /// An `@inheritdoc` tag.
    #[default]
    Tag,
    /// An `@inheritdoc` tag, or TypeScript's `override` on a class member.
    Override,
}

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// What documents an item by inheriting its docs.
    inherit: Inherit,
}

impl Feature for Inherit {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        keys.inherit
    }

    fn rules(&self, rules: &mut Rules) {
        rules.inherit = *self;
    }
}
