//! `--explain FILE`: in place of a report, each item in one file with whether it counts and is
//! documented, and why, and what the scan passed over there, and why.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Rules, collect,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Explain>()
}

/// The file to explain, if there's one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Explain(pub Option<PathBuf>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Say why each item in FILE counts or doesn't and is documented or isn't, and what the scan passed over there, in place of a report
    #[arg(long, value_name = "FILE")]
    explain: Option<PathBuf>,
}

/// It has no key: it's for one run.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct Keys {}

impl Feature for Explain {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, _keys: Keys) -> Self {
        Self(flags.explain)
    }
}

impl Explain {
    /// Writes its file's explanation by `rules` to `out`, when it has a file, or says why it
    /// couldn't (exit 2).
    pub fn write(&self, rules: &Rules, out: &mut dyn Write) -> Option<Result<(), String>> {
        self.0.as_deref().map(|path| explain(path, rules, out))
    }
}

fn explain(path: &Path, rules: &Rules, out: &mut dyn Write) -> Result<(), String> {
    let lines = collect::explain(path, rules).map_err(|e| e.to_string())?;
    lines.iter().try_for_each(|line| writeln!(out, "{line}")).map_err(|e| e.to_string())
}
