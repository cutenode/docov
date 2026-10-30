//! Output formats. Each lives in its own file in src/report/, which build.rs registers, so adding
//! one adds a file and no line in a shared list.

use std::{
    fs,
    io::{self, Write},
    iter::Sum,
    path::{MAIN_SEPARATOR, Path, PathBuf},
    time::SystemTime,
};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    FileReport, Item, Kind,
    feature::{color::Styled, sort::Sort},
    metric::Metric,
    packages::Package,
};

include!(concat!(env!("OUT_DIR"), "/formats.rs"));

pub use json::Base;

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
    /// The global threshold, when it is a percentage, which badges and `--color` color by.
    pub threshold: Option<f64>,
    /// How much `text` shows.
    pub text: Text,
    /// The earlier report `--compare` names, which `markdown` shows the change from.
    pub base: Option<&'a Base>,
    /// Each workspace package's totals, with `--by-package`, which `text` and `json` add.
    pub packages: Option<&'a [Package]>,
    /// Whether `text` is colored on stdout, which drops the colors where they don't belong.
    pub color: bool,
    /// The order of each file's undocumented items in `text`.
    pub sort: Sort,
    /// Whether `text`'s lines say how many of their items have an `@example`.
    pub examples: bool,
    /// The metrics past coverage and examples that `text` and `json` add, from the features.
    pub metrics: &'a [Metric],
}

/// How much `text` shows.
#[derive(Clone, Copy, Debug, Default, clap::Args)]
#[group(skip)]
pub struct Text {
    /// Leave fully documented files out of the text report
    #[arg(long)]
    pub omit_covered: bool,
    /// Print only the total, and why the scan fails, in the text report
    #[arg(long)]
    pub quiet: bool,
    /// After the text report's total, a line of totals by kind
    #[arg(long)]
    pub by_kind: bool,
}

/// `path` with `/` between its parts, as URLs and CI services want it, on Windows too.
fn slash(path: &Path) -> String {
    path.display().to_string().replace(MAIN_SEPARATOR, "/")
}

/// Each undocumented item that counts, with its file.
fn undocumented(files: &[FileReport]) -> impl Iterator<Item = (&FileReport, &Item)> {
    files.iter().flat_map(|file| file.counted().filter(|item| !item.documented).map(move |item| (file, item)))
}

/// `text` with what XML and HTML read as markup escaped, for text and attribute values alike.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// The rule each undocumented item breaks, in formats that name one.
const RULE: &str = "docov/undocumented";

/// `value` as pretty JSON, ending in a newline, with its keys in order.
fn json(value: Value) -> String {
    serde_json::to_string_pretty(&sorted(value)).expect("a report serializes") + "\n"
}

/// `value` with every object's keys in order, whether or not serde_json keeps them in the order
/// they were added (a dependency turns that on for the whole build), so what docov prints from a
/// JSON value doesn't change with the build.
pub(crate) fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().map(|(key, value)| (key, sorted(value))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(sorted).collect()),
        value => value,
    }
}

/// A line record for each line with counted items, hit when they're all documented. Items come
/// in source order, so those sharing a line are next to each other.
fn lines(file: &FileReport) -> Vec<(usize, bool)> {
    let mut lines: Vec<(usize, bool)> = Vec::new();
    for item in file.counted() {
        match lines.last_mut() {
            Some((line, documented)) if *line == item.line => *documented &= item.documented,
            _ => lines.push((item.line, item.documented)),
        }
    }
    lines
}

/// The total's percentage as badges show it: `66.7%`.
fn total(context: &Context) -> String {
    format!("{:.1}%", Totals::of(context.files).percent())
}

/// A badge's color, as shields.io names it and in hex: red under the threshold, yellow up to 10
/// points over it, green past that, and blue with no threshold to compare with.
fn color(context: &Context) -> (&'static str, &'static str) {
    let percent = Totals::of(context.files).percent();
    match context.threshold {
        None => ("blue", "#007ec6"),
        Some(threshold) if percent < threshold => ("red", "#e05d44"),
        Some(threshold) if percent <= threshold + 10.0 => ("yellow", "#dfb317"),
        Some(_) => ("green", "#97ca00"),
    }
}

/// What a finding at an undocumented item says: `function a is undocumented`.
fn message(item: &Item) -> String {
    format!("{} {} is undocumented", item.kind.as_str(), item.name)
}

/// How many items are documented, out of how many.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

    /// The sums for each kind of item there is, in [`Kind`]'s order.
    pub fn by_kind(files: &[FileReport]) -> Vec<(Kind, Self)> {
        let mut kinds: FxHashMap<Kind, Self> = FxHashMap::default();
        for item in files.iter().flat_map(FileReport::counted) {
            let totals = kinds.entry(item.kind).or_default();
            totals.documented += usize::from(item.documented);
            totals.total += 1;
        }
        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.sort_by_key(|(kind, _)| *kind);
        kinds
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
    /// Write an SVG badge of the total to FILE, as `--format badge=FILE` does
    #[arg(long, value_name = "FILE")]
    pub badge: Option<PathBuf>,
    /// Show the change from FILE, an earlier `--format json` report, in the markdown report
    #[arg(long, value_name = "FILE")]
    pub compare: Option<PathBuf>,
    #[expect(missing_docs, reason = "clap shows the fields' docs")]
    #[command(flatten)]
    pub text: Text,
}

impl Flags {
    /// Each format and where it goes: text without a `--format`, and GitHub annotations too when
    /// `github_actions`, which go to stdout for the runner to read. `--output` is the file for the
    /// one format without a file of its own, and `--badge` adds a badge.
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
        let badge = format("badge").expect("badge is a format");
        outputs.extend(self.badge.iter().map(|path| Output { format: badge, path: Some(path.clone()) }));
        Ok(outputs)
    }
}

/// Writes each output to its file, making the file's folder if it has to, or else to `stdout`.
/// One scan feeds them all.
pub fn write(outputs: &[Output], context: &Context, stdout: &mut dyn Write) -> Result<(), String> {
    for output in outputs {
        let text = output.format.render(context);
        let Some(path) = &output.path else {
            let styled = context.color && output.format.name == text::FORMAT.name;
            let written = if styled {
                Styled::new(&mut *stdout, context.threshold).write_all(text.as_bytes())
            } else {
                stdout.write_all(text.as_bytes())
            };
            written.map_err(|e| e.to_string())?;
            continue;
        };
        let failed = |e: io::Error| format!("{}: {e}", path.display());
        path.parent().map_or(Ok(()), fs::create_dir_all).map_err(failed)?;
        fs::write(path, text).map_err(failed)?;
    }
    Ok(())
}
