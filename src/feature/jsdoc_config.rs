//! `--jsdoc-config`: scans the files a jsdoc.json documents. docov.json's `jsdocConfig` is the
//! config's own, since it's relative to the file that writes it.

use std::path::PathBuf;

use crate::feature::{Entry, Feature, NoKeys};

pub(crate) fn entry() -> Entry {
    Entry::of::<JsdocConfig>()
}

/// The jsdoc.json the flag names, if it names one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsdocConfig(pub Option<PathBuf>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Scan the files this jsdoc.json documents [default: the config's `jsdocConfig`]
    #[arg(long, value_name = "FILE")]
    jsdoc_config: Option<PathBuf>,
}

impl Feature for JsdocConfig {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.jsdoc_config)
    }
}
