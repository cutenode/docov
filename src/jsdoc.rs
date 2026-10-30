//! jsdoc.json: the files jsdoc documents, for docov to scan the same ones.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use regex::Regex;
use serde::Deserialize;

use crate::walk::Exclude;

/// The part of a jsdoc.json docov reads.
#[derive(Default, Deserialize)]
#[serde(default)]
struct File {
    source: Source,
}

/// jsdoc's `source`.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Source {
    include: Vec<PathBuf>,
    exclude: Vec<PathBuf>,
    include_pattern: Option<String>,
    exclude_pattern: Option<String>,
}

/// The files a jsdoc.json documents.
#[derive(Debug, Default, PartialEq)]
pub struct Sources {
    /// `source.include`, as paths from the current folder.
    pub paths: Vec<PathBuf>,
    /// `source.exclude`, each path as an exclude of its own.
    pub exclude: Vec<Exclude>,
    /// `source.includePattern` and `source.excludePattern`.
    pub patterns: Patterns,
}

/// The regular expressions a file's path has to match, and not match, to be documented. jsdoc's
/// default `includePattern` leaves out TypeScript, so only patterns a jsdoc.json writes count.
#[derive(Clone, Debug, Default)]
pub struct Patterns {
    include: Option<Regex>,
    exclude: Option<Regex>,
}

impl PartialEq for Patterns {
    fn eq(&self, other: &Self) -> bool {
        let text = |p: &Patterns| [&p.include, &p.exclude].map(|r| r.as_ref().map(Regex::as_str).map(str::to_owned));
        text(self) == text(other)
    }
}

impl Patterns {
    /// Whether the file at `path` is documented, tested the way jsdoc does, on its absolute path.
    pub fn keep(&self, path: &Path) -> bool {
        // The current folder is gone only if a scan can't find anything anyway.
        let path = env::current_dir().unwrap_or_default().join(path);
        let path = path.to_string_lossy();
        self.include.as_ref().is_none_or(|r| r.is_match(&path))
            && !self.exclude.as_ref().is_some_and(|r| r.is_match(&path))
    }
}

/// Reads the jsdoc.json at `path`. Paths in it are relative to its folder. jsdoc also takes a
/// JavaScript config, which docov can't run.
pub fn read(path: &Path) -> Result<Sources, String> {
    let failed = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    if path.extension().is_some_and(|e| ["js", "cjs", "mjs"].iter().any(|js| e == *js)) {
        return Err(failed(&"docov reads a jsdoc config in JSON, not JavaScript"));
    }
    let mut text = fs::read_to_string(path).map_err(|e| failed(&e))?;
    // jsdoc strips comments from its config too.
    if json_strip_comments::strip(&mut text).is_err() {
        return Err(failed(&"a `/` that doesn't start a comment"));
    }
    let File { source } = serde_json::from_str(&text).map_err(|e| failed(&e))?;
    let pattern = |pattern: Option<String>| pattern.map(|p| Regex::new(&p)).transpose().map_err(|e| failed(&e));
    let patterns = Patterns { include: pattern(source.include_pattern)?, exclude: pattern(source.exclude_pattern)? };
    let dir = path.parent().unwrap_or(Path::new(""));
    let paths = source.include.iter().map(|p| dir.join(p).components().collect()).collect();
    // Each excluded path is a glob anchored in its own folder. One without a name, as `..`, is
    // skipped.
    let exclude = source
        .exclude
        .iter()
        .filter_map(|p| {
            let path = dir.join(p);
            let name = globset::escape(&path.file_name()?.to_string_lossy());
            let root = path.parent().expect("a path with a file name is in a folder");
            Some(Exclude { root: root.to_owned(), globs: vec![format!("/{name}")] })
        })
        .collect();
    Ok(Sources { paths, exclude, patterns })
}
