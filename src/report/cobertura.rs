//! `cobertura`: Cobertura XML, for GitLab's merge request coverage and other CI coverage views.

use std::{path::Path, time::UNIX_EPOCH};

use crate::report::{Context, Format, escape, lines, slash};
use crate::{FileReport, Totals};

/// The format.
pub const FORMAT: Format =
    Format { name: "cobertura", help: "Cobertura XML: the lcov line records, a package per folder", render };

/// A file and its line records.
type Lines<'a> = (&'a FileReport, Vec<(usize, bool)>);

/// The line records lcov writes, in coverage-04.dtd's shape: a package per folder and a class per
/// file. There are no branches or methods to count.
fn render(context: &Context) -> String {
    let mut files: Vec<Lines> = context.files.iter().map(|file| (file, lines(file))).collect();
    // The walk sorts files by path, which can put another folder's files between a folder's own.
    // The sort is stable, so each folder's files stay in order.
    files.sort_by(|a, b| a.0.path.parent().cmp(&b.0.path.parent()));
    let totals: Totals = files.iter().map(|(_, lines)| hits(lines)).sum();
    let timestamp = context.now.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis());
    let mut out = format!(
        "<?xml version=\"1.0\" ?>\n<!DOCTYPE coverage SYSTEM \"http://cobertura.sourceforge.net/xml/coverage-04.dtd\">\n<coverage line-rate=\"{}\" branch-rate=\"0\" lines-covered=\"{}\" lines-valid=\"{}\" branches-covered=\"0\" branches-valid=\"0\" complexity=\"0\" version=\"{}\" timestamp=\"{timestamp}\">\n  <sources>\n    <source>.</source>\n  </sources>\n  <packages>\n",
        rate(totals),
        totals.documented,
        totals.total,
        env!("CARGO_PKG_VERSION"),
    );
    for package in files.chunk_by(|a, b| a.0.path.parent() == b.0.path.parent()) {
        let folder = slash(package[0].0.path.parent().unwrap_or(Path::new("")));
        let name = if folder.is_empty() { ".".into() } else { escape(&folder) };
        let package_rate = rate(package.iter().map(|(_, lines)| hits(lines)).sum());
        out += &format!("    <package name=\"{name}\" line-rate=\"{package_rate}\" branch-rate=\"0\" complexity=\"0\">\n      <classes>\n");
        for (file, lines) in package {
            let name = escape(&file.path.file_name().unwrap_or_default().to_string_lossy());
            let (path, rate) = (escape(&slash(&file.path)), rate(hits(lines)));
            out += &format!("        <class name=\"{name}\" filename=\"{path}\" line-rate=\"{rate}\" branch-rate=\"0\" complexity=\"0\">\n          <methods/>\n          <lines>\n");
            for (line, hit) in lines {
                out += &format!("            <line number=\"{line}\" hits=\"{}\"/>\n", u8::from(*hit));
            }
            out += "          </lines>\n        </class>\n";
        }
        out += "      </classes>\n    </package>\n";
    }
    out + "  </packages>\n</coverage>\n"
}

/// The lines hit, out of all of them.
fn hits(lines: &[(usize, bool)]) -> Totals {
    Totals { documented: lines.iter().filter(|(_, hit)| *hit).count(), total: lines.len() }
}

/// A line rate, from 0 to 1.
fn rate(totals: Totals) -> f64 {
    totals.percent() / 100.0
}
