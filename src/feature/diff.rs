//! `--diff-base` and `--diff-threshold`: a threshold for the items on the lines a change added.

use std::{ops::RangeInclusive, path::PathBuf};

use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    FileReport, Totals, config,
    feature::{Entry, Feature, threshold::Limit},
    git,
    root::Root,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Diff>()
}

/// The threshold for the lines a change added, and what the change is from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    base: Option<String>,
    threshold: Option<f64>,
}

/// The lines a change added in each file, by absolute path: `None` for every line.
#[derive(Debug)]
pub struct Lines(FxHashMap<PathBuf, Option<Vec<RangeInclusive<usize>>>>);

impl Diff {
    /// The lines added since HEAD branched from the base, committed or not, when there's a
    /// threshold for them.
    pub fn added(&self, root: &Root) -> Result<Option<Lines>, String> {
        if self.threshold.is_none() {
            return Ok(None);
        }
        let added = git::added_lines(git::base(self.base.as_deref()))?;
        Ok(Some(Lines(added.into_iter().map(|a| (root.absolute(&a.path), a.lines)).collect())))
    }

    /// Why the items on the `added` lines of `reports` fail the threshold.
    pub fn gate(&self, added: Option<&Lines>, reports: &[FileReport], root: &Root) -> Option<String> {
        let (Some(added), Some(threshold)) = (added, self.threshold) else { return None };
        let mut totals = Totals::default();
        for report in reports {
            let Some(lines) = added.0.get(&root.absolute(&report.path)) else { continue };
            for item in &report.items {
                if lines.as_ref().is_none_or(|lines| lines.iter().any(|l| l.contains(&item.line))) {
                    totals.total += 1;
                    totals.documented += usize::from(item.documented);
                }
            }
        }
        Some(format!("changed lines: {}", Limit::Percent(threshold).check(totals, "diff threshold")?))
    }
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// With --diff-threshold, the branch or commit the change is from [default: origin/HEAD, or main]
    #[arg(long, value_name = "REF", requires = "diff_threshold")]
    diff_base: Option<String>,
    /// Fail when the items on lines a change added are under this documented percentage
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    diff_threshold: Option<f64>,
}

/// No keys: the change is up to each run.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct Keys {}

impl Feature for Diff {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, _: Keys) -> Self {
        Self { base: flags.diff_base, threshold: flags.diff_threshold }
    }
}
