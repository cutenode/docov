//! `json`: every item, documented or not, with totals. `--compare` reads one back.

use std::{fs, path::Path};

use serde::{Deserialize, Serialize, Serializer};

use crate::report::{Context, Format};
use crate::{
    Item, Kind, Totals,
    check::Diagnostic,
    metric::{EXAMPLES, Metric},
    packages::Package,
};

/// The format.
pub const FORMAT: Format = Format { name: "json", help: "Every item, documented or not, with totals", render };

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    totals: Totals,
    percent: f64,
    #[serde(flatten)]
    metrics: Metrics,
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

/// Each metric's count and percentage, under its keys.
struct Metrics(Vec<(Metric, Totals)>);

impl Serialize for Metrics {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().flat_map(|(metric, totals)| {
            let [count, percent] = metric.keys;
            [(count, serde_json::Value::from(totals.documented)), (percent, totals.percent().into())]
        }))
    }
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
    #[serde(flatten)]
    metrics: Metrics,
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
    /// What the doc checks found, when they found something, which in an ignored item they don't.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    diagnostics: &'a [Diagnostic],
}

impl<'a> JsonItem<'a> {
    fn of(item: &'a Item) -> Self {
        let ignore = item.ignore.as_ref().filter(|_| item.ignored());
        let diagnostics = if ignore.is_some() { &[] } else { &*item.facts.diagnostics };
        Self { item, ignored: ignore.is_some(), reason: ignore.and_then(|i| i.reason.as_deref()), diagnostics }
    }
}

/// Every item, with totals and each metric's figures: examples, and those the features add.
fn render(context: &Context) -> String {
    let (totals, kinds) = (Totals::of(context.files), Totals::by_kind(context.files));
    let metrics: Vec<Metric> = std::iter::once(EXAMPLES).chain(context.metrics.iter().copied()).collect();
    let of = |count: &dyn Fn(Metric) -> Totals| Metrics(metrics.iter().map(|&m| (m, count(m))).collect());
    let total = of(&|m| m.total(context.files));
    let files = context
        .files
        .iter()
        .map(|f| File {
            path: f.path.display().to_string(),
            totals: f.totals(),
            percent: f.totals().percent(),
            metrics: of(&|m| m.file(f)),
            items: f.items.iter().map(JsonItem::of).collect(),
        })
        .collect();
    let report = Report { totals, percent: totals.percent(), metrics: total, kinds, files, packages: context.packages };
    serde_json::to_string_pretty(&report).expect("a report serializes") + "\n"
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
