//! `--changed`, `--since` and `--staged`: scan only the files git says changed.

use std::path::PathBuf;

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    feature::{Entry, Feature},
    git,
    root::Root,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Changed>()
}

/// Which changed files to scan, if not every file.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Changed {
    /// Every file.
    #[default]
    All,
    /// Those that differ from where HEAD branched from this ref (or the default branch), committed
    /// or not, and untracked ones.
    Since(Option<String>),
    /// Those staged for the next commit.
    Staged,
}

impl Changed {
    /// `files`, narrowed to the changed ones.
    pub fn filter(&self, files: Vec<PathBuf>, root: &Root) -> Result<Vec<PathBuf>, String> {
        let changed = match self {
            Self::All => return Ok(files),
            Self::Since(since) => git::changed_since(git::base(since.as_deref()))?,
            Self::Staged => git::staged()?,
        };
        let changed: FxHashSet<PathBuf> = changed.iter().map(|path| root.absolute(path)).collect();
        Ok(files.into_iter().filter(|file| changed.contains(&root.absolute(file))).collect())
    }
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Scan only files changed since HEAD branched from the default branch, committed or not
    #[arg(long, conflicts_with = "staged")]
    changed: bool,
    /// With --changed, the branch or commit to compare with [default: origin/HEAD, or main]
    #[arg(long, value_name = "REF", requires = "changed")]
    since: Option<String>,
    /// Scan only files staged for the next commit
    #[arg(long)]
    staged: bool,
}

/// No keys: which files changed is up to each run.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct Keys {}

impl Feature for Changed {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, _: Keys) -> Self {
        match (flags.changed, flags.staged) {
            (true, _) => Self::Since(flags.since),
            (_, true) => Self::Staged,
            _ => Self::All,
        }
    }
}
