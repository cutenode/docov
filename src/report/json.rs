use std::io::{self, Write};

use serde::Serialize;

use crate::{FileReport, Item, Totals};

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

pub fn write(files: &[FileReport], out: &mut impl Write) -> io::Result<()> {
    let totals = Totals::of(files);
    let files = files
        .iter()
        .map(|f| File {
            path: f.path.display().to_string(),
            totals: f.totals(),
            percent: f.totals().percent(),
            items: f.items.iter().map(JsonItem::of).collect(),
        })
        .collect();
    serde_json::to_writer_pretty(&mut *out, &Report { totals, percent: totals.percent(), files })?;
    writeln!(out)
}
