//! `--report-unused-ignores` and `reportUnusedIgnores`: a `docov-ignore-next-line` over documented
//! items alone fails the scan.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::feature::{Entry, Feature, Run};

pub(crate) fn entry() -> Entry {
    Entry::of::<ReportUnusedIgnores>()
}

/// Whether a `docov-ignore-next-line` that covers only documented items fails the scan.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReportUnusedIgnores(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Fail on a docov-ignore-next-line whose items are documented anyway
    #[arg(long)]
    report_unused_ignores: bool,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// Fail on a docov-ignore-next-line whose items are documented anyway.
    report_unused_ignores: bool,
}

impl Feature for ReportUnusedIgnores {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self(flags.report_unused_ignores || keys.report_unused_ignores)
    }

    fn gate(&self, run: &Run) -> Vec<String> {
        if !self.0 {
            return Vec::new();
        }
        let mut unused = Vec::new();
        for report in run.reports {
            let covered: Vec<_> = report
                .items
                .iter()
                .filter_map(|i| i.ignore.as_ref().filter(|ignore| !ignore.file).map(|ignore| (ignore, i.documented)))
                .collect();
            // A comment's items are on the line after it, so they're next to each other.
            for items in covered.chunk_by(|a, b| a.0 == b.0) {
                if items.iter().all(|(_, documented)| *documented) {
                    let (path, ignore) = (report.path.display(), items[0].0);
                    unused.push(format!("{path}:{}:{}: unused docov-ignore", ignore.line, ignore.column));
                }
            }
        }
        unused
    }
}
