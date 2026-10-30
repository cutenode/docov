//! `--baseline`, `--write-baseline` and `baseline`: undocumented items the gates let pass.

use std::{
    fs,
    path::{Path, PathBuf},
};

use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    FileReport,
    feature::{Entry, Feature},
    report,
    root::Root,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Baseline>()
}

/// The file docov writes a baseline to when nothing names one, in the config's folder.
const DEFAULT: &str = "docov-baseline.json";

/// Where the baseline is, and whether to write it rather than read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Baseline {
    /// From `--baseline`, a path from the current folder.
    flag: Option<PathBuf>,
    /// From docov.json, a path from its folder.
    key: Option<PathBuf>,
    /// `--write-baseline`.
    pub write: bool,
}

/// A baseline file: undocumented items the gates let pass.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Baseline")]
pub struct File {
    /// This file's JSON Schema, for editors.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The items. Each entry lets one item with its path, kind and name through.
    pub items: Vec<Known>,
}

/// An undocumented item, by what stays the same when lines move.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Known {
    /// Its file, relative to the config's folder, with `/` between folders on every OS.
    pub path: String,
    /// Its kind, as reports name it: `function`, `class`, `method` and so on.
    pub kind: String,
    /// Its name, as reports give it.
    pub name: String,
}

impl Known {
    fn new(root: &Root, path: &Path, item: &crate::Item) -> Self {
        let path = root.relative(path);
        let path = path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        Self { path, kind: item.kind.as_str().to_owned(), name: item.name.clone() }
    }
}

impl File {
    /// The JSON Schema of a baseline file, as `docov --print-schema baseline` prints it.
    pub fn json_schema() -> String {
        serde_json::to_string_pretty(&report::sorted(schemars::schema_for!(File).to_value())).expect("a schema serializes") + "\n"
    }
}

/// The baseline's items, and how many times each is in it.
#[derive(Debug, Default)]
pub struct Excused(FxHashMap<Known, usize>);

impl Excused {
    /// `reports`, with the items in the baseline counted as documented, each entry once.
    pub fn apply(&self, root: &Root, mut reports: Vec<FileReport>) -> Vec<FileReport> {
        let mut left = self.0.clone();
        for report in &mut reports {
            // An ignored item is out of the total, and counting it as documented would bring it back.
            for item in report.items.iter_mut().filter(|i| !i.documented && i.ignore.is_none()) {
                if let Some(count) = left.get_mut(&Known::new(root, &report.path, item)).filter(|n| **n > 0) {
                    *count -= 1;
                    item.documented = true;
                }
            }
        }
        reports
    }
}

impl Baseline {
    /// The baseline file, as a path from the current folder: `--baseline`, else docov.json's.
    fn path(&self, dir: &Path) -> Option<PathBuf> {
        self.flag.clone().or_else(|| self.key.as_ref().map(|key| dir.join(key)))
    }

    /// The baseline's items, if there is one to read. `dir` is the config's folder.
    pub fn read(&self, dir: &Path) -> Result<Option<Excused>, String> {
        let Some(path) = self.path(dir).filter(|_| !self.write) else { return Ok(None) };
        let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let text = fs::read_to_string(&path).map_err(|e| error(&e))?;
        let file: File = serde_json::from_str(&text).map_err(|e| error(&e))?;
        let mut excused = Excused::default();
        for known in file.items {
            *excused.0.entry(known).or_default() += 1;
        }
        Ok(Some(excused))
    }

    /// The file `--write-baseline` writes, as a path from the current folder. `dir` is the
    /// config's folder.
    pub fn file(&self, dir: &Path) -> PathBuf {
        self.path(dir).unwrap_or_else(|| dir.join(DEFAULT))
    }

    /// Writes each undocumented item that counts in `reports` to the baseline file, and says where.
    pub fn save(&self, dir: &Path, root: &Root, reports: &[FileReport]) -> Result<PathBuf, String> {
        let path = self.file(dir);
        let items = reports
            .iter()
            .flat_map(|r| r.counted().filter(|i| !i.documented).map(|i| Known::new(root, &r.path, i)))
            .collect();
        let schema = format!(
            "https://raw.githubusercontent.com/cutenode/docov/v{}/docov-baseline.schema.json",
            env!("CARGO_PKG_VERSION")
        );
        let text = serde_json::to_string_pretty(&File { schema: Some(schema), items }).expect("a baseline serializes");
        fs::write(&path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// A file of undocumented items the gates let pass [default: the config's `baseline`]
    #[arg(long, value_name = "FILE")]
    baseline: Option<PathBuf>,
    /// Write every undocumented item to the baseline file and exit 0
    #[arg(long)]
    write_baseline: bool,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// A file of undocumented items the gates let pass, relative to this file's folder, as `docov --write-baseline` writes it.
    baseline: Option<PathBuf>,
}

impl Feature for Baseline {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self { flag: flags.baseline, key: keys.baseline, write: flags.write_baseline }
    }

    fn describe(&self) -> Option<serde_json::Value> {
        let path = self.flag.as_ref().or(self.key.as_ref()).map(|path| path.display().to_string());
        Some(serde_json::json!({ "path": path, "write": self.write }))
    }
}
