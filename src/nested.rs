//! Nested configs: with `"nested": true` in the root config, each file is scanned with the nearest
//! config file in a folder between it and the root config's, merged over the root one, the way
//! oxlint's nested configs work.

use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::ArgMatches;
use rustc_hash::FxHashMap;

use crate::{Config, FileReport, Scope, Settings, config::FILE_NAMES, feature::nested::Nested, walk};

/// Files scanned with the same settings.
pub struct Group {
    /// The nested config file they're scanned with, or none for the root config.
    pub config: Option<PathBuf>,
    /// Its settings.
    pub settings: Settings,
    /// Its files, sorted.
    pub files: Vec<PathBuf>,
}

/// The files to scan, by the settings each is scanned with.
pub struct Plan {
    /// The root config's group first, then each nested config's.
    pub groups: Vec<Group>,
}

impl Plan {
    /// Puts each of `files`, sorted, in the group of the config it's scanned with, leaving out
    /// those its nested config excludes. Without `nested`, that's the root config for all of them.
    pub fn new(root: &Config, settings: Settings, matches: &ArgMatches, files: Vec<PathBuf>) -> Result<Self, String> {
        if !settings.get::<Nested>().0 {
            return Ok(Self { groups: vec![Group { config: None, settings, files }] });
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
        let mut groups = vec![Group { config: None, settings, files }];
        for (config, mut files) in members {
            let path = config.expect("only the root group has no config");
            let nested = root.under(&path).map_err(|e| e.to_string())?;
            walk::retain(&mut files, &nested.exclude).map_err(|e| e.to_string())?;
            groups.push(Group { config: Some(path), settings: Settings::new(matches, nested.keys), files });
        }
        Ok(Self { groups })
    }

    /// The root config's settings.
    pub fn settings(&self) -> &Settings {
        &self.groups[0].settings
    }

    /// Each file to scan, with the scope it's scanned with, sorted.
    pub fn files(&self) -> Vec<(&Path, Scope)> {
        let mut files: Vec<_> = self
            .groups
            .iter()
            .flat_map(|group| group.files.iter().map(|file| (file.as_path(), *group.settings.get::<Scope>())))
            .collect();
        files.sort_by_key(|&(file, _)| file);
        files
    }

    /// Why the scan fails: each group's gates over its own files, naming its config file if it's
    /// a nested one.
    pub fn gates(&self, reports: &[FileReport]) -> Vec<String> {
        let gates = |group: &Group| {
            let own: Vec<_> = reports.iter().filter(|r| group.files.binary_search(&r.path).is_ok()).cloned().collect();
            let named = |failure| match &group.config {
                Some(path) => format!("{}: {failure}", path.display()),
                None => failure,
            };
            group.settings.gates(&own).into_iter().map(named).collect::<Vec<_>>()
        };
        self.groups.iter().flat_map(gates).collect()
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
