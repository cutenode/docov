//! Writes scan results in each output format, one module per format.

mod json;
mod lcov;
pub mod metric;
mod text;

use std::{
    io::{self, Write},
    iter::Sum,
};

use rustc_hash::FxHashMap;
use serde::Serialize;

use crate::{FileReport, Kind, feature::sort::Sort};

/// An output format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Each file's percentage, its undocumented items, and the total.
    #[default]
    Text,
    /// Every item, documented or not, with totals.
    Json,
    /// A line record for each line with items, for coverage services and editor gutters.
    Lcov,
}

/// How many items are documented, out of how many.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    /// Items with docs.
    pub documented: usize,
    /// All items.
    pub total: usize,
}

impl Totals {
    /// The documented percentage. Nothing to document is fully documented.
    pub fn percent(self) -> f64 {
        if self.total == 0 { 100.0 } else { self.documented as f64 * 100.0 / self.total as f64 }
    }

    /// The sum over every file.
    pub fn of(files: &[FileReport]) -> Self {
        files.iter().map(FileReport::totals).sum()
    }

    /// The sums for each kind of item there is, in [`Kind`]'s order.
    pub fn by_kind(files: &[FileReport]) -> Vec<(Kind, Self)> {
        let mut kinds: FxHashMap<Kind, Self> = FxHashMap::default();
        for item in files.iter().flat_map(FileReport::counted) {
            let totals = kinds.entry(item.kind).or_default();
            totals.documented += usize::from(item.documented);
            totals.total += 1;
        }
        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.sort_by_key(|(kind, _)| *kind);
        kinds
    }
}

impl Sum for Totals {
    fn sum<I: Iterator<Item = Self>>(totals: I) -> Self {
        totals.fold(Self::default(), |a, b| Self { documented: a.documented + b.documented, total: a.total + b.total })
    }
}

/// How to write a report, past its format: what features ask for.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The order of each file's undocumented items in the text report.
    pub sort: Sort,
    /// Whether the text report's lines have an examples figure.
    pub examples: bool,
}

/// Writes `files` to `out` in `format`.
pub fn write(format: Format, files: &[FileReport], out: &mut impl Write) -> io::Result<()> {
    write_with(format, files, &Options::default(), out)
}

/// Writes `files` to `out` in `format`, with `options`.
pub fn write_with(format: Format, files: &[FileReport], options: &Options, out: &mut impl Write) -> io::Result<()> {
    match format {
        Format::Text => text::write(files, options, out),
        Format::Json => json::write(files, out),
        Format::Lcov => lcov::write(files, out),
    }
}
