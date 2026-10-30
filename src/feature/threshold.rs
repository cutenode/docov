//! `--threshold` and `threshold`: the documented percentage under which the scan fails.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    FileReport, Totals, config,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Threshold>()
}

/// The lowest documented percentage that passes, if there is one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Threshold(pub Option<f64>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Fail under this documented percentage
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    threshold: Option<f64>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// The lowest documented percentage that passes.
    #[schemars(range(min = 0, max = 100))]
    #[serde(deserialize_with = "config::percentage")]
    threshold: Option<f64>,
}

impl Feature for Threshold {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self(flags.threshold.or(keys.threshold))
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }

    fn gate(&self, reports: &[FileReport]) -> Option<String> {
        let percent = Totals::of(reports).percent();
        let threshold = self.0.filter(|&threshold| percent < threshold)?;
        Some(format!("{percent:.1}% documented, under the {threshold}% threshold"))
    }
}
