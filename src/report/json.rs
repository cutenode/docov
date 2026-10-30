//! `json`: every item, documented or not, with totals. `--compare` reads one back.

use std::{fs, path::Path};

use serde::{Deserialize, Serialize, Serializer};

use crate::report::{Context, Format, json};
use crate::{Item, Kind, Totals, packages::Package};

/// The format.
pub const FORMAT: Format = Format { name: "json", help: "Every item, documented or not, with totals", render };

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    totals: Totals,
    percent: f64,
    #[serde(serialize_with = "by_kind")]
    kinds: Vec<(Kind, Totals)>,
    files: Vec<File<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    packages: Option<&'a [Package]>,
}

/// Totals with their percentage.
#[derive(Serialize)]
struct Share {
    #[serde(flatten)]
    totals: Totals,
    percent: f64,
}

/// Each kind's totals, as an object by kind.
fn by_kind<S: Serializer>(kinds: &[(Kind, Totals)], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_map(kinds.iter().map(|&(kind, totals)| (kind, Share { totals, percent: totals.percent() })))
}

#[derive(Serialize)]
struct File<'a> {
    path: String,
    #[serde(flatten)]
    totals: Totals,
    percent: f64,
    items: Vec<JsonItem<'a>>,
}

#[derive(Serialize)]
struct JsonItem<'a> {
    #[serde(flatten)]
    item: &'a Item,
    /// Only when it is, as is its ignore's reason.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    ignored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

impl<'a> JsonItem<'a> {
    fn of(item: &'a Item) -> Self {
        let ignore = item.ignore.as_ref().filter(|_| item.ignored());
        Self { item, ignored: ignore.is_some(), reason: ignore.and_then(|i| i.reason.as_deref()) }
    }
}

fn render(context: &Context) -> String {
    let (totals, kinds) = (Totals::of(context.files), Totals::by_kind(context.files));
    let files = context
        .files
        .iter()
        .map(|f| File {
            path: f.path.display().to_string(),
            totals: f.totals(),
            percent: f.totals().percent(),
            items: f.items.iter().map(JsonItem::of).collect(),
        })
        .collect();
    json(&Report { totals, percent: totals.percent(), kinds, files, packages: context.packages })
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
