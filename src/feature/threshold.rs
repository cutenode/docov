//! `--threshold` and `threshold`: the documented percentages under which the scan fails, overall,
//! per file and per glob.

use std::{borrow::Cow, fmt};

use indexmap::IndexMap;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor, value::MapAccessDeserializer},
};

use crate::{
    FileReport, Totals, config,
    feature::{Entry, Feature, Run},
    glob::Globs,
    root::Root,
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Threshold>()
}

/// The thresholds the scan has to meet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Threshold(pub Thresholds);

/// What a group of items has to meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Limit {
    /// The lowest documented percentage that passes.
    Percent(f64),
    /// The most undocumented items that pass.
    Undocumented(usize),
}

impl Limit {
    /// A docov.json number: a percentage from 0 to 100, or a negative whole number for at most
    /// that many undocumented items (Jest's convention).
    fn new(number: f64) -> Result<Self, String> {
        if number < 0.0 && number.fract() == 0.0 {
            return Ok(Self::Undocumented(-number as usize));
        }
        config::in_range(number).map(Self::Percent).map_err(|e| e + ", or a negative whole number")
    }

    /// Why `totals` doesn't meet it, calling it `what`.
    pub fn check(self, totals: Totals, what: &str) -> Option<String> {
        match self {
            Self::Percent(threshold) => {
                let percent = totals.percent();
                (percent < threshold).then(|| format!("{percent:.1}% documented, under the {threshold}% {what}"))
            }
            Self::Undocumented(most) => {
                let undocumented = totals.total - totals.documented;
                (undocumented > most).then(|| format!("{undocumented} undocumented, over the {what} of {most}"))
            }
        }
    }
}

/// Thresholds for a group of files: one for all of them together, one for each file on its own,
/// and one for the files each glob matches, together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Thresholds {
    /// For every file together.
    pub global: Option<Limit>,
    /// For each file.
    pub per_file: Option<Limit>,
    /// For the files each glob matches, together, in docov.json's order.
    pub paths: Vec<(Globs, Limit)>,
}

impl Thresholds {
    /// Why `reports` don't meet them, a line for each failure.
    pub fn check(&self, reports: &[&FileReport], root: &Root) -> Vec<String> {
        let mut failures = Vec::new();
        let all = reports.iter().map(|r| r.totals()).sum();
        failures.extend(self.global.and_then(|limit| limit.check(all, "threshold")));
        if let Some(limit) = self.per_file {
            for report in reports {
                if let Some(failure) = limit.check(report.totals(), "per-file threshold") {
                    failures.push(format!("{}: {failure}", report.path.display()));
                }
            }
        }
        for (globs, limit) in &self.paths {
            let matched = reports.iter().filter(|r| globs.contain(&root.relative(&r.path))).map(|r| r.totals()).sum();
            if let Some(failure) = limit.check(matched, "threshold") {
                failures.push(format!("{globs}: {failure}"));
            }
        }
        failures
    }
}

/// A number is the global threshold; an object has `global`, `perFile`, and globs.
impl<'de> Deserialize<'de> for Thresholds {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ThresholdsVisitor)
    }
}

struct ThresholdsVisitor;

impl<'de> Visitor<'de> for ThresholdsVisitor {
    type Value = Thresholds;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a number, or an object of them")
    }

    fn visit_f64<E: de::Error>(self, number: f64) -> Result<Thresholds, E> {
        Ok(Thresholds { global: Some(Limit::new(number).map_err(E::custom)?), ..Thresholds::default() })
    }

    fn visit_u64<E: de::Error>(self, number: u64) -> Result<Thresholds, E> {
        self.visit_f64(number as f64)
    }

    fn visit_i64<E: de::Error>(self, number: i64) -> Result<Thresholds, E> {
        self.visit_f64(number as f64)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Thresholds, A::Error> {
        let numbers = IndexMap::<String, f64>::deserialize(MapAccessDeserializer::new(map))?;
        let mut thresholds = Thresholds::default();
        for (key, number) in numbers {
            let limit = Limit::new(number).map_err(de::Error::custom)?;
            match key.as_str() {
                "global" => thresholds.global = Some(limit),
                "perFile" => thresholds.per_file = Some(limit),
                _ => thresholds.paths.push((Globs::new(vec![key]).map_err(de::Error::custom)?, limit)),
            }
        }
        Ok(thresholds)
    }
}

impl JsonSchema for Limit {
    fn inline_schema() -> bool {
        true
    }
    fn schema_name() -> Cow<'static, str> {
        "Limit".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                { "type": "number", "minimum": 0, "maximum": 100 },
                { "type": "integer", "maximum": -1 }
            ]
        })
    }
}

impl JsonSchema for Thresholds {
    fn inline_schema() -> bool {
        true
    }
    fn schema_name() -> Cow<'static, str> {
        "Thresholds".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let schema = generator.subschema_for::<Limit>();
        let limit = |description: &str| {
            let mut limit = schema.clone();
            limit.insert("description".into(), description.into());
            limit
        };
        let number = "A documented percentage from 0 to 100, or a negative whole number for at most that many undocumented items.";
        json_schema!({
            "anyOf": [
                limit(number),
                {
                    "type": "object",
                    "properties": {
                        "global": limit("For every file together."),
                        "perFile": limit("For each file on its own."),
                    },
                    "additionalProperties": limit("For the files a gitignore-style glob matches, relative to this file's folder, together."),
                }
            ]
        })
    }
}

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Fail under this documented percentage, overall
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    threshold: Option<f64>,
}

/// The key.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// The lowest documented percentage that passes, or thresholds by path: `global` for every file together, `perFile` for each file, and gitignore-style globs for the files each matches together.
    threshold: Option<Thresholds>,
}

impl Feature for Threshold {
    type Flags = Flags;
    type Keys = Keys;

    fn new(flags: Flags, keys: Keys) -> Self {
        let mut thresholds = keys.threshold.unwrap_or_default();
        if let Some(percent) = flags.threshold {
            thresholds.global = Some(Limit::Percent(percent));
        }
        Self(thresholds)
    }

    fn gate(&self, run: &Run) -> Vec<String> {
        self.0.check(&run.reports.iter().collect::<Vec<_>>(), run.root)
    }
}
