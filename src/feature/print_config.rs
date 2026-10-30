//! `--print-config`: prints the settings docov would scan with, and exits.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::{
    Config, report,
    feature::{Entry, Feature, NoKeys, Settings},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<PrintConfig>()
}

/// Whether to print the settings instead of scanning.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PrintConfig(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Print the settings, merged from the config files and flags, as JSON and exit
    #[arg(long)]
    print_config: bool,
}

impl Feature for PrintConfig {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.print_config)
    }
}

/// The settings: the files they came from, the paths to scan, the excludes, and each feature's
/// setting, with the keys in order.
pub fn json(config: &Config, paths: &[PathBuf], settings: &Settings) -> Value {
    let display = |paths: &[PathBuf]| paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>();
    let exclude: Vec<_> =
        config.exclude.iter().map(|e| json!({ "root": e.root.display().to_string(), "globs": e.globs })).collect();
    report::sorted(json!({
        "files": display(&config.files),
        "paths": display(paths),
        "exclude": exclude,
        "features": settings.describe(),
    }))
}
