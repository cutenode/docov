//! `--list-files`: prints the files docov would scan, and exits.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use crate::feature::{Entry, Feature, NoKeys};

pub(crate) fn entry() -> Entry {
    Entry::of::<ListFiles>()
}

/// Whether to list the files instead of scanning them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ListFiles(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Print the files that would be scanned, one per line, and exit
    #[arg(long)]
    list_files: bool,
}

impl Feature for ListFiles {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.list_files)
    }
}

/// Writes `files` to `out`, one per line.
pub fn write(files: &[PathBuf], out: &mut impl Write) -> io::Result<()> {
    files.iter().try_for_each(|file| writeln!(out, "{}", file.display()))
}
