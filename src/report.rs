//! Output formats. Each lives in its own file in src/report/, which build.rs registers, so adding
//! one adds a file and no line in a shared list.

use std::io::{self, Write};

use serde::Serialize;

use crate::FileReport;

include!(concat!(env!("OUT_DIR"), "/formats.rs"));

/// An output format. Register one by giving its file a `pub const FORMAT: Format`.
#[derive(Debug)]
pub struct Format {
    /// Its `--format` name.
    pub name: &'static str,
    /// What it writes, in a line, for `--help`.
    pub help: &'static str,
    render: fn(&Context) -> String,
}

impl Format {
    /// The report in this format.
    pub fn render(&self, context: &Context) -> String {
        (self.render)(context)
    }
}

/// The format named `name`. It parses `--format`.
pub fn format(name: &str) -> Result<&'static Format, String> {
    FORMATS.iter().find(|format| format.name == name).ok_or_else(|| {
        let names: Vec<_> = FORMATS.iter().map(|format| format.name).collect();
        format!("unknown format `{name}`, expected one of: {}", names.join(", "))
    })
}

/// `--format`'s help: each format and what it writes.
pub fn help() -> String {
    FORMATS.iter().fold("How to write the report\n".into(), |help, format| {
        help + &format!("\n- {}: {}", format.name, format.help)
    })
}

/// What every format reports from.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    /// Each file's items.
    pub files: &'a [FileReport],
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
        files
            .iter()
            .map(FileReport::totals)
            .fold(Self::default(), |a, b| Self { documented: a.documented + b.documented, total: a.total + b.total })
    }
}

/// Writes the report in `format` to `out`.
pub fn write(format: &Format, context: &Context, out: &mut impl Write) -> io::Result<()> {
    out.write_all(format.render(context).as_bytes())
}
