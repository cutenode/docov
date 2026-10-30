//! `--complete` and `completeThreshold`: the share of counted items that are complete, documented
//! and, for a function or method, with a matching `@param` for each parameter.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    config,
    feature::{Entry, Feature, Run},
    metric::{COMPLETE, Metric},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Complete>()
}

/// Whether the reports have the complete share, and the lowest that passes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complete {
    /// Whether the reports have it: asked for, or with a threshold.
    pub on: bool,
    /// The lowest percentage of complete items that passes, if there is one.
    pub threshold: Option<f64>,
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Also report how many items are documented with a @param for each parameter
    #[arg(long)]
    complete: bool,
    /// Fail under this percentage of complete items
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    complete_threshold: Option<f64>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// The lowest percentage of counted items documented with a @param for each parameter that passes.
    #[schemars(range(min = 0, max = 100))]
    #[serde(deserialize_with = "config::percentage")]
    complete_threshold: Option<f64>,
}

impl Feature for Complete {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        let threshold = flags.complete_threshold.or(keys.complete_threshold);
        Self { on: flags.complete || threshold.is_some(), threshold }
    }

    fn metric(&self) -> Option<Metric> {
        self.on.then_some(COMPLETE)
    }

    fn gate(&self, run: &Run) -> Vec<String> {
        COMPLETE.gate(self.threshold, run.reports).into_iter().collect()
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "on": self.on, "threshold": self.threshold }))
    }
}
