//! `junit`: JUnit XML, for CI test report views.

use crate::check::Level;
use crate::report::{Context, Format, escape, findings, message, slash};
use crate::{FileReport, Totals};

/// The format.
pub const FORMAT: Format = Format {
    name: "junit",
    help: "JUnit XML: a test per item, and a failed one per doc check error",
    render,
};

/// A test suite per file and a test case per item, which fails when the item is undocumented,
/// each followed by a failed test case per doc check finding at `error` in it. A finding at `warn`
/// doesn't fail, so it goes in its item's `<system-out>`, which CI views show with the test case;
/// properties are for how a suite ran, and few views show them.
fn render(context: &Context) -> String {
    let (tests, failures) = context.files.iter().map(counts).fold((0, 0), |(t, f), (a, b)| (t + a, f + b));
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"docov\" tests=\"{tests}\" failures=\"{failures}\">\n"
    );
    for file in context.files {
        let path = escape(&slash(&file.path));
        let (tests, failures) = counts(file);
        out += &format!("  <testsuite name=\"{path}\" tests=\"{tests}\" failures=\"{failures}\">\n");
        let case = |name: &str, line| {
            format!("<testcase name=\"{}\" classname=\"{path}\" file=\"{path}\" line=\"{line}\"", escape(name))
        };
        for item in file.counted() {
            let name = format!("{} {}", item.kind.as_str(), item.name);
            let (errors, warnings): (Vec<_>, Vec<_>) =
                item.facts.diagnostics.iter().partition(|d| d.severity == Level::Error);
            let mut body = String::new();
            if !item.documented {
                let (message, line, column) = (escape(&message(item)), item.line, item.column);
                body += &format!("      <failure message=\"{message}\" type=\"undocumented\">{path}:{line}:{column}</failure>\n");
            }
            if !warnings.is_empty() {
                let lines: String = warnings.iter().map(|d| escape(&format!("{}:{d}\n", slash(&file.path)))).collect();
                body += &format!("      <system-out>{lines}</system-out>\n");
            }
            out += &if body.is_empty() {
                format!("    {}/>\n", case(&name, item.line))
            } else {
                format!("    {}>\n{body}    </testcase>\n", case(&name, item.line))
            };
            for d in errors {
                let (message, rule, line, column) = (escape(&d.message), d.rule, d.line, d.column);
                out += &format!(
                    "    {}>\n      <failure message=\"{message}\" type=\"{rule}\">{path}:{line}:{column}</failure>\n    </testcase>\n",
                    case(&format!("{name}: {rule}"), d.line)
                );
            }
        }
        out += "  </testsuite>\n";
    }
    out + "</testsuites>\n"
}

/// How many test cases `file` has, and how many of them fail: one per counted item, failed when
/// undocumented, and a failed one per doc check finding at `error`.
fn counts(file: &FileReport) -> (usize, usize) {
    let Totals { documented, total } = file.totals();
    let errors = findings(file).filter(|d| d.severity == Level::Error).count();
    (total + errors, total - documented + errors)
}
