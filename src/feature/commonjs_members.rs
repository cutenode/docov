//! `commonjsMembers`: CommonJS ways of adding to an export, `X.prototype.m = …` and `X.m = …`, add
//! members to it, and `this.a = …` in a CommonJS file exports `a`.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<CommonjsMembers>()
}

/// Whether CommonJS assignments add members and `this` exports.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CommonjsMembers(pub bool);

/// It has no flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Count `X.prototype.m = …` and `X.m = …` at the top level as members of X, a function or class declared there, and `this.a = …` in a CommonJS file as an export.
    commonjs_members: bool,
}

impl Feature for CommonjsMembers {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_flags: Flags, keys: Keys) -> Self {
        Self(keys.commonjs_members)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.commonjs_members = self.0;
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }
}
