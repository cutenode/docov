//! Gitignore-style globs, which docov.json's keys give relative to its folder.

use std::{fmt, path::Path};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Gitignore-style globs, matched against paths relative to the config's folder. A `!` glob takes
/// back what one before it matched, as in .gitignore.
#[derive(Clone, Debug)]
pub struct Globs {
    globs: Vec<String>,
    matcher: Gitignore,
}

impl Globs {
    /// Builds `globs`, failing on one that doesn't parse or a set too big to build.
    pub fn new(globs: Vec<String>) -> Result<Self, ignore::Error> {
        let mut builder = GitignoreBuilder::new("");
        // As the walk's own globs do: `[z` is a mistake, not a literal `[`.
        builder.allow_unclosed_class(false);
        for glob in &globs {
            builder.add_line(None, glob)?;
        }
        Ok(Self { matcher: builder.build()?, globs })
    }

    /// Whether they match `path` or a folder it's in.
    pub fn contain(&self, path: &Path) -> bool {
        self.matcher.matched_path_or_any_parents(path, false).is_ignore()
    }
}

impl PartialEq for Globs {
    fn eq(&self, other: &Self) -> bool {
        self.globs == other.globs
    }
}

/// The globs, as docov.json gives them, for messages that name them.
impl fmt::Display for Globs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.globs.join(", "))
    }
}
