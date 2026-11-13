//! tsdoc.json: the tags and HTML elements a project's TSDoc defines, read the way
//! `@microsoft/tsdoc-config` reads them, with the files it `extends`, and found the way it finds
//! one for a source file.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use indexmap::IndexMap;
use oxc_resolver::{ResolveOptions, Resolver};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use serde_json::Value;

use super::{Configuration, SyntaxKind, TagDefinition, parse::valid_tag_name};
use crate::root::Root;

/// The file's name.
pub const FILE_NAME: &str = "tsdoc.json";

/// The `$schema` a tsdoc.json has to have.
pub const SCHEMA: &str = "https://developer.microsoft.com/json-schemas/tsdoc/v0/tsdoc.schema.json";

/// A tsdoc.json as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct File {
    #[serde(rename = "$schema")]
    _schema: String,
    #[serde(default)]
    extends: Vec<String>,
    no_standard_tags: Option<bool>,
    #[serde(default)]
    tag_definitions: Vec<Tag>,
    #[serde(default)]
    support_for_tags: IndexMap<String, bool>,
    supported_html_elements: Option<Vec<String>>,
    report_unsupported_html_elements: Option<bool>,
}

/// A tag a tsdoc.json defines.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Tag {
    tag_name: String,
    syntax_kind: SyntaxKind,
    #[serde(default)]
    allow_multiple: bool,
}

/// A tsdoc.json and the files it extends, read.
struct Loaded {
    path: PathBuf,
    file: File,
    extends: Vec<Loaded>,
}

/// Why a tsdoc.json can't be used: its path, TSDoc's message id and what's wrong.
fn error(path: &Path, id: &str, text: impl std::fmt::Display) -> String {
    format!("{}: {id}: {text}", path.display())
}

/// The configuration the tsdoc.json at `path` sets up, and the files it read, `path` first.
pub fn load(path: &Path) -> Result<(Configuration, Vec<PathBuf>), String> {
    let resolver = Resolver::new(ResolveOptions::default());
    let loaded = read(path, &resolver, &mut Vec::new())?;
    let standard = !no_standard_tags(&loaded).unwrap_or(false);
    let mut configuration = Configuration::standard(standard);
    configure(&loaded, &mut configuration)?;
    let mut sources = Vec::new();
    paths(&loaded, &mut sources);
    Ok((configuration, sources))
}

/// Reads the file at `path`, and the files it extends. `chain` holds the real paths of the files
/// that extend it, to stop at a cycle.
fn read(path: &Path, resolver: &Resolver, chain: &mut Vec<PathBuf>) -> Result<Loaded, String> {
    let not_found = |e: std::io::Error| error(path, "tsdoc-config-file-not-found", e);
    let real = dunce::canonicalize(path).map_err(not_found)?;
    if chain.contains(&real) {
        return Err(error(path, "tsdoc-config-cyclic-extends", "it extends itself through the files it extends"));
    }
    let mut text = fs::read_to_string(path).map_err(not_found)?;
    let invalid = |e: &dyn std::fmt::Display| error(path, "tsdoc-config-invalid-json", e);
    json_strip_comments::strip(&mut text).map_err(|e| invalid(&e))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| invalid(&e))?;
    if value.get("$schema").and_then(Value::as_str) != Some(SCHEMA) {
        return Err(error(path, "tsdoc-config-unsupported-schema", format!("expected a \"$schema\" of \"{SCHEMA}\"")));
    }
    let file = File::deserialize(value).map_err(|e| error(path, "tsdoc-config-schema-error", e))?;
    let html_name = |name: &String| name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if let Some(name) = file.supported_html_elements.iter().flatten().find(|name| html_name(name)) {
        return Err(error(path, "tsdoc-config-schema-error", format!("\"{name}\" isn't an HTML element name")));
    }
    let mut names: Vec<String> = Vec::new();
    for tag in &file.tag_definitions {
        if !valid_tag_name(&tag.tag_name) {
            let text = format!("\"{}\" isn't a tag name: an @, a letter, then letters and digits", tag.tag_name);
            return Err(error(path, "tsdoc-config-invalid-tag-name", text));
        }
        let upper = tag.tag_name.to_ascii_uppercase();
        if names.contains(&upper) {
            let text = format!("\"tagDefinitions\" defines \"{}\" more than once", tag.tag_name);
            return Err(error(path, "tsdoc-config-duplicate-tag-name", text));
        }
        names.push(upper);
    }
    let dir = real.parent().expect("a file is in a folder").to_owned();
    chain.push(real);
    let mut extends = Vec::new();
    for specifier in &file.extends {
        let resolved = resolver.resolve(&dir, specifier).map_err(|e| {
            error(path, "tsdoc-config-unresolved-extends", format!("can't resolve \"{specifier}\": {e}"))
        })?;
        extends.push(read(resolved.path(), resolver, chain)?);
    }
    chain.pop();
    Ok(Loaded { path: path.to_owned(), file, extends })
}

/// `noStandardTags`, from the file or else the last of the files it extends that sets it.
fn no_standard_tags(loaded: &Loaded) -> Option<bool> {
    loaded.file.no_standard_tags.or_else(|| loaded.extends.iter().rev().find_map(no_standard_tags))
}

/// Adds what `loaded` sets to `configuration`, after what the files it extends set.
fn configure(loaded: &Loaded, configuration: &mut Configuration) -> Result<(), String> {
    for base in &loaded.extends {
        configure(base, configuration)?;
    }
    let File { tag_definitions, support_for_tags, supported_html_elements, report_unsupported_html_elements, .. } =
        &loaded.file;
    for tag in tag_definitions {
        if let Some(defined) = configuration.tag(&tag.tag_name) {
            let text = format!("\"{}\" is defined already, as \"{}\"", tag.tag_name, defined.name);
            return Err(error(&loaded.path, "tsdoc-config-duplicate-tag-name", text));
        }
        let (name, kind, allow_multiple) = (tag.tag_name.clone(), tag.syntax_kind, tag.allow_multiple);
        configuration.tags.push(TagDefinition { name, kind, allow_multiple });
    }
    for (name, &supported) in support_for_tags {
        let Some(defined) = configuration.tag(name) else {
            let text = format!("\"supportForTags\" names \"{name}\", which isn't defined");
            return Err(error(&loaded.path, "tsdoc-config-undefined-tag", text));
        };
        let upper = defined.name.to_ascii_uppercase();
        let set = configuration.supported.get_or_insert_default();
        set.retain(|tag| *tag != upper);
        if supported {
            set.push(upper);
        }
    }
    if let Some(html) = supported_html_elements {
        configuration.html.clone_from(html);
        configuration.report_unsupported_html = true;
    }
    if let Some(report) = report_unsupported_html_elements {
        configuration.report_unsupported_html = *report;
    }
    Ok(())
}

/// The paths of `loaded` and the files it extends.
fn paths(loaded: &Loaded, out: &mut Vec<PathBuf>) {
    out.push(loaded.path.clone());
    loaded.extends.iter().for_each(|base| paths(base, out));
}

/// The tsdoc.json `@microsoft/tsdoc-config` reads for a file in `folder`: the one in the nearest
/// folder from there up that has a tsconfig.json or a package.json, if it's there, and the files
/// it looked for to find it. It looks above the current folder too, for a scan run inside a
/// package.
pub fn find(folder: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
    let folder = Root::new(Path::new("")).absolute(folder);
    let mut looked = Vec::new();
    for dir in folder.ancestors() {
        for marker in ["tsconfig.json", "package.json"] {
            looked.push(dir.join(marker));
            if dir.join(marker).is_file() {
                let config = dir.join(FILE_NAME);
                looked.push(config.clone());
                return (config.is_file().then_some(config), looked);
            }
        }
    }
    (None, looked)
}

/// The configuration each scanned file's comments are read with.
#[derive(Debug, Default)]
pub struct Configs {
    /// The one every file is read with, when the config names one.
    every: Option<Arc<Configuration>>,
    /// The one each folder's files are read with, by the folder, when it isn't the default.
    folders: FxHashMap<PathBuf, Arc<Configuration>>,
    /// The files read, and looked for, to find them.
    pub sources: Vec<PathBuf>,
}

impl Configs {
    /// The tsdoc.json at `path`, for every file.
    pub fn file(path: &Path) -> Result<Self, String> {
        let (configuration, sources) = load(path)?;
        Ok(Self { every: Some(Arc::new(configuration)), folders: FxHashMap::default(), sources })
    }

    /// The tsdoc.json [found](find) for each of `files`, each one read once.
    pub fn find<'p>(files: impl IntoIterator<Item = &'p Path>) -> Result<Self, String> {
        let mut configs = Self::default();
        let (mut seen, mut read) = (FxHashSet::default(), FxHashMap::default());
        for file in files {
            let folder = file.parent().unwrap_or(file);
            if !seen.insert(folder.to_owned()) {
                continue;
            }
            let (found, looked) = find(folder);
            configs.sources.extend(looked);
            let Some(found) = found else { continue };
            let configuration: Arc<Configuration> = match read.get(&found) {
                Some(configuration) => Arc::clone(configuration),
                None => {
                    let (configuration, sources) = load(&found)?;
                    configs.sources.extend(sources);
                    Arc::clone(read.entry(found).or_insert(Arc::new(configuration)))
                }
            };
            configs.folders.insert(folder.to_owned(), configuration);
        }
        Ok(configs)
    }

    /// The configuration the file at `path` is read with: TSDoc's default when nothing sets
    /// another.
    pub fn of(&self, path: &Path) -> &Configuration {
        let folder = path.parent().and_then(|folder| self.folders.get(folder));
        self.every.as_deref().or(folder.map(Arc::as_ref)).unwrap_or(&super::DEFAULT)
    }
}
