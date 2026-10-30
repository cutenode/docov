//! `--by-package`: adds each workspace package's totals to the report.

use crate::feature::{Entry, Feature, NoKeys};

pub(crate) fn entry() -> Entry {
    Entry::of::<ByPackage>()
}

/// Whether the report has each package's totals.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ByPackage(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Add each workspace package's total to the text and JSON reports
    #[arg(long)]
    by_package: bool,
}

impl Feature for ByPackage {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.by_package)
    }
}
