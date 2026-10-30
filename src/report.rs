//! Output formats. Each lives in its own file in src/report/, which build.rs registers, so adding
//! one adds a file and no line in a shared list.

use std::{
    fs,
    io::{self, Write},
    iter::Sum,
    path::{MAIN_SEPARATOR, Path, PathBuf},
    time::SystemTime,
};

use serde::Serialize;

use crate::{FileReport, Item};

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
fn help() -> String {
    FORMATS.iter().fold(
        "How to write the report, repeatable: a format, or FORMAT=FILE to write it to FILE [default: text]\n".into(),
        |help, format| help + &format!("\n- {}: {}", format.name, format.help),
    )
}

/// What every format reports from.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    /// Each file's items.
    pub files: &'a [FileReport],
    /// Why the scan fails, a message for each failed gate.
    pub failures: &'a [String],
    /// When the scan ran, for formats that say.
    pub now: SystemTime,
}

/// `path` with `/` between its parts, as URLs and CI services want it, on Windows too.
fn slash(path: &Path) -> String {
    path.display().to_string().replace(MAIN_SEPARATOR, "/")
}

/// Each undocumented item, with its file.
fn undocumented(files: &[FileReport]) -> impl Iterator<Item = (&FileReport, &Item)> {
    files.iter().flat_map(|file| file.items.iter().filter(|item| !item.documented).map(move |item| (file, item)))
}

/// `text` with what XML and HTML read as markup escaped, for text and attribute values alike.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// The rule each undocumented item breaks, in formats that name one.
const RULE: &str = "docov/undocumented";

/// `value` as pretty JSON, ending in a newline.
fn json(value: &impl Serialize) -> String {
    serde_json::to_string_pretty(value).expect("a report serializes") + "\n"
}

/// A line record for each line with items, hit when they're all documented. Items come in
/// source order, so those sharing a line are next to each other.
fn lines(file: &FileReport) -> Vec<(usize, bool)> {
    let mut lines: Vec<(usize, bool)> = Vec::new();
    for item in &file.items {
        match lines.last_mut() {
            Some((line, documented)) if *line == item.line => *documented &= item.documented,
            _ => lines.push((item.line, item.documented)),
        }
    }
    lines
}

/// What a finding at an undocumented item says: `function a is undocumented`.
fn message(item: &Item) -> String {
    format!("{} {} is undocumented", item.kind.as_str(), item.name)
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
}

impl Sum for Totals {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |a, b| Self { documented: a.documented + b.documented, total: a.total + b.total })
    }
}

/// A format, and the file it goes to.
#[derive(Clone, Debug)]
pub struct Output {
    /// The format.
    pub format: &'static Format,
    /// The file to write, or stdout.
    pub path: Option<PathBuf>,
}

impl Output {
    /// Parses `--format`: a format's name, then `=FILE` to write it to that file.
    pub fn parse(value: &str) -> Result<Self, String> {
        let Some((name, path)) = value.split_once('=') else { return Ok(Self { format: format(value)?, path: None }) };
        if path.is_empty() {
            return Err(format!("expected a file after `{name}=`"));
        }
        Ok(Self { format: format(name)?, path: Some(path.into()) })
    }
}

/// The flags that pick the formats and where they go.
#[derive(Debug, Default, clap::Args)]
#[group(skip)]
pub struct Flags {
    /// How to write the report, repeatable: a format, or FORMAT=FILE to write it to FILE [default: text]
    #[arg(long, value_name = "FORMAT[=FILE]", value_parser = Output::parse, long_help = help())]
    pub format: Vec<Output>,
    /// Write the one format without a file of its own to FILE
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

impl Flags {
    /// Each format and where it goes: text without a `--format`, and GitHub annotations too when
    /// `github_actions`, which go to stdout for the runner to read. `--output` is the file for the
    /// one format without a file of its own.
    pub fn outputs(&self, github_actions: bool) -> Result<Vec<Output>, String> {
        let text = Output { format: format("text").expect("text is a format"), path: None };
        let mut outputs = if self.format.is_empty() { vec![text] } else { self.format.clone() };
        if let Some(file) = &self.output {
            let mut unnamed = outputs.iter_mut().filter(|output| output.path.is_none());
            let (Some(output), None) = (unnamed.next(), unnamed.next()) else {
                return Err("--output needs exactly one --format without a file".into());
            };
            output.path = Some(file.clone());
        }
        if self.format.is_empty() && github_actions {
            outputs.push(Output { format: format("github").expect("github is a format"), path: None });
        }
        Ok(outputs)
    }
}

/// Writes each output to its file, making the file's folder if it has to, or else to `stdout`.
/// One scan feeds them all.
pub fn write(outputs: &[Output], context: &Context, stdout: &mut dyn Write) -> Result<(), String> {
    for output in outputs {
        let text = output.format.render(context);
        let Some(path) = &output.path else {
            stdout.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            continue;
        };
        let failed = |e: io::Error| format!("{}: {e}", path.display());
        path.parent().map_or(Ok(()), fs::create_dir_all).map_err(failed)?;
        fs::write(path, text).map_err(failed)?;
    }
    Ok(())
}
