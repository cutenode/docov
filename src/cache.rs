//! The result cache: each file's items from the last scan, reused while the file is unchanged.

use std::{
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use rustc_hash::{FxHashMap, FxHasher};
use serde::{Deserialize, Serialize};

use crate::{
    FileReport, Item, Rules,
    check::Diagnostic,
    collect::{Facts, Ignore, Stub},
};

/// What a file's items were scanned from: the file as it was, by its size and modified time, and
/// the rules, which change a scan's items and what the doc checks find.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    size: u64,
    /// Since 1970.
    modified: Duration,
    /// A hash of the rules.
    rules: u64,
}

impl Stamp {
    /// The file at `path` as it is now, scanned with `rules`, if it can be read.
    fn of(path: &Path, rules: &Rules) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        // A file from before 1970 isn't kept, having no time to keep it by.
        let modified = metadata.modified().ok().and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())?;
        // FxHasher has no random seed, so the same rules hash the same in every run.
        let mut hasher = FxHasher::default();
        serde_json::to_string(rules).expect("rules serialize").hash(&mut hasher);
        // Under the public scope, what the entry points reach in the file changes its items too.
        let mut reached: Option<Vec<&String>> =
            rules.public.as_ref().map(|api| api.reach(path).into_iter().flatten().collect());
        reached.iter_mut().for_each(|names| names.sort_unstable());
        reached.hash(&mut hasher);
        Some(Self { size: metadata.len(), modified, rules: hasher.finish() })
    }
}

/// A file's entry in the cache.
#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    stamp: Stamp,
    items: Vec<Kept>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unused_ignores: Vec<Ignore>,
    code_line: usize,
}

/// An item as the cache keeps it: with its `docov-ignore` comment, the facts the reports order and
/// measure by, what the doc checks found and its stub, which reports leave out of the item itself.
#[derive(Debug, Serialize, Deserialize)]
struct Kept {
    #[serde(flatten)]
    item: Item,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ignore: Option<Ignore>,
    /// Its facts' `params`, `exported`, `release_tagged` and `typed`.
    about: (usize, bool, bool, Option<bool>),
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stub: Option<Stub>,
}

impl Kept {
    fn of(item: &Item) -> Self {
        let Facts { params, exported, release_tagged, typed, ref diagnostics, ref stub, .. } = item.facts;
        let (ignore, about, diagnostics) =
            (item.ignore.clone(), (params, exported, release_tagged, typed), diagnostics.clone());
        Self { item: item.clone(), ignore, about, diagnostics, stub: stub.clone() }
    }

    /// The item it keeps.
    fn item(&self) -> Item {
        let (params, exported, release_tagged, typed) = self.about;
        let (diagnostics, stub) = (self.diagnostics.clone(), self.stub.clone());
        let facts = Facts { params, exported, release_tagged, typed, diagnostics, stub, ..self.item.facts.clone() };
        Item { ignore: self.ignore.clone(), facts, ..self.item.clone() }
    }
}

/// The cache file's contents.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Contents {
    /// The docov that wrote it. Another one's is ignored, since it may scan differently.
    version: String,
    /// Each file's entry, by its path, as it was named.
    files: FxHashMap<String, Entry>,
}

/// The cache, read at the start of a scan and written at the end.
#[derive(Debug, Default)]
pub struct Cache {
    /// Its file, if there's a cache.
    path: Option<PathBuf>,
    /// What it held.
    files: FxHashMap<String, Entry>,
}

impl Cache {
    /// The cache in the file at `path`, empty if there's none or it's from another docov or
    /// unreadable, in which case it's rebuilt.
    pub fn read(path: PathBuf) -> Self {
        let contents = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Contents>(&bytes).ok());
        let files = contents.filter(|c| c.version == env!("CARGO_PKG_VERSION")).map(|c| c.files);
        Self { path: Some(path), files: files.unwrap_or_default() }
    }

    /// The stamp to check and keep the file at `path` by, if there's a cache.
    pub fn stamp(&self, path: &Path, rules: &Rules) -> Option<Stamp> {
        self.path.as_ref().and_then(|_| Stamp::of(path, rules))
    }

    /// The report the file at `path` had when it last had `stamp`.
    pub fn get(&self, path: &Path, stamp: Stamp) -> Option<FileReport> {
        let entry = self.files.get(&*path.to_string_lossy()).filter(|entry| entry.stamp == stamp)?;
        let (items, unused_ignores) = (entry.items.iter().map(Kept::item).collect(), entry.unused_ignores.clone());
        Some(FileReport { path: path.to_owned(), items, unused_ignores, code_line: entry.code_line })
    }

    /// Writes `reports`, each with its stamp, to the cache file, in place of what it held.
    pub fn write(&self, reports: &[(FileReport, Option<Stamp>)]) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        let files = reports
            .iter()
            .filter_map(|(report, stamp)| {
                Some((
                    report.path.to_string_lossy().into_owned(),
                    Entry {
                        stamp: (*stamp)?,
                        items: report.items.iter().map(Kept::of).collect(),
                        unused_ignores: report.unused_ignores.clone(),
                        code_line: report.code_line,
                    },
                ))
            })
            .collect();
        let contents = Contents { version: env!("CARGO_PKG_VERSION").into(), files };
        fs::write(path, serde_json::to_vec(&contents).expect("the cache serializes"))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}
