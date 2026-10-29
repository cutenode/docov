//! Finds the files to scan.

use std::path::{Path, PathBuf};

use ignore::{WalkBuilder, overrides::OverrideBuilder};
use oxc_span::SourceType;

/// The JavaScript and TypeScript files under `paths` (the extensions oxc parses), sorted, leaving
/// out what .gitignore and `exclude` (gitignore-style globs, relative to `root`) do, and hidden
/// files. A file named in `paths` is always included, to fail if it isn't one of them.
pub fn files(paths: &[PathBuf], root: &Path, exclude: &[String]) -> Result<Vec<PathBuf>, ignore::Error> {
    let mut overrides = OverrideBuilder::new(root);
    for glob in exclude {
        overrides.add(&format!("!{glob}"))?;
    }

    let (first, rest) = paths.split_first().map_or((Path::new("."), &[][..]), |(p, rest)| (p.as_path(), rest));
    let mut walk = WalkBuilder::new(first);
    for path in rest {
        walk.add(path);
    }
    // .gitignore applies outside a git repository too, so node_modules stays out of a fresh checkout.
    walk.require_git(false).overrides(overrides.build()?);

    let mut files = Vec::new();
    for entry in walk.build() {
        let entry = entry?;
        let named = entry.depth() == 0;
        if entry.file_type().is_some_and(|t| t.is_file()) && (named || SourceType::from_path(entry.path()).is_ok()) {
            let path = entry.into_path();
            files.push(path.strip_prefix(".").map(Path::to_path_buf).unwrap_or(path));
        }
    }
    files.sort();
    Ok(files)
}
