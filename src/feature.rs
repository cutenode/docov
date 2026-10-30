//! Features: settings from a flag, a docov.json key, or both. Each lives in its own file in
//! src/feature/, which build.rs registers, so adding one adds a file and no line in a shared list.

use std::{any::Any, fmt::Debug, sync::LazyLock};

use clap::{ArgMatches, Args, Command, FromArgMatches};
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;

use crate::{FileReport, Rules};

include!(concat!(env!("OUT_DIR"), "/features.rs"));

/// Every feature, registered.
static FEATURES: LazyLock<Vec<Entry>> = LazyLock::new(|| ENTRIES.iter().map(|entry| entry()).collect());

/// A setting, or a group of them, read from flags and docov.json. Register one by giving its file
/// a `pub(crate) fn entry() -> Entry { Entry::of::<YourFeature>() }`.
pub trait Feature: Sized + Send + Sync + 'static {
    /// Its flags. Give the struct `#[group(skip)]`: clap names a group after each struct, and
    /// every feature's is called `Flags`.
    type Flags: Args + FromArgMatches;
    /// Its keys at the top level of docov.json. Each feature reads the whole file and keeps its own.
    type Keys: DeserializeOwned + JsonSchema + Default + PartialEq + Debug + Send + Sync + 'static;

    /// The setting, from its flags and keys. A flag wins over its key.
    fn new(flags: Self::Flags, keys: Self::Keys) -> Self;

    /// Sets its part of what counts and when an item is documented.
    fn rules(&self, _rules: &mut Rules) {}

    /// Why the scan fails, when this setting fails it (exit 1). Each line is a message of its own.
    fn gate(&self, _reports: &[FileReport]) -> Option<String> {
        None
    }
}

/// A feature, registered.
pub struct Entry {
    args: fn(Command) -> Command,
    schema: fn() -> Schema,
    keys: fn(&str) -> serde_json::Result<Box<dyn AnyKeys>>,
    load: fn(&ArgMatches, Box<dyn AnyKeys>) -> Box<dyn Loaded>,
}

impl Entry {
    /// The entry for `F`.
    pub fn of<F: Feature>() -> Self {
        Self { args: F::Flags::augment_args, schema: schema::<F>, keys: keys::<F>, load: load::<F> }
    }
}

fn schema<F: Feature>() -> Schema {
    schemars::schema_for!(F::Keys)
}

fn keys<F: Feature>(text: &str) -> serde_json::Result<Box<dyn AnyKeys>> {
    Ok(Box::new(serde_json::from_str::<F::Keys>(text)?))
}

fn load<F: Feature>(matches: &ArgMatches, keys: Box<dyn AnyKeys>) -> Box<dyn Loaded> {
    let flags = F::Flags::from_arg_matches(matches).expect("the flags were added by `args`");
    let keys = keys.into_any().downcast::<F::Keys>().expect("each entry reads its own keys");
    Box::new(F::new(flags, *keys))
}

/// Adds every feature's flags to `command`. clap would make each flags struct's doc the command's
/// own, so the command keeps the about text it had.
pub fn args(command: Command) -> Command {
    let (about, long_about) = (command.get_about().cloned(), command.get_long_about().cloned());
    FEATURES
        .iter()
        .fold(command, |command, entry| (entry.args)(command))
        .about(about.unwrap_or_default())
        .long_about(long_about.unwrap_or_default())
}

/// Every feature's keys' schemas, in registration order.
pub(crate) fn schemas() -> impl Iterator<Item = Schema> {
    FEATURES.iter().map(|entry| (entry.schema)())
}

/// A feature's keys, of a type only its entry knows.
trait AnyKeys: Debug + Send + Sync {
    fn eq(&self, other: &dyn AnyKeys) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: PartialEq + Debug + Send + Sync + 'static> AnyKeys for T {
    fn eq(&self, other: &dyn AnyKeys) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Every feature's keys from docov.json, one per entry.
#[derive(Debug)]
pub struct Keys(Vec<Box<dyn AnyKeys>>);

impl Keys {
    /// Each feature's keys in `text`, which has had its comments blanked.
    pub(crate) fn parse(text: &str) -> serde_json::Result<Self> {
        FEATURES.iter().map(|entry| (entry.keys)(text)).collect::<Result<_, _>>().map(Self)
    }
}

impl Default for Keys {
    fn default() -> Self {
        Self::parse("{}").expect("every feature's keys default")
    }
}

impl PartialEq for Keys {
    fn eq(&self, other: &Self) -> bool {
        self.0.iter().zip(&other.0).all(|(a, b)| AnyKeys::eq(a.as_ref(), b.as_ref()))
    }
}

/// A feature, made.
trait Loaded: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn rules(&self, rules: &mut Rules);
    fn gate(&self, reports: &[FileReport]) -> Option<String>;
}

impl<F: Feature> Loaded for F {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn rules(&self, rules: &mut Rules) {
        Feature::rules(self, rules);
    }
    fn gate(&self, reports: &[FileReport]) -> Option<String> {
        Feature::gate(self, reports)
    }
}

/// Every feature's setting, from the flags and docov.json.
pub struct Settings(Vec<Box<dyn Loaded>>);

impl Settings {
    /// Makes each feature from its flags in `matches` and its keys.
    pub fn new(matches: &ArgMatches, keys: Keys) -> Self {
        Self(FEATURES.iter().zip(keys.0).map(|(entry, keys)| (entry.load)(matches, keys)).collect())
    }

    /// Feature `F`'s setting.
    pub fn get<F: Feature>(&self) -> &F {
        self.0.iter().find_map(|f| f.as_any().downcast_ref()).expect("every feature is registered")
    }

    /// The scan's rules, with each feature's part set.
    pub fn rules(&self) -> Rules {
        let mut rules = Rules::default();
        for feature in &self.0 {
            feature.rules(&mut rules);
        }
        rules
    }

    /// Why the scan fails, from each feature that fails it.
    pub fn gates(&self, reports: &[FileReport]) -> Vec<String> {
        self.0.iter().filter_map(|f| f.gate(reports)).collect()
    }
}
