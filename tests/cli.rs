//! The binary, run the way a user or CI would.

use std::{fs, path::Path, process::Command};

fn docov(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_docov")).args(args).current_dir(dir).output().unwrap();
    let text = |b: Vec<u8>| String::from_utf8(b).unwrap();
    (out.status.code().unwrap(), text(out.stdout), text(out.stderr))
}

/// A project with three exports, two documented, and an ignored file that would lower it.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("gen")).unwrap();
    fs::write(src.join("a.ts"), "/** a */\nexport function a() {}\nexport const b = 1;\n").unwrap();
    fs::write(src.join("c.js"), "/** c */\nexport class C {}\n").unwrap();
    fs::write(src.join("gen/x.ts"), "export const x = 1;\n").unwrap();
    fs::write(src.join("notes.md"), "not code").unwrap();
    fs::write(dir.path().join(".gitignore"), "src/gen/\n").unwrap();
    dir
}

#[test]
fn reports_each_file_and_the_total() {
    let dir = project();
    let (code, out, err) = docov(dir.path(), &[]);
    assert_eq!((code, err.as_str()), (0, ""));
    let path = Path::new("src").join("a.ts").display().to_string();
    assert_eq!(
        out,
        format!(
            "  50.0%  1/2  {path}\n         {path}:3:1  variable b\n 100.0%  1/1  {}\n  66.7%  2/3  total\n",
            Path::new("src").join("c.js").display()
        )
    );
}

#[test]
fn coverage_under_the_threshold_fails() {
    let dir = project();
    let (code, _, err) = docov(dir.path(), &["--threshold", "70"]);
    assert_eq!((code, err.as_str()), (1, "docov: 66.7% documented, under the 70% threshold\n"));
    assert_eq!(docov(dir.path(), &["--threshold", "66"]).0, 0);
    assert_eq!(docov(dir.path(), &["--threshold", "101"]).0, 2);
}

#[test]
fn the_config_file_sets_paths_exclude_threshold_and_scope() {
    let dir = project();
    fs::write(dir.path().join("src/c.js"), "/** c */\nexport class C {}\nfunction hidden() {}\n").unwrap();
    fs::write(dir.path().join("docov.json"), r#"{ "paths": ["src"], "exclude": ["*.js"], "threshold": 60 }"#).unwrap();
    let (code, out, _) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((code, &json["documented"], &json["total"]), (1, &1.into(), &2.into()));

    fs::write(dir.path().join("docov.json"), r#"{ "scope": "all" }"#).unwrap();
    let (_, out, _) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["total"], 4);
    // The command line wins over the file.
    let (_, out, _) = docov(dir.path(), &["--format", "json", "--scope", "exports", "src/c.js"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((&json["documented"], &json["total"]), (&1.into(), &1.into()));
}

#[test]
fn the_config_file_is_found_from_subfolders_and_takes_comments() {
    let dir = project();
    let config =
        "// From src/ too.\n{\n  \"paths\": [\"./src\"], /* its folder's */\n  \"exclude\": [\"src/*.js\"],\n}\n";
    fs::write(dir.path().join(".docovrc"), config).unwrap();
    let (code, out, err) = docov(&dir.path().join("src/gen"), &[]);
    let a = Path::new("..").join("..").join("src").join("a.ts");
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("  50.0%  1/2  {0}\n         {0}:3:1  variable b\n  50.0%  1/2  total\n", a.display()));

    // docov.jsonc wins over .docovrc, docov.json over both, and a config nearer the current folder
    // over all of them.
    fs::write(dir.path().join("docov.jsonc"), r#"{ "threshold": 100 }"#).unwrap();
    assert_eq!(docov(&dir.path().join("src"), &[]).0, 1);
    fs::write(dir.path().join("docov.json"), "{}").unwrap();
    assert_eq!(docov(&dir.path().join("src"), &[]).0, 0);
    fs::write(dir.path().join("src/.docovrc"), r#"{ "threshold": 100 }"#).unwrap();
    assert_eq!(docov(&dir.path().join("src"), &[]).0, 1);
}

#[test]
fn bad_configs_and_files_fail_with_their_paths() {
    let dir = project();
    let (code, _, err) = docov(dir.path(), &["--config", "missing.json"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: missing.json: "), "{err}");

    fs::write(dir.path().join("docov.json"), r#"{ "treshold": 1 }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown field `treshold`"), "{err}");

    // The same range as --threshold.
    fs::write(dir.path().join("docov.json"), r#"{ "threshold": 150 }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: docov.json: expected a number from 0 to 100"), "{err}");
    fs::write(dir.path().join("docov.json"), r#"{ "threshold": "80" }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.contains("invalid type: string \"80\""), "{err}");
    // Stripped comments leave the lines and columns where they were.
    fs::write(dir.path().join("docov.json"), "// settings\n{ /* a typo */ \"treshold\": 1, }").unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: docov.json: unknown field `treshold`, expected one of "), "{err}");
    assert!(err.ends_with(" at line 2 column 25\n"), "{err}");
    // Features add keys, so the list is checked key by key.
    for key in ["$schema", "paths", "exclude", "threshold", "scope"] {
        assert!(err.contains(&format!("`{key}`")), "{key}: {err}");
    }
    // Not an object, a key that isn't a string, a value that isn't JSON, and a known key of the
    // wrong type, each with where it is.
    let bad = [
        ("[]", "invalid type: sequence, expected an object at line 1 column 0"),
        ("{ 1: 2 }", "key must be a string at line 1 column 3"),
        (r#"{ "paths": [1 }"#, "expected `,` or `]` at line 1 column 15"),
        (r#"{ "paths": 1 }"#, "invalid type: integer `1`, expected a sequence at line 1 column 12"),
    ];
    for (text, message) in bad {
        fs::write(dir.path().join("docov.json"), text).unwrap();
        let (code, _, err) = docov(dir.path(), &[]);
        assert_eq!((code, err), (2, format!("docov: docov.json: {message}\n")), "{text}");
    }
    // A slash that starts no comment would otherwise vanish.
    fs::write(dir.path().join("docov.json"), r#"{ "threshold": 5 / }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!((code, err.as_str()), (2, "docov: docov.json: a `/` that doesn't start a comment\n"));
    fs::remove_file(dir.path().join("docov.json")).unwrap();

    fs::write(dir.path().join("src/broken.ts"), "export function (").unwrap();
    let (code, out, err) = docov(dir.path(), &[]);
    let path = Path::new("src").join("broken.ts").display().to_string();
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.starts_with(&format!("docov: {path}:1:")), "{err}");

    let (code, _, err) = docov(dir.path(), &["missing-folder"]);
    assert_eq!(code, 2);
    assert!(err.contains("missing-folder"), "{err}");
}

#[test]
fn help_describes_docov_and_lists_every_features_flags() {
    let dir = project();
    let (code, out, _) = docov(dir.path(), &["--help"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("Checks how much of a JavaScript or TypeScript project's API has JSDoc."), "{out}");
    for flag in ["--threshold <PERCENT>", "--scope <SCOPE>", "--max-undocumented <N>"] {
        assert!(out.contains(flag), "{flag}: {out}");
    }
}

#[test]
fn lcov_marks_each_items_line() {
    let dir = project();
    // Named files keep the paths they were given, separators and all.
    let (code, out, _) = docov(dir.path(), &["--format", "lcov", "src/c.js", "src/a.ts"]);
    let a = "SF:src/a.ts\nDA:2,1\nDA:3,0\nLH:1\nLF:2\nend_of_record\n";
    let c = "SF:src/c.js\nDA:2,1\nLH:1\nLF:1\nend_of_record\n";
    assert_eq!((code, out), (0, format!("{a}{c}")));
}

#[test]
fn the_checked_in_schema_is_current() {
    let dir = tempfile::tempdir().unwrap();
    let (code, out, _) = docov(dir.path(), &["--print-schema"]);
    let checked_in = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docov.schema.json")).unwrap();
    assert_eq!(code, 0);
    assert_eq!(out, checked_in, "regenerate it: cargo run -- --print-schema > docov.schema.json");
}

#[test]
fn the_readme_points_at_this_versions_schema() {
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")).unwrap();
    let url =
        format!("https://raw.githubusercontent.com/cutenode/docov/v{}/docov.schema.json", env!("CARGO_PKG_VERSION"));
    assert!(readme.contains(&url), "point the README's $schema at {url}");
}

#[test]
fn thresholds_by_file_and_glob_each_fail_on_their_own_line() {
    let dir = project();
    let a = Path::new("src").join("a.ts").display().to_string();
    // `src/*.js` and `src/a.*` pass: one item, documented, and one undocumented item of at most 1.
    let config = r#"{ "threshold": { "global": 60, "perFile": 60, "src/*.js": 100, "src/a.*": -1, "src/": 70 } }"#;
    fs::write(dir.path().join("docov.json"), config).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    let wanted = format!(
        "docov: {a}: 50.0% documented, under the 60% per-file threshold\ndocov: src/: 66.7% documented, under the 70% threshold\n"
    );
    assert_eq!((code, err), (1, wanted));
    // --threshold is the global one.
    let (code, _, err) = docov(dir.path(), &["--threshold", "70", "src/c.js"]);
    assert_eq!((code, err.as_str()), (0, ""));

    // A negative threshold is the most undocumented items, and -0 is 0%.
    fs::write(dir.path().join("docov.json"), r#"{ "threshold": { "global": -0, "src/**/*.ts": -1, "src/a.ts": -2 } }"#)
        .unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!((code, err.as_str()), (0, ""));
    fs::write(dir.path().join("src/b.ts"), "export const z = 1;\n").unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!((code, err.as_str()), (1, "docov: src/**/*.ts: 2 undocumented, over the threshold of 1\n"));
    fs::write(dir.path().join("docov.json"), r#"{ "threshold": -1 }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!((code, err.as_str()), (1, "docov: 2 undocumented, over the threshold of 1\n"));

    let bad = [
        (r#"{ "threshold": -0.5 }"#, "expected a number from 0 to 100, or a negative whole number at line 1"),
        (
            r#"{ "threshold": { "perFile": 101 } }"#,
            "expected a number from 0 to 100, or a negative whole number at line 1",
        ),
        (r#"{ "threshold": { "src/[z": 1 } }"#, "error parsing glob 'src/[z'"),
        (r#"{ "threshold": { "global": "80" } }"#, "invalid type: string \"80\", expected f64"),
        (r#"{ "threshold": true }"#, "invalid type: boolean `true`, expected a number, or an object of them"),
    ];
    for (text, message) in bad {
        fs::write(dir.path().join("docov.json"), text).unwrap();
        let (code, _, err) = docov(dir.path(), &[]);
        assert_eq!(code, 2, "{text}");
        assert!(err.contains(message), "{text}: {err}");
    }
}

#[test]
fn overrides_change_exclude_and_threshold_for_their_files() {
    let dir = project();
    fs::create_dir_all(dir.path().join("src/legacy")).unwrap();
    fs::write(dir.path().join("src/legacy/old.ts"), "export const o = 1;\nexport const p = 2;\n").unwrap();
    fs::write(dir.path().join("src/legacy/old.test.ts"), "export const t = 1;\n").unwrap();
    let config = r#"{
        "exclude": ["*.test.ts", "src/c.js"],
        "threshold": 60,
        "overrides": [
            { "files": ["src/legacy/"], "exclude": [], "threshold": 50 },
            { "files": ["src/legacy/old.ts"], "threshold": { "perFile": 10 } }
        ]
    }"#;
    fs::write(dir.path().join("docov.json"), config).unwrap();
    let (code, out, err) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let paths: Vec<_> =
        json["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap().replace('\\', "/")).collect();
    // The legacy folder keeps its test file, and old.ts has the last override's threshold.
    assert_eq!(paths, ["src/a.ts", "src/legacy/old.test.ts", "src/legacy/old.ts"]);
    let old = Path::new("src").join("legacy").join("old.ts").display().to_string();
    let wanted = format!(
        "docov: 50.0% documented, under the 60% threshold\ndocov: src/legacy/: 0.0% documented, under the 50% threshold\ndocov: {old}: 0.0% documented, under the 10% per-file threshold\n"
    );
    assert_eq!((code, err), (1, wanted));

    fs::write(dir.path().join("docov.json"), r#"{ "overrides": [{ "files": ["src"], "scope": "all" }] }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown field `scope`, expected one of `files`, `exclude`, `threshold`"), "{err}");
}

#[test]
fn max_undocumented_is_a_gate_of_its_own() {
    let dir = project();
    fs::write(dir.path().join("docov.json"), r#"{ "maxUndocumented": 0, "threshold": 70 }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    // Gates run in their files' name order.
    let wanted = "docov: 1 undocumented, over the limit of 0\ndocov: 66.7% documented, under the 70% threshold\n";
    assert_eq!((code, err.as_str()), (1, wanted));
    let (code, _, err) = docov(dir.path(), &["--max-undocumented", "1", "--threshold", "0"]);
    assert_eq!((code, err.as_str()), (0, ""));
}

#[test]
fn the_checked_in_baseline_schema_is_current() {
    let dir = tempfile::tempdir().unwrap();
    let (code, out, _) = docov(dir.path(), &["--print-schema", "baseline"]);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docov-baseline.schema.json");
    assert_eq!(code, 0);
    assert_eq!(
        out,
        fs::read_to_string(path).unwrap(),
        "cargo run -- --print-schema baseline > docov-baseline.schema.json"
    );
}

#[test]
fn a_baseline_keys_items_by_path_kind_and_name() {
    let dir = project();
    let (code, _, err) = docov(&dir.path().join("src"), &["--write-baseline", "--threshold", "100"]);
    assert_eq!((code, err.as_str()), (0, "docov: wrote the undocumented items to docov-baseline.json\n"));
    let written = fs::read_to_string(dir.path().join("src/docov-baseline.json")).unwrap();
    let schema = format!(
        "https://raw.githubusercontent.com/cutenode/docov/v{}/docov-baseline.schema.json",
        env!("CARGO_PKG_VERSION")
    );
    let wanted = format!(
        "{{\n  \"$schema\": \"{schema}\",\n  \"items\": [\n    {{\n      \"path\": \"a.ts\",\n      \"kind\": \"variable\",\n      \"name\": \"b\"\n    }}\n  ]\n}}\n"
    );
    assert_eq!(written, wanted);

    // From the config's folder, with `/` on every OS, wherever docov runs.
    fs::write(dir.path().join("docov.json"), r#"{ "baseline": "known.json", "threshold": 100 }"#).unwrap();
    let known = r#"{ "items": [{ "path": "src/a.ts", "kind": "variable", "name": "b" }] }"#;
    fs::write(dir.path().join("known.json"), known).unwrap();
    let (code, out, err) = docov(&dir.path().join("src"), &[]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("variable b"), "the report still lists it: {out}");
    // Another kind or name isn't the same item.
    fs::write(dir.path().join("known.json"), known.replace("variable", "function")).unwrap();
    assert_eq!(docov(dir.path(), &[]).0, 1);
    let (code, _, err) = docov(dir.path(), &["--write-baseline", "--baseline", "gone/known.json"]);
    assert_eq!(code, 2);
    assert!(err.contains("known.json: "), "{err}");
    fs::write(dir.path().join("known.json"), r#"{ "items": [], "extra": 1 }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: known.json: unknown field `extra`"), "{err}");
}
