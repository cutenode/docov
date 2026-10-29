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
fn bad_configs_and_files_fail_with_their_paths() {
    let dir = project();
    let (code, _, err) = docov(dir.path(), &["--config", "missing.json"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: missing.json: "), "{err}");

    fs::write(dir.path().join("docov.json"), r#"{ "treshold": 1 }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown field `treshold`"), "{err}");
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
