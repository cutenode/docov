//! `--explain FILE`: in place of a report, each item in one file with whether it counts and is
//! documented, and why, and what the scan passed over there, and why, after the override that set
//! its scope, if one did.

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
    #[arg(long, value_name = "FILE", conflicts_with = "stdin_filename")]
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
    /// Writes its file's explanation to `out`, when it has a file, or says why it couldn't (exit 2).
    /// `files` is the plan for that file alone, with the rules its config scans it with, or nothing
    /// when the config leaves it out.
    pub fn write(&self, files: &[(&Path, &Rules)], out: &mut dyn Write) -> Option<Result<(), String>> {
        self.0.as_deref().map(|path| explain(path, files, out))
    }
}

fn explain(path: &Path, files: &[(&Path, &Rules)], out: &mut dyn Write) -> Result<(), String> {
    let lines = match files {
        [(_, rules), ..] => {
            let scope = format!("{:?}", rules.scope).to_lowercase();
            let set_by = rules.scope_set_by.iter().map(|(i, globs)| {
                format!("{}  scope {scope}: set by overrides[{i}], for {globs}", path.display())
            });
            set_by.chain(collect::explain(path, rules).map_err(|e| e.to_string())?).collect()
        }
        [] => vec![format!("{}  left out: the config excludes it", path.display())],
    };
    lines.iter().try_for_each(|line| writeln!(out, "{line}")).map_err(|e| e.to_string())
}
