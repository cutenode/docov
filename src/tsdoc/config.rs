//! The tsdoc.json each scanned file's blocks are read with, as tsdocrs's `config_file` loads and
//! finds them, each read once.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rustc_hash::{FxHashMap, FxHashSet};
pub use tsdocrs::config_file::{FILE_NAME, SCHEMA, find};

use super::Configuration;
use crate::root::Root;

/// The configuration the tsdoc.json at `path` sets up, and the files it read, `path` first. The
/// error names the file and TSDoc's message id, as `path: tsdoc-config-...: what`.
pub fn load(path: &Path) -> Result<(Configuration, Vec<PathBuf>), String> {
    tsdocrs::config_file::load(path).map_err(|error| error.to_string())
}

/// The configuration each scanned file's comments are read with.
#[derive(Debug, Default)]
pub struct Configs {
    /// The one every file is read with, when the config names one.
    every: Option<Arc<Configuration>>,
    /// The one each folder's files are read with, by the folder, when it isn't the default.
    folders: FxHashMap<PathBuf, Arc<Configuration>>,
    /// The files read, and looked for, to find them.
    pub sources: Vec<PathBuf>,
}

impl Configs {
    /// The tsdoc.json at `path`, for every file.
    pub fn file(path: &Path) -> Result<Self, String> {
        let (configuration, sources) = load(path)?;
        Ok(Self { every: Some(Arc::new(configuration)), folders: FxHashMap::default(), sources })
    }

    /// The tsdoc.json [found](find) for each of `files`, each one read once. It looks above the
    /// current folder too, for a scan run inside a package.
    pub fn find<'p>(files: impl IntoIterator<Item = &'p Path>) -> Result<Self, String> {
        let mut configs = Self::default();
        let (mut seen, mut read) = (FxHashSet::default(), FxHashMap::default());
        for file in files {
            let folder = file.parent().unwrap_or(file);
            if !seen.insert(folder.to_owned()) {
                continue;
            }
            let (found, looked) = find(&Root::new(Path::new("")).absolute(folder));
            configs.sources.extend(looked);
            let Some(found) = found else { continue };
            let configuration: Arc<Configuration> = match read.get(&found) {
                Some(configuration) => Arc::clone(configuration),
                None => {
                    let (configuration, sources) = load(&found)?;
                    configs.sources.extend(sources);
                    Arc::clone(read.entry(found).or_insert(Arc::new(configuration)))
                }
            };
            configs.folders.insert(folder.to_owned(), configuration);
        }
        Ok(configs)
    }

    /// The configuration the file at `path` is read with: TSDoc's default when nothing sets
    /// another.
    pub fn of(&self, path: &Path) -> &Configuration {
        let folder = path.parent().and_then(|folder| self.folders.get(folder));
        self.every.as_deref().or(folder.map(Arc::as_ref)).unwrap_or(&super::DEFAULT)
    }
}
