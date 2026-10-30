//! `badge`, or `--badge FILE`: an SVG badge of the total, drawn here without a network call.

use crate::report::{Context, Format, color, total};

/// The format.
pub const FORMAT: Format = Format { name: "badge", help: "An SVG badge of the total, colored by the threshold", render };

/// badge.svg, with the total's percentage and color. The label's side is a fixed width; the
/// value's is about 7 pixels a character, which fits Verdana's digits at 11px.
fn render(context: &Context) -> String {
    let message = total(context);
    let value_width = message.len() * 7 + 10;
    include_str!("badge.svg")
        .replace("{width}", &(37 + value_width).to_string())
        .replace("{value_width}", &value_width.to_string())
        .replace("{value_x}", &format!("{}", 37.0 + value_width as f64 / 2.0))
        .replace("{color}", color(context).1)
        .replace("{message}", &message)
}
