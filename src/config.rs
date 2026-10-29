use std::{fs, io, path::Path, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The file `docov` reads its settings from when no `--config` is given, if it's there.
pub const DEFAULT_PATH: &str = "docov.json";

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
pub struct Config {
    /// This file's JSON Schema, for editors.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    /// Files and folders to scan, relative to where docov runs. Defaults to the current folder.
    pub paths: Vec<PathBuf>,
    /// Gitignore-style globs of files to leave out, on top of what .gitignore leaves out.
    pub exclude: Vec<String>,
    /// The lowest documented percentage that passes.
    #[schemars(range(min = 0, max = 100))]
    pub threshold: Option<f64>,
    /// Which declarations count.
    pub scope: Scope,
}

/// Parses `--threshold`: a number from 0 to 100. It lives here rather than in the binary so tests
/// can call it: a bad value makes clap exit the process, and on Windows a process that exits that
/// way never writes its coverage.
pub fn percent(value: &str) -> Result<f64, String> {
    match value.parse::<f64>() {
        Ok(p) if (0.0..=100.0).contains(&p) => Ok(p),
        _ => Err("expected a number from 0 to 100".into()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{}: {source}", path.display())]
    Parse { path: PathBuf, source: serde_json::Error },
}

impl Config {
    /// Reads `path`, or `docov.json` when `path` is `None`, which may be missing.
    pub fn load(path: Option<&Path>) -> Result<Self, Error> {
        let (path, required) = match path {
            Some(path) => (path, true),
            None => (Path::new(DEFAULT_PATH), false),
        };
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if !required && e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(Error::Read { path: path.to_owned(), source }),
        };
        serde_json::from_str(&text).map_err(|source| Error::Parse { path: path.to_owned(), source })
    }

    /// The JSON Schema for this file, as `docov --print-schema` prints it.
    pub fn json_schema() -> String {
        let schema = schemars::schema_for!(Config);
        serde_json::to_string_pretty(&schema).expect("a schema serializes") + "\n"
    }
}
