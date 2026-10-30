//! `--kind` and `kinds`: which kinds of item count. `--by-kind` breaks the text report's total down
//! by kind.

use std::io::{self, Write};

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    FileReport, Format, Kind, Rules, Totals,
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Kinds>()
}

/// Which kinds of item count, and whether the text report breaks its total down by kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kinds {
    /// The kinds that count, or every kind.
    pub only: Option<Vec<Kind>>,
    /// Whether the text report has a line of totals by kind.
    pub by_kind: bool,
}

/// The flags.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Count only these kinds of item, comma-separated [default: the config's, or every kind]
    #[arg(long, value_enum, value_name = "KIND", value_delimiter = ',')]
    kind: Vec<Kind>,
    /// After the text report's total, a line of totals by kind
    #[arg(long)]
    by_kind: bool,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Count only these kinds of item. Every kind counts without it.
    kinds: Option<Vec<Kind>>,
}

impl Feature for Kinds {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        let only = if flags.kind.is_empty() { keys.kinds } else { Some(flags.kind) };
        Self { only, by_kind: flags.by_kind }
    }

    fn rules(&self, rules: &mut Rules) {
        rules.kinds.clone_from(&self.only);
    }

    fn append(&self, format: Format, reports: &[FileReport], out: &mut dyn Write) -> io::Result<()> {
        if !self.by_kind || format != Format::Text {
            return Ok(());
        }
        let kinds: Vec<String> = Totals::by_kind(reports)
            .into_iter()
            .map(|(kind, totals)| format!("{} {}/{}", kind.as_str(), totals.documented, totals.total))
            .collect();
        writeln!(out, "by kind: {}", kinds.join(", "))
    }
}
