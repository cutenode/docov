//! `html`: one self-contained page, with a table of the files and each file's source, its
//! undocumented declarations highlighted and its doc check findings marked.

use std::fs;

use crate::check::Diagnostic;
use crate::report::{Context, Format, escape, findings, lines, severity};
use crate::{FileReport, Totals};

/// The format.
pub const FORMAT: Format =
    Format { name: "html", help: "A page of each file's source, with undocumented items and findings marked", render };

/// The page, styled inline for light and dark and without scripts, so it opens from anywhere.
fn render(context: &Context) -> String {
    let totals = Totals::of(context.files);
    let (percent, documented, total) = (totals.percent(), totals.documented, totals.total);
    let mut out = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\"/>\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\n<title>docov: {percent:.1}% documented</title>\n<style>\n{}</style>\n</head>\n<body>\n<h1>docov</h1>\n<p class=\"total\">{percent:.1}% documented ({documented}/{total})</p>\n",
        include_str!("html.css")
    );
    for failure in context.failures {
        out += &format!("<p class=\"failed\">Failed: {}</p>\n", escape(failure));
    }
    let files: Vec<(usize, &FileReport)> = context.files.iter().enumerate().filter(|(_, f)| f.counted().next().is_some()).collect();
    out += "<table class=\"files\">\n<thead><tr><th class=\"num\">Documented</th><th class=\"num\">Items</th><th>File</th></tr></thead>\n<tbody>\n";
    for (i, file) in &files {
        let totals = file.totals();
        let path = escape(&file.path.display().to_string());
        out += &format!(
            "<tr><td class=\"num\">{:.1}%</td><td class=\"num\">{}/{}</td><td><a href=\"#file-{i}\">{path}</a></td></tr>\n",
            totals.percent(),
            totals.documented,
            totals.total
        );
    }
    out += "</tbody>\n</table>\n";
    for (i, file) in &files {
        let path = escape(&file.path.display().to_string());
        out += &format!("<h2 id=\"file-{i}\">{path} ({:.1}%)</h2>\n{}", file.totals().percent(), source(file));
        let found: String = findings(file)
            .map(|d| format!("<li class=\"{}\">{}</li>\n", severity(d), escape(&format!("{}:{} {}", d.line, d.column, finding(d)))))
            .collect();
        if !found.is_empty() {
            out += &format!("<ul class=\"findings\">\n{found}</ul>\n");
        }
    }
    out + "</body>\n</html>\n"
}

/// What a doc check found, at its level: `warning param-names: …`.
fn finding(d: &Diagnostic) -> String {
    format!("{} {}: {}", severity(d), d.rule, d.message)
}

/// The file's source, a row per line, with the lines that have items marked: missed when one of
/// them is undocumented, hit otherwise. A line with doc check findings is marked too, and its note
/// lists them after its undocumented items. A file that can't be read now is said so, not fatal:
/// the rest of the report still holds, and so does the list of its findings.
fn source(file: &FileReport) -> String {
    let text = match fs::read_to_string(&file.path) {
        Ok(text) => text,
        Err(e) => return format!("<p class=\"missing\">Couldn't read it: {}</p>\n", escape(&e.to_string())),
    };
    let marks = lines(file);
    let mut out = "<div class=\"scroll\">\n<table class=\"source\">\n<tbody>\n".to_owned();
    for (n, line) in text.lines().enumerate().map(|(i, line)| (i + 1, line)) {
        let found: Vec<&Diagnostic> = findings(file).filter(|d| d.line == n).collect();
        let mark = marks.iter().find(|(at, _)| *at == n).map(|(_, hit)| if *hit { "hit" } else { "miss" });
        let classes: Vec<&str> = mark.into_iter().chain((!found.is_empty()).then_some("finding")).collect();
        let mark = if classes.is_empty() { String::new() } else { format!(" class=\"{}\"", classes.join(" ")) };
        let notes: Vec<String> = file
            .counted()
            .filter(|i| i.line == n && !i.documented)
            .map(|i| escape(&format!("{} {}", i.kind.as_str(), i.name)))
            .chain(found.iter().map(|d| escape(&finding(d))))
            .collect();
        out += &format!("<tr{mark}><td class=\"ln\">{n}</td><td>{}</td><td class=\"note\">{}</td></tr>\n", escape(line), notes.join(", "));
    }
    out + "</tbody>\n</table>\n</div>\n"
}
