//! `--scope` and `scope`: which declarations count.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::feature::{Entry, Feature};

pub(crate) fn entry() -> Entry {
    Entry::of::<Scope>()
}

/// Which declarations count toward coverage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// What a module exports, with the public members of exported classes and interfaces.
    #[default]
    Exports,
    /// Every module-level declaration, exported or not.
    All,
}

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Which declarations count [default: the config's, or exports]
    #[arg(long, value_enum)]
    scope: Option<Scope>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Which declarations count.
    scope: Scope,
}

impl Feature for Scope {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        flags.scope.unwrap_or(keys.scope)
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(self).expect("a scope serializes"))
    }
}
