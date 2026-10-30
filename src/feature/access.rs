//! `--access` and `access`: which access levels count, as jsdoc's `--access` picks which to show.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<AccessLevels>()
}

/// An item's access level: its JSDoc's `@public`, `@protected`, `@package`, `@private` or
/// `@access` tag, or else `protected` for a TypeScript `protected` member and `public` for the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// No access tag, or `@public`.
    Public,
    /// `@protected`, or a TypeScript `protected` member.
    Protected,
    /// `@package`.
    Package,
    /// `@private`.
    Private,
}

/// The access levels that count, or `None` to look at no access tags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessLevels(pub Option<Vec<Access>>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Count only items with these access levels, comma-separated [default: the config's, or every item]
    #[arg(long, value_enum, value_name = "LEVEL", value_delimiter = ',')]
    access: Vec<Access>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Count only items with these access levels, by their access tags. Without it, access tags change nothing.
    access: Option<Vec<Access>>,
}

impl Feature for AccessLevels {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self(if flags.access.is_empty() { keys.access } else { Some(flags.access) })
    }

    fn rules(&self, rules: &mut Rules) {
        rules.access.clone_from(&self.0);
    }
}
