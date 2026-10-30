//! `--examples`, `--examples-threshold` and `examplesThreshold`: example coverage, the share of
//! counted items whose JSDoc has an `@example`, as a second figure on the text report's lines and a
//! gate. The JSON report always has it.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    FileReport, config,
    feature::{Entry, Feature},
    report::{self, metric::EXAMPLES},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Examples>()
}

/// Whether the text report shows example coverage, and the lowest that passes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Examples {
    /// Whether each of the text report's file lines, and its total, has an examples figure.
    pub text: bool,
    /// The lowest percentage of items with examples that passes, if there is one.
    pub threshold: Option<f64>,
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// On each line of the text report, how many of its items have an @example
    #[arg(long)]
    examples: bool,
    /// Fail under this percentage of items with an @example
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    examples_threshold: Option<f64>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// The lowest percentage of counted items with an @example that passes.
    #[schemars(range(min = 0, max = 100))]
    #[serde(deserialize_with = "config::percentage")]
    examples_threshold: Option<f64>,
}

impl Feature for Examples {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self { text: flags.examples, threshold: flags.examples_threshold.or(keys.examples_threshold) }
    }

    fn report(&self, options: &mut report::Options) {
        options.examples = self.text;
    }

    fn gate(&self, reports: &[FileReport]) -> Option<String> {
        EXAMPLES.gate(self.threshold, reports)
    }
}
