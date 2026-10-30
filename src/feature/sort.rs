//! `--sort`: the order of the text report's undocumented items.

use std::cmp::Reverse;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Item, Kind,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Sort>()
}

/// The order of each file's undocumented items in the text report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// Where they are in the file.
    #[default]
    Position,
    /// What to document first: exported functions and methods, those with more parameters first,
    /// then classes, then the rest, each in file order.
    Priority,
}

impl Sort {
    /// Puts `items`, which are in file order, in this order.
    pub fn order(self, items: &mut [&Item]) {
        if self == Self::Priority {
            items.sort_by_key(|item| match item.kind {
                Kind::Function | Kind::Method if item.facts.exported => (0, Reverse(item.facts.params)),
                Kind::Class => (1, Reverse(0)),
                _ => (2, Reverse(0)),
            });
        }
    }
}

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// The order of the text report's undocumented items
    #[arg(long, value_enum, default_value_t)]
    sort: Sort,
}

/// It has no key: it's how one run reads.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct Keys {}

impl Feature for Sort {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, _keys: Keys) -> Self {
        flags.sort
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!(self))
    }
}
