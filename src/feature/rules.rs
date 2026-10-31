//! `--rule` and `rules`: which doc checks are on, at what level, and with what options. Every check
//! is off unless one of them turns it on. A check's findings go to stderr as
//! `docov: path:line:column: rule-id: message` and to the formats that list problems, and those at
//! `error` fail the scan.

use std::{borrow::Cow, sync::Arc};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value, json};

use crate::{
    Rules,
    check::{self, Checks, Level, Setting},
    feature::{Entry, Feature, Run},
    report,
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
    /// Doc checks to turn on, by rule id: "off", "warn" or "error", or that level with the check's options, as ["warn", { … }]. Every check is off by default. A rule set here replaces that rule's setting in a config this one extends or nests under, keeping its options if this gives a level alone, and leaves that config's other rules be.
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

    /// Fails the scan when a counted item has a finding at `error`. The findings themselves are
    /// reported with the items.
    fn gate(&self, run: &Run) -> Vec<String> {
        let errors = report::diagnostics(run.reports).filter(|(_, d)| d.severity == Level::Error).count();
        let plural = if errors == 1 { "" } else { "s" };
        Vec::from_iter((errors > 0).then(|| format!("the doc checks found {errors} error{plural}")))
    }

    /// Merges `rules` per rule id, the way ESLint's flat config and oxlint's nested configs do.
    fn merge(base: &Map<String, Value>, top: &mut Map<String, Value>) {
        let (Some(Value::Object(base)), Some(Value::Object(top))) = (base.get("rules"), top.get_mut("rules")) else {
            return;
        };
        let mut merged = base.clone();
        for (id, setting) in std::mem::take(top) {
            // A level alone is `"warn"` or `["warn"]`, which has no second item.
            let options = merged.get(&id).and_then(|base| base.get(1)).filter(|_| setting.get(1).is_none());
            let setting = match options {
                Some(options) => json!([setting.get(0).unwrap_or(&setting), options]),
                None => setting,
            };
            merged.insert(id, setting);
        }
        *top = merged;
    }

    fn describe(&self) -> Option<Value> {
        Some(self.0.as_deref().map_or_else(|| Value::Object(serde_json::Map::new()), |checks| {
            serde_json::to_value(checks).expect("checks serialize")
        }))
    }
}
