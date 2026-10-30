//! `--max-undocumented` and `maxUndocumented`: the most undocumented items that pass.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Totals,
    feature::{Entry, Feature, Run, threshold::Limit},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<MaxUndocumented>()
}

/// The most undocumented items that pass, if there's a limit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MaxUndocumented(pub Option<usize>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Fail with more than this many undocumented items
    #[arg(long, value_name = "N")]
    max_undocumented: Option<usize>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// The most undocumented items that pass.
    max_undocumented: Option<usize>,
}

impl Feature for MaxUndocumented {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self(flags.max_undocumented.or(keys.max_undocumented))
    }

    fn gate(&self, run: &Run) -> Vec<String> {
        let failure = self.0.and_then(|most| Limit::Undocumented(most).check(Totals::of(run.reports), "limit"));
        failure.into_iter().collect()
    }
}
