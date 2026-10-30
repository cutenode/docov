//! The rules a language server's document is scanned with: by the config `docov` finds where it
//! would run for that file, with nested configs, and none for a file a scan of the project leaves
//! out.

use std::path::{Path, PathBuf};

use clap::ArgMatches;

use crate::{
    Config, Rules, Settings, config,
    feature::{jsdoc_config::JsdocConfig, overrides::Overrides},
    nested::Plan,
    walk,
};

/// The rules for the file at `path`, an absolute one, or none when the config's `paths`, its
/// excludes or a nested config's leave it out. docov finds the config as if it ran in the deepest
/// of `roots` (the workspace folders) the file is in, or else in the file's folder. `matches` has
/// no flags.
pub(super) fn rules(path: &Path, roots: &[PathBuf], matches: &ArgMatches) -> Result<Option<Rules>, String> {
    let folder = path.parent().unwrap_or(path);
    let roots = roots.iter().filter(|root| path.starts_with(root));
    let here = roots.max_by_key(|root| root.components().count()).map_or(folder, PathBuf::as_path);
    let file = config::nearest(here).map(|(up, name)| here.ancestors().nth(up).expect("found there").join(name));
    let mut config = match file {
        Some(file) => Config::load(Some(&file)).map_err(|e| e.to_string())?,
        None => Config::default(),
    };
    let settings = Settings::new(matches, std::mem::take(&mut config.keys));
    config.read_jsdoc(settings.get::<JsdocConfig>().0.as_deref())?;
    let root = config.root();
    let inside = |dir: &PathBuf| path.starts_with(root.absolute(dir));
    let excluded = settings.get::<Overrides>().clone().excluder(config.exclude.clone(), &root);
    if !(config.paths.is_empty() || config.paths.iter().any(inside)) || walk::excludes(&excluded, path) {
        return Ok(None);
    }
    let mut plan = Plan::new(&config, settings, matches, vec![path.to_owned()])?;
    // With no paths, the public scope looks for package.json above where docov runs.
    let paths = if config.paths.is_empty() { vec![here.to_owned()] } else { config.paths.clone() };
    plan.load_public(&paths).map_err(|errors| errors.join("\n"))?;
    Ok(plan.files().first().map(|(_, rules)| (*rules).clone()))
}
