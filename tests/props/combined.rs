//! Properties of features working together: gates under extended and nested configs, the cache
//! with the rules, and `--print-config` over every feature's setting.

use std::{
    fs,
    path::{Path, PathBuf},
};

use docov::{
    Kind, Rules, Scope, Settings, Totals, batch, cache::Cache, config::Config, feature::overrides::Overrides, jsdoc,
    nested, scan_file, walk,
};
use hegel::{TestCase, generators as gs};
use serde_json::json;

use super::{
    config_cli::{docov, docov_with_stdin, matches},
    write,
};

/// The config file that writes a key: the project folder's, or pkg/'s.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Writer {
    Root,
    Pkg,
}

impl Writer {
    fn draw(tc: &TestCase) -> Self {
        if tc.draw(gs::booleans()) { Self::Root } else { Self::Pkg }
    }

    /// The start of a glob that, from this writer's folder, is in pkg/.
    fn anchor(self) -> &'static str {
        if self == Self::Root { "/pkg/" } else { "/" }
    }
}

/// How pkg/'s config gets the project folder's keys.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    /// pkg/docov.json extends ../base.json.
    Extends,
    /// docov.json has `"nested": true`, over pkg/docov.json.
    Nested,
}

/// Thresholds by glob, overrides, `maxUndocumented` and the baseline hold for pkg/'s files the
/// same whichever config file writes them, pkg/'s or the project folder's (which pkg/'s extends,
/// or nests under): each glob and path is relative to the file that wrote it. A nested config's
/// failures name it. `--write-baseline` writes the baseline of each config with files to scan,
/// those that share one together, with paths relative to it.
#[hegel::test(test_cases = 60)]
fn gates_go_by_the_file_that_wrote_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let layout = tc.draw(gs::sampled_from(vec![Layout::Extends, Layout::Nested]));
    // pkg/x<f>.ts or pkg/y<f>.ts, each with one item, documented or not.
    let files: Vec<(char, bool)> = (0..tc.draw(gs::integers::<usize>().max_value(6)))
        .map(|_| (if tc.draw(gs::booleans()) { 'x' } else { 'y' }, tc.draw(gs::booleans())))
        .collect();
    let name = |f: usize| format!("{}{f}.ts", files[f].0);
    for (f, (_, documented)) in files.iter().enumerate() {
        let doc = if *documented { "/** Docs. */\n" } else { "" };
        write(root, &format!("pkg/{}", name(f)), &format!("{doc}export const v{f} = 1;\n"));
    }

    // The project folder's keys, then pkg/'s.
    let mut configs = [json!({}), json!({})];
    let at = |writer: Writer| usize::from(writer == Writer::Pkg);
    let x = (tc.draw(gs::optional(gs::integers::<u8>().max_value(100))), Writer::draw(&tc));
    if let (Some(limit), writer) = x {
        configs[at(writer)]["threshold"] = json!({ format!("{}x*.ts", writer.anchor()): limit });
    }
    let y = (tc.draw(gs::optional(gs::integers::<u8>().max_value(100))), Writer::draw(&tc));
    if let (Some(limit), writer) = y {
        configs[at(writer)]["overrides"] = json!([{ "files": [format!("{}y*", writer.anchor())], "threshold": limit }]);
    }
    let most = tc.draw(gs::optional(gs::integers::<usize>().max_value(6)));
    if let Some(most) = most {
        configs[at(Writer::draw(&tc))]["maxUndocumented"] = most.into();
    }
    let known: Vec<usize> = (0..files.len()).filter(|_| tc.draw(gs::booleans())).collect();
    let baseline = tc.draw(gs::optional(gs::just(()))).map(|()| Writer::draw(&tc));
    if let Some(writer) = baseline {
        let (folder, to) = if writer == Writer::Root { ("", "pkg/") } else { ("pkg/", "") };
        let items: Vec<_> = known
            .iter()
            .map(|&f| json!({ "path": format!("{to}{}", name(f)), "kind": "variable", "name": format!("v{f}") }))
            .collect();
        write(root, &format!("{folder}known.json"), &json!({ "items": items }).to_string());
        configs[at(writer)]["baseline"] = "known.json".into();
    }
    let (config, prefix) = match layout {
        Layout::Extends => {
            configs[1]["extends"] = "../base.json".into();
            write(root, "base.json", &configs[0].to_string());
            (root.join("pkg/docov.json"), String::new())
        }
        Layout::Nested => {
            configs[0]["nested"] = true.into();
            write(root, "docov.json", &configs[0].to_string());
            (root.join("docov.json"), format!("{}: ", root.join("pkg").join("docov.json").display()))
        }
    };
    write(root, "pkg/docov.json", &configs[1].to_string());
    let plan = |args: &[&str]| {
        let mut config = Config::load(Some(&config)).unwrap();
        let settings = Settings::new(&matches(args), std::mem::take(&mut config.keys));
        let excluded = settings.get::<Overrides>().clone().excluder(config.exclude.clone(), &config.root());
        let files = walk::files(&[root.join("pkg")], excluded).unwrap();
        nested::Plan::new(&config, settings, &matches(args), files)
    };
    let scanned = plan(&[]).unwrap();
    let reports = batch::scan(&scanned.files(), &Cache::default()).unwrap();

    let documented = |f: usize| files[f].1 || (baseline.is_some() && known.contains(&f));
    let totals = |pick: &dyn Fn(usize) -> bool| -> Totals {
        let picked = (0..files.len()).filter(|&f| pick(f));
        picked.map(|f| Totals { documented: usize::from(documented(f)), total: 1 }).sum()
    };
    // An override with a threshold takes the y files from the top-level threshold.
    let overridden = |f: usize| y.0.is_some() && files[f].0 == 'y';
    let mut wanted = Vec::new();
    let all = totals(&|_| true);
    if let Some(most) = most.filter(|&most| all.total - all.documented > most) {
        wanted.push(format!("{prefix}{} undocumented, over the limit of {most}", all.total - all.documented));
    }
    for ((limit, writer), glob, pick) in [
        (x, "x*.ts", &(|f: usize| files[f].0 == 'x' && !overridden(f)) as &dyn Fn(usize) -> bool),
        (y, "y*", &overridden),
    ] {
        let matched = totals(pick);
        if let Some(limit) = limit.filter(|&limit| matched.percent() < f64::from(limit)) {
            let percent = matched.percent();
            wanted.push(format!(
                "{prefix}{}{glob}: {percent:.1}% documented, under the {limit}% threshold",
                writer.anchor()
            ));
        }
    }
    assert_eq!(scanned.gates(&reports).1, wanted, "{configs:?}");

    // What --write-baseline writes, and the items each file has, as paths from its folder.
    let written = plan(&["--write-baseline"]).unwrap().save_baselines(&reports).unwrap();
    let real = |path: &Path| fs::canonicalize(path).unwrap();
    let default = if layout == Layout::Extends { "pkg/docov-baseline.json" } else { "docov-baseline.json" };
    let expected: Vec<&str> = match (layout, baseline) {
        (_, None) => vec![default],
        (_, Some(Writer::Root)) => vec!["known.json"],
        (Layout::Extends, Some(Writer::Pkg)) => vec!["pkg/known.json"],
        // Without files, pkg/'s config has none to scan.
        (Layout::Nested, Some(Writer::Pkg)) if files.is_empty() => vec!["docov-baseline.json"],
        (Layout::Nested, Some(Writer::Pkg)) => vec!["docov-baseline.json", "pkg/known.json"],
    };
    let expected: Vec<PathBuf> = expected.iter().map(|path| real(&root.join(path))).collect();
    assert_eq!(written.iter().map(|path| real(path)).collect::<Vec<_>>(), expected);
    let mut items = Vec::new();
    for path in &written {
        let file: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        for item in file["items"].as_array().unwrap() {
            items.push((real(&path.parent().unwrap().join(item["path"].as_str().unwrap())), item["name"].clone()));
        }
    }
    let undocumented = (0..files.len()).filter(|&f| !files[f].1);
    let mut wanted: Vec<_> =
        undocumented.map(|f| (real(&root.join("pkg").join(name(f))), json!(format!("v{f}")))).collect();
    items.sort_by_key(|item| item.0.clone());
    wanted.sort_by_key(|item| item.0.clone());
    assert_eq!(items, wanted);

    // A baseline that isn't there fails the plan, naming it, unless no config with files reads it.
    if let Some(writer) = baseline {
        fs::remove_file(root.join(if writer == Writer::Root { "known.json" } else { "pkg/known.json" })).unwrap();
        let read = layout == Layout::Extends || writer == Writer::Root || !files.is_empty();
        match plan(&[]) {
            Ok(_) => assert!(!read),
            Err(error) => assert!(read && error.contains("known.json"), "{error}"),
        }
    }
}

/// The rules a cache test scans with, by index: each scans its file differently from the others.
fn rules(index: u8) -> Rules {
    let mut rules = Rules::default();
    match index {
        0 => {}
        1 => rules.scope = Scope::All,
        2 => rules.require_description = true,
        3 => rules.exclude_tags = vec!["internal".into()],
        4 => rules.kinds = Some(vec![Kind::Function]),
        _ => rules.ignore_names = vec![regex::Regex::new("^C$").unwrap()],
    }
    rules
}

/// A cache gives back a file's items only for the rules they were scanned with, `docov-ignore`
/// comments and all, and other rules scan the file again.
#[hegel::test(test_cases = 30)]
fn the_cache_keys_on_the_rules(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.ts");
    write(
        dir.path(),
        "a.ts",
        "/** @internal */\nexport const a = 1;\n// docov-ignore-next-line: later\nexport function b() {}\n/** C. */\nexport class C {}\nconst d = 1;\n",
    );
    let (first, second) = (tc.draw(gs::integers::<u8>().max_value(5)), tc.draw(gs::integers::<u8>().max_value(5)));
    let (first_rules, second_rules) = (rules(first), rules(second));
    let cache_path = dir.path().join(".docov-cache");
    let scanned = batch::scan(&[(path.as_path(), &first_rules)], &Cache::read(cache_path.clone())).unwrap();
    assert_eq!(scanned, vec![scan_file(&path, &first_rules).unwrap()]);

    let cache = Cache::read(cache_path);
    let kept = cache.stamp(&path, &second_rules).and_then(|stamp| cache.get(&path, stamp));
    assert_eq!(kept.as_ref(), (first == second).then_some(&scanned[0].items));
    let again = batch::scan(&[(path.as_path(), &second_rules)], &cache).unwrap();
    assert_eq!(again, vec![scan_file(&path, &second_rules).unwrap()]);
}

/// `--print-config` describes each feature with a setting worth printing as it was written, the
/// flags winning: the gates (thresholds by glob and negative ones too, overrides,
/// `maxUndocumented`, the baseline, the changed files and the diff threshold), what counts, and
/// the metrics and order the reports add.
#[hegel::test(test_cases = 30)]
fn print_config_describes_every_setting(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let threshold = tc.draw(gs::sampled_from(vec![
        json!(null),
        json!(80.0),
        json!(-3),
        json!({ "global": 50.0, "src/**": 70.0 }),
        json!({ "perFile": -2 }),
    ]));
    let overrides = tc.draw(gs::sampled_from(vec![
        json!([]),
        json!([{ "files": ["a/"], "exclude": ["*.js"] }, { "files": ["b/"], "threshold": 20.0 }]),
    ]));
    let most = tc.draw(gs::optional(gs::integers::<usize>().max_value(9)));
    let flags = tc.draw(gs::booleans());
    let config = json!({
        "threshold": threshold,
        "overrides": overrides,
        "maxUndocumented": most,
        "baseline": "known.json",
        "access": ["public"],
        "excludeTags": ["internal"],
        "exports": { "esm": true, "cjs": false, "globals": true },
        "ignoreDeprecated": flags,
        "ignore": { "names": ["^_"] },
        "inherit": "override",
        "kinds": ["function", "class"],
        "requireDescription": flags,
        "skipGenerated": flags,
        "reportUnusedIgnores": flags,
        "fileDocs": "require",
        "virtual": flags,
        "members": "all",
        "commonjsMembers": flags,
        "examplesThreshold": 30,
        "releaseTagsThreshold": 40,
    });
    write(dir.path(), "docov.json", &config.to_string());
    let changed = tc.draw(gs::sampled_from(vec![
        (vec![], json!("all")),
        (vec!["--changed"], json!({ "since": null })),
        (vec!["--changed", "--since", "dev"], json!({ "since": "dev" })),
        (vec!["--staged"], json!("staged")),
    ]));
    let mut args = changed.0;
    if flags {
        args.extend(["--baseline", "other.json", "--write-baseline", "--diff-threshold", "90", "--diff-base", "main"]);
        args.extend(["--sort", "priority", "--examples", "--types"]);
    }
    let config = Config::load(Some(&dir.path().join("docov.json"))).unwrap();
    let described = Settings::new(&matches(&args), config.keys).describe();

    let (baseline, diff) = if flags {
        (json!({ "path": "other.json", "write": true }), json!({ "base": "main", "threshold": 90.0 }))
    } else {
        (json!({ "path": "known.json", "write": false }), json!({ "base": null, "threshold": null }))
    };
    let expected = [
        ("threshold", threshold),
        ("overrides", overrides),
        ("max_undocumented", json!(most)),
        ("baseline", baseline),
        ("changed", changed.1),
        ("diff", diff),
        ("access", json!(["public"])),
        ("exclude_tags", json!(["internal"])),
        ("exports", json!({ "esm": true, "cjs": false, "globals": true })),
        ("ignore_deprecated", json!(flags)),
        ("ignore_names", json!(["^_"])),
        ("inherit", json!("override")),
        ("kinds", json!(["function", "class"])),
        ("require_description", json!(flags)),
        ("skip_generated", json!(flags)),
        ("report_unused_ignores", json!(flags)),
        ("file_docs", json!("require")),
        ("virtual_types", json!(flags)),
        ("members", json!("all")),
        ("commonjs_members", json!(flags)),
        ("examples", json!({ "text": flags, "threshold": 30.0 })),
        ("release_tags", json!({ "on": true, "threshold": 40.0 })),
        ("types", json!({ "on": flags, "threshold": null })),
        ("sort", json!(if flags { "priority" } else { "position" })),
    ];
    for (name, value) in expected {
        assert_eq!(described.get(name), Some(&value), "{name}");
    }
}

/// A jsdoc.json's excluded path whose name ends in a `\`, which escapes nothing in a glob, fails
/// naming the jsdoc.json. On Windows that's a separator, and the path has another name.
#[hegel::test(test_cases = 2)]
fn a_jsdoc_exclude_that_is_no_glob_fails(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let name = tc.draw(gs::sampled_from(vec!["a", "b"]));
    write(dir.path(), "jsdoc.json", &json!({ "source": { "exclude": [format!("{name}\\")] } }).to_string());
    let read = jsdoc::read(&dir.path().join("jsdoc.json"));
    if cfg!(unix) {
        let error = read.unwrap_err();
        assert!(error.contains("jsdoc.json") && error.contains(name), "{error}");
    } else {
        assert_eq!(read.unwrap().exclude.len(), 1);
    }
}

/// The binary with features from different lanes at once: stdin scans by the rules and refuses
/// what would act on its one file, `--list-files`
/// leaves out what a nested config excludes, and `--by-package` fails naming a package.json that
/// doesn't parse.
#[hegel::test(test_cases = 2)]
fn the_cli_combines_its_features(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Tags alone document an item unless a description is required.
    let required = tc.draw(gs::booleans());
    let args: &[&str] =
        if required { &["--stdin-filename", "a.ts", "--require-description"] } else { &["--stdin-filename", "a.ts"] };
    let (code, out, _) = docov_with_stdin(root, args, b"/** @internal */\nexport const a = 1;\n");
    let total = if required { "   0.0%  0/1  total" } else { " 100.0%  1/1  total" };
    assert_eq!((code, out.lines().last()), (0, Some(total)));
    // Nor does it write files, scan again or pick files by git, which would go by the one buffer.
    let other = tc.draw(gs::sampled_from(vec![
        "--add-ignore",
        "--write-baseline",
        "--update-threshold",
        "--watch",
        "--staged",
    ]));
    let (code, _, err) = docov_with_stdin(root, &["--stdin-filename", "a.ts", other], b"");
    assert_eq!(code, 2);
    assert!(err.contains("cannot be used with"), "{err}");

    write(root, "docov.json", r#"{ "nested": true }"#);
    write(root, "a/docov.json", r#"{ "exclude": ["skip*"] }"#);
    for name in ["a/keep.ts", "a/skip.ts", "skip.ts"] {
        write(root, name, "export const a = 1;\n");
    }
    let (code, out, _) = docov(root, &["--list-files"], false);
    let listed = [Path::new("a").join("keep.ts"), PathBuf::from("skip.ts")].map(|p| p.display().to_string() + "\n");
    assert_eq!((code, out), (0, listed.concat()));

    write(root, "package.json", "{");
    let (code, out, err) = docov(root, &["--by-package"], false);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.starts_with("docov: package.json: "), "{err}");
}

/// A function each way the metrics tell apart: documented with an `@example` and a typed `@param`,
/// documented without either, undocumented, or undocumented under a `docov-ignore`, which leaves it
/// out of every metric.
const FUNCTIONS: [&str; 4] = [
    "/**\n * F.\n * @param {number} a A.\n * @example f(1)\n */\nexport function f{i}(a) {}\n",
    "/**\n * F.\n * @param a A.\n */\nexport function f{i}(a) {}\n",
    "export function f{i}(a) {}\n",
    "// docov-ignore-next-line\nexport function f{i}(a) {}\n",
];

/// Example and type coverage gate each nested config's own files, with the thresholds it has or
/// gets from the root config, counting only the items that aren't ignored, and a nested config's
/// failures name it.
#[hegel::test(test_cases = 40)]
fn metric_gates_go_by_each_nested_config(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let threshold = || tc.draw(gs::optional(gs::integers::<u8>().max_value(100)));
    let (root_examples, pkg_examples, pkg_types) = (threshold(), threshold(), threshold());
    // A key written as null would take the root config's away.
    let keys = |pairs: [(&str, Option<u8>); 2]| {
        serde_json::Value::Object(
            pairs.into_iter().filter_map(|(key, value)| Some((key.into(), value?.into()))).collect(),
        )
    };
    let mut top = keys([("examplesThreshold", root_examples), ("typesThreshold", None)]);
    top["nested"] = true.into();
    write(root, "docov.json", &top.to_string());
    write(
        root,
        "pkg/docov.json",
        &keys([("examplesThreshold", pkg_examples), ("typesThreshold", pkg_types)]).to_string(),
    );
    // Each group's functions, by their kind in FUNCTIONS.
    let mut groups: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for (g, folder) in ["", "pkg/"].into_iter().enumerate() {
        let kinds = tc.draw(gs::vecs(gs::integers::<usize>().max_value(3)).max_size(4));
        let source: String =
            kinds.iter().enumerate().map(|(i, &k)| FUNCTIONS[k].replace("{i}", &i.to_string())).collect();
        write(root, &format!("{folder}m.js"), &source);
        groups[g] = kinds;
    }

    let mut config = Config::load(Some(&root.join("docov.json"))).unwrap();
    let settings = Settings::new(&matches(&[]), std::mem::take(&mut config.keys));
    let plan =
        nested::Plan::new(&config, settings, &matches(&[]), walk::files(&[root.to_owned()], |_, _| false).unwrap())
            .unwrap();
    let reports = batch::scan(&plan.files(), &Cache::default()).unwrap();

    let failure = |kinds: &[usize], has: usize, name: &str, threshold: Option<u8>| {
        let counted = kinds.iter().filter(|&&k| k != 3).count();
        let percent = Totals { documented: kinds.iter().filter(|&&k| k == has).count(), total: counted }.percent();
        let threshold = threshold.filter(|&t| percent < f64::from(t))?;
        Some(format!("{percent:.1}% with {name}, under the {threshold}% {name} threshold"))
    };
    let pkg = format!("{}: ", root.join("pkg").join("docov.json").display());
    let mut wanted: Vec<String> = failure(&groups[0], 0, "examples", root_examples).into_iter().collect();
    let pkg_failures = [
        failure(&groups[1], 0, "examples", pkg_examples.or(root_examples)),
        failure(&groups[1], 0, "types", pkg_types),
    ];
    wanted.extend(pkg_failures.into_iter().flatten().map(|f| pkg.clone() + &f));
    assert_eq!(plan.gates(&reports).1, wanted);
}

/// The cache keeps every fact about an item, those the JSON report leaves out too, so the metrics
/// and `--sort priority` read the same from it as from a fresh scan.
#[hegel::test(test_cases = 30)]
fn facts_survive_the_cache(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let tags = tc.draw(gs::vecs(gs::sampled_from(vec![
        "@example f(1)",
        "@since 1.2",
        "@deprecated",
        "@public",
        "@beta",
        "@param {number} a A.",
        "@param b B.",
        "@returns {number} R.",
    ])));
    let doc: String = tags.iter().map(|tag| format!(" * {tag}\n")).collect();
    let documented = tc.draw(gs::booleans());
    let block = if documented { format!("/**\n * F.\n{doc} */\n") } else { String::new() };
    write(dir.path(), "a.js", &format!("{block}export function f(a, b) {{}}\nexport class C {{ m(x) {{}} }}\n"));
    let path = dir.path().join("a.js");
    let rules = Rules::default();
    let cache_path = dir.path().join(".docov-cache");
    let fresh = scan_file(&path, &rules).unwrap();
    batch::scan(&[(path.as_path(), &rules)], &Cache::read(cache_path.clone())).unwrap();

    let cache = Cache::read(cache_path);
    let kept = cache.stamp(&path, &rules).and_then(|stamp| cache.get(&path, stamp));
    assert_eq!(kept.as_ref(), Some(&fresh.items));
}

/// Under the public scope, each nested config's files count by what its own folder's package.json
/// reaches, or all their exports when it sets another scope. A file the cache kept counts again
/// when what the entry points reach in it changes, though the file didn't, and an entry point with
/// no source fails naming its package.json.
#[hegel::test(test_cases = 20)]
fn public_scope_goes_by_each_nested_config(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "docov.json", r#"{ "nested": true, "scope": "public" }"#);
    write(root, "package.json", r#"{ "main": "index.js" }"#);
    write(root, "index.ts", "export const a = 1;\n");
    write(root, "other.ts", "export const b = 1;\n");
    let exports = tc.draw(gs::booleans());
    write(root, "pkg/docov.json", if exports { r#"{ "scope": "exports" }"# } else { "{}" });
    write(root, "pkg/x.ts", "export const c = 1;\n");
    write(root, "pkg/y.ts", "export const d = 1;\n");
    let cache_path = root.join(".docov-cache");
    let scan = |main: &str| {
        write(root, "pkg/package.json", &json!({ "main": main }).to_string());
        let mut config = Config::load(Some(&root.join("docov.json"))).unwrap();
        let settings = Settings::new(&matches(&[]), std::mem::take(&mut config.keys));
        let files = walk::files(&[root.to_owned()], |path, _| path.ends_with(".docov-cache")).unwrap();
        let mut plan = nested::Plan::new(&config, settings, &matches(&[]), files).unwrap();
        plan.load_public(&[root.to_owned()])?;
        let reports = batch::scan(&plan.files(), &Cache::read(cache_path.clone())).unwrap();
        let fresh: Vec<_> = plan.files().into_iter().map(|(path, rules)| scan_file(path, rules).unwrap()).collect();
        assert_eq!(reports, fresh);
        Ok::<_, Vec<String>>(reports.iter().flat_map(|r| r.items.iter().map(|i| i.name.clone())).collect::<Vec<_>>())
    };
    for main in [tc.draw(gs::sampled_from(vec!["x.js", "y.js"])), "x.js", "y.js"] {
        let wanted = match (exports, main) {
            (true, _) => ["a", "c", "d"].as_slice(),
            (false, "x.js") => &["a", "c"],
            (false, _) => &["a", "d"],
        };
        assert_eq!(scan(main).unwrap(), wanted, "{main}");
    }
    let errors = scan("gone.js");
    if exports {
        assert!(errors.is_ok());
    } else {
        let errors = errors.unwrap_err();
        assert!(
            errors.len() == 1 && errors[0].contains(&root.join("pkg").join("package.json").display().to_string()),
            "{errors:?}"
        );
    }
}

/// `--explain` explains a file by the config it's scanned with: a nested config's rules, and a line
/// saying so when that config leaves the file out. It reads the file, so stdin can't stand in.
#[hegel::test(test_cases = 2)]
fn the_cli_explains_by_the_effective_config(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "docov.json", r#"{ "nested": true }"#);
    write(root, "pkg/docov.json", r#"{ "excludeTags": ["internal"], "exclude": ["skip.ts"] }"#);
    let source = "/** @internal */\nexport const a = 1;\n";
    for path in ["a.ts", "pkg/a.ts", "pkg/skip.ts"] {
        write(root, path, source);
    }
    let explained = [
        ("a.ts", "a.ts:2:1  variable a  documented by the block at 1:1"),
        ("pkg/a.ts", "pkg/a.ts:2:1  variable a  left out: its JSDoc has @internal"),
        ("pkg/skip.ts", "pkg/skip.ts  left out: the config excludes it"),
    ];
    for (path, line) in explained {
        assert_eq!(docov(root, &["--explain", path], false), (0, format!("{line}\n"), String::new()));
    }
    let path = tc.draw(gs::sampled_from(explained.map(|(path, _)| path).to_vec()));
    let (code, _, err) = docov_with_stdin(root, &["--explain", path, "--stdin-filename", path], b"");
    assert_eq!(code, 2);
    assert!(err.contains("cannot be used with"), "{err}");
}
