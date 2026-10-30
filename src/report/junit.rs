//! `junit`: JUnit XML, for CI test report views.

use crate::Totals;
use crate::report::{Context, Format, escape, message, slash};

/// The format.
pub const FORMAT: Format = Format { name: "junit", help: "JUnit XML: a test per item, failed when undocumented", render };

/// A test suite per file and a test case per item, which fails when the item is undocumented.
fn render(context: &Context) -> String {
    let totals = Totals::of(context.files);
    let (tests, failures) = (totals.total, totals.total - totals.documented);
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"docov\" tests=\"{tests}\" failures=\"{failures}\">\n"
    );
    for file in context.files {
        let totals = file.totals();
        let path = escape(&slash(&file.path));
        let (tests, failures) = (totals.total, totals.total - totals.documented);
        out += &format!("  <testsuite name=\"{path}\" tests=\"{tests}\" failures=\"{failures}\">\n");
        for item in &file.items {
            let name = escape(&format!("{} {}", item.kind.as_str(), item.name));
            let case = format!("<testcase name=\"{name}\" classname=\"{path}\" file=\"{path}\" line=\"{}\"", item.line);
            if item.documented {
                out += &format!("    {case}/>\n");
                continue;
            }
            let (message, line, column) = (escape(&message(item)), item.line, item.column);
            out += &format!("    {case}>\n      <failure message=\"{message}\" type=\"undocumented\">{path}:{line}:{column}</failure>\n    </testcase>\n");
        }
        out += "  </testsuite>\n";
    }
    out + "</testsuites>\n"
}
