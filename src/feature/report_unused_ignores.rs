//! `--report-unused-ignores` and `reportUnusedIgnores`: a `docov-ignore` comment that changes
//! nothing fails the scan.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules,
    collect::Ignore,
    feature::{Entry, Feature, Run},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<ReportUnusedIgnores>()
}

/// Whether a `docov-ignore` comment that changes nothing fails the scan: one that covers no item, or
/// only documented ones or ones another comment covers too.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReportUnusedIgnores(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Fail on a docov-ignore comment that changes nothing
    #[arg(long)]
    report_unused_ignores: bool,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Fail on a docov-ignore comment that changes nothing.
    report_unused_ignores: bool,
}

impl Feature for ReportUnusedIgnores {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self(flags.report_unused_ignores || keys.report_unused_ignores)
    }

    fn rules(&self, rules: &mut Rules) {
        rules.unused_ignores = self.0;
    }

    fn gate(&self, run: &Run) -> Vec<String> {
        let mut unused = Vec::new();
        for report in run.reports {
            let path = report.path.display();
            let line = |ignore: &Ignore| format!("{path}:{}:{}: unused docov-ignore", ignore.line, ignore.column);
            unused.extend(report.unused_ignores.iter().map(line));
        }
        unused
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }
}
