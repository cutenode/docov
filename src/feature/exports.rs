//! `exports`: which ways of exporting count under the `exports` scope.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Exports>()
}

/// Which ways of exporting count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Exports {
    /// ES modules: `export`, `export { a }` and `export default`.
    pub esm: bool,
    /// CommonJS: `exports.a = …`, `module.exports = …` and TypeScript's `export =`.
    pub cjs: bool,
    /// Globals: `window.a = …` and `globalThis.a = …` at the top level.
    pub globals: bool,
}

impl Default for Exports {
    fn default() -> Self {
        Self { esm: true, cjs: true, globals: false }
    }
}

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Which ways of exporting count under the `exports` scope. Globals are items under either scope only when on.
    exports: Exports,
}

impl Feature for Exports {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        keys.exports
    }

    fn rules(&self, rules: &mut Rules) {
        rules.exports = *self;
    }
}
