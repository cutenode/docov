//! The config's folder, which docov.json's globs and the baseline's paths are relative to.

use std::{
    env, iter,
    path::{Component, Path, PathBuf},
};

use rustc_hash::FxHashMap;

/// The config's folder, for turning paths from the current folder into paths from it, and the
/// folders of the files it extends, which their own keys are relative to.
#[derive(Clone, Debug)]
pub struct Root {
    here: PathBuf,
    dir: PathBuf,
    /// The folder of the file that wrote each key, absolute.
    keys: FxHashMap<String, PathBuf>,
}

impl Root {
    /// The folder at `dir`, a path from the current folder.
    pub fn new(dir: &Path) -> Self {
        Self::with_keys(dir, &FxHashMap::default())
    }

    /// The folder at `dir`, where each of `keys` was written in the folder it maps to: that of a
    /// file the config extends, or for a nested config, the root config's. All are paths from the
    /// current folder.
    pub fn with_keys(dir: &Path, keys: &FxHashMap<String, PathBuf>) -> Self {
        // This fails only when the current folder is gone, and then there's nothing to scan.
        let here = env::current_dir().unwrap_or_default();
        let keys = keys.iter().map(|(key, dir)| (key.clone(), normal(&here.join(dir)))).collect();
        Self { dir: normal(&here.join(dir)), here, keys }
    }

    /// The folder of the file that wrote `key`, which its paths and globs are relative to.
    pub fn of(&self, key: &str) -> Self {
        let dir = self.keys.get(key).unwrap_or(&self.dir).clone();
        Self { here: self.here.clone(), dir, keys: FxHashMap::default() }
    }

    /// `path`, from the current folder, as a path from this folder, if it's in it. Globs relative
    /// to a folder match only what's in it.
    pub fn inside(&self, path: &Path) -> Option<PathBuf> {
        let path = self.relative(path);
        (path.components().next() != Some(Component::ParentDir)).then_some(path)
    }

    /// `path`, from the current folder, as an absolute path without `.` or `..` parts. It doesn't
    /// follow symlinks, the way the walk doesn't.
    pub fn absolute(&self, path: &Path) -> PathBuf {
        normal(&self.here.join(path))
    }

    /// `path`, from the current folder, as a path from this folder, starting with `..` when it's
    /// outside it.
    pub fn relative(&self, path: &Path) -> PathBuf {
        let path = self.absolute(path);
        let common = self.dir.components().zip(path.components()).take_while(|(a, b)| a == b).count();
        let up = self.dir.components().count() - common;
        iter::repeat_n(Component::ParentDir, up).chain(path.components().skip(common)).collect()
    }
}

/// `path`, an absolute one, with each `..` taking off the part before it. `components` already
/// leaves out `.` parts after the first.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}
