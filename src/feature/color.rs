//! `--color`: colors the text report where it helps, when stdout is a terminal or it's asked for.

use std::io::{self, Write};

use anstream::{AutoStream, ColorChoice};
use anstyle::{AnsiColor, Style};

use crate::{
    Totals,
    feature::{Entry, Feature, NoKeys},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Color>()
}

/// When to color the report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    /// When stdout is a terminal and `NO_COLOR` isn't set.
    #[default]
    Auto,
    /// Always.
    Always,
    /// Never.
    Never,
}

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// When to color the text report
    #[arg(long, value_enum, default_value_t)]
    color: Color,
}

impl Feature for Color {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        flags.color
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(self).expect("a color choice serializes"))
    }
}

impl Color {
    /// `out`, which drops the colors unless this says to keep them.
    pub fn stream<W: anstream::stream::RawStream>(self, out: W) -> AutoStream<W> {
        let choice = match self {
            Self::Auto => ColorChoice::Auto,
            Self::Always => ColorChoice::Always,
            Self::Never => ColorChoice::Never,
        };
        AutoStream::new(out, choice)
    }
}

/// Colors the text report's lines on their way to `out`: undocumented items dim, and the
/// percentage of a file, package or total under `threshold` red. Other formats go through as
/// they are.
pub struct Styled<'a> {
    // Not generic: coverage counts the best of a generic's copies, and each writer covers some.
    out: &'a mut dyn Write,
    /// Whether it's the text report.
    text: bool,
    threshold: Option<f64>,
    /// What's been written since the last line ended.
    line: Vec<u8>,
}

impl<'a> Styled<'a> {
    /// Colors what's written to `out`, if it's the `text` report.
    pub fn new(out: &'a mut dyn Write, text: bool, threshold: Option<f64>) -> Self {
        Self { out, text, threshold, line: Vec::new() }
    }

    /// Writes one line, without its newline, and the newline.
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        let dim = Style::new().dimmed();
        let red = Style::new().fg_color(Some(AnsiColor::Red.into()));
        // Items are indented past the percentages.
        if line.starts_with("         ") {
            return writeln!(self.out, "{dim}{line}{dim:#}");
        }
        // The other lines start with a percentage and `documented/total`, as `  50.0%  1/2`, which
        // are compared the way the threshold is, unrounded.
        let under = totals(line).zip(self.threshold).is_some_and(|(totals, threshold)| totals.percent() < threshold);
        match line.split_once('%') {
            Some((percent, rest)) if under => writeln!(self.out, "{red}{percent}%{red:#}{rest}"),
            _ => writeln!(self.out, "{line}"),
        }
    }
}

/// The totals a report line starts with, as `  50.0%  1/2`.
fn totals(line: &str) -> Option<Totals> {
    let mut words = line.split_whitespace();
    words.next()?.strip_suffix('%')?;
    let (documented, total) = words.next()?.split_once('/')?;
    Some(Totals { documented: documented.parse().ok()?, total: total.parse().ok()? })
}

impl Write for Styled<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.text {
            return self.out.write(buf);
        }
        self.line.extend_from_slice(buf);
        while let Some(end) = self.line.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.line.drain(..=end).collect();
            self.write_line(&String::from_utf8_lossy(&line[..end]))?;
        }
        Ok(buf.len())
    }

    /// Writes what's left of an unfinished line as it is.
    fn flush(&mut self) -> io::Result<()> {
        self.out.write_all(&std::mem::take(&mut self.line))?;
        self.out.flush()
    }
}
