//! Properties of features working together: gates under extended and nested configs, the cache
//! with the rules, `--print-config` over every feature's setting, and the doc checks with nested
//! configs, ignored items and the cache.

use std::{
    fs,
    path::{Path, PathBuf},
};

use docov::{
    Kind, Rules, Scope, Settings, Totals, batch, cache::Cache, check, config::Config, feature::overrides::Overrides,
    jsdoc, nested, scan_file, walk,
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
/// `maxUndocumented`, the baseline, the changed files and the diff threshold), what counts, the
/// metrics and order the reports add, and the doc checks' levels and options.
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
        "completeThreshold": 50,
        "rules": { "require-param": "error", "summary-sentence": ["warn", { "maxLength": 80 }] },
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
        args.extend(["--sort", "priority", "--examples", "--types", "--rule", "require-param=warn"]);
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
        ("complete", json!({ "on": true, "threshold": 50.0 })),
        (
            "rules",
            json!({ "require-param": if flags { "warn" } else { "error" }, "summary-sentence": ["warn", { "maxLength": 80 }] }),
        ),
    ];
    for (name, value) in expected {
        assert_eq!(described.get(name), Some(&value), "{name}");
    }
}

/// A jsdoc.json's excluded path leaves out the file with just that name, whatever glob syntax is in
/// it: `\`, which escapes the next character in a glob, included. On Windows a `\` is a
/// separator, so the name is what follows the last one.
#[hegel::test(test_cases = 10)]
fn a_jsdoc_exclude_matches_just_its_name(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    // Each with a name it would match as a glob, unescaped.
    let (excluded, globbed) = tc.draw(gs::sampled_from(vec![
        ("a\\", "a"),
        ("a\\b", "ab"),
        ("\\a", "a"),
        ("a*b", "axb"),
        ("a[b]", "ab"),
        ("a{b,c}", "ac"),
        ("a?", "ax"),
    ]));
    write(dir.path(), "jsdoc.json", &json!({ "source": { "exclude": [excluded] } }).to_string());
    let sources = jsdoc::read(&dir.path().join("jsdoc.json")).unwrap();
    let [exclude] = sources.exclude.as_slice() else { panic!("{:?}", sources.exclude) };
    let name = Path::new(excluded).file_name().unwrap();
    assert!(exclude.globs.matches(Path::new(name), false), "{excluded}");
    let separated = name != excluded;
    assert!(separated || !exclude.globs.matches(Path::new(globbed), false), "{excluded} matched {globbed}");
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

/// A function a doc check property scans: `require-param` finds `a` at its block, and
/// `param-names` finds `b` on the block's third line.
const CHECKED: &str = "/**\n * F.\n * @param b B.\n */\nexport function f{i}(a) {}\n";

/// Each nested config's files go through its own doc checks: its `rules` in place of the root
/// config's, as a key it sets always replaces the root's whole, and a `--rule` flag's level over
/// both. Each config's gate fails on its own files' errors alone, a nested one's naming it.
#[hegel::test(test_cases = 40)]
fn doc_checks_go_by_each_nested_config(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let levels = || {
        let level = || tc.draw(gs::optional(gs::sampled_from(vec!["off", "warn", "error"])));
        let pairs = [("require-param", level()), ("param-names", level())];
        pairs
            .into_iter()
            .filter_map(|(id, level)| Some((id.to_owned(), json!(level?))))
            .collect::<serde_json::Map<_, _>>()
    };
    let top_rules = levels();
    let pkg_rules = tc.draw(gs::booleans()).then(levels);
    let flag = tc.draw(gs::optional(gs::sampled_from(vec!["off", "warn", "error"])));
    write(root, "docov.json", &json!({ "nested": true, "rules": top_rules }).to_string());
    let pkg = pkg_rules.as_ref().map_or_else(|| json!({}), |rules| json!({ "rules": rules }));
    write(root, "pkg/docov.json", &pkg.to_string());
    let counts = [tc.draw(gs::integers::<usize>().max_value(3)), tc.draw(gs::integers::<usize>().max_value(3))];
    for (folder, count) in ["", "pkg/"].into_iter().zip(counts) {
        let source: String = (0..count).map(|i| CHECKED.replace("{i}", &i.to_string())).collect();
        write(root, &format!("{folder}m.ts"), &source);
    }

    let args: Vec<String> =
        flag.iter().flat_map(|level| ["--rule".to_owned(), format!("require-param={level}")]).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut config = Config::load(Some(&root.join("docov.json"))).unwrap();
    let settings = Settings::new(&matches(&args), std::mem::take(&mut config.keys));
    let files = walk::files(&[root.to_owned()], |_, _| false).unwrap();
    let plan = nested::Plan::new(&config, settings, &matches(&args), files).unwrap();
    let reports = batch::scan(&plan.files(), &Cache::default()).unwrap();

    // Whether a rule is an error by a group's rules, if it's on.
    let level = |rules: &serde_json::Map<String, serde_json::Value>, id: &str| -> Option<bool> {
        let flagged = flag.filter(|_| id == "require-param");
        let level = flagged.or_else(|| rules.get(id).and_then(serde_json::Value::as_str))?;
        (level != "off").then_some(level == "error")
    };
    let pkg = format!("{}: ", root.join("pkg").join("docov.json").display());
    let mut failures = Vec::new();
    for (g, count) in counts.into_iter().enumerate() {
        let rules = if g == 1 { pkg_rules.as_ref().unwrap_or(&top_rules) } else { &top_rules };
        let mut wanted = Vec::new();
        for i in 0..count {
            for (id, line) in [("require-param", 5 * i + 1), ("param-names", 5 * i + 3)] {
                wanted.extend(level(rules, id).map(|error| (line, id, error)));
            }
        }
        wanted.sort_unstable();
        let path = root.join(if g == 1 { "pkg/m.ts" } else { "m.ts" });
        let report = reports.iter().find(|r| r.path == path).unwrap();
        let mut found: Vec<_> = report
            .items
            .iter()
            .flat_map(|i| i.facts.diagnostics.iter().map(|d| (d.line, d.rule, d.severity == check::Level::Error)))
            .collect();
        found.sort_unstable();
        assert_eq!(found, wanted, "{path:?}");
        let errors = wanted.iter().filter(|(_, _, error)| *error).count();
        if errors > 0 {
            let failure = format!("the doc checks found {errors} error{}", if errors == 1 { "" } else { "s" });
            failures.push(if g == 1 { format!("{pkg}{failure}") } else { failure });
        }
    }
    assert_eq!(plan.gates(&reports).1, failures);
}

/// The doc checks run on each counted item with a block, and on nothing else: an item a
/// `docov-ignore` comment takes out of the total (undocumented, with tags alone and
/// `requireDescription` on) gets no findings, while a documented one under the same comment
/// does, and an item `excludeTags` leaves out isn't there to check. The checks change nothing about
/// the items past their findings.
#[hegel::test(test_cases = 60)]
fn doc_checks_leave_ignored_items_be(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.ts");
    let comment = tc.draw(gs::sampled_from(vec!["", "// docov-ignore-next-line\n", "/* docov-ignore-file */\n"]));
    let described = tc.draw(gs::booleans());
    let internal = tc.draw(gs::booleans());
    let block = format!(
        "/**\n{} * @param b B.\n{} */\n",
        if described { " * F.\n" } else { "" },
        if internal { " * @internal\n" } else { "" }
    );
    // The file comment goes first, and the line comment right above the function.
    let source = match comment {
        "/* docov-ignore-file */\n" => format!("{comment}{block}export function f(a) {{}}\n"),
        _ => format!("{block}{comment}export function f(a) {{}}\n"),
    };
    write(dir.path(), "a.ts", &source);
    let require_description = tc.draw(gs::booleans());
    let exclude_tags = tc.draw(gs::booleans());
    let plain = Rules {
        require_description,
        exclude_tags: if exclude_tags { vec!["internal".into()] } else { Vec::new() },
        ..Rules::default()
    };
    let setting = check::Setting { level: check::Level::Warn, options: None };
    let checks = check::Checks::new([("require-param", &setting), ("param-names", &setting)]).unwrap();
    let rules = Rules { checks: Some(std::sync::Arc::new(checks)), ..plain.clone() };

    let checked = scan_file(&path, &rules).unwrap();
    let mut items = checked.items.clone();
    for item in &mut items {
        item.facts.diagnostics.clear();
    }
    assert_eq!(items, scan_file(&path, &plain).unwrap().items);
    let counted = !(internal && exclude_tags);
    let ignored = !comment.is_empty() && require_description && !described;
    let found: Vec<_> = checked.items.iter().flat_map(|i| i.facts.diagnostics.iter().map(|d| d.rule)).collect();
    let wanted = if counted && !ignored { vec!["param-names", "require-param"] } else { Vec::new() };
    let mut found = found;
    found.sort_unstable();
    assert_eq!(found, wanted, "{source}");
    assert_eq!(checked.items.len(), usize::from(counted));
}

/// A doc check setting a cache test scans with, by index: each finds something else in [`CHECKED`].
fn setting(index: u8) -> Option<(&'static str, check::Setting)> {
    let (id, level, options) = match index {
        0 => return None,
        1 => ("require-param", check::Level::Off, None),
        2 => ("require-param", check::Level::Warn, None),
        3 => ("require-param", check::Level::Error, None),
        4 => ("summary-sentence", check::Level::Warn, Some(json!({ "maxLength": 1 }))),
        _ => ("summary-sentence", check::Level::Warn, Some(json!({ "maxLength": 1, "period": false }))),
    };
    Some((id, check::Setting { level, options }))
}

/// The cache keeps what the doc checks found, and gives a file's items back only for the levels
/// and options of the checks they were scanned with: a check that's off is as good as none. A
/// cache whose finding names no rule docov has, or no rule at all, is read as empty, and rebuilt.
#[hegel::test(test_cases = 30)]
fn the_cache_keys_on_the_rule_levels(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.ts");
    write(dir.path(), "a.ts", &CHECKED.replace("{i}", ""));
    let draw = || tc.draw(gs::integers::<u8>().max_value(5));
    let (first, second) = (draw(), draw());
    let rules = |index: u8| {
        let setting = setting(index);
        let checks = check::Checks::new(setting.iter().map(|(id, setting)| (*id, setting)));
        Rules { checks: checks.map(std::sync::Arc::new), ..Rules::default() }
    };
    let (first_rules, second_rules) = (rules(first), rules(second));
    let cache_path = dir.path().join(".docov-cache");
    let scanned = batch::scan(&[(path.as_path(), &first_rules)], &Cache::read(cache_path.clone())).unwrap();
    let fresh = scan_file(&path, &first_rules).unwrap();
    assert_eq!(scanned, vec![fresh.clone()]);
    let findings = fresh.items.iter().any(|i| !i.facts.diagnostics.is_empty());
    assert_eq!(findings, ![0, 1].contains(&first));

    let cache = Cache::read(cache_path.clone());
    let kept = cache.stamp(&path, &second_rules).and_then(|stamp| cache.get(&path, stamp));
    assert_eq!(cache.stamp(&path, &rules(0)), cache.stamp(&path, &rules(1)));
    let off = |index: u8| index < 2;
    let same = first == second || (off(first) && off(second));
    assert_eq!(kept.as_ref(), same.then_some(&scanned[0].items));
    let again = batch::scan(&[(path.as_path(), &second_rules)], &cache).unwrap();
    assert_eq!(again, vec![scan_file(&path, &second_rules).unwrap()]);

    let text = fs::read_to_string(&cache_path).unwrap();
    for rule in ["\"nope\"", "1"] {
        let renamed = text.replace("\"rule\":\"require-param\"", &format!("\"rule\":{rule}"));
        fs::write(&cache_path, &renamed).unwrap();
        let cache = Cache::read(cache_path.clone());
        let kept = cache.stamp(&path, &second_rules).and_then(|stamp| cache.get(&path, stamp));
        assert_eq!(kept.is_some(), renamed == text);
    }
}

/// The binary prints each doc check finding to stderr, whatever the format, before why the scan
/// fails, which a finding at `error` makes it do.
#[hegel::test(test_cases = 4)]
fn the_cli_prints_doc_check_findings(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.ts", &CHECKED.replace("{i}", ""));
    let level = tc.draw(gs::sampled_from(vec!["warn", "error"]));
    let format = tc.draw(gs::sampled_from(vec!["text", "unix", "json"]));
    let rule = format!("require-param={level}");
    let (code, out, err) = docov(dir.path(), &["--rule", &rule, "--format", format], false);
    let found = "a.ts:1:1: require-param: missing @param for `a`";
    let failure = if level == "error" { "docov: the doc checks found 1 error\n" } else { "" };
    assert_eq!((code, err), (i32::from(level == "error"), format!("docov: {found}\n{failure}")));
    assert_eq!(out.contains(found), format == "unix", "{out}");
}
