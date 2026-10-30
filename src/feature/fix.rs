//! `--fix`: writes a JSDoc stub above every undocumented item, prefilled from its code, for
//! someone to fill in.

use crate::{
    FileReport, Kind, rewrite,
    feature::{Entry, Feature, NoKeys},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Fix>()
}

/// Whether to write the stubs in place of a report.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fix(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Write a JSDoc stub, prefilled from the code, above each undocumented item with no block, and report nothing else
    // A buffer from stdin has no file to write to.
    #[arg(long, conflicts_with_all = ["add_ignore", "stdin_filename"])]
    fix: bool,
}

impl Feature for Fix {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.fix)
    }

    fn edit(&self, reports: &[FileReport]) -> Option<Result<String, String>> {
        if !self.0 {
            return None;
        }
        let added = rewrite::insert_into(reports, |report| {
            let mut stubbed: Vec<_> = report.counted().filter(|i| i.facts.stub.is_some()).collect();
            // Items that start on one line share a block. A file's own goes above them.
            stubbed.dedup_by(|b, a| a.line == b.line && a.kind != Kind::Module);
            stubbed.iter().filter_map(|i| i.facts.stub.as_ref()).map(|stub| (stub.line, stub.text.as_str())).collect()
        });
        Some(added.map(|added| format!("added {added} doc stub{}", if added == 1 { "" } else { "s" })))
    }
}
