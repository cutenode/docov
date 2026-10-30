//! `--stdin-filename`: scans one file's source from stdin, as an editor's unsaved buffer.

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use crate::{
    FileReport, Rules, collect,
    feature::{Entry, Feature, NoKeys},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<StdinFilename>()
}

/// The path to scan stdin as, if docov reads stdin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StdinFilename(pub Option<PathBuf>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Scan stdin as the file at this path, whose extension picks the language, in place of the paths
    // What writes files from the scan, rescans or picks files by git would act on one buffer.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with_all = ["add_ignore", "write_baseline", "update_threshold", "watch", "changed", "staged"]
    )]
    stdin_filename: Option<PathBuf>,
}

impl Feature for StdinFilename {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.stdin_filename)
    }
}

/// Scans `stdin` as each of `files`: the stdin filename, unless a nested config leaves it out.
pub fn scan(files: &[(&Path, &Rules)], stdin: &mut dyn Read) -> Result<Vec<FileReport>, Vec<String>> {
    let mut source = String::new();
    let mut scan = |&(path, rules): &(&Path, &Rules)| {
        stdin.read_to_string(&mut source).map_err(|source| collect::Error::Read { path: path.to_owned(), source })?;
        collect::scan_source(path, &source, rules)
    };
    files.iter().map(&mut scan).collect::<Result<_, _>>().map_err(|e| vec![e.to_string()])
}
