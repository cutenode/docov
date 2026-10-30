//! docov.json (or docov.jsonc, or .docovrc), the settings file.

use std::{borrow::Cow, env, fmt, fs, io, iter, path::Path, path::PathBuf, sync::OnceLock};

use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, Visitor},
};
use serde_json::{Map, Value};

use crate::{
    feature,
    jsdoc::{self, Patterns},
    walk::Exclude,
};

/// The files `docov` reads its settings from when no `--config` is given: the first of these in
/// the current folder, or else in the nearest folder above it that has one.
pub const FILE_NAMES: [&str; 3] = ["docov.json", "docov.jsonc", ".docovrc"];

/// docov's settings, from a config file and the files it extends. Flags on the command line win
/// over them.
#[derive(Debug, Default, PartialEq)]
pub struct Config {
    /// The files they came from, each before the files it wins over: the config file first.
    pub files: Vec<PathBuf>,
    /// Files and folders to scan, as paths from the current folder. Empty for the current folder.
    pub paths: Vec<PathBuf>,
    /// Files to leave out, on top of .gitignore.
    pub exclude: Vec<Exclude>,
    /// The jsdoc.json whose files to scan, as a path from the current folder.
    pub jsdoc_config: Option<PathBuf>,
    /// The patterns a file's path has to match, and not match, from a jsdoc.json.
    pub patterns: Patterns,
    /// Each feature's keys, which the features' own files define.
    pub keys: feature::Keys,
    /// The same keys as JSON, for nested configs to merge over.
    json: Map<String, Value>,
    /// The config file's folder, as a path from the current folder (empty for the current folder,
    /// or with no file).
    pub dir: PathBuf,
}

/// docov's settings. Flags on the command line win over them.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(default)]
// Tells editors (VS Code's JSON service reads these) that the file may have comments and trailing
// commas, which `load` strips.
#[schemars(title = "Config", extend("allowComments" = true, "allowTrailingCommas" = true))]
struct File {
    /// This file's JSON Schema, for editors.
    #[serde(rename = "$schema")]
    schema: Option<String>,
    /// Config files this one builds on, relative to its folder. Its keys win over theirs, and a later one's over an earlier one's.
    extends: Extends,
    /// Files and folders to scan, relative to this file's folder. Defaults to the folder docov runs in.
    paths: Vec<PathBuf>,
    /// Gitignore-style globs of files to leave out, relative to this file's folder, on top of .gitignore.
    exclude: Vec<String>,
    /// A jsdoc.json, relative to this file's folder, whose `source` picks the files: its `include` is the paths unless `paths` is set, and its `exclude`, `includePattern` and `excludePattern` leave files out.
    #[serde(rename = "jsdocConfig")]
    jsdoc_config: Option<PathBuf>,
}

/// A config file to extend, or a list of them.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum Extends {
    /// One file.
    One(PathBuf),
    /// Files, each winning over the ones before it.
    Many(Vec<PathBuf>),
}

impl Default for Extends {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}

impl Extends {
    fn into_paths(self) -> Vec<PathBuf> {
        match self {
            Self::One(path) => vec![path],
            Self::Many(paths) => paths,
        }
    }
}

/// One config file's settings over those of the files it extends.
#[derive(Default)]
struct Layer {
    /// Each feature's keys, as written.
    keys: Map<String, Value>,
    /// `paths`, if a file has it.
    paths: Option<Vec<PathBuf>>,
    /// `exclude`, if a file has it.
    exclude: Option<Exclude>,
    /// `jsdocConfig`, if a file has it.
    jsdoc_config: Option<PathBuf>,
    /// The files, the topmost first.
    files: Vec<PathBuf>,
}

impl Layer {
    /// Puts `top`'s settings over these.
    fn over(&mut self, top: Self) {
        self.keys.extend(top.keys);
        self.paths = top.paths.or(self.paths.take());
        self.exclude = top.exclude.or(self.exclude.take());
        self.jsdoc_config = top.jsdoc_config.or(self.jsdoc_config.take());
        self.files = [top.files, std::mem::take(&mut self.files)].concat();
    }
}

/// Parses `--threshold`: a number from 0 to 100. It lives here rather than in the binary so tests
/// can call it: a bad value makes clap exit the process, and on Windows a process that exits that
/// way never writes its coverage.
pub fn percent(value: &str) -> Result<f64, String> {
    // Something that isn't a number fails the range check as NaN.
    in_range(value.parse().unwrap_or(f64::NAN))
}

/// `value`, if it's from 0 to 100. [`percent`] and [`percentage`] both check with this.
fn in_range(value: f64) -> Result<f64, String> {
    if (0.0..=100.0).contains(&value) { Ok(value) } else { Err("expected a number from 0 to 100".into()) }
}

/// Reads an optional percentage key, such as `threshold`: a number from 0 to 100.
pub fn percentage<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Option::<f64>::deserialize(deserializer)?.map(in_range).transpose().map_err(de::Error::custom)
}

/// The JSON Schema of the whole file: [`File`]'s own keys, then each feature's.
fn schema() -> Value {
    let mut root = schemars::schema_for!(File).to_value();
    for schema in feature::schemas() {
        let mut schema = schema.to_value();
        for part in ["properties", "$defs"] {
            if let Some(Value::Object(add)) = schema.get_mut(part).map(Value::take) {
                // The file's own schema has both, `$defs` for `extends`.
                root[part].as_object_mut().expect("the file's schema has properties and $defs").extend(add);
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

/// Why the config file couldn't be loaded. Each `path` is the file's.
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
    /// It extends itself, through the files in `chain`, which starts and ends with the same one.
    #[error("config files extend each other in a cycle: {}", chain.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" -> "))]
    Cycle { chain: Vec<PathBuf> },
}

impl Config {
    /// Reads `path`, or else the nearest of [`FILE_NAMES`], if there is one, with the files it
    /// extends.
    pub fn load(path: Option<&Path>) -> Result<Self, Error> {
        let Some(path) = path.map(Path::to_path_buf).or_else(find) else { return Ok(Self::default()) };
        Ok(Self::of(layer(&path, &mut Vec::new())?, &path))
    }

    /// The config file at `path` merged over this one, the way a nested config is: its `paths`
    /// and `exclude` are its own, with none of this one's.
    pub fn under(&self, path: &Path) -> Result<Self, Error> {
        let mut merged = Layer { keys: self.json.clone(), files: self.files.clone(), ..Layer::default() };
        merged.over(layer(path, &mut Vec::new())?);
        Ok(Self::of(merged, path))
    }

    /// The settings in `layer`, from the config file at `path`.
    fn of(layer: Layer, path: &Path) -> Self {
        let keys =
            feature::Keys::parse(&Value::Object(layer.keys.clone()).to_string()).expect("each file's keys parse");
        Self {
            files: layer.files,
            paths: layer.paths.unwrap_or_default(),
            exclude: layer.exclude.into_iter().collect(),
            jsdoc_config: layer.jsdoc_config,
            patterns: Patterns::default(),
            keys,
            json: layer.keys,
            dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        }
    }

    /// Reads the jsdoc.json at `flag`, or else at `jsdocConfig`: its `source.include` is the paths
    /// if there are none, its `source.exclude` adds to the excludes, and its patterns filter the
    /// files.
    pub fn read_jsdoc(&mut self, flag: Option<&Path>) -> Result<(), String> {
        let Some(path) = flag.map(Path::to_path_buf).or(self.jsdoc_config.take()) else { return Ok(()) };
        let sources = jsdoc::read(&path)?;
        if self.paths.is_empty() {
            self.paths = sources.paths;
        }
        self.exclude.extend(sources.exclude);
        self.patterns = sources.patterns;
        Ok(())
    }

    /// The JSON Schema for this file, as `docov --print-schema` prints it.
    pub fn json_schema() -> String {
        serde_json::to_string_pretty(&schema()).expect("a schema serializes") + "\n"
    }
}

/// The settings in the config file at `path`, over those of the files it extends. `chain` is the
/// files that extend it, each as its real path and as it was named, to stop at a cycle.
fn layer(path: &Path, chain: &mut Vec<(PathBuf, PathBuf)>) -> Result<Layer, Error> {
    let read = |source| Error::Read { path: path.to_owned(), source };
    let real = fs::canonicalize(path).map_err(read)?;
    if let Some(at) = chain.iter().position(|(seen, _)| *seen == real) {
        let names = chain[at..].iter().map(|(_, name)| name.clone());
        return Err(Error::Cycle { chain: names.chain([path.to_owned()]).collect() });
    }
    let mut text = fs::read_to_string(path).map_err(read)?;
    // Comments and trailing commas turn into spaces, so serde_json's lines and columns still
    // match the file. A stray `/` stops it partway, after blanking that `/`, so it can't go on
    // to serde_json.
    if json_strip_comments::strip(&mut text).is_err() {
        return Err(Error::Slash { path: path.to_owned() });
    }
    let parse = |source| Error::Parse { path: path.to_owned(), source };
    KnownKeys.deserialize(&mut serde_json::Deserializer::from_str(&text)).map_err(parse)?;
    let file: File = serde_json::from_str(&text).map_err(parse)?;
    // Each feature's keys are checked here, where an error can say where it is, and read once
    // every file is merged.
    feature::Keys::parse(&text).map_err(parse)?;
    let mut keys: Map<String, Value> = serde_json::from_str(&text).expect("KnownKeys read an object");
    keys.remove("$schema");
    keys.remove("extends");

    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    // Collecting the components drops `.` parts after the first (`.././src` is `../src`), which
    // would keep the excludes from matching.
    let paths = keys.remove("paths").map(|_| file.paths.iter().map(|p| dir.join(p).components().collect()).collect());
    let exclude = keys.remove("exclude").map(|_| Exclude { root: dir.clone(), globs: file.exclude });
    let jsdoc_config = file.jsdoc_config.map(|path| dir.join(path));
    keys.remove("jsdocConfig");

    chain.push((real, path.to_owned()));
    let mut merged = Layer::default();
    for base in file.extends.into_paths() {
        merged.over(layer(&dir.join(base), chain)?);
    }
    chain.pop();
    merged.over(Layer { keys, paths, exclude, jsdoc_config, files: vec![path.to_owned()] });
    Ok(merged)
}
