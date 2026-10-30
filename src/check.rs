//! Doc checks: rules about what an item's JSDoc says, past whether it has any. Each is off unless
//! docov.json's `rules` or `--rule` turns it on, and each lives in its own file in src/check/, which
//! build.rs registers under its file name in kebab case (`param_names.rs` is `param-names`), so a
//! new check is a new file and nothing else.
//!
//! oxc_linter has eslint-plugin-jsdoc's rules, but it isn't published (its Cargo.toml says
//! `publish = false`, and crates.io has no `oxc_linter`), and its rules run on oxc_semantic inside
//! the whole linter. These are written on oxc_jsdoc, the parser oxc_linter's rules use too.

use std::{any::Any, fmt, sync::LazyLock};

use oxc_jsdoc::JSDoc;
use rustc_hash::FxHashSet;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::Value;

use crate::collect::{Shape, doc};

/// A check's rule id, and what makes its entry.
type Registered = (&'static str, fn() -> Entry);

include!(concat!(env!("OUT_DIR"), "/checks.rs"));

/// Every check, registered, by its rule id.
static CHECKS: LazyLock<Vec<(&'static str, Entry)>> =
    LazyLock::new(|| ENTRIES.iter().map(|(id, entry)| (*id, entry())).collect());

/// How much a check's findings matter.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// The check doesn't run.
    #[default]
    Off,
    /// Its findings are reported, and the scan still passes.
    Warn,
    /// Its findings are reported, and fail the scan.
    Error,
}

/// A problem a check found in an item's block.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Diagnostic {
    /// 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
    /// The check's rule id.
    #[serde(deserialize_with = "rule")]
    pub rule: RuleId,
    /// Its level.
    pub severity: Level,
    /// What's wrong.
    pub message: String,
}

/// Its place in the file and what it says: `3:11: param-names: …`, which goes after the file's path.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self { line, column, rule, message, .. } = self;
        write!(f, "{line}:{column}: {rule}: {message}")
    }
}

/// A registered rule id. Written out in a field's type, `&'static str` would have serde read the
/// field borrowed from its input, which can't live that long, though `rule` reads it.
pub type RuleId = &'static str;

/// Reads a rule id, as the cache keeps it, as the registered one.
fn rule<'de, D: Deserializer<'de>>(deserializer: D) -> Result<&'static str, D::Error> {
    let id = String::deserialize(deserializer)?;
    entry(&id).map(|(id, _)| id).map_err(serde::de::Error::custom)
}

/// An item with a JSDoc block, as a check sees it.
pub struct Subject<'a, 's> {
    /// Its name, `Class.member` for a member.
    pub name: &'a str,
    /// Whether it's exported, by the ways of exporting that count.
    pub exported: bool,
    /// Its block.
    pub doc: &'a JSDoc<'s>,
    /// For a function or method, each parameter's name, or `None` for a destructured one.
    pub params: Option<&'a [Option<&'s str>]>,
    /// What its code says.
    pub shape: &'a Shape<'s>,
    /// The file's text, which the block's spans are offsets into.
    pub source: &'s str,
    /// The names the file declares or imports at its top level, and the types its blocks declare.
    pub names: &'a FxHashSet<&'s str>,
}

impl Subject<'_, '_> {
    /// Where its block starts: its `/**`.
    pub fn block(&self) -> u32 {
        // The block's span starts past its `/**`.
        self.doc.span.start - 3
    }

    /// Where its description's text starts, if it has one.
    pub fn description(&self) -> Option<u32> {
        lines(self.source, self.doc.comment().span).find(|(_, line)| !line.is_empty()).map(|(at, _)| at)
    }

    /// Whether it has any of the tags `names` (without their `@`).
    pub fn has(&self, names: &[&str]) -> bool {
        names.iter().any(|name| doc::has_tag(self.doc, name))
    }

    /// Whether its block inherits its docs with `@inheritdoc` (in any case), so the checks that
    /// want tags leave it be.
    pub fn inherits(&self) -> bool {
        self.doc.tags().iter().any(|tag| tag.kind.parsed().eq_ignore_ascii_case("inheritdoc"))
    }

    /// Adds a finding at its block that it's missing `what`, unless it inherits its docs.
    pub fn missing(&self, what: String, out: &mut Vec<Finding>) {
        if !self.inherits() {
            out.push(Finding { at: self.block(), message: format!("missing {what}") });
        }
    }
}

/// What a check found: a message at a byte offset into the file.
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    /// Where, in bytes.
    pub at: u32,
    /// What's wrong.
    pub message: String,
}

/// A doc check. Register one by giving its file a
/// `pub(crate) fn entry() -> Entry { Entry::of::<YourCheck>() }`.
pub trait Check: 'static {
    /// Its options, the second item of `"rules": { "id": [level, options] }`.
    type Options: DeserializeOwned + JsonSchema + Default + Send + Sync + 'static;

    /// What it wants, in a line, for the schema.
    const ABOUT: &'static str;

    /// Adds what it finds wrong with `subject`'s block to `out`.
    fn check(options: &Self::Options, subject: &Subject, out: &mut Vec<Finding>);
}

/// The options of a check that has none.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoOptions {}

/// Options, of a type only their check's entry knows.
type AnyOptions = Box<dyn Any + Send + Sync>;

/// A check, registered.
pub struct Entry {
    about: &'static str,
    schema: fn(&mut SchemaGenerator) -> Schema,
    options: fn(Option<&Value>) -> serde_json::Result<AnyOptions>,
    run: fn(&AnyOptions, &Subject, &mut Vec<Finding>),
}

impl Entry {
    /// The entry for `C`.
    pub fn of<C: Check>() -> Self {
        Self {
            about: C::ABOUT,
            schema: |generator| generator.subschema_for::<C::Options>(),
            options: options::<C>,
            run: run::<C>,
        }
    }
}

fn options<C: Check>(value: Option<&Value>) -> serde_json::Result<AnyOptions> {
    let options: C::Options = value.map(C::Options::deserialize).transpose()?.unwrap_or_default();
    Ok(Box::new(options))
}

fn run<C: Check>(options: &AnyOptions, subject: &Subject, out: &mut Vec<Finding>) {
    C::check(options.downcast_ref().expect("each entry makes its own options"), subject, out);
}

/// Every rule id, in file name order.
pub fn ids() -> impl Iterator<Item = &'static str> {
    CHECKS.iter().map(|(id, _)| *id)
}

/// Every rule id with what its check wants, in a line, in file name order.
pub fn catalog() -> impl Iterator<Item = (&'static str, &'static str)> {
    CHECKS.iter().map(|(id, entry)| (*id, entry.about))
}

fn entry(id: &str) -> Result<(&'static str, &'static Entry), String> {
    let known = || ids().collect::<Vec<_>>().join(", ");
    CHECKS
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(id, entry)| (*id, entry))
        .ok_or_else(|| format!("unknown rule `{id}`, expected one of {}", known()))
}

/// A rule's level, and its options, if it has some, as docov.json sets them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Setting {
    /// Its level.
    pub level: Level,
    /// Its options.
    pub options: Option<Value>,
}

impl Setting {
    /// Rule `id`'s setting in docov.json, `"off"`, `"warn"` or `"error"`, or that level with the
    /// rule's options: `["warn", { … }]`.
    pub fn parse(id: &str, value: &Value) -> Result<Self, String> {
        let (_, entry) = entry(id)?;
        let wrong = || format!("{id}: expected \"off\", \"warn\" or \"error\", or [level, options]");
        let (level, options) = match value {
            Value::Array(parts) if (1..=2).contains(&parts.len()) => (&parts[0], parts.get(1)),
            Value::Array(_) => return Err(wrong()),
            level => (level, None),
        };
        let level = Level::deserialize(level).map_err(|_| wrong())?;
        (entry.options)(options).map_err(|e| format!("{id}: {e}"))?;
        Ok(Self { level, options: options.cloned() })
    }
}

/// As docov.json writes it: the level, or the level and the options.
impl Serialize for Setting {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.options {
            None => self.level.serialize(serializer),
            Some(options) => (self.level, options).serialize(serializer),
        }
    }
}

/// Parses `--rule ID=LEVEL`.
pub fn flag(value: &str) -> Result<(String, Level), String> {
    let (id, level) = value.split_once('=').ok_or("expected ID=LEVEL")?;
    entry(id)?;
    let level =
        <Level as clap::ValueEnum>::from_str(level, false).map_err(|_| format!("{id}: expected off, warn or error"))?;
    Ok((id.to_owned(), level))
}

/// The JSON Schema of docov.json's `rules`: each rule id, with its level, or its level and options.
pub fn schema(generator: &mut SchemaGenerator) -> Schema {
    let level = generator.subschema_for::<Level>();
    let properties: serde_json::Map<String, Value> = CHECKS
        .iter()
        .map(|(id, entry)| {
            let options = (entry.schema)(generator);
            let with_options = json_schema!({
                "type": "array",
                "prefixItems": [level, options],
                "minItems": 1,
                "items": false,
            });
            let schema = json_schema!({ "description": entry.about, "anyOf": [level, with_options] });
            ((*id).to_owned(), schema.to_value())
        })
        .collect();
    json_schema!({ "type": "object", "properties": properties, "additionalProperties": false })
}

/// A check that's on.
struct On {
    rule: &'static str,
    /// Its level and options as docov.json gave them, which the cache stamps by.
    setting: Setting,
    options: AnyOptions,
    run: fn(&AnyOptions, &Subject, &mut Vec<Finding>),
}

/// The checks that are on, which scans run on each counted item with a block.
pub struct Checks {
    on: Vec<On>,
}

impl fmt::Debug for Checks {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_map().entries(self.levels()).finish()
    }
}

/// Each check's setting by its rule id, as docov.json writes it. The cache stamps by it, since the
/// levels and options change what a scan finds.
impl Serialize for Checks {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.on.iter().map(|on| (on.rule, &on.setting)))
    }
}

impl Checks {
    /// The checks `settings` turns on, with their options, or `None` when it turns none on. Each
    /// setting was [parsed](Setting::parse), or has no options.
    pub fn new<'a>(settings: impl IntoIterator<Item = (&'a str, &'a Setting)>) -> Option<Self> {
        let on: Vec<On> = settings
            .into_iter()
            .filter(|(_, setting)| setting.level != Level::Off)
            .map(|(id, setting)| {
                let (rule, entry) = entry(id).expect("a parsed rule id");
                let options = (entry.options)(setting.options.as_ref()).expect("parsed options");
                On { rule, setting: setting.clone(), options, run: entry.run }
            })
            .collect();
        (!on.is_empty()).then_some(Self { on })
    }

    /// Each check that's on, by rule id, and its level.
    pub fn levels(&self) -> impl Iterator<Item = (&'static str, Level)> {
        self.on.iter().map(|on| (on.rule, on.setting.level))
    }

    /// What each check finds in `subject`, by position, whose offsets `position` turns into lines
    /// and columns.
    pub(crate) fn run(&self, subject: &Subject, position: impl Fn(u32) -> (usize, usize)) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for on in &self.on {
            let mut findings = Vec::new();
            (on.run)(&on.options, subject, &mut findings);
            diagnostics.extend(findings.into_iter().map(|Finding { at, message }| {
                let (line, column) = position(at);
                Diagnostic { line, column, rule: on.rule, severity: on.setting.level, message }
            }));
        }
        diagnostics.sort();
        diagnostics
    }
}

/// The lines of the text at `span` in `source`, past each one's leading `*`, with where each
/// starts.
pub fn lines(source: &str, span: oxc_span::Span) -> impl Iterator<Item = (u32, &str)> {
    let text = span.source_text(source);
    text.split('\n').scan(span.start, |at, line| {
        let start = *at;
        *at += line.len() as u32 + 1;
        let trimmed = line.trim_start();
        let rest = trimmed.strip_prefix('*').map_or(trimmed, |rest| rest.strip_prefix(' ').unwrap_or(rest));
        Some((start + (line.len() - rest.len()) as u32, rest.trim_end()))
    })
}
