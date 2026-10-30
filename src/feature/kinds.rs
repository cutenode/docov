//! `--kind` and `kinds`: which kinds of item count. The text report's `--by-kind` is in
//! report.rs, with its other options.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Kind, Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Kinds>()
}

/// Which kinds of item count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kinds {
    /// The kinds that count, or every kind.
    pub only: Option<Vec<Kind>>,
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Count only these kinds of item, comma-separated [default: the config's, or every kind]
    #[arg(long, value_enum, value_name = "KIND", value_delimiter = ',')]
    kind: Vec<Kind>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Count only these kinds of item. Every kind counts without it.
    kinds: Option<Vec<Kind>>,
}

impl Feature for Kinds {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self { only: if flags.kind.is_empty() { keys.kinds } else { Some(flags.kind) } }
    }

    fn rules(&self, rules: &mut Rules) {
        rules.kinds.clone_from(&self.only);
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.only))
    }
}
