//! `overrides`: settings that change for the files globs match, the last matching override
//! winning.

use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    feature::{Entry, Feature, threshold::Thresholds},
    glob::Globs,
    root::Root,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Overrides>()
}

/// Every override, in docov.json's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides(pub Vec<Override>);

/// Settings for the files some globs match, in place of the top-level ones.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Override {
    /// Gitignore-style globs of the files it's for, relative to this file's folder.
    pub files: Globs,
    /// In place of the top-level `exclude`, for these files.
    pub exclude: Option<Globs>,
    /// In place of the top-level `threshold`, for these files.
    pub threshold: Option<Thresholds>,
}

impl Overrides {
    /// The index of the last override for `path` (from the config's folder) that has the setting
    /// `has` picks out.
    pub fn last<T>(&self, path: &Path, is_dir: bool, has: impl Fn(&Override) -> Option<&T>) -> Option<(usize, &T)> {
        self.0.iter().enumerate().rev().find_map(|(i, o)| has(o).filter(|_| o.files.contain(path, is_dir)).map(|s| (i, s)))
    }

    /// Whether the walk leaves out `path`, from the current folder: what `exclude` (the top-level
    /// one) does, or the last override for it with its own.
    pub fn excluder(self, exclude: Globs, root: Root) -> impl Fn(&Path, bool) -> bool + Send + Sync + 'static {
        move |path, is_dir| {
            let path = root.relative(path);
            let exclude = self.last(&path, is_dir, |o| o.exclude.as_ref()).map_or(&exclude, |(_, e)| e);
            exclude.matches(&path, is_dir)
        }
    }
}

/// No flags: overrides come from docov.json alone.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Settings for the files globs match, in place of the top-level ones. For each file and setting, the last override that matches it and has the setting wins.
    overrides: Vec<Override>,
}

impl Feature for Overrides {
    type Flags = Flags;
    type Keys = Keys;

    fn new(_: Flags, keys: Keys) -> Self {
        Self(keys.overrides)
    }
}
