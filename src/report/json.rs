//! `json`: every item, documented or not, with totals.

use serde::Serialize;

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
