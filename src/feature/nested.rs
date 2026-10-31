//! `nested`: scans each file with the nearest config file above it, merged over the root one.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::feature::{Entry, Feature};

pub(crate) fn entry() -> Entry {
    Entry::of::<Nested>()
}

/// Whether config files in subfolders apply to the files under them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Nested(pub bool);

/// No flag: it's the root config's to say.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Scan each file with the nearest docov.json, docov.jsonc or .docovrc above it, up to this file's folder, merged over this one. Only the root config's counts.
    nested: bool,
}

impl Feature for Nested {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_: Flags, keys: Keys) -> Self {
        Self(keys.nested)
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }
}
