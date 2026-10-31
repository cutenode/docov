//! The rules a language server's document is scanned with: by the config `docov` finds where it
//! would run for that file, with nested configs, and none for a file a scan of the project leaves
//! out. Each document's rules, and what each package's entry points reach, are kept until a file
//! they were read from changes, or one docov looked for turns up: the client says so when it
//! watches files for the server, and otherwise each one's modified time and size tell.

use std::{
    fs,
    hash::Hash,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use clap::ArgMatches;
use globset::{Glob, GlobSet, GlobSetBuilder};
use rustc_hash::FxHashMap;

use crate::{
    Config, Rules, Settings,
    config::{self, FILE_NAMES},
    feature::{self, jsdoc_config::JsdocConfig, overrides::Overrides},
    nested::Plan,
    public::Api,
    walk,
};

/// The files the server asks the client to watch: every config, package.json, tsconfig.json and
/// module docov reads, by their names.
pub(super) const WATCHED: [&str; 2] = ["**/*.{json,jsonc,js,jsx,mjs,cjs,ts,tsx,mts,cts}", "**/.docovrc"];

/// How far behind the clock a file's modified time may be: a file system stamps a change with a
/// clock that ticks every few milliseconds, so a file stamped this close to a read may have
/// changed during it, to a modified time and size that then look the same.
const LAG: Duration = Duration::from_millis(25);

/// Each document's rules and each package's public API, kept until what they were read from
/// changes.
pub(super) struct Projects {
    /// The workspace folders, which docov finds a document's config from as if it ran there.
    roots: Vec<PathBuf>,
    /// No flags, which the settings are made from with the config's keys.
    matches: ArgMatches,
    /// [`WATCHED`].
    watched: GlobSet,
    /// Whether the client says when a file `watched` matches under `roots` changes.
    pub(super) watching: bool,
    /// Each document's rules, or none when its config leaves it out, by its path.
    rules: FxHashMap<PathBuf, Kept<Option<Rules>>>,
    /// What each package's entry points reach, by the folder and paths [`Api::load`] took.
    apis: FxHashMap<(PathBuf, Vec<PathBuf>), Kept<Arc<Api>>>,
}

/// A value, and what it was read from.
struct Kept<T> {
    value: T,
    sources: Sources,
}

/// Each file a value was read from, and each one looked for, as it was then.
type Sources = FxHashMap<PathBuf, Seen>;

/// A file or folder as it was when a value was read from it.
#[derive(Clone, Copy)]
struct Seen {
    stamp: Stamp,
    /// Whether the client says when it changes: it isn't a folder, and it's under a workspace
    /// folder by a name the watcher matches.
    watched: bool,
}

/// A file's modified time and size, or none when it isn't there.
type Stamp = Option<(Option<SystemTime>, u64)>;

/// What a value is being read from.
struct Reading {
    /// When the reading started.
    since: SystemTime,
    sources: Sources,
    /// Whether a file changed so close to `since` that it may have changed during the reading,
    /// when the value can't be kept.
    racy: bool,
}

impl Reading {
    fn new(since: SystemTime) -> Self {
        Self { since, sources: Sources::default(), racy: false }
    }

    /// Keeps `value` in `kept` by `key`, unless a file may have changed while it was read.
    fn keep<K: Eq + Hash, T: Clone>(self, kept: &mut FxHashMap<K, Kept<T>>, key: K, value: T) -> T {
        if !self.racy {
            kept.insert(key, Kept { value: value.clone(), sources: self.sources });
        }
        value
    }
}

impl Projects {
    /// Nothing kept yet, for a client whose workspace folders are `roots`.
    pub(super) fn new(roots: Vec<PathBuf>) -> Self {
        let mut watched = GlobSetBuilder::new();
        for glob in WATCHED {
            watched.add(Glob::new(glob).expect("the watched globs parse"));
        }
        let watched = watched.build().expect("the watched globs build");
        let matches = feature::args(clap::Command::new("docov")).get_matches_from(["docov"]);
        Self { roots, matches, watched, watching: false, rules: FxHashMap::default(), apis: FxHashMap::default() }
    }

    /// Forgets what was read from, or looked for at, `path`, which changed.
    pub(super) fn changed(&mut self, path: &Path) {
        self.rules.retain(|_, kept| !kept.sources.contains_key(path));
        self.apis.retain(|_, kept| !kept.sources.contains_key(path));
    }

    /// Forgets the rules of the document at `path`, which closed.
    pub(super) fn forget(&mut self, path: &Path) {
        self.rules.remove(path);
    }

    /// The rules for the file at `path`, an absolute one, or none when the config's `paths`, its
    /// excludes or a nested config's leave it out.
    pub(super) fn rules(&mut self, path: &Path) -> Result<Option<Rules>, String> {
        if let Some(kept) = self.rules.get(path).filter(|kept| self.fresh(&kept.sources)) {
            return Ok(kept.value.clone());
        }
        let mut reading = Reading::new(SystemTime::now());
        let rules = self.load(path, &mut reading)?;
        Ok(reading.keep(&mut self.rules, path.to_owned(), rules))
    }

    /// Reads the rules for the file at `path`. docov finds the config as if it ran in the deepest
    /// of the workspace folders the file is in, or else in the file's folder.
    fn load(&mut self, path: &Path, reading: &mut Reading) -> Result<Option<Rules>, String> {
        let folder = path.parent().unwrap_or(path);
        self.see(reading, looked_for(folder));
        let roots = self.roots.iter().filter(|root| path.starts_with(root));
        let here = roots.max_by_key(|root| root.components().count()).map_or(folder, PathBuf::as_path).to_owned();
        let file = config::nearest(&here).map(|(up, name)| here.ancestors().nth(up).expect("found there").join(name));
        let mut config = match file {
            Some(file) => Config::load(Some(&file)).map_err(|e| e.to_string())?,
            None => Config::default(),
        };
        self.see(reading, config.files.iter().chain(&config.jsdoc_config).cloned());
        let settings = Settings::new(&self.matches, std::mem::take(&mut config.keys));
        config.read_jsdoc(settings.get::<JsdocConfig>().0.as_deref())?;
        let root = config.root();
        let inside = |dir: &PathBuf| path.starts_with(root.absolute(dir));
        let excluded = settings.get::<Overrides>().clone().excluder(config.exclude.clone(), &root);
        if !(config.paths.is_empty() || config.paths.iter().any(inside)) || walk::excludes(&excluded, path) {
            return Ok(None);
        }
        let mut plan = Plan::new(&config, settings, &self.matches, vec![path.to_owned()])?;
        self.see(reading, plan.groups.iter().flat_map(|group| group.configs.clone()));
        // With no paths, the public scope looks for package.json above where docov runs.
        let paths = if config.paths.is_empty() { vec![here] } else { config.paths.clone() };
        plan.load_public_with(&paths, |dir, paths| self.api(dir, paths, reading))
            .map_err(|errors| errors.join("\n"))?;
        Ok(plan.files().first().map(|(_, rules)| (*rules).clone()))
    }

    /// What the entry points of the package.json [`Api::load`] finds from `dir` and `paths` reach,
    /// adding what it was read from to `reading`.
    fn api(&mut self, dir: &Path, paths: &[PathBuf], reading: &mut Reading) -> Result<Arc<Api>, Vec<String>> {
        let key = (dir.to_owned(), paths.to_vec());
        if let Some(kept) = self.apis.get(&key).filter(|kept| self.fresh(&kept.sources)) {
            reading.sources.extend(kept.sources.clone());
            return Ok(Arc::clone(&kept.value));
        }
        let mut own = Reading::new(reading.since);
        let api = Arc::new(Api::load(dir, paths)?);
        let starts = std::iter::once(dir).chain(paths.iter().map(PathBuf::as_path));
        self.see(&mut own, starts.flat_map(looked_for).chain(api.sources().iter().cloned()));
        reading.racy |= own.racy;
        reading.sources.extend(own.sources.clone());
        Ok(own.keep(&mut self.apis, key, api))
    }

    /// Adds each of `paths` to `reading`, as it is now.
    fn see(&self, reading: &mut Reading, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            let (stamp, folder) = look(&path);
            let modified = stamp.and_then(|(modified, _)| modified);
            reading.racy |= modified.is_some_and(|modified| modified + LAG >= reading.since);
            let watched = !folder && self.watches(&path);
            reading.sources.insert(path, Seen { stamp, watched });
        }
    }

    /// Whether the client's watcher would say when the file at `path` changes: it's under a
    /// workspace folder, by a name [`WATCHED`] matches. The client names a file by its path
    /// there, which has no `..`, and editors leave node_modules out of what they watch.
    fn watches(&self, path: &Path) -> bool {
        let plain = path.components().all(|c| c != Component::ParentDir && c.as_os_str() != "node_modules");
        plain && self.roots.iter().any(|root| path.starts_with(root)) && self.watched.is_match(path)
    }

    /// Whether nothing in `sources` changed: the client said nothing about those it watches, and
    /// the others look as they did.
    fn fresh(&self, sources: &Sources) -> bool {
        sources.iter().all(|(path, seen)| (self.watching && seen.watched) || look(path).0 == seen.stamp)
    }
}

/// The file at `path` as it is now, and whether it's a folder.
fn look(path: &Path) -> (Stamp, bool) {
    let metadata = fs::metadata(path).ok();
    (metadata.as_ref().map(|m| (m.modified().ok(), m.len())), metadata.is_some_and(|m| m.is_dir()))
}

/// The config files and package.json docov looks for in `folder` and each folder above it.
fn looked_for(folder: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let names = FILE_NAMES.into_iter().chain(["package.json"]);
    folder.ancestors().flat_map(move |dir| names.clone().map(move |name| dir.join(name)))
}
