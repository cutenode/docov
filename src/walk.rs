//! Finds the files to scan.

use std::path::{Path, PathBuf};

use ignore::{
    WalkBuilder,
    gitignore::{Gitignore, GitignoreBuilder},
};
use oxc_span::SourceType;

/// Gitignore-style globs of files to leave out, relative to `root`, a path from the current folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exclude {
    /// The folder the globs are relative to: the folder of the file that wrote them.
    pub root: PathBuf,
    /// The globs.
    pub globs: Vec<String>,
}

impl Exclude {
    /// The globs as a matcher.
    fn matcher(&self) -> Result<Gitignore, ignore::Error> {
        let mut builder = GitignoreBuilder::new(&self.root);
        // A `[` with no `]` is an error, as it is in `--glob` for ripgrep, rather than a literal `[`.
        builder.allow_unclosed_class(false);
        for glob in &self.globs {
            builder.add_line(None, glob)?;
        }
        builder.build()
    }
}

/// The JavaScript and TypeScript files under `paths` (the extensions oxc parses), sorted, leaving
/// out what .gitignore and `exclude` do, and hidden files. A file named in `paths` is always
/// included, to fail if it isn't one of them.
pub fn files(paths: &[PathBuf], exclude: &[Exclude]) -> Result<Vec<PathBuf>, ignore::Error> {
    let matchers = matchers(exclude)?;

    let (first, rest) = paths.split_first().map_or((Path::new("."), &[][..]), |(p, rest)| (p.as_path(), rest));
    let mut walk = WalkBuilder::new(first);
    for path in rest {
        walk.add(path);
    }
    // .gitignore applies outside a git repository too, so node_modules stays out of a fresh checkout.
    // Each exclude applies only under its own folder, since its globs are relative to it.
    walk.require_git(false).filter_entry(move |entry| {
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        let path = plain(entry.path());
        !matchers.iter().any(|m| under(m, path) && m.matched(path, is_dir).is_ignore())
    });

    let mut files = Vec::new();
    for entry in walk.build() {
        let entry = entry?;
        let named = entry.depth() == 0;
        if entry.file_type().is_some_and(|t| t.is_file()) && (named || SourceType::from_path(entry.path()).is_ok()) {
            files.push(plain(entry.path()).to_owned());
        }
    }
    files.sort();
    Ok(files)
}

/// Leaves out of `files` those `exclude` does, by their own paths or a folder's above them.
pub fn retain(files: &mut Vec<PathBuf>, exclude: &[Exclude]) -> Result<(), ignore::Error> {
    let matchers = matchers(exclude)?;
    files.retain(|file| {
        let file = plain(file);
        !matchers.iter().any(|m| under(m, file) && m.matched_path_or_any_parents(file, false).is_ignore())
    });
    Ok(())
}

fn matchers(exclude: &[Exclude]) -> Result<Vec<Gitignore>, ignore::Error> {
    exclude.iter().map(Exclude::matcher).collect()
}

/// Whether `path` is under the folder `matcher`'s globs are relative to, the only place they apply.
fn under(matcher: &Gitignore, path: &Path) -> bool {
    path.starts_with(plain(matcher.path()))
}

/// `path` without a leading `./`, which the walk adds to what it finds under `.`.
fn plain(path: &Path) -> &Path {
    path.strip_prefix(".").unwrap_or(path)
}
