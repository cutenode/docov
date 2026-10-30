//! The config's folder, which docov.json's globs and the baseline's paths are relative to.

use std::{
    env, iter,
    path::{Component, Path, PathBuf},
};

/// The config's folder, for turning paths from the current folder into paths from it.
#[derive(Clone, Debug)]
pub struct Root {
    here: PathBuf,
    dir: PathBuf,
}

impl Root {
    /// The folder at `dir`, a path from the current folder.
    pub fn new(dir: &Path) -> Self {
        // This fails only when the current folder is gone, and then there's nothing to scan.
        let here = env::current_dir().unwrap_or_default();
        let dir = normal(&here.join(dir));
        Self { here, dir }
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
