//! Metrics past coverage: of the items a metric is over, how many have what it measures.

use crate::{FileReport, Item, Totals};

/// What share of some items have something, such as an `@example`.
#[derive(Clone, Copy, Debug)]
pub struct Metric {
    /// What it measures, as its messages name it: `examples`.
    pub name: &'static str,
    /// The JSON report's keys for how many items have it and their percentage.
    pub keys: [&'static str; 2],
    /// Whether a counted item is one it's over.
    pub over: fn(&Item) -> bool,
    /// Whether an item it's over has what it measures.
    pub has: fn(&Item) -> bool,
}

/// The share of counted items with an `@example`, as rustdoc's `--show-coverage` counts them.
pub const EXAMPLES: Metric = Metric {
    name: "examples",
    keys: ["withExamples", "examplesPercent"],
    over: |_| true,
    has: |i| i.facts.has_example,
};

impl Metric {
    /// How many of `file`'s items it's over have it (`documented`), out of how many (`total`).
    pub fn file(self, file: &FileReport) -> Totals {
        let over: Vec<&Item> = file.counted().filter(|i| (self.over)(i)).collect();
        Totals { documented: over.iter().filter(|i| (self.has)(i)).count(), total: over.len() }
    }

    /// The same, summed over every file.
    pub fn total(self, files: &[FileReport]) -> Totals {
        files.iter().map(|f| self.file(f)).sum()
    }

    /// Why the scan fails, when its percentage is under `threshold`.
    pub fn gate(self, threshold: Option<f64>, files: &[FileReport]) -> Option<String> {
        let percent = self.total(files).percent();
        let threshold = threshold.filter(|&threshold| percent < threshold)?;
        Some(format!("{percent:.1}% with {name}, under the {threshold}% {name} threshold", name = self.name))
    }
}
