//! The result cache: each file's items from the last scan, reused while the file is unchanged.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::{FileReport, Item, Scope};

/// What a file's items were scanned from: the file as it was, by its size and modified time, and
/// the settings that change a scan's items.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    size: u64,
    /// Since 1970.
    modified: Duration,
    scope: Scope,
}

impl Stamp {
    /// The file at `path` as it is now, scanned with `scope`, if it can be read.
    fn of(path: &Path, scope: Scope) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        // A file from before 1970 isn't kept, having no time to keep it by.
        let modified = metadata.modified().ok().and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())?;
        Some(Self { size: metadata.len(), modified, scope })
    }
}

/// A file's entry in the cache.
#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    stamp: Stamp,
    items: Vec<Item>,
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
    pub fn stamp(&self, path: &Path, scope: Scope) -> Option<Stamp> {
        self.path.as_ref().and_then(|_| Stamp::of(path, scope))
    }

    /// The items the file at `path` had when it last had `stamp`.
    pub fn get(&self, path: &Path, stamp: Stamp) -> Option<Vec<Item>> {
        self.files.get(&*path.to_string_lossy()).filter(|entry| entry.stamp == stamp).map(|entry| entry.items.clone())
    }

    /// Writes `reports`, each with its stamp, to the cache file, in place of what it held.
    pub fn write(&self, reports: &[(FileReport, Option<Stamp>)]) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        let files = reports
            .iter()
            .filter_map(|(report, stamp)| {
                Some((
                    report.path.to_string_lossy().into_owned(),
                    Entry { stamp: (*stamp)?, items: report.items.clone() },
                ))
            })
            .collect();
        let contents = Contents { version: env!("CARGO_PKG_VERSION").into(), files };
        fs::write(path, serde_json::to_vec(&contents).expect("the cache serializes"))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}
