//! `benchmark`: the total for github-action-benchmark, to chart it over time.

use serde_json::json;

use crate::Totals;
use crate::report::{Context, Format};

/// The format.
pub const FORMAT: Format = Format {
    name: "benchmark",
    help: "The total for github-action-benchmark's customBiggerIsBetter",
    render,
};

/// <https://github.com/benchmark-action/github-action-benchmark#examples>, to one decimal place as
/// the other formats show it.
fn render(context: &Context) -> String {
    let percent = (Totals::of(context.files).percent() * 10.0).round() / 10.0;
    json!([{ "name": "docov", "unit": "%", "value": percent }]).to_string() + "\n"
}
