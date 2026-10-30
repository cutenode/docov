//! `json`: every item, documented or not, with totals. `--compare` reads one back.

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::report::{Context, Format, json};
use crate::{Item, Totals};

/// The format.
pub const FORMAT: Format = Format { name: "json", help: "Every item, documented or not, with totals", render };

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    totals: Totals,
    percent: f64,
    files: Vec<File<'a>>,
}

#[derive(Serialize)]
struct File<'a> {
    path: String,
    #[serde(flatten)]
    totals: Totals,
    percent: f64,
    items: &'a [Item],
}

fn render(context: &Context) -> String {
    let totals = Totals::of(context.files);
    let files = context
        .files
        .iter()
        .map(|f| File {
            path: f.path.display().to_string(),
            totals: f.totals(),
            percent: f.totals().percent(),
            items: &f.items,
        })
        .collect();
    json(&Report { totals, percent: totals.percent(), files })
}

/// An earlier `json` report, which `--compare` reads to show what changed.
#[derive(Debug, Deserialize)]
pub struct Base {
    /// The total.
    #[serde(flatten)]
    pub totals: Totals,
    pub(crate) files: Vec<BaseFile>,
}

/// A file in an earlier report.
#[derive(Debug, Deserialize)]
pub(crate) struct BaseFile {
    /// Its path, as `json` writes it.
    pub(crate) path: String,
    #[serde(flatten)]
    pub(crate) totals: Totals,
}

impl Base {
    /// Reads the report at `path`. An error names the file.
    pub fn read(path: &Path) -> Result<Self, String> {
        let failed = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let text = fs::read_to_string(path).map_err(|e| failed(&e))?;
        serde_json::from_str(&text).map_err(|e| failed(&e))
    }
}
