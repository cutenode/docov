//! `--add-ignore`: writes a `// docov-ignore-next-line` above every undocumented item, for adopting
//! docov in a project that's far from documented.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    FileReport, rewrite,
    feature::{Entry, Feature},
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

/// It has no key: it's for one run.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct Keys {}

impl Feature for AddIgnore {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, _keys: Keys) -> Self {
        Self(flags.add_ignore.map(|reason| match reason.trim() {
            "" => "// docov-ignore-next-line".to_owned(),
            reason => format!("// docov-ignore-next-line: {reason}"),
        }))
    }

    fn edit(&self, reports: &[FileReport]) -> Option<Result<String, String>> {
        let comment = self.0.as_deref()?;
        let mut added = 0;
        for report in reports {
            let mut lines: Vec<usize> = report.counted().filter(|i| !i.documented).map(|i| i.line).collect();
            // Items on one line are next to each other, and one comment covers them all.
            lines.dedup();
            if lines.is_empty() {
                continue;
            }
            let inserts: Vec<_> = lines.iter().map(|&line| (line, comment)).collect();
            if let Err(e) = rewrite::insert_above_in(&report.path, &inserts) {
                return Some(Err(format!("{}: {e}", report.path.display())));
            }
            added += lines.len();
        }
        Some(Ok(format!("added {added} docov-ignore-next-line comment{}", if added == 1 { "" } else { "s" })))
    }
}
