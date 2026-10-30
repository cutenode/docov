//! `--types` and `typesThreshold`: type coverage in JavaScript, the share of functions and methods
//! in `.js`, `.mjs`, `.cjs` and `.jsx` files whose JSDoc types each `@param`, and its `@returns`
//! when it has one.

use std::io::{self, Write};

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    FileReport, Format, config,
    feature::{Entry, Feature},
    report::{self, metric::TYPES},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Types>()
}

/// Whether the reports have type coverage, and the lowest that passes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Types {
    /// Whether the reports have it: asked for, or with a threshold.
    pub on: bool,
    /// The lowest percentage of typed functions that passes, if there is one.
    pub threshold: Option<f64>,
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Also report how many JavaScript functions have a {type} on each @param and @returns
    #[arg(long)]
    types: bool,
    /// Fail under this percentage of typed JavaScript functions
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    types_threshold: Option<f64>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// The lowest percentage of JavaScript functions and methods with a {type} on each @param and @returns that passes.
    #[schemars(range(min = 0, max = 100))]
    #[serde(deserialize_with = "config::percentage")]
    types_threshold: Option<f64>,
}

impl Feature for Types {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        let threshold = flags.types_threshold.or(keys.types_threshold);
        Self { on: flags.types || threshold.is_some(), threshold }
    }

    fn report(&self, options: &mut report::Options) {
        options.metrics.extend(self.on.then_some(TYPES));
    }

    fn gate(&self, reports: &[FileReport]) -> Option<String> {
        TYPES.gate(self.threshold, reports)
    }

    fn append(&self, format: Format, reports: &[FileReport], out: &mut dyn Write) -> io::Result<()> {
        if !self.on || format != Format::Text {
            return Ok(());
        }
        TYPES.line(reports, out)
    }
}
