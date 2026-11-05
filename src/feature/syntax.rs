//! `syntax`: the doc comment syntax docov writes, JSDoc's or TSDoc's: the stubs `--fix` and the
//! language server write, and the tag `require-template`'s findings ask for. It also says how
//! `valid-link` reads `A#b`, where JSDoc's `A` is a class and TSDoc's a package. The scan and the
//! other checks read both the same.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Syntax>()
}

/// The doc comment syntax docov writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Syntax {
    /// JSDoc's: stubs have `@template T`, `@param x` and `@file`, `require-template` asks for
    /// `@template`, and `valid-link` looks up the class in `A#b`.
    #[default]
    Jsdoc,
    /// TSDoc's: stubs have `@typeParam T -`, `@param x -` and `@packageDocumentation`,
    /// `require-template` asks for `@typeParam`, and `valid-link` takes the `A` in `A#b` for a
    /// package and doesn't look it up.
    Tsdoc,
}

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// The doc comment syntax docov writes and reads `#` in: `jsdoc` (stubs with `@template T`, `@param x` and `@file`, and `A#b` is a class's member) or `tsdoc` (stubs with `@typeParam T -`, `@param x -` and `@packageDocumentation`, `require-template` asking for `@typeParam`, and `pkg#A` a package's export, which `valid-link` doesn't look up).
    syntax: Syntax,
}

impl Feature for Syntax {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        keys.syntax
    }

    fn rules(&self, rules: &mut Rules) {
        rules.syntax = *self;
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!(self))
    }
}
