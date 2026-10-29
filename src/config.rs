//! docov.json (or docov.jsonc, or .docovrc), the settings file.

use std::{env, fs, io, iter, path::Path, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};

/// The files `docov` reads its settings from when no `--config` is given: the first of these in
/// the current folder, or else in the nearest folder above it that has one.
pub const FILE_NAMES: [&str; 3] = ["docov.json", "docov.jsonc", ".docovrc"];

/// Which declarations count toward coverage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// What a module exports, with the public members of exported classes and interfaces.
    #[default]
    Exports,
    /// Every module-level declaration, exported or not.
    All,
}

/// docov's settings. Flags on the command line win over them.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
// Tells editors (VS Code's JSON service reads these) that the file may have comments and trailing
// commas, which `load` strips.
#[schemars(extend("allowComments" = true, "allowTrailingCommas" = true))]
pub struct Config {
    /// This file's JSON Schema, for editors.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    /// Files and folders to scan, relative to this file's folder. Defaults to the folder docov runs in.
    pub paths: Vec<PathBuf>,
    /// Gitignore-style globs of files to leave out, relative to this file's folder, on top of .gitignore.
    pub exclude: Vec<String>,
    /// The lowest documented percentage that passes.
    #[schemars(range(min = 0, max = 100))]
    #[serde(deserialize_with = "threshold")]
    pub threshold: Option<f64>,
    /// Which declarations count.
    pub scope: Scope,
    /// The folder the file is in, as a path from the current folder (empty for the current folder,
    /// or with no file). `paths` and `exclude` are relative to it.
    #[serde(skip)]
    pub dir: PathBuf,
}

/// Parses `--threshold`: a number from 0 to 100. It lives here rather than in the binary so tests
/// can call it: a bad value makes clap exit the process, and on Windows a process that exits that
/// way never writes its coverage.
pub fn percent(value: &str) -> Result<f64, String> {
    // Something that isn't a number fails the range check as NaN.
    in_range(value.parse().unwrap_or(f64::NAN))
}

/// `value`, if it's from 0 to 100. `--threshold` and the config's `threshold` both check with this.
fn in_range(value: f64) -> Result<f64, String> {
    if (0.0..=100.0).contains(&value) { Ok(value) } else { Err("expected a number from 0 to 100".into()) }
}

fn threshold<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Option::<f64>::deserialize(deserializer)?.map(in_range).transpose().map_err(de::Error::custom)
}

/// The nearest of [`FILE_NAMES`], as a path from the current folder (`../docov.json` one folder
/// up).
fn find() -> Option<PathBuf> {
    // This fails only when the current folder is gone, and then there's nothing to find.
    let here = env::current_dir().unwrap_or_default();
    here.ancestors().enumerate().find_map(|(up, dir)| {
        let name = FILE_NAMES.iter().find(|name| dir.join(name).is_file())?;
        Some(iter::repeat_n(Path::new(".."), up).collect::<PathBuf>().join(name))
    })
}

/// Why the config file couldn't be loaded. Each variant's `path` is the file's.
#[derive(Debug, thiserror::Error)]
#[expect(missing_docs, reason = "the variants document their fields")]
pub enum Error {
    /// It couldn't be read.
    #[error("{}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    /// It has a `/` that doesn't start a `//` or `/* */` comment.
    #[error("{}: a `/` that doesn't start a comment", path.display())]
    Slash { path: PathBuf },
    /// It isn't valid JSON, or doesn't match the schema.
    #[error("{}: {source}", path.display())]
    Parse { path: PathBuf, source: serde_json::Error },
}

impl Config {
    /// Reads `path`, or else the nearest of [`FILE_NAMES`], if there is one.
    pub fn load(path: Option<&Path>) -> Result<Self, Error> {
        let Some(path) = path.map(Path::to_path_buf).or_else(find) else { return Ok(Self::default()) };
        let mut text = fs::read_to_string(&path).map_err(|source| Error::Read { path: path.clone(), source })?;
        // Comments and trailing commas turn into spaces, so serde_json's lines and columns still
        // match the file. A stray `/` stops it partway, after blanking that `/`, so it can't go on
        // to serde_json.
        if json_strip_comments::strip(&mut text).is_err() {
            return Err(Error::Slash { path });
        }
        let mut config: Self =
            serde_json::from_str(&text).map_err(|source| Error::Parse { path: path.clone(), source })?;
        config.dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(config)
    }

    /// The JSON Schema for this file, as `docov --print-schema` prints it.
    pub fn json_schema() -> String {
        let schema = schemars::schema_for!(Config);
        serde_json::to_string_pretty(&schema).expect("a schema serializes") + "\n"
    }
}
