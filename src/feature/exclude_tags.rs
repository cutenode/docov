//! `--exclude-tag` and `excludeTags`: items whose JSDoc has one of these tags don't count.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<ExcludeTags>()
}

/// The JSDoc tags, without their `@`, that take an item out of the total.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExcludeTags(pub Vec<String>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Leave out items whose JSDoc has this tag (`internal` for @internal), repeatable [default: the config's]
    #[arg(long, value_name = "NAME")]
    exclude_tag: Vec<String>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Leave out items whose JSDoc has one of these tags, named without the @ (`internal` for @internal).
    exclude_tags: Vec<String>,
}

impl Feature for ExcludeTags {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self(if flags.exclude_tag.is_empty() { keys.exclude_tags } else { flags.exclude_tag })
    }

    fn rules(&self, rules: &mut Rules) {
        rules.exclude_tags.clone_from(&self.0);
    }
}
