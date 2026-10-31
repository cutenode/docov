//! Finds the files to scan.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use oxc_span::SourceType;

use crate::glob::Globs;

/// Gitignore-style globs of files to leave out, relative to `root`, a path from the current folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exclude {
    /// The folder the globs are relative to: the folder of the file that wrote them.
    pub root: PathBuf,
    /// The globs.
    pub globs: Globs,
}

/// The JavaScript and TypeScript files under `paths` (the extensions oxc parses), sorted, leaving
/// out what .gitignore does, hidden files, and the files and folders `excluded` says to (given each
/// path, and whether it's a folder). A file named in `paths` is always included, to fail if it
/// isn't one of them.
pub fn files(
    paths: &[PathBuf],
    excluded: impl Fn(&Path, bool) -> bool + Send + Sync + 'static,
) -> Result<Vec<PathBuf>, ignore::Error> {
    let (first, rest) = paths.split_first().map_or((Path::new("."), &[][..]), |(p, rest)| (p.as_path(), rest));
    let mut walk = WalkBuilder::new(first);
    for path in rest {
        walk.add(path);
    }
    // .gitignore applies outside a git repository too, so node_modules stays out of a fresh checkout.
    // The walk doesn't ask the filter about the named paths, so they're always scanned.
    walk.require_git(false)
        .filter_entry(move |entry| !excluded(entry.path(), entry.file_type().is_some_and(|t| t.is_dir())));

    let mut files = Vec::new();
    for entry in walk.build() {
        let entry = entry?;
        let named = entry.depth() == 0;
        if entry.file_type().is_some_and(|t| t.is_file()) && (named || SourceType::from_path(entry.path()).is_ok()) {
            // Without the `./` the walk adds to what it finds under `.`.
            let path = entry.into_path();
            files.push(path.strip_prefix(".").map(Path::to_path_buf).unwrap_or(path));
        }
    }
    files.sort();
    Ok(files)
}

/// Whether `excluded`, as [`files`] takes it, leaves out `file` or a folder it's in, the way the
/// walk would have.
pub fn excludes(excluded: &dyn Fn(&Path, bool) -> bool, file: &Path) -> bool {
    let mut folders = file.ancestors().skip(1).filter(|dir| !dir.as_os_str().is_empty());
    excluded(file, false) || folders.any(|dir| excluded(dir, true))
}
