//! `--by-package`: coverage totals for each workspace package, the folders with a package.json
//! that the root package.json's `workspaces` or pnpm-workspace.yaml's `packages` list.

use std::{
    fmt::Display,
    fs,
    path::{Path, PathBuf},
};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use rustc_hash::FxHashMap;
use serde::Serialize;
use serde_json::Value;

use crate::{FileReport, Totals};

/// A workspace package's totals.
#[derive(Debug, PartialEq, Serialize)]
pub struct Package {
    /// Its folder, as a path from the current folder.
    pub path: String,
    /// Its package.json's `name`, if it has one.
    pub name: Option<String>,
    /// Its files' totals.
    #[serde(flatten)]
    pub totals: Totals,
    /// Its documented percentage.
    pub percent: f64,
}

/// The workspace packages under `root` that `reports` have files in, with those files' totals,
/// sorted by folder.
pub fn packages(root: &Path, reports: &[FileReport]) -> Result<Vec<Package>, String> {
    let (include, exclude) = patterns(root)?;
    let mut package_of: FxHashMap<PathBuf, Option<PathBuf>> = FxHashMap::default();
    let mut totals: FxHashMap<PathBuf, Totals> = FxHashMap::default();
    for report in reports {
        let Ok(path) = report.path.strip_prefix(root) else { continue };
        let folder = path.parent().expect("a file is in a folder");
        let package = package_of.entry(folder.to_owned()).or_insert_with(|| {
            // The nearest folder above the file that's a package. The root itself isn't one.
            let package = folder.ancestors().filter(|dir| !dir.as_os_str().is_empty()).find(|dir| {
                include.is_match(dir) && !exclude.is_match(dir) && root.join(dir).join("package.json").is_file()
            });
            package.map(|dir| root.join(dir))
        });
        if let Some(package) = package {
            let sum = totals.entry(package.clone()).or_default();
            let file = report.totals();
            *sum = Totals { documented: sum.documented + file.documented, total: sum.total + file.total };
        }
    }
    let mut totals: Vec<_> = totals.into_iter().collect();
    totals.sort_by(|a, b| a.0.cmp(&b.0));
    let package = |(path, totals): (PathBuf, Totals)| {
        let json = fs::read_to_string(path.join("package.json")).ok();
        let json = json.and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let name = json.and_then(|json| json["name"].as_str().map(str::to_owned));
        Package { path: path.display().to_string(), name, totals, percent: totals.percent() }
    };
    Ok(totals.into_iter().map(package).collect())
}

/// The workspace globs in `root`'s package.json and pnpm-workspace.yaml: those that match
/// packages, and those that start with `!` and leave them out.
fn patterns(root: &Path) -> Result<(GlobSet, GlobSet), String> {
    let mut globs = read(&root.join("package.json"), &|text| {
        let json: Value = serde_json::from_str(text).map_err(message)?;
        // npm and yarn take a list, and yarn 1 an object with one too.
        let list = json["workspaces"].as_array().or_else(|| json["workspaces"]["packages"].as_array());
        Ok(list.into_iter().flatten().filter_map(|glob| glob.as_str().map(str::to_owned)).collect())
    })?;
    globs.extend(read(&root.join("pnpm-workspace.yaml"), &|text| {
        let yaml = yaml_rust2::YamlLoader::load_from_str(text).map_err(message)?;
        let list = yaml.first().and_then(|doc| doc["packages"].as_vec());
        Ok(list.into_iter().flatten().filter_map(|glob| glob.as_str().map(str::to_owned)).collect())
    })?);
    let (exclude, include): (Vec<_>, Vec<_>) = globs.iter().map(String::as_str).partition(|glob| glob.starts_with('!'));
    let exclude: Vec<_> = exclude.iter().map(|glob| &glob[1..]).collect();
    Ok((set(&include)?, set(&exclude)?))
}

/// `globs` as one matcher. `*` stays in one folder, as it does for npm and pnpm, and a trailing `/`
/// adds nothing.
fn set(globs: &[&str]) -> Result<GlobSet, String> {
    let mut set = GlobSetBuilder::new();
    for glob in globs {
        set.add(GlobBuilder::new(glob.trim_end_matches('/')).literal_separator(true).build().map_err(message)?);
    }
    set.build().map_err(message)
}

/// The globs `parse` finds in the file at `path`, or none if there's no such file.
fn read(path: &Path, parse: &dyn Fn(&str) -> Result<Vec<String>, String>) -> Result<Vec<String>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    fs::read_to_string(path)
        .map_err(message)
        .and_then(|text| parse(&text))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn message(error: impl Display) -> String {
    error.to_string()
}
