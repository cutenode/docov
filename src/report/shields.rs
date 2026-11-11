//! `shields`: a shields.io endpoint badge's JSON, for a badge of the total from a served file.
//!
//! Publish the file and point a
//! [shields.io endpoint badge](https://shields.io/badges/endpoint-badge) at its URL. Its color is
//! the one `badge` picks.

use serde_json::json;

use crate::report::{Context, Format, color, sorted, total};

/// The format.
pub const FORMAT: Format =
    Format { name: "shields", help: "JSON for a shields.io endpoint badge of the total", render };

/// <https://shields.io/badges/endpoint-badge>
fn render(context: &Context) -> String {
    let badge = json!({ "schemaVersion": 1, "label": "docs", "message": total(context), "color": color(context).0 });
    sorted(badge).to_string() + "\n"
}
