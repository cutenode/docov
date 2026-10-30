//! `--update-threshold`: raises docov.json's threshold to the coverage the scan found, so it
//! ratchets up and never down.

use std::{fs, io, path::Path};

use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstRootNode},
};

use crate::{
    Totals,
    config::FILE_NAMES,
    feature::{Entry, Feature, NoKeys},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<UpdateThreshold>()
}

/// Whether to raise docov.json's threshold.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UpdateThreshold(pub bool);

impl UpdateThreshold {
    /// Writes the documented percentage of `totals`, rounded down to a tenth, as the global
    /// threshold in the config file at `path` (or a new docov.json here) when it's over the one
    /// there. Says what it did, if anything.
    pub fn update(self, path: Option<&Path>, totals: Totals) -> Result<Option<String>, String> {
        if !self.0 {
            return Ok(None);
        }
        // In tenths, so rounding down is exact. Nothing to document is fully documented.
        let tenths = (totals.documented * 1000).checked_div(totals.total).unwrap_or(1000);
        let percent = match tenths % 10 {
            0 => (tenths / 10).to_string(),
            tenth => format!("{}.{tenth}", tenths / 10),
        };
        let file = path.unwrap_or(Path::new(FILE_NAMES[0]));
        let text = path.map_or_else(|| Ok("{}\n".to_owned()), fs::read_to_string);
        let written = text.and_then(|text| match raise(&text, tenths, &percent) {
            Some(raised) => fs::write(file, raised).map(|()| true),
            None => Ok(false),
        });
        let written = written.map_err(|e: io::Error| format!("{}: {e}", file.display()))?;
        Ok(written.then(|| format!("raised the threshold to {percent}% in {}", file.display())))
    }
}

/// `text` with its global threshold set to `percent` (`tenths` tenths of a percent), if that
/// raises it. Only that number changes, so comments and formatting stay.
fn raise(text: &str, tenths: usize, percent: &str) -> Option<String> {
    // Config::load read it already, and jsonc-parser reads everything serde_json does once
    // comments are blanked, and comments too.
    let root = CstRootNode::parse(text, &ParseOptions::default()).expect("the config loaded");
    let config = root.object_value_or_set();
    // An object of thresholds keeps its others and changes `global`.
    let (object, key) = match config.object_value("threshold") {
        Some(thresholds) => (thresholds, "global"),
        None => (config, "threshold"),
    };
    let property = object.get(key);
    let number = property.as_ref().and_then(|p| p.value()).and_then(|value| value.as_number_lit());
    let current = number.and_then(|number| number.to_string().parse::<f64>().ok());
    // A negative threshold counts undocumented items, which a percentage can't replace. A tenth
    // divided by 10 rounds to the same float as the number written out.
    if current.is_some_and(|current| current < 0.0 || current >= tenths as f64 / 10.0) {
        return None;
    }
    let value = CstInputValue::Number(percent.to_owned());
    match property {
        Some(property) => property.set_value(value),
        None => {
            object.append(key, value);
        }
    }
    Some(root.to_string())
}

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Raise the config's threshold to the documented percentage, rounded down, when that's higher
    #[arg(long)]
    update_threshold: bool,
}


impl Feature for UpdateThreshold {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.update_threshold)
    }
}
