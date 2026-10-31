//! `--threads`: how many files to scan at once.

use crate::feature::{Entry, Feature, NoKeys};

pub(crate) fn entry() -> Entry {
    Entry::of::<Threads>()
}

/// How many threads scan files, if not rayon's default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Threads(pub Option<usize>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Scan this many files at once [default: RAYON_NUM_THREADS, or one per CPU]
    #[arg(long, value_name = "N")]
    threads: Option<usize>,
}

impl Feature for Threads {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.threads)
    }

    fn describe(&self) -> Option<serde_json::Value> {
        Some(self.0.into())
    }
}

impl Threads {
    /// Sizes the pool that scans files. rayon makes its pool once, so only a process's first call
    /// does anything, and only before the first scan.
    pub fn apply(self) {
        if let Some(threads) = self.0 {
            // It fails only once the pool is made, which leaves it as it was.
            let _ = rayon::ThreadPoolBuilder::new().num_threads(threads).build_global();
        }
    }
}
