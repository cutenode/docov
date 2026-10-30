//! `fileDocs`: whether each file needs a JSDoc block of its own, which makes it an item.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<FileDocs>()
}

/// Whether each file needs a JSDoc block of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileDocs {
    /// Files aren't items.
    #[default]
    Off,
    /// Each file is an item of kind `module`, documented when its first JSDoc block has `@file`,
    /// `@fileoverview`, `@overview`, `@module` or `@packageDocumentation`, with text. That block then
    /// documents nothing else.
    Require,
}

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Whether each file needs a JSDoc block of its own (`@file`, `@fileoverview`, `@overview`, `@module` or `@packageDocumentation`, with text), counted as an item of kind `module`.
    file_docs: FileDocs,
}

impl Feature for FileDocs {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        keys.file_docs
    }

    fn rules(&self, rules: &mut Rules) {
        rules.file_docs = *self;
    }
}
