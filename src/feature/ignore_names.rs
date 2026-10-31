//! `--ignore-name` and `ignore.names`: items whose names match one of these regexes don't count.

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, de};

use crate::{
    Rules,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<IgnoreNames>()
}

/// The regexes that leave out the items whose names match.
#[derive(Clone, Debug, Default)]
pub struct IgnoreNames(pub Vec<Regex>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Leave out items whose name (or member name) matches this regex, repeatable [default: the config's]
    #[arg(long, value_name = "REGEX", value_parser = Regex::new)]
    ignore_name: Vec<Regex>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// What to leave out.
    ignore: Ignore,
}

/// What to leave out.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Ignore {
    /// Regexes: an item whose name matches one doesn't count. A member matches by its own name or as `Class.member`.
    #[serde(deserialize_with = "patterns")]
    #[schemars(with = "Vec<String>")]
    names: Vec<Pattern>,
}

/// A regex, equal to another with the same source.
#[derive(Debug)]
struct Pattern(Regex);

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

fn patterns<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Pattern>, D::Error> {
    let sources = Vec::<String>::deserialize(deserializer)?;
    sources.iter().map(|s| Regex::new(s).map(Pattern).map_err(de::Error::custom)).collect()
}

impl Feature for IgnoreNames {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        let from_keys = || keys.ignore.names.into_iter().map(|p| p.0).collect();
        Self(if flags.ignore_name.is_empty() { from_keys() } else { flags.ignore_name })
    }

    fn rules(&self, rules: &mut Rules) {
        rules.ignore_names.clone_from(&self.0);
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.iter().map(Regex::as_str).collect())
    }
}
