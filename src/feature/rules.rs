//! `--rule` and `rules`: which doc checks are on, at what level, and with what options. Every check
//! is off unless one of them turns it on. A check's findings are printed as
//! `docov: path:line:column: rule-id: message`, and those at `error` fail the scan.

use std::{borrow::Cow, sync::Arc};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, de};
use serde_json::Value;

use crate::{
    FileReport, Rules,
    check::{self, Checks, Level, Setting},
    feature::{Entry, Feature},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<DocChecks>()
}

/// The doc checks that are on, if any are.
#[derive(Clone, Debug, Default)]
pub struct DocChecks(pub Option<Arc<Checks>>);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Set a doc check's level (off, warn or error), over the config's (repeatable)
    #[arg(long = "rule", value_name = "ID=LEVEL", value_parser = check::flag)]
    rules: Vec<(String, Level)>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// Doc checks to turn on, by rule id: "off", "warn" or "error", or that level with the check's options, as ["warn", { … }]. Every check is off by default.
    rules: Settings,
}

/// Each rule's setting, in the order docov.json gives them.
#[derive(Debug, Default, PartialEq)]
pub struct Settings(Vec<(String, Setting)>);

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rules = serde_json::Map::<String, Value>::deserialize(deserializer)?;
        let parse = |(id, value): (String, Value)| Setting::parse(&id, &value).map(|setting| (id, setting));
        rules.into_iter().map(parse).collect::<Result<_, _>>().map(Self).map_err(de::Error::custom)
    }
}

impl JsonSchema for Settings {
    fn schema_name() -> Cow<'static, str> {
        "Rules".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        check::schema(generator)
    }
}

impl Feature for DocChecks {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        let mut settings = keys.rules.0;
        for (id, level) in flags.rules {
            match settings.iter_mut().find(|(known, _)| *known == id) {
                Some((_, setting)) => setting.level = level,
                None => settings.push((id, Setting { level, options: None })),
            }
        }
        Self(Checks::new(settings.iter().map(|(id, setting)| (id.as_str(), setting))).map(Arc::new))
    }

    fn rules(&self, rules: &mut Rules) {
        rules.checks.clone_from(&self.0);
    }

    /// Prints each warning, and fails the scan with each error.
    fn gate(&self, _reports: &[FileReport]) -> Option<String> {
        let (errors, warnings): (Vec<_>, Vec<_>) =
            self.0.as_ref()?.take().into_iter().partition(|d| d.severity == Level::Error);
        for warning in warnings {
            eprintln!("docov: {warning}");
        }
        (!errors.is_empty()).then(|| errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))
    }
}
