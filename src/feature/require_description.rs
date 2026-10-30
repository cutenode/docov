//! `--require-description` and `requireDescription`: a JSDoc block with tags alone
//! (`/** @type {string} */`) doesn't document its item.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<RequireDescription>()
}

/// Whether a JSDoc block needs a description to document its item.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RequireDescription(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Count a JSDoc block only with a description: text before its first tag, or @description,
    /// @summary, @classdesc or @inheritdoc
    #[arg(long)]
    require_description: bool,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Count a JSDoc block only with text before its first tag, or @description, @summary, @classdesc or @inheritdoc.
    require_description: bool,
}

impl Feature for RequireDescription {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self(flags.require_description || keys.require_description)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.require_description = self.0;
    }
}
