//! `--release-tags` and `releaseTagsThreshold`: release-tag coverage, the share of top-level
//! exported items with exactly one of `@public`, `@beta`, `@alpha` and `@internal`, which API
//! Extractor asks for (ae-missing-release-tag). Members take their container's.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    config,
    feature::{Entry, Feature, Run},
    metric::{RELEASE_TAGS, Metric},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<ReleaseTags>()
}

/// Whether the reports have release-tag coverage, and the lowest that passes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReleaseTags {
    /// Whether the reports have it: asked for, or with a threshold.
    pub on: bool,
    /// The lowest percentage of release-tagged items that passes, if there is one.
    pub threshold: Option<f64>,
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Also report how many top-level exports have exactly one of @public, @beta, @alpha and @internal
    #[arg(long)]
    release_tags: bool,
    /// Fail under this percentage of release-tagged top-level exports
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    release_tags_threshold: Option<f64>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// The lowest percentage of top-level exports with exactly one of @public, @beta, @alpha and @internal that passes.
    #[schemars(range(min = 0, max = 100))]
    #[serde(deserialize_with = "config::percentage")]
    release_tags_threshold: Option<f64>,
}

impl Feature for ReleaseTags {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        let threshold = flags.release_tags_threshold.or(keys.release_tags_threshold);
        Self { on: flags.release_tags || threshold.is_some(), threshold }
    }

    fn metric(&self) -> Option<Metric> {
        self.on.then_some(RELEASE_TAGS)
    }

    fn gate(&self, run: &Run) -> Vec<String> {
        RELEASE_TAGS.gate(self.threshold, run.reports).into_iter().collect()
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "on": self.on, "threshold": self.threshold }))
    }
}
