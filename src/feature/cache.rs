//! `--cache` and `--cache-location`: reuses each unchanged file's items from the last scan.

use std::path::{Path, PathBuf};

use crate::{
    cache::Cache,
    feature::{Entry, Feature, NoKeys},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<CacheFile>()
}

/// Where the cache is, if docov keeps one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CacheFile {
    on: bool,
    location: Option<PathBuf>,
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Keep each file's items, and reuse them while the file's size and modified time stay the same
    #[arg(long)]
    cache: bool,
    /// The cache file [default: .docov-cache in the config's folder]
    #[arg(long, value_name = "FILE", requires = "cache")]
    cache_location: Option<PathBuf>,
}

impl Feature for CacheFile {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self { on: flags.cache, location: flags.cache_location }
    }

    fn describe(&self) -> Option<serde_json::Value> {
        let location = self.location.as_ref().map(|path| path.display().to_string());
        Some(serde_json::json!({ "on": self.on, "location": location }))
    }
}

impl CacheFile {
    /// The cache, read from its file, or one that keeps nothing without `--cache`. `dir` is the
    /// config's folder.
    pub fn open(&self, dir: &Path) -> Cache {
        let path = self.location.clone().unwrap_or_else(|| dir.join(".docov-cache"));
        if self.on { Cache::read(path) } else { Cache::default() }
    }
}
