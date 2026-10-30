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
    for flag in ["--threshold <PERCENT>", "--scope <SCOPE>"] {
        assert!(out.contains(flag), "{flag}: {out}");
    }
    // Every format, with what it writes.
    for format in docov::report::FORMATS {
        assert!(out.contains(&format!("- {}: {}", format.name, format.help)), "{}: {out}", format.name);
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
fn several_formats_go_to_stdout_and_their_files_from_one_scan() {
    let dir = project();
    let lcov = Path::new("coverage").join("docov.lcov");
    let (code, text, _) = docov(dir.path(), &[]);
    let (_, json, _) = docov(dir.path(), &["--format", "json"]);
    let args = ["--format", "text", "--format", &format!("lcov={}", lcov.display()), "--format", "json"];
    let (_, out, _) = docov(dir.path(), &args);
    assert_eq!((code, out), (0, format!("{text}{json}")));
    assert!(fs::read_to_string(dir.path().join(&lcov)).unwrap().starts_with("SF:"));

    // --output is the file for the one format without its own.
    let (code, out, _) = docov(dir.path(), &["--output", "out.txt"]);
    assert_eq!((code, out.as_str(), fs::read_to_string(dir.path().join("out.txt")).unwrap()), (0, "", text));
    let (code, _, err) = docov(dir.path(), &["--output", "out.txt", "--format", "json", "--format", "text"]);
    assert_eq!((code, err.as_str()), (2, "docov: --output needs exactly one --format without a file\n"));
    let (code, _, err) = docov(dir.path(), &["--format", "html"]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown format `html`, expected one of: "), "{err}");
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
