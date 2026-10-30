//! Properties of config loading and the command line's conveniences.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use docov::{
    Context, FileReport, Item, Kind, Output, Rules, Scope, Settings, Totals, batch,
    cache::Cache,
    config,
    config::Config,
    feature,
    feature::{by_package::ByPackage, color, overrides::Overrides, stdin_filename, threads::Threads, watch},
    glob::Globs,
    init, jsdoc, nested, packages, report, scan, scan_file, walk,
    walk::Exclude,
};
use hegel::{TestCase, generators as gs};
use oxc_span::SourceType;

use super::{Room, plain, render, reports, write};

/// The folders generated config files go in.
const FOLDERS: [&str; 4] = ["", "x/", "x/y/", "z/"];

/// A generated config file.
struct File {
    folder: &'static str,
    threshold: Option<u8>,
    scope: Option<&'static str>,
    paths: bool,
    exclude: bool,
    /// The files it extends, by index.
    extends: Vec<usize>,
}

impl File {
    fn name(&self, i: usize) -> String {
        format!("{}f{i}.json", self.folder)
    }
}

/// The path from `folder` to `to`, both in the generated project.
fn from(folder: &str, to: &str) -> String {
    "../".repeat(folder.matches('/').count()) + to
}

/// What loading a file should find.
#[derive(Default)]
struct Model {
    threshold: Option<u8>,
    scope: Option<&'static str>,
    paths: Option<Vec<PathBuf>>,
    exclude: Option<Exclude>,
    files: Vec<PathBuf>,
}

/// What loading file `i`, named `path`, should find: its own keys over those of the files it
/// extends, each over the ones before it.
fn model(files: &[File], i: usize, path: &Path) -> Model {
    let dir = path.parent().unwrap();
    let mut merged = Model::default();
    for &base in &files[i].extends {
        let base = model(files, base, &dir.join(from(files[i].folder, &files[base].name(base))));
        merged = Model {
            threshold: base.threshold.or(merged.threshold),
            scope: base.scope.or(merged.scope),
            paths: base.paths.or(merged.paths),
            exclude: base.exclude.or(merged.exclude),
            files: [base.files, merged.files].concat(),
        };
    }
    let file = &files[i];
    Model {
        threshold: file.threshold.or(merged.threshold),
        scope: file.scope.or(merged.scope),
        paths: if file.paths { Some(vec![dir.join(format!("p{i}"))]) } else { merged.paths },
        exclude: if file.exclude {
            Some(Exclude { root: dir.to_owned(), globs: Globs::new(vec![format!("e{i}/**")]).unwrap() })
        } else {
            merged.exclude
        },
        files: [vec![path.to_owned()], merged.files].concat(),
    }
}

/// How a generated chain of config files goes wrong, if it does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Twist {
    None,
    /// The last file extends the first, which reaches it.
    Cycle,
    /// The last file extends one that isn't there.
    Missing,
    /// The last file extends a folder.
    Folder,
}

/// Config files in different folders extending each other, one path or a list of them at a time:
/// loading the first finds each key in the nearest file that has it, `paths` and `exclude`
/// relative to the file that wrote them, and lists the files nearest first. A file that extends
/// itself through others, one that isn't there and a folder each fail, naming the culprit.
#[hegel::test(test_cases = 60)]
fn extended_configs_merge_nearest_first(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let twist = tc.draw(gs::sampled_from(vec![Twist::None, Twist::Cycle, Twist::Missing, Twist::Folder]));
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let mut files: Vec<File> = (0..count)
        .map(|i| File {
            folder: tc.draw(gs::sampled_from(FOLDERS.to_vec())),
            threshold: tc.draw(gs::optional(gs::integers::<u8>().max_value(100))),
            scope: tc.draw(gs::optional(gs::sampled_from(vec!["exports", "all"]))),
            paths: tc.draw(gs::booleans()),
            exclude: tc.draw(gs::booleans()),
            // Only later files, so there's no cycle unless the twist makes one.
            extends: tc.draw(gs::vecs(gs::integers::<usize>().min_value(i + 1).max_value(count)).max_size(2)),
        })
        .collect();
    for (i, file) in files.iter_mut().enumerate() {
        file.extends.retain(|&j| j < count);
        // A twist needs the last file reached.
        if twist != Twist::None && i + 1 < count && !file.extends.contains(&(i + 1)) {
            file.extends.push(i + 1);
        }
    }
    for (i, file) in files.iter().enumerate() {
        let mut extends: Vec<String> = file.extends.iter().map(|&j| from(file.folder, &files[j].name(j))).collect();
        if i + 1 == count {
            match twist {
                Twist::None => {}
                Twist::Cycle => extends.push(from(file.folder, &files[0].name(0))),
                Twist::Missing => extends.push("missing.json".into()),
                Twist::Folder => extends.push(".".into()),
            }
        }
        let mut json = serde_json::json!({ "$schema": "https://example.com/docov.schema.json" });
        if extends.len() == 1 && tc.draw(gs::booleans()) {
            json["extends"] = extends[0].clone().into();
        } else if !extends.is_empty() {
            json["extends"] = extends.into();
        }
        if let Some(threshold) = file.threshold {
            json["threshold"] = threshold.into();
        }
        if let Some(scope) = file.scope {
            json["scope"] = scope.into();
        }
        if file.paths {
            // `./` adds nothing.
            json["paths"] = vec![format!("{}p{i}", if tc.draw(gs::booleans()) { "./" } else { "" })].into();
        }
        if file.exclude {
            json["exclude"] = vec![format!("e{i}/**")].into();
        }
        write(dir.path(), &file.name(i), &json.to_string());
    }

    let first = dir.path().join(files[0].name(0));
    let loaded = Config::load(Some(&first));
    match twist {
        Twist::None => {}
        Twist::Cycle => {
            let error = loaded.unwrap_err();
            let config::Error::Cycle { chain } = &error else { panic!("{error:?}") };
            // It goes back to the first file, the only one extended twice.
            let real = |p: &PathBuf| std::fs::canonicalize(p).unwrap();
            assert_eq!((&chain[0], real(&chain[0])), (&first, real(chain.last().unwrap())), "{chain:?}");
            let names: Vec<_> = chain.iter().map(|p| p.display().to_string()).collect();
            assert!(error.to_string().ends_with(&names.join(" -> ")), "{error}");
            return;
        }
        Twist::Missing | Twist::Folder => {
            let Err(config::Error::Read { path, .. }) = loaded else { panic!("{loaded:?}") };
            let name = if twist == Twist::Missing { "missing.json" } else { "." };
            assert!(path.to_str().unwrap().ends_with(name), "{path:?}");
            return;
        }
    }
    let loaded = loaded.unwrap();
    let expected = model(&files, 0, &first);
    assert_eq!(loaded.paths, expected.paths.unwrap_or_default());
    assert_eq!(loaded.exclude, expected.exclude.into_iter().collect::<Vec<_>>());
    assert_eq!(loaded.files, expected.files);
    assert_eq!(loaded.dir, first.parent().unwrap());
    // The merged keys are those of one file with each key's winning value.
    let flat = serde_json::json!({ "threshold": expected.threshold, "scope": expected.scope.unwrap_or("exports") });
    write(dir.path(), "flat.json", &flat.to_string());
    assert_eq!(loaded.keys, Config::load(Some(&dir.path().join("flat.json"))).unwrap().keys);
}

/// Runs the binary in `dir`, with stdout closed if `closed` (a pipe whose reader has gone, as with
/// `docov | head` after head exits), and returns its code, stdout and stderr.
pub(super) fn docov(dir: &Path, args: &[&str], closed: bool) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
    command.args(args).current_dir(dir);
    if closed {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        command.stdout(writer);
    }
    let out = command.output().unwrap();
    let text = |bytes| String::from_utf8(bytes).unwrap();
    (out.status.code().unwrap(), text(out.stdout), text(out.stderr))
}

/// `--print-config` prints the files the settings came from, the paths, the excludes and each
/// feature's setting, flags over keys, and `--list-files` the files a scan would read. Neither
/// scans, so a file that doesn't parse changes nothing, and each fails when stdout is closed.
#[hegel::test(test_cases = 5)]
fn the_cli_prints_its_settings_and_files(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("lib/gen")).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let mut listed = Vec::new();
    for f in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let folder = tc.draw(gs::sampled_from(vec!["src", "lib", "lib/gen"]));
        let path = format!("{folder}/f{f}.ts");
        write(dir.path(), &path, "export function (");
        if folder != "lib/gen" {
            listed.push(Path::new(folder).join(format!("f{f}.ts")).display().to_string());
        }
    }
    listed.sort();
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100)));
    let scope = tc.draw(gs::sampled_from(vec!["exports", "all"]));
    let config =
        serde_json::json!({ "paths": ["src", "lib"], "exclude": ["gen/"], "threshold": threshold, "scope": scope });
    write(dir.path(), "docov.json", &config.to_string());
    let flag = tc.draw(gs::optional(gs::sampled_from(vec!["exports", "all"])));
    let mut args = vec!["--print-config"];
    if let Some(flag) = flag {
        args.extend(["--scope", flag]);
    }

    let (code, out, err) = docov(dir.path(), &args, false);
    assert_eq!((code, err.as_str()), (0, ""));
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let expected = serde_json::json!({
        "files": ["docov.json"],
        "paths": ["src", "lib"],
        "exclude": [{ "root": "", "globs": ["gen/"] }],
        "features": { "scope": flag.unwrap_or(scope), "threshold": threshold.map(f64::from) },
    });
    assert_eq!(json["features"]["scope"], expected["features"]["scope"]);
    assert_eq!(json["features"]["threshold"], expected["features"]["threshold"]);
    for key in ["files", "paths", "exclude"] {
        assert_eq!(json[key], expected[key], "{key}");
    }

    let (code, out, err) = docov(dir.path(), &["--list-files"], false);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out.lines().collect::<Vec<_>>(), listed);

    for args in [["--print-config"], ["--list-files"]] {
        let (code, _, err) = docov(dir.path(), &args, true);
        // With nothing to list, there's nothing to fail to write.
        let expected = if args[0] == "--list-files" && listed.is_empty() { 0 } else { 2 };
        assert_eq!(code, expected, "{args:?} {err}");
        assert!(code == 0 || err.starts_with("docov: "), "{err}");
    }
}

/// `docov init` writes a docov.json pinned to this version's schema, with the coverage it measures,
/// rounded down, as the threshold, which a scan then meets. It won't write over a config file,
/// and fails on a file that doesn't scan, a folder that isn't there, or a folder in its way.
#[hegel::test(test_cases = 5)]
fn init_writes_a_threshold_the_project_meets(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let docs = tc.draw(gs::vecs(gs::booleans()).max_size(7));
    let mut text = String::new();
    for (i, doc) in docs.iter().enumerate() {
        text.push_str(&format!("{}export const e{i} = 1;\n", if *doc { "/** Docs. */\n" } else { "" }));
    }
    write(dir.path(), "src/a.ts", &text);
    let totals = Totals { documented: docs.iter().filter(|d| **d).count(), total: docs.len() };

    let (code, _, err) = docov(dir.path(), &["init"], false);
    let threshold = totals.percent().floor();
    assert_eq!((code, err), (0, format!("docov: wrote docov.json, with a {threshold}% threshold\n")));
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("docov.json")).unwrap()).unwrap();
    assert_eq!(json, serde_json::json!({ "$schema": init::SCHEMA_URL, "threshold": threshold as u8 }));
    assert_eq!(docov(dir.path(), &[], false).0, 0);
    let (code, _, err) = docov(dir.path(), &["init"], false);
    assert_eq!((code, err.as_str()), (2, "docov: docov.json is already there\n"));

    // The rest in-process: each run of the binary is slow under coverage.
    fs::remove_file(dir.path().join("docov.json")).unwrap();
    let name = tc.draw(gs::sampled_from(config::FILE_NAMES.to_vec()));
    write(dir.path(), name, "{}");
    assert_eq!(init::init(dir.path()), Err(vec![format!("{name} is already there")]));
    fs::remove_file(dir.path().join(name)).unwrap();
    fs::create_dir(dir.path().join("docov.json")).unwrap();
    let error = init::init(dir.path()).unwrap_err();
    assert!(error[0].starts_with("docov.json: "), "{error:?}");
    fs::remove_dir(dir.path().join("docov.json")).unwrap();
    write(dir.path(), "src/broken.ts", "export function (");
    let error = init::init(dir.path()).unwrap_err();
    assert!(error[0].contains("broken.ts"), "{error:?}");
    let error = init::init(&dir.path().join("gone")).unwrap_err();
    assert!(error[0].contains("gone"), "{error:?}");
}

/// The flags in `args` (after `docov`), parsed the way the binary parses them.
pub(super) fn matches(args: &[&str]) -> clap::ArgMatches {
    feature::args(clap::Command::new("docov")).get_matches_from(std::iter::once("docov").chain(args.iter().copied()))
}

/// The folders a generated project with nested configs has, under its root config's.
const NESTED: [&str; 4] = ["", "a/", "a/b/", "c/"];

/// With `"nested": true`, each file is scanned with the nearest config file above it, up to the
/// root config's folder, merged over the root config alone, and left out if that config excludes
/// it. Each config's threshold holds for its own files, and fails naming the nested config. A
/// nested config that doesn't load fails the plan. Without `nested`, every file has the root's.
#[hegel::test(test_cases = 40)]
fn nested_configs_set_their_own_files(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let nested = tc.draw(gs::booleans());
    let root_scope = tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::All]));
    let name = |scope| if scope == Scope::All { "all" } else { "exports" };
    let root = serde_json::json!({ "nested": nested, "scope": name(root_scope), "threshold": 50 });
    write(dir.path(), "docov.json", &root.to_string());
    // Each nested config: its file, scope and threshold, if it has them.
    let mut configs: Vec<(String, Option<Scope>, Option<u8>)> = Vec::new();
    for folder in &NESTED[1..] {
        if tc.draw(gs::booleans()) {
            let scope = tc.draw(gs::optional(gs::sampled_from(vec![Scope::Exports, Scope::All])));
            let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100)));
            let file = format!("{folder}{}", tc.draw(gs::sampled_from(config::FILE_NAMES.to_vec())));
            let mut json = serde_json::json!({ "exclude": ["skip*.ts"] });
            if let Some(scope) = scope {
                json["scope"] = name(scope).into();
            }
            if let Some(threshold) = threshold {
                json["threshold"] = threshold.into();
            }
            write(dir.path(), &file, &json.to_string());
            configs.push((file, scope, threshold));
        }
    }
    // The nearest config above a folder, if there's one under the root.
    let nearest = |folder: &str| {
        let mut above = Path::new(folder).ancestors();
        above.find_map(|a| configs.iter().find(|c| Path::new(&c.0).parent() == Some(a)))
    };
    // Each file, documented or not, with the scope and the config it's scanned with, unless its
    // config leaves it out.
    let mut expected = Vec::new();
    for f in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let folder = tc.draw(gs::sampled_from(NESTED.to_vec()));
        let skip = tc.draw(gs::booleans());
        let path = format!("{folder}{}{f}.ts", if skip { "skip" } else { "f" });
        let documented = tc.draw(gs::booleans());
        let doc = if documented { "/** Docs. */\n" } else { "" };
        write(dir.path(), &path, &format!("{doc}export const a = 1;\nconst b = 2;\n"));
        let config = nearest(folder).filter(|_| nested);
        if !(skip && config.is_some()) {
            let scope = config.and_then(|c| c.1).unwrap_or(root_scope);
            expected.push((dir.path().join(&path), scope, documented, config.map(|c| (c.0.clone(), c.2))));
        }
    }
    expected.sort_by(|a, b| a.0.cmp(&b.0));

    let plan_of = |files: Vec<PathBuf>| {
        let mut root = Config::load(Some(&dir.path().join("docov.json"))).unwrap();
        let settings = Settings::new(&matches(&[]), std::mem::take(&mut root.keys));
        nested::Plan::new(&root, settings, &matches(&[]), files)
    };
    let plan = plan_of(walk::files(&[dir.path().to_owned()], |_, _| false).unwrap()).unwrap();
    let files: Vec<_> = plan.files().into_iter().map(|(path, rules)| (path.to_owned(), rules.scope)).collect();
    assert_eq!(files, expected.iter().map(|e| (e.0.clone(), e.1)).collect::<Vec<_>>());

    // Each group's threshold, the root's 50 unless a nested config has its own, over its files.
    let reports = batch::scan(&plan.files(), &Cache::default()).unwrap();
    let mut groups: Vec<(Option<String>, u8, Totals)> = Vec::new();
    for (_, scope, documented, config) in &expected {
        let (file, threshold) = config.clone().map_or((None, Some(50)), |(file, t)| (Some(file), t.or(Some(50))));
        let at = groups.iter().position(|g| g.0 == file).unwrap_or_else(|| {
            groups.push((file, threshold.unwrap(), Totals::default()));
            groups.len() - 1
        });
        let totals = &mut groups[at].2;
        *totals = Totals {
            documented: totals.documented + usize::from(*documented),
            total: totals.total + 1 + usize::from(*scope == Scope::All),
        };
    }
    let mut failures: Vec<_> = groups
        .iter()
        .filter(|g| g.2.percent() < f64::from(g.1))
        .map(|(file, threshold, totals)| {
            let failure = format!("{:.1}% documented, under the {threshold}% threshold", totals.percent());
            file.as_ref().map_or(failure.clone(), |file| format!("{}: {failure}", dir.path().join(file).display()))
        })
        .collect();
    let (_, mut gates) = plan.gates(&reports);
    failures.sort();
    gates.sort();
    assert_eq!(gates, failures);

    // A file whose folder is gone, as stdin's can be, has the root config.
    let gone = dir.path().join("a/gone/x.ts");
    assert_eq!(plan_of(vec![gone.clone()]).unwrap().groups[0].files, [gone]);
    // A nested config that doesn't load, or whose exclude doesn't parse, fails naming it.
    if nested {
        let (text, culprit) = tc.draw(gs::sampled_from(vec![
            (r#"{ "threshold": 150 }"#, "docov.json"),
            (r#"{ "exclude": ["[z"] }"#, "[z"),
        ]));
        write(dir.path(), "c/docov.json", text);
        write(dir.path(), "c/z.ts", "");
        let error = plan_of(walk::files(&[dir.path().to_owned()], |_, _| false).unwrap()).err().unwrap();
        assert!(error.contains(culprit), "{error}");
    }
}

/// How a generated workspace lists its packages.
#[derive(Clone, Copy, Debug)]
enum Workspace {
    /// package.json's `workspaces` list, as npm and yarn read it.
    List,
    /// package.json's `workspaces.packages`, as yarn 1 reads it.
    Object,
    /// pnpm-workspace.yaml's `packages`.
    Pnpm,
}

/// `--by-package` adds each workspace package's totals to the text and JSON reports: the files in
/// folders with a package.json that the workspace's globs match and its `!` globs don't, each in
/// its nearest package. A package.json or pnpm-workspace.yaml that doesn't parse, or a glob that
/// doesn't, fails naming it, and so does a write that doesn't fit.
#[hegel::test(test_cases = 40)]
fn packages_total_their_own_files(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let globs = vec!["packages/*", "apps/**/", "!packages/private"];
    match tc.draw(gs::sampled_from(vec![Workspace::List, Workspace::Object, Workspace::Pnpm])) {
        Workspace::List => write(root, "package.json", &serde_json::json!({ "workspaces": globs }).to_string()),
        Workspace::Object => {
            write(root, "package.json", &serde_json::json!({ "workspaces": { "packages": globs } }).to_string());
        }
        Workspace::Pnpm => {
            write(root, "package.json", "{}");
            write(
                root,
                "pnpm-workspace.yaml",
                &format!("packages:\n{}", globs.iter().map(|g| format!("  - '{g}'\n")).collect::<String>()),
            );
        }
    }
    // Folders, and the package each one's files are in: `packages/x/sub` is in `packages/x`, a
    // folder without a package.json and `packages/private` in none.
    let folders = [
        ("packages/a", Some("packages/a")),
        ("packages/a/src/deep", Some("packages/a")),
        ("packages/b", Some("packages/b")),
        ("packages/private", None),
        ("packages/none", None),
        ("apps/web/site", Some("apps/web/site")),
        ("tools", None),
        ("", None),
    ];
    for (folder, package) in &folders {
        if Some(*folder) == *package || folder.ends_with("private") {
            let name = tc.draw(gs::booleans()).then(|| format!("@w/{}", folder.replace('/', "-")));
            write(root, &format!("{folder}/package.json"), &serde_json::json!({ "name": name }).to_string());
        }
    }
    // A file outside the root is in no package.
    let outside = Item { name: "a".into(), kind: Kind::Variable, line: 1, column: 1, documented: false, ignore: None };
    let mut reports = vec![FileReport { path: PathBuf::from("outside.ts"), items: vec![outside] }];
    let mut expected: Vec<(String, Totals)> = Vec::new();
    for f in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        let (folder, package) = tc.draw(gs::sampled_from(folders.to_vec()));
        let documented = tc.draw(gs::booleans());
        let item = Item { name: "a".into(), kind: Kind::Variable, line: 1, column: 1, documented, ignore: None };
        reports.push(FileReport { path: root.join(folder).join(format!("f{f}.ts")), items: vec![item] });
        if let Some(package) = package {
            let path = root.join(package).display().to_string();
            let at = expected.iter().position(|e| e.0 == path).unwrap_or_else(|| {
                expected.push((path, Totals::default()));
                expected.len() - 1
            });
            expected[at].1 = Totals {
                documented: expected[at].1.documented + usize::from(documented),
                total: expected[at].1.total + 1,
            };
        }
    }
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    let found = packages::packages(root, &reports).unwrap();
    assert_eq!(found.iter().map(|p| (p.path.clone(), p.totals)).collect::<Vec<_>>(), expected);
    for package in &found {
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(Path::new(&package.path).join("package.json")).unwrap()).unwrap();
        assert_eq!(package.name.as_deref(), json["name"].as_str());
    }

    // The reports, with and without the flag.
    let by_package = Settings::new(&matches(&["--by-package"]), Default::default());
    let without = Settings::new(&matches(&[]), Default::default());
    assert_eq!(without.get::<ByPackage>().packages(root, &reports), Ok(None));
    let listed = by_package.get::<ByPackage>().packages(root, &reports).unwrap().unwrap();
    assert_eq!(listed, found);
    for name in ["text", "json", "lcov"] {
        let context = |packages| Context { packages, ..plain(&reports) };
        let written = |packages| report::format(name).unwrap().render(&context(packages));
        let (plain, with) = (written(None), written(Some(&found)));
        match name {
            "text" => {
                let lines: Vec<_> = found
                    .iter()
                    .map(|p| {
                        format!("{:>6.1}%  {}/{}  {} total\n", p.percent, p.totals.documented, p.totals.total, p.path)
                    })
                    .collect();
                assert_eq!(with, plain + &lines.concat());
            }
            "json" => {
                let mut json: serde_json::Value = serde_json::from_str(&plain).unwrap();
                json["packages"] = serde_json::to_value(&found).unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&with).unwrap(), json);
            }
            _ => assert_eq!(with, plain),
        }
        // A writer with room for less than all of it fails.
        let format = report::format(name).unwrap();
        let room = tc.draw(gs::integers::<usize>().max_value(with.len() - 1));
        assert!(report::write(&[Output { format, path: None }], &context(Some(&found)), &mut Room(room)).is_err());
    }

    // What doesn't parse, and what its error says.
    let (file, text, culprit) = tc.draw(gs::sampled_from(vec![
        ("package.json", "{", "package.json"),
        ("package.json", r#"{ "workspaces": ["[z"] }"#, "[z"),
        ("package.json", r#"{ "workspaces": ["![z"] }"#, "[z"),
        ("pnpm-workspace.yaml", "packages: [", "pnpm-workspace.yaml"),
    ]));
    write(root, file, text);
    let error = by_package.get::<ByPackage>().packages(root, &reports).unwrap_err();
    assert!(error.contains(culprit), "{error}");
}

/// The binary with nested configs and `--by-package`: a package's total follows the text total,
/// a nested threshold fails naming its config, and a nested config that doesn't load fails the
/// scan.
#[hegel::test(test_cases = 3)]
fn the_cli_scans_nested_configs_and_packages(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "docov.json", r#"{ "nested": true }"#);
    write(root, "package.json", r#"{ "workspaces": ["packages/*"] }"#);
    write(root, "packages/a/package.json", "{}");
    let documented = tc.draw(gs::booleans());
    let doc = if documented { "/** Docs. */\n" } else { "" };
    write(root, "packages/a/a.ts", &format!("{doc}export const a = 1;\n"));
    write(root, "packages/a/docov.json", r#"{ "threshold": 100 }"#);

    let (code, out, err) = docov(root, &["--by-package"], false);
    let a = Path::new("packages").join("a");
    let percent = if documented { "100.0%  1/1" } else { "  0.0%  0/1" };
    assert!(out.ends_with(&format!("total\n {percent}  {} total\n", a.display())), "{out}");
    if documented {
        assert_eq!((code, err.as_str()), (0, ""));
    } else {
        let config = a.join("docov.json");
        assert_eq!(
            (code, err),
            (1, format!("docov: {}: 0.0% documented, under the 100% threshold\n", config.display()))
        );
    }

    write(root, "packages/a/docov.json", r#"{ "threshold": 150 }"#);
    let (code, out, err) = docov(root, &[], false);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("expected a number from 0 to 100"), "{err}");
}

/// A jsdoc.json's `source.include` becomes the paths (unless docov.json has its own),
/// `source.exclude` leaves out those files and folders, and `includePattern` and `excludePattern`
/// filter the rest only when written, all relative to the jsdoc.json. One that's JavaScript, isn't
/// there, or doesn't parse fails naming it.
#[hegel::test(test_cases = 40)]
fn jsdoc_configs_pick_the_files(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let names = ["src/a.ts", "src/_b.ts", "src/legacy/c.ts", "src/d.js", "lib/e.ts", "other/f.ts"];
    for name in names {
        write(root, name, "export const a = 1;\n");
    }
    let include_pattern = tc.draw(gs::booleans());
    let exclude_pattern = tc.draw(gs::booleans());
    let mut source =
        serde_json::json!({ "include": ["../src", "../lib"], "exclude": ["../src/legacy", "../lib/e.ts", ".."] });
    if include_pattern {
        source["includePattern"] = r"\.ts$".into();
    }
    if exclude_pattern {
        // Of the file's name: the temporary folder's path can have a `/_` in it.
        source["excludePattern"] = r"(^|\/|\\)_[^\/\\]*$".into();
    }
    // Comments are fine, as they are for jsdoc.
    write(root, "docs/jsdoc.json", &format!("// jsdoc\n{}", serde_json::json!({ "source": source })));
    // From the flag, or from docov.json's key, relative to it.
    let by_flag = tc.draw(gs::booleans());
    let own_paths = tc.draw(gs::booleans());
    let mut docov = serde_json::json!({});
    if !by_flag {
        docov["jsdocConfig"] = "docs/jsdoc.json".into();
    }
    if own_paths {
        docov["paths"] = vec!["other"].into();
    }
    write(root, "docov.json", &docov.to_string());
    let mut config = Config::load(Some(&root.join("docov.json"))).unwrap();
    let keys = std::mem::take(&mut config.keys);
    let jsdoc = root.join("docs/jsdoc.json");
    config.read_jsdoc(by_flag.then_some(jsdoc.as_path())).unwrap();
    let files =
        walk::files(&config.paths, Overrides::default().excluder(config.exclude.clone(), &config.root())).unwrap();
    let plan = nested::Plan::new(&config, Settings::new(&matches(&[]), keys), &matches(&[]), files).unwrap();
    let found: Vec<_> = plan.files().into_iter().map(|(path, _)| path.to_owned()).collect();
    let expected: Vec<_> = if own_paths {
        vec![root.join("other/f.ts")]
    } else {
        let mut kept = vec!["src/a.ts", "src/_b.ts", "src/d.js"];
        kept.retain(|n| !(include_pattern && n.ends_with(".js")) && !(exclude_pattern && n.contains("/_")));
        kept.iter().map(|n| root.join("docs/../").join(n).components().collect()).collect()
    };
    let mut expected = expected;
    expected.sort();
    assert_eq!(found, expected);

    // What doesn't read, and what its error names.
    let (name, text, culprit) = tc.draw(gs::sampled_from(vec![
        ("jsdoc.js", "module.exports = {};", "not JavaScript"),
        ("jsdoc.cjs", "module.exports = {};", "not JavaScript"),
        ("gone.json", "", "gone.json"),
        ("bad.json", "{ / }", "doesn't start a comment"),
        ("bad.json", "{ \"source\": 1 }", "invalid type"),
        ("bad.json", r#"{ "source": { "includePattern": "(" } }"#, "regex parse error"),
        ("bad.json", r#"{ "source": { "excludePattern": "(" } }"#, "regex parse error"),
    ]));
    if name != "gone.json" {
        write(root, name, text);
    }
    let error = jsdoc::read(&root.join(name)).unwrap_err();
    assert!(error.contains(culprit) && error.contains(name), "{error}");
}

/// The binary scans what `--jsdoc-config` or `jsdocConfig` documents, and fails naming a jsdoc
/// config that doesn't read.
#[hegel::test(test_cases = 2)]
fn the_cli_reads_jsdoc_configs(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "lib/a.ts", "export const a = 1;\n");
    write(dir.path(), "other.ts", "export const other = 1;\n");
    write(dir.path(), "jsdoc.json", r#"{ "source": { "include": ["lib"] } }"#);
    let by_flag = tc.draw(gs::booleans());
    let args: &[&str] = if by_flag { &["--jsdoc-config", "jsdoc.json"] } else { &[] };
    if !by_flag {
        write(dir.path(), "docov.json", r#"{ "jsdocConfig": "jsdoc.json" }"#);
    }
    let (code, out, _) = docov(dir.path(), args, false);
    assert_eq!((code, out.lines().last()), (0, Some("   0.0%  0/1  total")));
    let (code, out, err) = docov(dir.path(), &["--jsdoc-config", "missing.json"], false);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.starts_with("docov: missing.json: "), "{err}");
}

/// `--threads` scans the same files to the same report on however many threads, and
/// `--print-config` shows it.
#[hegel::test(test_cases = 3)]
fn threads_change_only_how_many_scan_at_once(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    for f in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let doc = if tc.draw(gs::booleans()) { "/** Docs. */\n" } else { "" };
        write(dir.path(), &format!("f{f}.ts"), &format!("{doc}export const a = 1;\n"));
    }
    let threads = tc.draw(gs::integers::<usize>().min_value(1).max_value(4)).to_string();
    let (code, out, err) = docov(dir.path(), &["--threads", &threads], false);
    assert_eq!((code, out, err), docov(dir.path(), &[], false));
    let (_, out, _) = docov(dir.path(), &["--threads", &threads, "--print-config"], false);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["features"]["threads"].to_string(), threads);
}

/// `--threads` sizes rayon's pool, which a process makes once: the first size stays, and no size
/// changes nothing. nextest runs each test in a process of its own, so this one's first case
/// makes the pool.
#[hegel::test(test_cases = 5)]
fn threads_size_the_pool_once(tc: TestCase) {
    static FIRST: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let threads = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    Threads(Some(threads)).apply();
    Threads(None).apply();
    assert_eq!(rayon::current_num_threads(), *FIRST.get_or_init(|| threads));
}

/// Runs the binary in `dir` with `stdin` as its standard input.
pub(super) fn docov_with_stdin(dir: &Path, args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_docov"))
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    let text = |bytes| String::from_utf8(bytes).unwrap();
    (out.status.code().unwrap(), text(out.stdout), text(out.stderr))
}

/// `--stdin-filename` scans stdin as the file it names, whose extension picks the language, which
/// needn't exist: the items are those of the same text in a file. Text that isn't UTF-8, doesn't
/// parse, or names a file that isn't JavaScript fails naming the path.
#[hegel::test(test_cases = 20)]
fn stdin_scans_as_the_file_it_names(tc: TestCase) {
    let mut source = String::new();
    for i in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let doc = if tc.draw(gs::booleans()) { "/** Docs. */\n" } else { "" };
        source.push_str(&format!("{doc}export function f{i}(): void {{}}\n"));
    }
    let name = tc.draw(gs::sampled_from(vec!["src/buffer.ts", "buffer.tsx", "unsaved.mts"]));
    let path = Path::new(name);
    let rules = Rules::from(Scope::Exports);
    let scanned = stdin_filename::scan(&[(path, &rules)], &mut source.as_bytes()).unwrap();
    let expected = scan(&source, SourceType::from_path(path).unwrap(), &rules).unwrap();
    assert_eq!(scanned, vec![FileReport { path: path.to_owned(), items: expected }]);
    // With nothing to scan, stdin isn't read.
    assert_eq!(stdin_filename::scan(&[], &mut &b"\xff"[..]), Ok(Vec::new()));

    let (bytes, culprit): (&[u8], _) = tc.draw(gs::sampled_from(vec![
        (&b"// caf\xe9"[..], "stream did not contain valid UTF-8"),
        (b"export function (", ":1:"),
    ]));
    let error = stdin_filename::scan(&[(path, &rules)], &mut &bytes[..]).unwrap_err();
    assert!(error[0].starts_with(&path.display().to_string()) && error[0].contains(culprit), "{error:?}");
    let error = stdin_filename::scan(&[(Path::new("notes.md"), &rules)], &mut source.as_bytes()).unwrap_err();
    assert_eq!(error, vec!["notes.md: not a JavaScript or TypeScript file".to_owned()]);
}

/// The binary reads stdin as the file `--stdin-filename` names, in place of the paths.
#[hegel::test(test_cases = 2)]
fn the_cli_reads_stdin_in_place_of_files(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "other.ts", "export const other = 1;\n");
    let documented = tc.draw(gs::booleans());
    let source = format!("{}export const a = 1;\n", if documented { "/** Docs. */\n" } else { "" });
    let (code, out, err) =
        docov_with_stdin(dir.path(), &["--stdin-filename", "src/new.ts", "--threshold", "100"], source.as_bytes());
    let path = Path::new("src").join("new.ts").display().to_string();
    let percent = if documented { "100.0%  1/1" } else { "  0.0%  0/1" };
    let line = |label: &str| format!(" {percent}  {label}\n");
    let item = if documented { String::new() } else { format!("         {path}:1:1  variable a\n") };
    assert_eq!(out, format!("{}{item}{}", line(&path), line("total")));
    assert_eq!(code, i32::from(!documented), "{err}");
    let (code, out, err) = docov_with_stdin(dir.path(), &["--stdin-filename", "src/new.ts"], b"export function (");
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.starts_with(&format!("docov: {path}:1:")), "{err}");
}

/// The text report's lines through [`color::Styled`]: items dim, the percentage of each line whose
/// totals are under the threshold red, unrounded, and every other line, JSON included, as it was.
/// What's left of an unfinished line goes out on a flush, and a write that doesn't fit fails.
#[hegel::test(test_cases = 50)]
fn color_marks_items_and_what_is_under_the_threshold(tc: TestCase) {
    let files = reports(&tc, false);
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
    let text = render("text", &files);
    // Lines that aren't the report's pass through too.
    let text = format!("{text}\nx\n5%\n5% a\n5% 1/x\n5% x/1\n");

    let mut out = Vec::new();
    let mut styled = color::Styled::new(&mut out, threshold);
    // In pieces, split anywhere.
    let at = tc.draw(gs::integers::<usize>().max_value(text.len()));
    let (head, tail) = text.as_bytes().split_at(at);
    std::io::Write::write_all(&mut styled, head).unwrap();
    std::io::Write::write_all(&mut styled, tail).unwrap();
    std::io::Write::write_all(&mut styled, b"unfinished").unwrap();
    std::io::Write::flush(&mut styled).unwrap();
    let out = String::from_utf8(out).unwrap();

    let (dim, red) = ("\x1b[2m", "\x1b[31m");
    let mut expected = String::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let percent = words.next().is_some_and(|word| word.ends_with('%'));
        let totals = words.next().and_then(|word| word.split_once('/'));
        let totals = totals.and_then(|(d, t)| Some(Totals { documented: d.parse().ok()?, total: t.parse().ok()? }));
        let under = percent && totals.zip(threshold).is_some_and(|(totals, threshold)| totals.percent() < threshold);
        if line.starts_with("         ") {
            expected += &format!("{dim}{line}\x1b[0m\n");
        } else if under {
            let (percent, rest) = line.split_once('%').unwrap();
            expected += &format!("{red}{percent}%\x1b[0m{rest}\n");
        } else {
            expected += &format!("{line}\n");
        }
    }
    assert_eq!(out, expected + "unfinished");

    // Written with colors on, the text report goes through it, and JSON as it is.
    let context = Context { threshold, color: true, ..plain(&files) };
    for name in ["text", "json"] {
        let mut out = Vec::new();
        report::write(&[Output { format: report::format(name).unwrap(), path: None }], &context, &mut out).unwrap();
        let mut styled = Vec::new();
        std::io::Write::write_all(&mut color::Styled::new(&mut styled, threshold), render(name, &files).as_bytes())
            .unwrap();
        let expected = if name == "text" { styled } else { render(name, &files).into_bytes() };
        assert_eq!(out, expected, "{name}");
    }

    // A writer with room for less fails.
    let room = tc.draw(gs::integers::<usize>().max_value(text.len().saturating_sub(1)));
    let mut room = Room(room);
    let mut styled = color::Styled::new(&mut room, threshold);
    let wrote =
        std::io::Write::write_all(&mut styled, text.as_bytes()).and_then(|()| std::io::Write::flush(&mut styled));
    assert!(wrote.is_err() || text.is_empty());
    // And so does a flush with an unfinished line that doesn't fit.
    let mut full = Room(0);
    let mut styled = color::Styled::new(&mut full, threshold);
    std::io::Write::write_all(&mut styled, b"unfinished").unwrap();
    assert!(std::io::Write::flush(&mut styled).is_err());
}

/// `--color always` colors the text report, `--color never` and `NO_COLOR` don't, and `auto`
/// doesn't when stdout isn't a terminal, as it isn't here.
#[hegel::test(test_cases = 2)]
fn the_cli_colors_only_when_asked_or_on_a_terminal(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.ts", "export const a = 1;\n");
    let threshold = tc.draw(gs::sampled_from(vec!["50", "100"]));
    for (color, no_color, colored) in
        [("always", false, true), ("never", false, false), ("auto", false, false), ("auto", true, false)]
    {
        let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
        command.args(["--color", color, "--threshold", threshold]).current_dir(dir.path());
        if no_color {
            command.env("NO_COLOR", "1");
        } else {
            command.env_remove("NO_COLOR");
        }
        let out = String::from_utf8(command.output().unwrap().stdout).unwrap();
        assert_eq!(out.contains('\x1b'), colored, "{color} {out:?}");
        assert!(out.ends_with("total\n") || out.ends_with("total\n\x1b[0m"), "{out:?}");
    }
}

/// Without `--watch`, one scan and its code. With it, a scan again after each change to a config,
/// JavaScript or TypeScript file under the paths the last scan named, following them when they
/// change, and an end with 2 for paths that can't be watched. A watch never returns otherwise, so
/// it runs on a thread of its own, left to end with the test.
#[hegel::test(test_cases = 3)]
fn watch_scans_again_after_changes(tc: TestCase) {
    let code = tc.draw(gs::integers::<u8>().max_value(2));
    let mut runs = 0;
    assert_eq!(
        watch::run(false, &mut |_| {
            runs += 1;
            code
        }),
        code
    );
    assert_eq!(runs, 1);
    let gone = tempfile::tempdir().unwrap().path().join("gone");
    assert_eq!(
        watch::run(true, &mut |paths| {
            *paths = vec![gone.clone()];
            0
        }),
        2
    );

    let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folders = [first.path().to_owned(), second.path().to_owned()];
    let (send, scans) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut count = 0;
        watch::run(true, &mut |paths| {
            // The first scan names the first folder, the rest the second.
            *paths = vec![folders[usize::from(count > 0)].clone()];
            count += 1;
            // The test may be done, and the receiver gone.
            let _ = send.send(count);
            code
        })
    });
    assert_eq!(scans.recv_timeout(Duration::from_secs(30)), Ok(1));
    // Writes until a scan follows, as the watch starts after the first scan, and may miss one
    // write before it's ready. Each wait is bounded.
    let scanned_after = |dir: &Path, name: &str| {
        (0..30).find_map(|i| {
            write(dir, name, &format!("// {i}\n"));
            scans.recv_timeout(Duration::from_secs(1)).ok()
        })
    };
    let after_config = scanned_after(first.path(), "docov.json").expect("a scan after a config change");
    let after_source = scanned_after(second.path(), "a.ts").expect("a scan after a source change");
    assert!(after_config >= 2 && after_source > after_config, "{after_config} {after_source}");
}

/// With `--cache`, a scan reuses each file's items while its size and modified time and the scope
/// are what they were, from a cache another docov version's or a broken one doesn't count for, and
/// rescans a file that changed. Without it, nothing's kept. A cache file that can't be written
/// fails the scan naming it.
#[hegel::test(test_cases = 30)]
fn the_cache_keeps_unchanged_files_items(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join(".docov-cache");
    let path = dir.path().join("a.ts");
    let scope = tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::All]));
    let rules = Rules::from(scope);
    write(dir.path(), "a.ts", "export const a = 1;\nconst b = 2;\n");
    let files = [(path.as_path(), &rules)];
    let scanned = batch::scan(&files, &Cache::read(cache_path.clone())).unwrap();
    // What a cache holds is what it gives back, whatever the file says now, while the stamp's
    // the same: the file is rewritten with its size and time kept.
    let (modified, size) = (fs::metadata(&path).unwrap().modified().unwrap(), fs::metadata(&path).unwrap().len());
    let documented = "/** A */\nexport const a = 1;\n";
    write(dir.path(), "a.ts", &format!("{documented}{}\n", "/".repeat(size as usize - documented.len() - 1)));
    fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
    let kept = tc.draw(gs::integers::<u8>().max_value(3));
    let cache = match kept {
        0 => Cache::read(cache_path.clone()),
        // Unchecked: the version, or the cache itself, or a scope that isn't the same.
        1 => {
            let text = fs::read_to_string(&cache_path).unwrap().replace(env!("CARGO_PKG_VERSION"), "0.0.0-other");
            fs::write(&cache_path, text).unwrap();
            Cache::read(cache_path.clone())
        }
        2 => {
            fs::write(&cache_path, "not json").unwrap();
            Cache::read(cache_path.clone())
        }
        _ => Cache::default(),
    };
    let reused = cache.stamp(&path, &rules).and_then(|stamp| cache.get(&path, stamp)).is_some();
    let again = batch::scan(&files, &cache).unwrap();
    assert_eq!(again, if reused { scanned.clone() } else { scanned_now(&path, &rules) });
    assert_eq!(reused, kept == 0);
    assert_ne!(scanned, scanned_now(&path, &rules));
    // A scope that's changed doesn't count.
    let other = Rules::from(if scope == Scope::All { Scope::Exports } else { Scope::All });
    let cache = Cache::read(cache_path.clone());
    assert!(cache.stamp(&path, &other).and_then(|stamp| cache.get(&path, stamp)).is_none());

    // A file with no time to keep it by, from before 1970, isn't kept, nor one that's gone.
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(cache.stamp(&path, &rules), None);
    assert_eq!(cache.stamp(&dir.path().join("gone.ts"), &rules), None);
    let report = FileReport { path: path.clone(), items: Vec::new() };
    cache.write(&[(report, None)]).unwrap();
    assert!(!fs::read_to_string(&cache_path).unwrap().contains("a.ts"));

    let unwritable = Cache::read(dir.path().to_owned());
    let error = batch::scan(&files, &unwritable).unwrap_err();
    assert!(error[0].starts_with(&dir.path().display().to_string()), "{error:?}");
}

fn scanned_now(path: &Path, rules: &Rules) -> Vec<FileReport> {
    vec![scan_file(path, rules).unwrap()]
}

/// The binary keeps its cache in .docov-cache in the config's folder, or at `--cache-location`,
/// and scans the same with it as without.
#[hegel::test(test_cases = 2)]
fn the_cli_caches_where_it_is_told(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let doc = if tc.draw(gs::booleans()) { "/** Docs. */\n" } else { "" };
    write(dir.path(), "src/a.ts", &format!("{doc}export const a = 1;\n"));
    write(dir.path(), "docov.json", r#"{ "paths": ["src"] }"#);
    let plain = docov(dir.path(), &[], false);
    for (args, cache, location) in [
        (&["--cache"][..], ".docov-cache", None),
        (&["--cache", "--cache-location", "c.json"], "c.json", Some("c.json")),
    ] {
        // The first run writes the cache, and the second reads it.
        for _ in 0..2 {
            assert_eq!(docov(dir.path(), args, false), plain);
        }
        assert!(fs::read_to_string(dir.path().join(cache)).unwrap().contains("a.ts"));
        let (_, out, _) = docov(dir.path(), &[args, &["--print-config"]].concat(), false);
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["features"]["cache"], serde_json::json!({ "on": true, "location": location }));
    }
}
