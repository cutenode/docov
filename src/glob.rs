//! Gitignore-style globs, which docov.json's keys give relative to its folder.

use std::{borrow::Cow, fmt, path::Path};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

/// Gitignore-style globs, matched against paths relative to the config's folder. A `!` glob takes
/// back what one before it matched, as in .gitignore.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
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

    /// Whether they match `path` (a folder when `is_dir`) or a folder it's in.
    pub fn contain(&self, path: &Path, is_dir: bool) -> bool {
        self.matcher.matched_path_or_any_parents(path, is_dir).is_ignore()
    }

    /// Whether they match `path` itself, a folder when `is_dir`. The walk, which skips the folders
    /// they match, asks this.
    pub fn matches(&self, path: &Path, is_dir: bool) -> bool {
        self.matcher.matched(path, is_dir).is_ignore()
    }
}

impl TryFrom<Vec<String>> for Globs {
    type Error = ignore::Error;
    fn try_from(globs: Vec<String>) -> Result<Self, ignore::Error> {
        Self::new(globs)
    }
}

impl From<Globs> for Vec<String> {
    fn from(globs: Globs) -> Self {
        globs.globs
    }
}

/// No globs, which match nothing.
impl Default for Globs {
    fn default() -> Self {
        Self::new(Vec::new()).expect("no globs build")
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

impl JsonSchema for Globs {
    fn inline_schema() -> bool {
        true
    }
    fn schema_name() -> Cow<'static, str> {
        "Globs".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        Vec::<String>::json_schema(generator)
    }
}
