//! `--scope` and `scope`: which declarations count.

use clap::{ArgMatches, FromArgMatches};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Rules,
    feature::{Entry, Feature},
};

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
    /// What the package's entry points (package.json's `exports`, `main`, `module`, `types` and
    /// `typings`) export, following re-exports through the module graph.
    Public,
}

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Which declarations count, over the config's and its overrides' [default: the config's, or exports]
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

impl Scope {
    /// The scope `--scope` gives in `matches`, if it's given. It wins over every override's too.
    pub fn flag(matches: &ArgMatches) -> Option<Self> {
        Flags::from_arg_matches(matches).expect("the flags were added by `args`").scope
    }
}

impl Feature for Scope {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        flags.scope.unwrap_or(keys.scope)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.scope = *self;
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(self).expect("a scope serializes"))
    }
}
