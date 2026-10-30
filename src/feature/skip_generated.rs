//! `skipGenerated`: files whose first comment has `@generated` have nothing to count.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<SkipGenerated>()
}

/// Whether files whose first comment has `@generated` are left out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkipGenerated(pub bool);

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Leave out files whose first comment has @generated.
    skip_generated: bool,
}

impl Feature for SkipGenerated {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        Self(keys.skip_generated)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.skip_generated = self.0;
    }
}
