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
    items: &'a [Item],
}

pub fn write(files: &[FileReport], out: &mut impl Write) -> io::Result<()> {
    let totals = Totals::of(files);
    let files = files
        .iter()
        .map(|f| File {
            path: f.path.display().to_string(),
            totals: f.totals(),
            percent: f.totals().percent(),
            items: &f.items,
        })
        .collect();
    serde_json::to_writer_pretty(&mut *out, &Report { totals, percent: totals.percent(), files })?;
    writeln!(out)
}
