//! `--add-ignore`: writes a `// docov-ignore-next-line` above every undocumented item, for adopting
//! docov in a project that's far from documented.

use crate::{
    FileReport, rewrite,
    feature::{Entry, Feature, NoKeys},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<AddIgnore>()
}

/// The comment to write above each undocumented item, if there is one to write.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddIgnore(pub Option<String>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Write `// docov-ignore-next-line: REASON` (or with no reason) above each undocumented item, and report nothing else
    #[arg(long, value_name = "REASON", num_args = 0..=1, require_equals = true, default_missing_value = "")]
    add_ignore: Option<String>,
}

impl Feature for AddIgnore {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.add_ignore.map(|reason| match reason.trim() {
            "" => "// docov-ignore-next-line".to_owned(),
            reason => format!("// docov-ignore-next-line: {reason}"),
        }))
    }

    fn edit(&self, reports: &[FileReport]) -> Option<Result<String, String>> {
        let comment = self.0.as_deref()?;
        let added = rewrite::insert_into(reports, |report| {
            let mut lines: Vec<usize> = report.counted().filter(|i| !i.documented).map(|i| i.line).collect();
            // Items on one line are next to each other, and one comment covers them all.
            lines.dedup();
            lines.into_iter().map(|line| (line, comment)).collect()
        });
        Some(added.map(|added| format!("added {added} docov-ignore-next-line comment{}", if added == 1 { "" } else { "s" })))
    }
}
