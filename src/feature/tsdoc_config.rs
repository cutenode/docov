//! `--tsdoc-config` and `tsdocConfig`: the tsdoc.json the `tsdoc-*` checks read every file's
//! blocks with. Without one, and with `"syntax": "tsdoc"`, each file is read with the tsdoc.json
//! `@microsoft/tsdoc-config` finds for it, in the nearest folder with a tsconfig.json or a
//! package.json. Otherwise they're read with TSDoc's standard tags.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Config,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<TsdocConfig>()
}

/// The tsdoc.json the flag or the key names, if one does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TsdocConfig {
    flag: Option<PathBuf>,
    key: Option<PathBuf>,
}

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Read TSDoc blocks with this tsdoc.json [default: the config's `tsdocConfig`, or with "syntax": "tsdoc", each file's nearest]
    #[arg(long, value_name = "FILE")]
    tsdoc_config: Option<PathBuf>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Keys {
    /// A tsdoc.json, relative to this file's folder, that the tsdoc-* checks read every file's blocks with: its tagDefinitions, noStandardTags, supportForTags, supportedHtmlElements, reportUnsupportedHtmlElements and the files it extends (paths or package names, resolved the way Node resolves them). Without it, and with "syntax": "tsdoc", each file gets the tsdoc.json in the nearest folder above it with a tsconfig.json or a package.json, as @microsoft/tsdoc-config finds one; otherwise TSDoc's standard tags. Unknown keys in a tsdoc.json are an error, as in tsdoc-config.
    tsdoc_config: Option<PathBuf>,
}

impl Feature for TsdocConfig {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        Self { flag: flags.tsdoc_config, key: keys.tsdoc_config }
    }
}

impl TsdocConfig {
    /// The tsdoc.json it names, as a path from the current folder: the flag's, or else the key's,
    /// which is relative to the folder of the file in `config` that wrote it.
    pub fn path(&self, config: &Config) -> Option<PathBuf> {
        let dir = || config.dirs.get("tsdocConfig").unwrap_or(&config.dir);
        self.flag.clone().or_else(|| self.key.as_ref().map(|key| dir().components().chain(key.components()).collect()))
    }
}
