//! Nested configs: with `"nested": true` in the root config, each file is scanned with the nearest
//! config file in a folder between it and the root config's, merged over the root one, the way
//! oxlint's nested configs work.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::ArgMatches;
use indexmap::IndexMap;
use rustc_hash::FxHashMap;

use crate::{
    Config, FileReport, Rules, Scope, Settings,
    config::FILE_NAMES,
    feature::{
        baseline::{Baseline, Excused},
        nested::Nested,
        overrides::Overrides,
    },
    public::Api,
    root::Root,
    walk,
};

/// Files scanned with the same settings.
pub struct Group {
    /// The nested config file they're scanned with, or none for the root config.
    pub config: Option<PathBuf>,
    /// The config files its settings came from, each before the files it wins over.
    pub configs: Vec<PathBuf>,
    /// Its settings.
    pub settings: Settings,
    /// What counts in its files, by its settings.
    pub rules: Rules,
    /// Its folder, and those of the files it extends, which its keys' globs are relative to.
    pub root: Root,
    /// Its config file's folder, where the public scope looks for package.json.
    dir: PathBuf,
    /// The folder its baseline's path and items are relative to, as a path from the current
    /// folder: that of the file that names it, else the root config's.
    baseline_dir: PathBuf,
    /// The items its baseline lets through, if it reads one.
    excused: Option<Excused>,
    /// Its files, sorted.
    pub files: Vec<PathBuf>,
}

impl Group {
    /// The group for the files of the config `config` (the merged settings of the file at `path`,
    /// or the root config's with none), whose baseline is in `top`'s folder unless it names
    /// its own.
    fn new(
        path: Option<PathBuf>,
        config: &Config,
        settings: Settings,
        top: &Config,
        files: Vec<PathBuf>,
    ) -> Result<Self, String> {
        let baseline_dir = config.dirs.get("baseline").unwrap_or(&top.dir).clone();
        let excused = settings.get::<Baseline>().read(&baseline_dir)?;
        let (rules, root, dir) = (settings.rules(), config.root(), config.dir.clone());
        let configs = config.files.clone();
        Ok(Self { config: path, configs, settings, rules, root, dir, baseline_dir, excused, files })
    }

    /// Its own of `reports`.
    pub fn members(&self, reports: &[FileReport]) -> Vec<FileReport> {
        reports.iter().filter(|r| self.files.binary_search(&r.path).is_ok()).cloned().collect()
    }

    /// Its own of `reports`, with its baseline's items let through.
    fn own(&self, reports: &[FileReport]) -> Vec<FileReport> {
        let own = self.members(reports);
        match &self.excused {
            Some(excused) => excused.apply(&Root::new(&self.baseline_dir), own),
            None => own,
        }
    }
}

/// The files to scan, by the settings each is scanned with.
pub struct Plan {
    /// The root config's group first, then each nested config's.
    pub groups: Vec<Group>,
}

impl Plan {
    /// Puts each of `files`, sorted, in the group of the config it's scanned with, leaving out
    /// those the jsdoc config's patterns or the file's nested config excludes, and reads each
    /// group's baseline. Without `nested`, that's the root config for all of them.
    pub fn new(
        root: &Config,
        settings: Settings,
        matches: &ArgMatches,
        mut files: Vec<PathBuf>,
    ) -> Result<Self, String> {
        files.retain(|file| root.patterns.keep(file));
        if !settings.get::<Nested>().0 {
            return Ok(Self { groups: vec![Group::new(None, root, settings, root, files)?] });
        }
        let top = real(&root.dir);
        let mut nearest_in: FxHashMap<PathBuf, Option<PathBuf>> = FxHashMap::default();
        let mut index: FxHashMap<Option<PathBuf>, usize> = FxHashMap::from_iter([(None, 0)]);
        let mut members: Vec<(Option<PathBuf>, Vec<PathBuf>)> = vec![(None, Vec::new())];
        for file in files {
            let folder = file.parent().expect("a file is in a folder");
            let config = nearest_in.entry(folder.to_owned()).or_insert_with(|| nearest(folder, &top)).clone();
            let next = members.len();
            let group = *index.entry(config.clone()).or_insert(next);
            if group == next {
                members.push((config, Vec::new()));
            }
            members[group].1.push(file);
        }

        let mut members = members.into_iter();
        let (_, files) = members.next().expect("the root group is first");
        let mut groups = vec![Group::new(None, root, settings, root, files)?];
        for (config, mut files) in members {
            let path = config.expect("only the root group has no config");
            let mut nested = root.under(&path).map_err(|e| e.to_string())?;
            let settings = Settings::new(matches, std::mem::take(&mut nested.keys));
            // The walk left out what the root config excludes; this is what the nested one does.
            let excluded = settings.get::<Overrides>().clone().excluder(nested.exclude.clone(), &nested.root());
            files.retain(|file| !walk::excludes(&excluded, file));
            groups.push(Group::new(Some(path), &nested, settings, root, files)?);
        }
        Ok(Self { groups })
    }

    /// Reads what the package's entry points reach for each group under the public scope: the
    /// package.json in its config's folder, or else the nearest one above one of `paths`.
    pub fn load_public(&mut self, paths: &[PathBuf]) -> Result<(), Vec<String>> {
        self.load_public_with(paths, |dir, paths| Api::load(dir, paths).map(Arc::new))
    }

    /// [`Plan::load_public`], with each group's API from `load`, given the folder and paths
    /// [`Api::load`] takes, for a caller that keeps them.
    pub fn load_public_with(
        &mut self,
        paths: &[PathBuf],
        mut load: impl FnMut(&Path, &[PathBuf]) -> Result<Arc<Api>, Vec<String>>,
    ) -> Result<(), Vec<String>> {
        for group in self.groups.iter_mut().filter(|group| group.rules.scope == Scope::Public) {
            group.rules.public = Some(load(&group.dir, paths)?);
        }
        Ok(())
    }

    /// The root config's settings.
    pub fn settings(&self) -> &Settings {
        &self.groups[0].settings
    }

    /// Each file to scan, with the rules it's scanned with, sorted.
    pub fn files(&self) -> Vec<(&Path, &Rules)> {
        let mut files: Vec<_> = self
            .groups
            .iter()
            .flat_map(|group| group.files.iter().map(|file| (file.as_path(), &group.rules)))
            .collect();
        files.sort_by_key(|&(file, _)| file);
        files
    }

    /// Each group's files of `reports`, with its baseline's items let through, and why the scan
    /// fails: each group's gates over those, naming its config file if it's a nested one.
    pub fn gates(&self, reports: &[FileReport]) -> (Vec<FileReport>, Vec<String>) {
        let (mut gated, mut failures) = (Vec::new(), Vec::new());
        for group in &self.groups {
            let own = group.own(reports);
            let named = |failure| match &group.config {
                Some(path) => format!("{}: {failure}", path.display()),
                None => failure,
            };
            failures.extend(group.settings.gates(&own, &group.root).into_iter().map(named));
            gated.extend(own);
        }
        (gated, failures)
    }

    /// Writes each group's undocumented items to its baseline file, the groups that share one
    /// together, and says where.
    pub fn save_baselines(&self, reports: &[FileReport]) -> Result<Vec<PathBuf>, String> {
        let mut files: IndexMap<PathBuf, (&Group, Vec<FileReport>)> = IndexMap::new();
        for group in &self.groups {
            let file = group.settings.get::<Baseline>().file(&group.baseline_dir);
            files.entry(file).or_insert((group, Vec::new())).1.extend(group.members(reports));
        }
        let save = |(group, reports): (&Group, Vec<FileReport>)| {
            let dir = &group.baseline_dir;
            group.settings.get::<Baseline>().save(dir, &Root::new(dir), &reports)
        };
        files.into_values().map(save).collect()
    }
}

/// The nearest config file in `folder` or a folder above it, below `top`, the root config's real
/// folder.
fn nearest(folder: &Path, top: &Path) -> Option<PathBuf> {
    // The real path has the same folders above it, as long as they're under `top`: the walk names
    // files with no `.` or `..` past the start.
    let real = real(folder);
    folder
        .ancestors()
        .zip(real.ancestors())
        .take_while(|(_, real)| *real != top && real.starts_with(top))
        .find_map(|(dir, _)| FILE_NAMES.iter().map(|name| dir.join(name)).find(|path| path.is_file()))
}

/// `folder`'s real path, or an empty one if it's gone, which is under no folder.
fn real(folder: &Path) -> PathBuf {
    fs::canonicalize(Path::new(".").join(folder)).unwrap_or_default()
}
