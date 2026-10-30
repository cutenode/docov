//! `html`: one self-contained page, with a table of the files and each file's source, its
//! undocumented declarations highlighted.

use std::fs;

use crate::report::{Context, Format, escape, lines};
use crate::{FileReport, Totals};

/// The format.
pub const FORMAT: Format =
    Format { name: "html", help: "A page with each file's source, undocumented lines highlighted", render };

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
    let files: Vec<(usize, &FileReport)> = context.files.iter().enumerate().filter(|(_, f)| !f.items.is_empty()).collect();
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
    }
    out + "</body>\n</html>\n"
}

/// The file's source, a row per line, with the lines that have items marked: missed when one of
/// them is undocumented, hit otherwise. A file that can't be read now is said so, not fatal: the
/// rest of the report still holds.
fn source(file: &FileReport) -> String {
    let text = match fs::read_to_string(&file.path) {
        Ok(text) => text,
        Err(e) => return format!("<p class=\"missing\">Couldn't read it: {}</p>\n", escape(&e.to_string())),
    };
    let marks = lines(file);
    let mut out = "<div class=\"scroll\">\n<table class=\"source\">\n<tbody>\n".to_owned();
    for (n, line) in text.lines().enumerate().map(|(i, line)| (i + 1, line)) {
        let mark = marks.iter().find(|(at, _)| *at == n).map_or("", |(_, hit)| if *hit { " class=\"hit\"" } else { " class=\"miss\"" });
        let notes: Vec<String> = file
            .items
            .iter()
            .filter(|i| i.line == n && !i.documented)
            .map(|i| escape(&format!("{} {}", i.kind.as_str(), i.name)))
            .collect();
        out += &format!("<tr{mark}><td class=\"ln\">{n}</td><td>{}</td><td class=\"note\">{}</td></tr>\n", escape(line), notes.join(", "));
    }
    out + "</tbody>\n</table>\n</div>\n"
}
