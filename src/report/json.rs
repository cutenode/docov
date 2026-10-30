use std::io::{self, Write};

use serde::{Serialize, Serializer};

use super::{
    Options,
    metric::{EXAMPLES, Metric},
};
use crate::{FileReport, Item, Kind, Totals};

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
}

impl<'a> JsonItem<'a> {
    fn of(item: &'a Item) -> Self {
        let ignore = item.ignore.as_ref().filter(|_| item.ignored());
        Self { item, ignored: ignore.is_some(), reason: ignore.and_then(|i| i.reason.as_deref()) }
    }
}

/// Every item, with totals and each metric's figures: examples, and those `options` add.
pub fn write(files: &[FileReport], options: &Options, out: &mut impl Write) -> io::Result<()> {
    let (totals, kinds) = (Totals::of(files), Totals::by_kind(files));
    let metrics: Vec<Metric> = std::iter::once(EXAMPLES).chain(options.metrics.iter().copied()).collect();
    let of = |count: &dyn Fn(Metric) -> Totals| Metrics(metrics.iter().map(|&m| (m, count(m))).collect());
    let total = of(&|m| m.total(files));
    let files = files
        .iter()
        .map(|f| File {
            path: f.path.display().to_string(),
            totals: f.totals(),
            percent: f.totals().percent(),
            metrics: of(&|m| m.file(f)),
            items: f.items.iter().map(JsonItem::of).collect(),
        })
        .collect();
    serde_json::to_writer_pretty(
        &mut *out,
        &Report { totals, percent: totals.percent(), metrics: total, kinds, files },
    )?;
    writeln!(out)
}
