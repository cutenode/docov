//! Properties of overrides that set the scope: the last match wins, each config's overrides hold
//! for its own files, and the cache, `--explain`, `--print-config`, `--scope` and the public scope
//! all go by the scope each file gets.

use std::{fs, path::Path, sync::Arc};

use docov::{
    Rules, Scope, Settings, batch,
    cache::Cache,
    config::Config,
    feature::{explain::Explain, print_config},
    nested, scan_file, walk,
};
use hegel::{TestCase, generators as gs};
use serde_json::{Value, json};

use super::{config_cli::matches, write};

/// Whether a glob matches a path relative to the folder of the file that wrote it.
type Matches = fn(&str) -> bool;

/// Globs an override can have, and what each matches, as gitignore has it.
const GLOBS: [(&str, Matches); 4] = [
    ("x*.ts", |p| p.rsplit('/').next().unwrap().starts_with('x')),
    ("/x*.ts", |p| !p.contains('/') && p.starts_with('x')),
    ("pkg/", |p| p.starts_with("pkg/")),
    ("*.ts", |_| true),
];

/// A drawn override: its globs, by index, and the scope it sets, if it sets one. One without a
/// scope sets a threshold that always passes, so it's an override for another setting.
type Override = (Vec<usize>, Option<Scope>);

/// A drawn config file's scope and overrides, each if it has them.
#[derive(Clone, Debug)]
struct Drawn {
    scope: Option<Scope>,
    overrides: Option<Vec<Override>>,
}

impl Drawn {
    fn draw(tc: &TestCase) -> Self {
        let scope = || gs::sampled_from(vec![Scope::Exports, Scope::All]);
        let globs = || gs::vecs(gs::integers::<usize>().max_value(GLOBS.len() - 1)).min_size(1).max_size(2);
        let overrides = gs::vecs(gs::tuples!(globs(), gs::optional(scope()))).max_size(3);
        Self { scope: tc.draw(gs::optional(scope())), overrides: tc.draw(gs::optional(overrides)) }
    }

    fn json(&self, mut json: Value) -> Value {
        if let Some(scope) = self.scope {
            json["scope"] = json!(scope);
        }
        if let Some(overrides) = &self.overrides {
            json["overrides"] = overrides
                .iter()
                .map(|(globs, scope)| {
                    let files: Vec<_> = globs.iter().map(|&g| GLOBS[g].0).collect();
                    match scope {
                        Some(scope) => json!({ "files": files, "scope": scope }),
                        None => json!({ "files": files, "threshold": 0 }),
                    }
                })
                .collect();
        }
        json
    }
}

/// A file's lines in `--explain`: `export const a` counts, and `const b` only under the `all`
/// scope.
fn explained(path: &Path, scope: Scope) -> String {
    let b =
        if scope == Scope::All { "undocumented" } else { "left out: not exported by a way of exporting that counts" };
    format!("{0}:1:1  variable a  undocumented\n{0}:2:1  variable b  {b}\n", path.display())
}

/// Each file is scanned with the scope of the last override of its config that matches it and
/// sets one, each glob relative to the file that wrote it, or else its config's scope, and
/// `--scope` wins over both. A nested config's overrides hold for its own files, and the root
/// config's for them when it has none. Overrides for other settings don't change the scope. The
/// cache scans a file again when its scope changes, `--explain` names the override that set it,
/// and `--print-config` prints each override's.
#[hegel::test(test_cases = 60)]
fn overrides_set_the_scope_by_the_last_match(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: Vec<String> = (0..tc.draw(gs::integers::<usize>().max_value(6)))
        .map(|i| {
            let folder = tc.draw(gs::sampled_from(vec!["", "pkg/"]));
            format!("{folder}{}{i}.ts", tc.draw(gs::sampled_from(vec!["x", "y"])))
        })
        .collect();
    for file in &files {
        write(root, file, "export const a = 1;\nconst b = 2;\n");
    }
    let cache = root.join(".docov-cache");
    // Two scans of the same files, each with configs of its own, through one cache.
    for _ in 0..2 {
        let nested = tc.draw(gs::booleans());
        let (top, pkg) = (Drawn::draw(&tc), tc.draw(gs::optional(gs::just(()))).map(|()| Drawn::draw(&tc)));
        let flag = tc.draw(gs::optional(gs::sampled_from(vec![Scope::Exports, Scope::All])));
        write(root, "docov.json", &top.json(json!({ "nested": nested })).to_string());
        match &pkg {
            Some(pkg) => write(root, "pkg/docov.json", &pkg.json(json!({})).to_string()),
            None => drop(fs::remove_file(root.join("pkg/docov.json"))),
        }
        let args: Vec<String> =
            flag.iter().flat_map(|s| ["--scope".into(), json!(s).as_str().unwrap().into()]).collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();

        // Each file's scope, and the override that set it, if one did.
        let none = Vec::new();
        let model = |file: &str| {
            let own = pkg.as_ref().filter(|_| nested && file.starts_with("pkg/"));
            let base = own.and_then(|pkg| pkg.scope).or(top.scope).unwrap_or_default();
            let (overrides, writer) = match own.and_then(|pkg| pkg.overrides.as_ref()) {
                Some(overrides) => (overrides, "pkg/"),
                None => (top.overrides.as_ref().unwrap_or(&none), ""),
            };
            let at = file.strip_prefix(writer);
            let set_by = overrides
                .iter()
                .rposition(|(globs, scope)| scope.is_some() && at.is_some_and(|p| globs.iter().any(|&g| GLOBS[g].1(p))))
                .filter(|_| flag.is_none())
                .map(|i| (i, &overrides[i]));
            let scope = flag.or(set_by.and_then(|(_, (_, scope))| *scope)).unwrap_or(base);
            (scope, set_by.map(|(i, (globs, _))| (i, globs.iter().map(|&g| GLOBS[g].0).collect::<Vec<_>>().join(", "))))
        };
        let plan = |files| {
            let mut config = Config::load(Some(&root.join("docov.json"))).unwrap();
            let settings = Settings::new(&matches(&args), std::mem::take(&mut config.keys));
            (nested::Plan::new(&config, settings, &matches(&args), files).unwrap(), config)
        };
        let (scanned, config) =
            plan(walk::files(&[root.to_owned()], |path, _| path.ends_with(".docov-cache")).unwrap());

        let mut sorted = files.clone();
        sorted.sort();
        let scopes: Vec<_> = scanned.files().into_iter().map(|(path, rules)| (path.to_owned(), rules.scope)).collect();
        let wanted: Vec<_> = sorted.iter().map(|file| (root.join(file), model(file).0)).collect();
        assert_eq!(scopes, wanted);
        let reports = batch::scan(&scanned.files(), &Cache::read(cache.clone())).unwrap();
        let totals: Vec<_> = reports.iter().map(|r| r.totals().total).collect();
        assert_eq!(totals, wanted.iter().map(|(_, scope)| 1 + usize::from(*scope == Scope::All)).collect::<Vec<_>>());

        // The root config's overrides, each with its scope if it sets one.
        let printed = print_config::json(&config, &[], scanned.settings());
        let printed: Vec<_> =
            printed["features"]["overrides"].as_array().unwrap().iter().map(|o| o["scope"].clone()).collect();
        let written = top.overrides.iter().flatten().map(|(_, scope)| scope.map_or(Value::Null, |s| json!(s)));
        assert_eq!(printed, written.collect::<Vec<_>>());

        let Some(file) = files.first() else { continue };
        let path = root.join(file);
        let (one, _) = plan(vec![path.clone()]);
        let mut out = Vec::new();
        Explain(Some(path.clone())).write(&one.files(), &mut out).unwrap().unwrap();
        let (scope, set_by) = model(file);
        let set_by = set_by.map(|(i, globs)| {
            let name = json!(scope).as_str().unwrap().to_owned();
            format!("{}  scope {name}: set by overrides[{i}], for {globs}\n", path.display())
        });
        assert_eq!(String::from_utf8(out).unwrap(), set_by.unwrap_or_default() + &explained(&path, scope));
    }
}

/// An override's public scope counts what the entry points of the package.json in its config's
/// folder reach, read once for the config and the overrides that set it, and an entry point with
/// no source fails the plan. Nothing's read for an override that matches no file, or under
/// `--scope`, which wins over the overrides'.
#[hegel::test(test_cases = 40)]
fn public_overrides_share_their_configs_entry_points(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let main = tc.draw(gs::sampled_from(vec!["x.js", "gone.js"]));
    write(root, "package.json", &json!({ "main": main }).to_string());
    for file in ["x.ts", "y.ts"] {
        write(root, file, "export const a = 1;\n");
    }
    let top = tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::Public]));
    let (glob, matched) = tc.draw(gs::sampled_from(vec![("x*", "x"), ("y*", "y"), ("*.ts", "xy"), ("z*", "")]));
    let set = tc.draw(gs::sampled_from(vec![Scope::Public, Scope::All]));
    let flag = tc.draw(gs::booleans());
    let config = json!({ "scope": top, "overrides": [{ "files": [glob], "scope": set }] });
    write(root, "docov.json", &config.to_string());

    let args: &[&str] = if flag { &["--scope", "exports"] } else { &[] };
    let mut config = Config::load(Some(&root.join("docov.json"))).unwrap();
    let settings = Settings::new(&matches(args), std::mem::take(&mut config.keys));
    let files = vec![root.join("x.ts"), root.join("y.ts")];
    let mut plan = nested::Plan::new(&config, settings, &matches(args), files).unwrap();
    let loaded = plan.load_public(&[root.to_owned()]);

    let scope = |name: &str| match () {
        () if flag => Scope::Exports,
        () if matched.contains(name) => set,
        () => top,
    };
    let reads = !flag && (top == Scope::Public || (set == Scope::Public && !matched.is_empty()));
    if reads && main == "gone.js" {
        let errors = loaded.unwrap_err();
        assert!(errors.len() == 1 && errors[0].contains("entry point gone.js has no source file"), "{errors:?}");
        return;
    }
    loaded.unwrap();
    let files = plan.files();
    let rules: Vec<&Rules> = files.iter().map(|(_, rules)| *rules).collect();
    assert_eq!(rules.iter().map(|r| r.scope).collect::<Vec<_>>(), [scope("x"), scope("y")]);
    let apis: Vec<_> = rules.iter().filter_map(|r| r.public.clone()).collect();
    assert_eq!(apis.len(), rules.iter().filter(|r| r.scope == Scope::Public).count());
    assert!(apis.windows(2).all(|pair| Arc::ptr_eq(&pair[0], &pair[1])));
    // x.ts is the entry point's source, which reaches its export; nothing reaches y.ts.
    let counted: Vec<_> = files.iter().map(|(path, rules)| scan_file(path, rules).unwrap().totals().total).collect();
    let counts = |name: &str| usize::from(scope(name) != Scope::Public || name == "x");
    assert_eq!(counted, [counts("x"), counts("y")]);
}
