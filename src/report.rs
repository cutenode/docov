//! Writes scan results in each output format, one module per format.

mod json;
mod lcov;
mod text;

use std::io::{self, Write};

use serde::Serialize;

use crate::FileReport;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Each file's percentage, its undocumented items, and the total.
    #[default]
    Text,
    /// Every item, documented or not, with totals.
    Json,
    /// One line record per item, for coverage services and editor gutters.
    Lcov,
}

/// How many items are documented, out of how many.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    pub documented: usize,
    pub total: usize,
}

impl Totals {
    /// The documented percentage. Nothing to document is fully documented.
    pub fn percent(self) -> f64 {
        if self.total == 0 { 100.0 } else { self.documented as f64 * 100.0 / self.total as f64 }
    }

    pub fn of(files: &[FileReport]) -> Self {
        files
            .iter()
            .map(FileReport::totals)
            .fold(Self::default(), |a, b| Self { documented: a.documented + b.documented, total: a.total + b.total })
    }
}

pub fn write(format: Format, files: &[FileReport], out: &mut impl Write) -> io::Result<()> {
    match format {
        Format::Text => text::write(files, out),
        Format::Json => json::write(files, out),
        Format::Lcov => lcov::write(files, out),
    }
}
