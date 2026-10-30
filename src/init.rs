//! `docov init`: writes a docov.json that holds coverage where it is.

use std::{fs, path::Path};

use crate::{Rules, Totals, batch, cache::Cache, config::FILE_NAMES, walk};

/// The `$schema` of this version's docov.json, pinned to its release.
pub const SCHEMA_URL: &str =
    concat!("https://raw.githubusercontent.com/cutenode/docov/v", env!("CARGO_PKG_VERSION"), "/docov.schema.json");

/// Writes docov.json in `dir` with [`SCHEMA_URL`] and a threshold of the folder's coverage with
/// the default settings, rounded down, and returns the threshold. Fails if `dir` has a config file
/// already, or a file there doesn't scan.
pub fn init(dir: &Path) -> Result<u8, Vec<String>> {
    if let Some(name) = FILE_NAMES.iter().find(|name| dir.join(name).is_file()) {
        return Err(vec![format!("{name} is already there")]);
    }
    let files = walk::files(&[dir.to_owned()], |_, _| false).map_err(|e| vec![e.to_string()])?;
    let rules = Rules::default();
    let files: Vec<_> = files.iter().map(|file| (file.as_path(), &rules)).collect();
    let reports = batch::scan(&files, &Cache::default())?;
    // A percentage, from 0 to 100.
    let threshold = Totals::of(&reports).percent().floor() as u8;
    let json = serde_json::json!({ "$schema": SCHEMA_URL, "threshold": threshold });
    let text = serde_json::to_string_pretty(&json).expect("JSON serializes") + "\n";
    fs::write(dir.join("docov.json"), text).map_err(|e| vec![format!("docov.json: {e}")])?;
    Ok(threshold)
}
