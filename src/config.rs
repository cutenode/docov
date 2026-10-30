//! docov.json (or docov.jsonc, or .docovrc), the settings file.

use std::{borrow::Cow, env, fmt, fs, io, iter, path::Path, path::PathBuf, sync::OnceLock};

use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, Visitor},
};
use serde_json::Value;

use crate::feature;

/// The files `docov` reads its settings from when no `--config` is given: the first of these in
/// the current folder, or else in the nearest folder above it that has one.
pub const FILE_NAMES: [&str; 3] = ["docov.json", "docov.jsonc", ".docovrc"];

/// docov's settings. Flags on the command line win over them.
#[derive(Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(default)]
// Tells editors (VS Code's JSON service reads these) that the file may have comments and trailing
// commas, which `load` strips.
#[schemars(extend("allowComments" = true, "allowTrailingCommas" = true))]
pub struct Config {
    /// This file's JSON Schema, for editors.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    /// Files and folders to scan, relative to this file's folder. Defaults to the folder docov runs in.
    pub paths: Vec<PathBuf>,
    /// Gitignore-style globs of files to leave out, relative to this file's folder, on top of .gitignore.
    pub exclude: Vec<String>,
    /// Each feature's keys, which the features' own files define.
    #[serde(skip)]
    #[schemars(skip)]
    pub keys: feature::Keys,
    /// The folder the file is in, as a path from the current folder (empty for the current folder,
    /// or with no file). `paths` and `exclude` are relative to it.
    #[serde(skip)]
    pub dir: PathBuf,
    /// The file, as a path from the current folder, if there is one.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

/// Parses `--threshold`: a number from 0 to 100. It lives here rather than in the binary so tests
/// can call it: a bad value makes clap exit the process, and on Windows a process that exits that
/// way never writes its coverage.
pub fn percent(value: &str) -> Result<f64, String> {
    // Something that isn't a number fails the range check as NaN.
    in_range(value.parse().unwrap_or(f64::NAN))
}

/// `value`, if it's from 0 to 100. [`percent`] and docov.json's thresholds both check with this.
pub(crate) fn in_range(value: f64) -> Result<f64, String> {
    if (0.0..=100.0).contains(&value) { Ok(value) } else { Err("expected a number from 0 to 100".into()) }
}

/// The JSON Schema of the whole file: [`Config`]'s own keys, then each feature's.
fn schema() -> Value {
    let mut root = schemars::schema_for!(Config).to_value();
    for schema in feature::schemas() {
        let mut schema = schema.to_value();
        for part in ["properties", "$defs"] {
            if let Some(Value::Object(add)) = schema.get_mut(part).map(Value::take) {
                let into = root.as_object_mut().expect("a schema is an object").entry(part);
                into.or_insert_with(|| Value::Object(Default::default()))
                    .as_object_mut()
                    .expect("properties and $defs are objects")
                    .extend(add);
            }
        }
    }
    root["additionalProperties"] = false.into();
    root
}

/// Every key the file may have, in the schema's order.
fn known_keys() -> &'static [&'static str] {
    static KEYS: OnceLock<Vec<&'static str>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let schema = schema();
        let properties = schema["properties"].as_object().expect("the schema has properties");
        properties.keys().map(|key| &*key.clone().leak()).collect()
    })
}

/// Fails on a key no feature reads, at that key, the way `deny_unknown_fields` would.
struct KnownKeys;

impl<'de> DeserializeSeed<'de> for KnownKeys {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for KnownKeys {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<Cow<str>>()? {
            if !known_keys().contains(&&*key) {
                return Err(de::Error::unknown_field(&key, known_keys()));
            }
            map.next_value::<IgnoredAny>()?;
        }
        Ok(())
    }
}

/// The nearest of [`FILE_NAMES`], as a path from the current folder (`../docov.json` one folder
/// up).
fn find() -> Option<PathBuf> {
    // This fails only when the current folder is gone, and then there's nothing to find.
    let here = env::current_dir().unwrap_or_default();
    here.ancestors().enumerate().find_map(|(up, dir)| {
        let name = FILE_NAMES.iter().find(|name| dir.join(name).is_file())?;
        Some(iter::repeat_n(Path::new(".."), up).collect::<PathBuf>().join(name))
    })
}

/// Why the config file couldn't be loaded. Each variant's `path` is the file's.
#[derive(Debug, thiserror::Error)]
#[expect(missing_docs, reason = "the variants document their fields")]
pub enum Error {
    /// It couldn't be read.
    #[error("{}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    /// It has a `/` that doesn't start a `//` or `/* */` comment.
    #[error("{}: a `/` that doesn't start a comment", path.display())]
    Slash { path: PathBuf },
    /// It isn't valid JSON, or doesn't match the schema.
    #[error("{}: {source}", path.display())]
    Parse { path: PathBuf, source: serde_json::Error },
}

impl Config {
    /// Reads `path`, or else the nearest of [`FILE_NAMES`], if there is one.
    pub fn load(path: Option<&Path>) -> Result<Self, Error> {
        let Some(path) = path.map(Path::to_path_buf).or_else(find) else { return Ok(Self::default()) };
        let mut text = fs::read_to_string(&path).map_err(|source| Error::Read { path: path.clone(), source })?;
        // Comments and trailing commas turn into spaces, so serde_json's lines and columns still
        // match the file. A stray `/` stops it partway, after blanking that `/`, so it can't go on
        // to serde_json.
        if json_strip_comments::strip(&mut text).is_err() {
            return Err(Error::Slash { path });
        }
        let parse = |source| Error::Parse { path: path.clone(), source };
        KnownKeys.deserialize(&mut serde_json::Deserializer::from_str(&text)).map_err(parse)?;
        let mut config: Self = serde_json::from_str(&text).map_err(parse)?;
        config.keys = feature::Keys::parse(&text).map_err(parse)?;
        config.dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        config.path = Some(path);
        Ok(config)
    }

    /// The JSON Schema for this file, as `docov --print-schema` prints it.
    pub fn json_schema() -> String {
        serde_json::to_string_pretty(&schema()).expect("a schema serializes") + "\n"
    }
}
