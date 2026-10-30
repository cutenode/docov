//! The binary, run the way a user or CI would.

use std::{fs, path::Path, process::Command};

fn docov(dir: &Path, args: &[&str]) -> (i32, String, String) {
    docov_in(dir, args, &[])
}

/// docov with these environment variables, and without the ones it reads to change its output
/// (GitHub Actions' annotations and color) unless they set them: CI would otherwise change it.
fn docov_in(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
    for var in ["GITHUB_ACTIONS", "NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE"] {
        command.env_remove(var);
    }
    command.envs(env.iter().copied());
    let out = command.args(args).current_dir(dir).output().unwrap();
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
    for flag in [
        "--threshold <PERCENT>",
        "--scope <SCOPE>",
        "--max-undocumented <N>",
        "--baseline <FILE>",
        "--update-threshold",
    ] {
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
    let a = "TN:docov\nSF:src/a.ts\nFN:2,a\nFN:3,b\nFNDA:1,a\nFNDA:0,b\nFNF:2\nFNH:1\nDA:2,1\nDA:3,0\nLH:1\nLF:2\nend_of_record\n";
    let c = "TN:docov\nSF:src/c.js\nFN:2,C\nFNDA:1,C\nFNF:1\nFNH:1\nDA:2,1\nLH:1\nLF:1\nend_of_record\n";
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
    assert!(fs::read_to_string(dir.path().join(&lcov)).unwrap().starts_with("TN:docov\nSF:"));

    // --output is the file for the one format without its own.
    let (code, out, _) = docov(dir.path(), &["--output", "out.txt"]);
    assert_eq!((code, out.as_str(), fs::read_to_string(dir.path().join("out.txt")).unwrap()), (0, "", text));
    let (code, _, err) = docov(dir.path(), &["--output", "out.txt", "--format", "json", "--format", "text"]);
    assert_eq!((code, err.as_str()), (2, "docov: --output needs exactly one --format without a file\n"));
    let (code, _, err) = docov(dir.path(), &["--format", "pdf"]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown format `pdf`, expected one of: "), "{err}");
}

#[test]
fn github_actions_get_annotations_after_the_text() {
    let dir = project();
    let (_, text, _) = docov(dir.path(), &["--threshold", "70"]);
    let (code, out, _) = docov_in(dir.path(), &["--threshold", "70"], &[("GITHUB_ACTIONS", "true")]);
    let annotations = "::warning file=src/a.ts,line=3,col=1,title=docov::variable b is undocumented\n\
        ::error title=docov::66.7%25 documented, under the 70%25 threshold\n";
    assert_eq!((code, out), (1, format!("{text}{annotations}")));
    // Only without a --format.
    let (_, json, _) = docov(dir.path(), &["--format", "json"]);
    assert_eq!(docov_in(dir.path(), &["--format", "json"], &[("GITHUB_ACTIONS", "true")]).1, json);
}

#[test]
fn markdown_shows_the_change_from_an_earlier_json_report() {
    let dir = project();
    assert_eq!(docov(dir.path(), &["--format", "json=base.json"]).0, 0);
    fs::write(dir.path().join("src/a.ts"), "/** a */\nexport function a() {}\n/** b */\nexport const b = 1;\n")
        .unwrap();
    fs::write(dir.path().join("src/d.ts"), "export const d = 1;\nexport const e = 2;\n").unwrap();
    fs::remove_file(dir.path().join("src/c.js")).unwrap();
    let (code, out, err) = docov(dir.path(), &["--format", "markdown", "--compare", "base.json"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let table = "| Documented | Before | Change | Items | File |\n| ---: | ---: | ---: | ---: | :--- |\n\
        | 100.0% | 50.0% | +50.0 | 2/2 | `src/a.ts` |\n\
        | 0.0% |  | new | 0/2 | `src/d.ts` |\n\
        |  | 100.0% | gone |  | `src/c.js` |\n";
    let details = "<details>\n<summary><code>src/d.ts</code>: 2 undocumented</summary>\n\n\
        - 1:1 variable `d`\n- 2:1 variable `e`\n\n</details>\n";
    assert_eq!(out, format!("## docov\n\n**50.0%** documented (2/4), -16.7 from 66.7%\n\n{table}\n{details}"));
    // The json format doesn't change.
    let (_, json, _) = docov(dir.path(), &["--format", "json"]);
    assert_eq!(docov(dir.path(), &["--format", "json", "--compare", "base.json"]).1, json);

    // A base that isn't there or isn't a report fails, naming it.
    let (code, out, err) = docov(dir.path(), &["--format", "markdown", "--compare", "missing.json"]);
    assert_eq!(code, 2);
    assert!(out.is_empty() && err.starts_with("docov: missing.json: "), "{err}");
    fs::write(dir.path().join("base.json"), "{}").unwrap();
    let (code, _, err) = docov(dir.path(), &["--format", "markdown", "--compare", "base.json"]);
    assert_eq!((code, err.as_str()), (2, "docov: base.json: missing field `files` at line 1 column 2\n"));
}

#[test]
fn gitlab_fingerprints_are_fnv_1a_of_the_path_kind_and_name() {
    // GitLab matches issues between runs by fingerprint, so a change here reopens every one.
    let (_, out, _) = docov(project().path(), &["--format", "gitlab"]);
    let issues: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(issues[0]["fingerprint"], "0ed237bda4d88c42");
}

#[test]
fn badge_writes_an_svg_beside_the_report() {
    let dir = project();
    let (_, text, _) = docov(dir.path(), &[]);
    let (code, out, _) = docov(dir.path(), &["--badge", "docs/badge.svg", "--threshold", "60"]);
    assert_eq!((code, out), (0, text));
    let svg = fs::read_to_string(dir.path().join("docs/badge.svg")).unwrap();
    assert!(svg.contains("aria-label=\"docs: 66.7%\"") && svg.contains("#dfb317"), "{svg}");
    let (_, shields, _) = docov(dir.path(), &["--format", "shields"]);
    assert_eq!(shields, "{\"color\":\"blue\",\"label\":\"docs\",\"message\":\"66.7%\",\"schemaVersion\":1}\n");
}

#[test]
fn benchmark_is_the_total_for_github_action_benchmark() {
    let (_, out, _) = docov(project().path(), &["--format", "benchmark"]);
    assert_eq!(out, "[{\"name\":\"docov\",\"unit\":\"%\",\"value\":66.7}]\n");
}

#[test]
fn unix_writes_a_warning_line_per_undocumented_item() {
    let (_, out, _) = docov(project().path(), &["--format", "unix"]);
    let a = Path::new("src").join("a.ts");
    assert_eq!(out, format!("{}:3:1: variable b undocumented [docov]\n", a.display()));
}

#[test]
fn html_is_one_page_with_the_sources() {
    let dir = project();
    let (code, out, _) = docov(dir.path(), &["--format", "html=report.html"]);
    let page = fs::read_to_string(dir.path().join("report.html")).unwrap();
    assert_eq!((code, out.as_str()), (0, ""));
    let row =
        "<tr class=\"miss\"><td class=\"ln\">3</td><td>export const b = 1;</td><td class=\"note\">variable b</td></tr>";
    assert!(page.contains(row) && page.contains("prefers-color-scheme: dark") && !page.contains("<script"), "{page}");
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
fn require_description_comes_from_the_flag_or_the_config() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.ts"), "/** @internal */\nexport const a = 1;\n/** A. */\nexport const b = 1;\n")
        .unwrap();
    let total = |args: &[&str]| docov(dir.path(), args).1.lines().last().unwrap().to_owned();
    assert_eq!(total(&[]), " 100.0%  2/2  total");
    assert_eq!(total(&["--require-description"]), "  50.0%  1/2  total");
    fs::write(dir.path().join("docov.json"), r#"{ "requireDescription": true }"#).unwrap();
    assert_eq!(total(&[]), "  50.0%  1/2  total");
}

#[test]
fn excluded_tags_come_from_the_flags_or_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/** @internal */\nexport const a = 1;\n/** @hidden */\nexport const b = 1;\nexport const c = 1;\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let total = |args: &[&str]| docov(dir.path(), args).1.lines().last().unwrap().to_owned();
    assert_eq!(total(&[]), "  66.7%  2/3  total");
    assert_eq!(total(&["--exclude-tag", "internal", "--exclude-tag", "hidden"]), "   0.0%  0/1  total");
    fs::write(dir.path().join("docov.json"), r#"{ "excludeTags": ["internal"] }"#).unwrap();
    assert_eq!(total(&[]), "  50.0%  1/2  total");
    // The flags win over the key.
    assert_eq!(total(&["--exclude-tag", "hidden"]), "  50.0%  1/2  total");
}

#[test]
fn ignored_items_are_left_out_of_the_text_and_marked_in_the_json() {
    let dir = tempfile::tempdir().unwrap();
    let source = "// docov-ignore-next-line: legacy\nexport const a = 1;\nexport const b = 1;\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    fs::write(dir.path().join("b.ts"), "/* docov-ignore-file */\nexport const c = 1;\n").unwrap();
    let (code, out, _) = docov(dir.path(), &[]);
    assert_eq!((code, out.as_str()), (0, "   0.0%  0/1  a.ts\n         a.ts:3:1  variable b\n   0.0%  0/1  total\n"));
    let (_, out, _) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let a = &json["files"][0]["items"];
    assert_eq!((&a[0]["ignored"], &a[0]["reason"]), (&true.into(), &"legacy".into()));
    assert_eq!((a[1].get("ignored"), a[1].get("reason")), (None, None));
    let c = &json["files"][1]["items"][0];
    assert_eq!((&c["ignored"], c.get("reason")), (&true.into(), None));
}

#[test]
fn unused_ignores_fail_when_reported() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/** A. */\n// docov-ignore-next-line\nexport const a = 1;\n  // docov-ignore-next-line\n  /** B. */ export const b = 1;\n// docov-ignore-next-line\nexport const c = 1;\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    assert_eq!(docov(dir.path(), &[]).0, 0);
    let (code, _, err) = docov(dir.path(), &["--report-unused-ignores"]);
    assert_eq!(
        (code, err.as_str()),
        (1, "docov: a.ts:2:1: unused docov-ignore\ndocov: a.ts:4:3: unused docov-ignore\n")
    );
    fs::write(dir.path().join("docov.json"), r#"{ "reportUnusedIgnores": true }"#).unwrap();
    assert_eq!(docov(dir.path(), &[]).0, 1);
}

#[test]
fn each_line_of_a_gate_is_an_annotation_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let source = "// docov-ignore-next-line\n/** A. */ export const a = 1;\n// docov-ignore-next-line\n/** B. */ export const b = 1;\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let (code, out, _) = docov(dir.path(), &["--report-unused-ignores", "--format", "github"]);
    let errors =
        "::error title=docov::a.ts:1:1: unused docov-ignore\n::error title=docov::a.ts:3:1: unused docov-ignore\n";
    assert_eq!((code, out.as_str()), (1, errors));
}

#[test]
fn add_ignore_writes_a_comment_above_each_undocumented_item() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/** A. */\r\nexport const a = 1;\r\nexport class B {\r\n\tc() {}\r\n}";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let (code, out, err) = docov(dir.path(), &["--add-ignore=to do", "a.ts"]);
    assert_eq!((code, out.as_str(), err.as_str()), (0, "", "docov: added 2 docov-ignore-next-line comments\n"));
    let ignored = "/** A. */\r\nexport const a = 1;\r\n// docov-ignore-next-line: to do\r\nexport class B {\r\n\t// docov-ignore-next-line: to do\r\n\tc() {}\r\n}";
    assert_eq!(fs::read_to_string(dir.path().join("a.ts")).unwrap(), ignored);
    assert_eq!(docov(dir.path(), &[]).1, " 100.0%  1/1  a.ts\n 100.0%  1/1  total\n");
    assert_eq!(docov(dir.path(), &["--add-ignore"]).2, "docov: added 0 docov-ignore-next-line comments\n");
}

#[test]
fn kinds_pick_what_counts_and_break_the_total_down() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/** f */\nexport function f() {}\nexport class C { m() {} }\nexport const v = 1;\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let (_, out, _) = docov(dir.path(), &["--by-kind"]);
    assert!(
        out.ends_with("  25.0%  1/4  total\nby kind: function 1/1, class 0/1, method 0/1, variable 0/1\n"),
        "{out}"
    );
    let (_, out, _) = docov(dir.path(), &["--kind", "function,method", "--by-kind"]);
    assert!(out.ends_with("  50.0%  1/2  total\nby kind: function 1/1, method 0/1\n"), "{out}");
    fs::write(dir.path().join("docov.json"), r#"{ "kinds": ["class"] }"#).unwrap();
    let (_, out, _) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["kinds"], serde_json::json!({ "class": { "documented": 0, "total": 1, "percent": 0.0 } }));
}

#[test]
fn name_patterns_leave_out_what_they_match() {
    let dir = tempfile::tempdir().unwrap();
    let source = "export const _a = 1;\nexport const b = 1;\nexport class C { _d() {} e() {} }\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let total = |args: &[&str]| docov(dir.path(), args).1.lines().last().unwrap().to_owned();
    assert_eq!(total(&[]), "   0.0%  0/5  total");
    assert_eq!(total(&["--ignore-name", "^_"]), "   0.0%  0/3  total");
    assert_eq!(total(&["--ignore-name", "^C\\.e$", "--ignore-name", "^b"]), "   0.0%  0/3  total");
    fs::write(dir.path().join("docov.json"), r#"{ "ignore": { "names": ["^_", "^C$"] } }"#).unwrap();
    assert_eq!(total(&[]), "   0.0%  0/2  total");
    let (code, _, err) = docov(dir.path(), &["--ignore-name", "("]);
    assert_eq!(code, 2);
    assert!(err.contains("unclosed group"), "{err}");
    fs::write(dir.path().join("docov.json"), r#"{ "ignore": { "names": ["("] } }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: docov.json: regex parse error"), "{err}");
}

#[test]
fn access_levels_come_from_the_flag_or_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/** @private */\nexport const a = 1;\nexport class B { protected c = 1; }\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let total = |args: &[&str]| docov(dir.path(), args).1.lines().last().unwrap().to_owned();
    assert_eq!(total(&[]), "  33.3%  1/3  total");
    assert_eq!(total(&["--access", "public,protected"]), "   0.0%  0/2  total");
    fs::write(dir.path().join("docov.json"), r#"{ "access": ["public"] }"#).unwrap();
    assert_eq!(total(&[]), "   0.0%  0/1  total");
}

#[test]
fn override_members_count_as_documented_with_inherit_override() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.ts"), "/** A. */\nexport class A extends B {\n    override m() {}\n}\n").unwrap();
    let total = |args: &[&str]| docov(dir.path(), args).1.lines().last().unwrap().to_owned();
    assert_eq!(total(&[]), "  50.0%  1/2  total");
    fs::write(dir.path().join("docov.json"), r#"{ "inherit": "override" }"#).unwrap();
    assert_eq!(total(&[]), " 100.0%  2/2  total");
}

#[test]
fn the_config_picks_which_exports_count() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.js"), "export const a = 1;\nexports.b = 1;\nwindow.c = 1;\n").unwrap();
    let total = || docov(dir.path(), &[]).1.lines().last().unwrap().to_owned();
    assert_eq!(total(), "   0.0%  0/2  total");
    fs::write(dir.path().join("docov.json"), r#"{ "exports": { "esm": false, "globals": true } }"#).unwrap();
    assert_eq!(total(), "   0.0%  0/2  total");
    fs::write(dir.path().join("docov.json"), r#"{ "exports": { "cjs": false } }"#).unwrap();
    assert_eq!(total(), "   0.0%  0/1  total");
    fs::write(dir.path().join("docov.json"), r#"{ "exports": { "amd": true } }"#).unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown field `amd`"), "{err}");
}

#[test]
fn the_config_leaves_out_deprecated_items_and_generated_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.ts"), "/** @deprecated */\nexport const a = 1;\nexport const b = 1;\n").unwrap();
    fs::write(dir.path().join("b.ts"), "// @generated\nexport const c = 1;\n").unwrap();
    let total = || docov(dir.path(), &[]).1.lines().last().unwrap().to_owned();
    assert_eq!(total(), "  33.3%  1/3  total");
    fs::write(dir.path().join("docov.json"), r#"{ "ignoreDeprecated": true, "skipGenerated": true }"#).unwrap();
    assert_eq!(total(), "   0.0%  0/1  total");
}

#[test]
fn the_pre_commit_hook_installs_this_version() {
    let hooks = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".pre-commit-hooks.yaml")).unwrap();
    let package = format!("\"@docov/cli@{}\"", env!("CARGO_PKG_VERSION"));
    assert!(hooks.contains(&package), "point .pre-commit-hooks.yaml's additional_dependencies at {package}");
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

    fs::write(dir.path().join("docov.json"), r#"{ "overrides": [{ "files": ["src"], "kinds": ["function"] }] }"#)
        .unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown field `kinds`, expected one of `files`, `exclude`, `threshold`, `scope`"), "{err}");
}

#[test]
fn an_overrides_scope_is_for_its_files_and_the_flag_wins() {
    let dir = project();
    fs::write(dir.path().join("src/d.ts"), "const d = 1;\n").unwrap();
    fs::write(dir.path().join("docov.json"), r#"{ "overrides": [{ "files": ["src/d.ts"], "scope": "all" }] }"#)
        .unwrap();
    // `--explain` names the file as it's given.
    let d = "src/d.ts";
    let (code, out, _) = docov(dir.path(), &["--explain", d]);
    let wanted = format!("{d}  scope all: set by overrides[0], for src/d.ts\n{d}:1:1  variable d  undocumented\n");
    assert_eq!((code, out), (0, wanted));
    let (code, out, _) = docov(dir.path(), &["--explain", d, "--scope", "exports"]);
    let wanted = format!("{d}:1:1  variable d  left out: not exported by a way of exporting that counts\n");
    assert_eq!((code, out), (0, wanted));
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
fn json_items_say_what_their_docs_have_and_priority_sorts_the_text() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/**\n * Adds.\n * @param a The a.\n * @example add(1, 2)\n * @since 1.2.0\n * @deprecated\n */
export function add(a, b) {}
export const v = 1;
export class C {}
export function one(a) {}
export function two(a, b) {}
";
    fs::write(dir.path().join("a.js"), source).unwrap();
    let (_, out, _) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let add = &json["files"][0]["items"][0];
    let facts = |item: &serde_json::Value| -> [serde_json::Value; 5] {
        ["hasDescription", "hasExample", "paramsDocumented", "deprecated", "since"].map(|key| item[key].clone())
    };
    assert_eq!(facts(add), serde_json::json!([true, true, false, true, "1.2.0"]).as_array().unwrap().as_slice());
    let v = serde_json::json!([false, false, null, false, null]);
    assert_eq!(facts(&json["files"][0]["items"][1]), v.as_array().unwrap().as_slice());

    let listed = |args: &[&str]| -> Vec<String> {
        let (_, out, _) = docov(dir.path(), args);
        let items = out.lines().filter(|l| l.starts_with("         "));
        items.map(|l| l.trim_start().split_once("  ").unwrap().1.to_owned()).collect()
    };
    assert_eq!(listed(&[]), ["variable v", "class C", "function one", "function two"]);
    assert_eq!(listed(&["--sort", "priority"]), ["function two", "function one", "class C", "variable v"]);
}

#[test]
fn examples_are_counted_and_gated() {
    let dir = project();
    fs::write(dir.path().join("src/c.js"), "/**\n * C.\n * @example new C()\n */\nexport class C {}\n").unwrap();
    let (code, out, _) = docov(dir.path(), &["--examples"]);
    let a = Path::new("src").join("a.ts").display().to_string();
    let c = Path::new("src").join("c.js").display().to_string();
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!(
            "  50.0%  1/2  examples 0/2  {a}\n         {a}:3:1  variable b\n 100.0%  1/1  examples 1/1  {c}\n  66.7%  2/3  examples 1/3  total\n"
        )
    );
    let (_, out, _) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((&json["withExamples"], &json["files"][1]["examplesPercent"]), (&1.into(), &100.0.into()));

    let (code, _, err) = docov(dir.path(), &["--examples-threshold", "50"]);
    assert_eq!((code, err.as_str()), (1, "docov: 33.3% with examples, under the 50% examples threshold\n"));
    fs::write(dir.path().join("docov.json"), r#"{ "examplesThreshold": 30 }"#).unwrap();
    assert_eq!(docov(dir.path(), &[]).0, 0);
}

#[test]
fn file_docs_make_each_file_an_item() {
    let dir = project();
    fs::write(dir.path().join("src/c.js"), "/** @module c The c module. */\n\n/** c */\nexport class C {}\n").unwrap();
    fs::write(dir.path().join("docov.json"), r#"{ "fileDocs": "require" }"#).unwrap();
    let (code, out, _) = docov(dir.path(), &["--format", "json", "--kind", "module"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let file =
        |f: usize| (json["files"][f]["items"][0]["name"].clone(), json["files"][f]["items"][0]["documented"].clone());
    let a = Path::new("src").join("a.ts").display().to_string();
    assert_eq!((code, file(0), file(1)), (0, (a.into(), false.into()), ("c".into(), true.into())));
}

#[test]
fn virtual_types_count_typedefs_in_javascript() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/**\n * A point.\n * @typedef {Object} Point\n */\n/** @callback Done */\nexport const a = 1;\n";
    fs::write(dir.path().join("a.js"), source).unwrap();
    fs::write(dir.path().join("docov.json"), r#"{ "virtual": true }"#).unwrap();
    let (code, out, _) = docov(dir.path(), &[]);
    assert_eq!(
        (code, out.as_str()),
        (0, "  33.3%  1/3  a.js\n         a.js:5:1  type Done\n         a.js:6:1  variable a\n  33.3%  1/3  total\n")
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

#[test]
fn update_threshold_ratchets_the_config_in_place() {
    let dir = project();
    let (code, _, err) = docov(dir.path(), &["--update-threshold"]);
    assert_eq!((code, err.as_str()), (0, "docov: raised the threshold to 66.6% in docov.json\n"));
    assert_eq!(fs::read_to_string(dir.path().join("docov.json")).unwrap(), "{\n  \"threshold\": 66.6\n}\n");

    let config =
        "// ours\n{\n  \"threshold\": { \"src/**\": 10, \"global\": 50 }, // floor\n  \"scope\": \"exports\",\n}\n";
    fs::write(dir.path().join("docov.json"), config).unwrap();
    assert_eq!(docov(dir.path(), &["--update-threshold"]).0, 0);
    let raised = fs::read_to_string(dir.path().join("docov.json")).unwrap();
    assert_eq!(raised, config.replace("\"global\": 50", "\"global\": 66.6"));
    // Never down.
    fs::write(dir.path().join("src/b.ts"), "export const z = 1;\n").unwrap();
    let (code, _, err) = docov(dir.path(), &["--update-threshold"]);
    assert_eq!((code, err.as_str()), (1, "docov: 50.0% documented, under the 66.6% threshold\n"));
    assert_eq!(fs::read_to_string(dir.path().join("docov.json")).unwrap(), raised);
}

/// Runs git in `dir`, a throwaway test repository, with an identity and no signing.
fn git(dir: &Path, args: &[&str]) {
    let config =
        ["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"];
    let out = Command::new("git").args(config).args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn changed_and_staged_scan_what_git_says_changed() {
    let dir = project();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "base"]);
    git(dir.path(), &["checkout", "-q", "-b", "change"]);
    fs::write(dir.path().join("src/c.js"), "/** c */\nexport class C {}\nexport const d = 1;\n").unwrap();
    fs::write(dir.path().join("src/new.ts"), "export const n = 1;\n").unwrap();
    let json = |args: &[&str]| {
        let (code, out, err) = docov(dir.path(), &[&["--format", "json"], args].concat());
        assert_eq!((code, err.as_str()), (0, ""));
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap().replace('\\', "/"))
            .collect::<Vec<_>>()
    };
    assert_eq!(json(&["--changed"]), ["src/c.js", "src/new.ts"]);
    assert_eq!(json(&["--changed", "--since", "HEAD", "src/new.ts"]), ["src/new.ts"]);
    assert!(json(&["--staged"]).is_empty());
    git(dir.path(), &["add", "src/c.js"]);
    assert_eq!(json(&["--staged"]), ["src/c.js"]);

    let (code, _, err) = docov(dir.path(), &["--changed", "--since", "nope"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: git diff --name-only -z --merge-base nope: "), "{err}");
    // --since is for --changed, and --changed and --staged don't go together.
    assert_eq!(docov(dir.path(), &["--since", "main"]).0, 2);
    assert_eq!(docov(dir.path(), &["--changed", "--staged"]).0, 2);
}

#[test]
fn the_diff_threshold_counts_the_items_on_added_lines() {
    let dir = project();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "base"]);
    git(dir.path(), &["checkout", "-q", "-b", "change"]);
    // b, already undocumented, isn't on an added line; e and f are, one documented.
    fs::write(
        dir.path().join("src/a.ts"),
        "/** a */\nexport function a() {}\nexport const b = 1;\n/** e */\nexport const e = 1;\nexport const f = 1;\n",
    )
    .unwrap();
    git(dir.path(), &["commit", "-q", "-am", "e and f"]);
    fs::write(dir.path().join("src/g.ts"), "/** g */\nexport const g = 1;\n").unwrap();
    let (code, _, err) = docov(dir.path(), &["--diff-threshold", "70", "--threshold", "0"]);
    assert_eq!((code, err.as_str()), (1, "docov: changed lines: 66.7% documented, under the 70% diff threshold\n"));
    assert_eq!(docov(dir.path(), &["--diff-threshold", "66", "--diff-base", "main"]).0, 0);
    // From HEAD, only the untracked file is new.
    assert_eq!(docov(dir.path(), &["--diff-threshold", "100", "--diff-base", "HEAD"]).0, 0);
    // --diff-base is for --diff-threshold.
    assert_eq!(docov(dir.path(), &["--diff-base", "main"]).0, 2);
}

#[test]
fn each_path_threshold_failure_is_an_annotation_and_ignored_items_stay_out() {
    let dir = project();
    let a = Path::new("src").join("a.ts").display().to_string();
    fs::write(dir.path().join("docov.json"), r#"{ "threshold": { "perFile": 60, "src/": 70 } }"#).unwrap();
    let (code, out, _) = docov(dir.path(), &["--format", "github"]);
    let annotations = format!(
        "::warning file=src/a.ts,line=3,col=1,title=docov::variable b is undocumented\n\
        ::error title=docov::{a}: 50.0%25 documented, under the 60%25 per-file threshold\n\
        ::error title=docov::src/: 66.7%25 documented, under the 70%25 threshold\n"
    );
    assert_eq!((code, out), (1, annotations));
    // Ignoring b takes it out of both thresholds, as it is out of the report.
    fs::write(
        dir.path().join("src/a.ts"),
        "/** a */\nexport function a() {}\n// docov-ignore-next-line\nexport const b = 1;\n",
    )
    .unwrap();
    assert_eq!(docov(dir.path(), &["--format", "github"]), (0, String::new(), String::new()));
}

#[test]
fn a_config_extends_others_relative_to_itself() {
    let dir = project();
    fs::create_dir(dir.path().join("pkg")).unwrap();
    // `paths` and `exclude` stay relative to the base's folder, and the extending file's keys win.
    let base = r#"{ "paths": ["src"], "exclude": ["*.js"], "threshold": 100, "scope": "all" }"#;
    fs::write(dir.path().join("base.json"), base).unwrap();
    fs::write(dir.path().join("pkg/docov.json"), r#"{ "extends": "../base.json", "scope": "exports" }"#).unwrap();
    let (code, out, _) = docov(&dir.path().join("pkg"), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((code, &json["documented"], &json["total"]), (1, &1.into(), &2.into()));

    fs::write(dir.path().join("base.json"), r#"{ "extends": ["pkg/docov.json"] }"#).unwrap();
    let (code, _, err) = docov(&dir.path().join("pkg"), &[]);
    // docov puts the OS's separator between the parts of the files it extends.
    let up = Path::new("..");
    let chain = [Path::new("docov.json"), &up.join("base.json"), &up.join("pkg").join("docov.json")]
        .map(|p| p.display().to_string());
    assert_eq!((code, err), (2, format!("docov: config files extend each other in a cycle: {}\n", chain.join(" -> "))));
}

#[test]
fn print_config_and_list_files_show_what_a_scan_would_use() {
    let dir = project();
    fs::write(dir.path().join("docov.json"), r#"{ "paths": ["src"], "threshold": 80 }"#).unwrap();
    let (code, out, _) = docov(dir.path(), &["--print-config", "--scope", "all"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(code, 0);
    assert_eq!(json["files"], serde_json::json!(["docov.json"]));
    assert_eq!((&json["features"]["scope"], &json["features"]["threshold"]), (&"all".into(), &80.0.into()));

    let (code, out, _) = docov(dir.path(), &["--list-files"]);
    let files = [Path::new("src").join("a.ts"), Path::new("src").join("c.js")].map(|p| p.display().to_string() + "\n");
    assert_eq!((code, out), (0, files.concat()));
}

#[test]
fn init_writes_a_config_with_the_current_coverage() {
    let dir = project();
    let (code, _, err) = docov(dir.path(), &["init"]);
    assert_eq!((code, err.as_str()), (0, "docov: wrote docov.json, with a 66% threshold\n"));
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("docov.json")).unwrap()).unwrap();
    let schema =
        format!("https://raw.githubusercontent.com/cutenode/docov/v{}/docov.schema.json", env!("CARGO_PKG_VERSION"));
    assert_eq!(json, serde_json::json!({ "$schema": schema, "threshold": 66 }));
    assert_eq!(docov(dir.path(), &["init"]), (2, String::new(), "docov: docov.json is already there\n".into()));
    // Paths still scan: a folder named init as `./init`, and one named help as it is.
    fs::create_dir(dir.path().join("init")).unwrap();
    fs::create_dir(dir.path().join("help")).unwrap();
    for path in ["./init", "help"] {
        assert_eq!(docov(dir.path(), &[path]), (0, " 100.0%  0/0  total\n".into(), String::new()), "{path}");
    }
}

#[test]
fn every_member_counts_with_members_all() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/**\n * Colors.\n * @property Red The red one.\n */\nexport enum Color {\n  Red,\n  Green,\n}\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    fs::write(dir.path().join("docov.json"), r#"{ "members": "all" }"#).unwrap();
    let (code, out, _) = docov(dir.path(), &[]);
    assert_eq!(
        (code, out.as_str()),
        (0, "  66.7%  2/3  a.ts\n         a.ts:7:3  property Color.Green\n  66.7%  2/3  total\n")
    );
}

#[test]
fn commonjs_members_count_prototype_and_this_assignments() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/** A. */\nfunction A() {}\n/** Runs. */\nA.prototype.run = function () {};\nA.stop = function () {};\nmodule.exports = A;\nthis.b = 1;\n";
    fs::write(dir.path().join("a.cjs"), source).unwrap();
    assert_eq!(docov(dir.path(), &[]).1, " 100.0%  1/1  a.cjs\n 100.0%  1/1  total\n");
    fs::write(dir.path().join("docov.json"), r#"{ "commonjsMembers": true }"#).unwrap();
    let (code, out, _) = docov(dir.path(), &[]);
    assert_eq!(
        (code, out.as_str()),
        (
            0,
            "  50.0%  2/4  a.cjs\n         a.cjs:5:1  method A.stop\n         a.cjs:7:1  variable b\n  50.0%  2/4  total\n"
        )
    );
}

#[test]
fn release_tags_are_counted_over_top_level_exports() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/**\n * A.\n * @public\n */\nexport class A {\n  m() {}\n}\n/** B. */\nexport const b = 1;\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let (code, out, _) = docov(dir.path(), &["--release-tags"]);
    assert_eq!((code, out.lines().last()), (0, Some("  50.0%  1/2  release tags")));
    let (_, out, _) = docov(dir.path(), &["--format", "json"]);
    assert!(!out.contains("releaseTagged"));
    fs::write(dir.path().join("docov.json"), r#"{ "releaseTagsThreshold": 60 }"#).unwrap();
    let (code, out, err) = docov(dir.path(), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((code, &json["releaseTagged"], &json["releaseTagsPercent"]), (1, &1.into(), &50.0.into()));
    assert_eq!(err, "docov: 50.0% with release tags, under the 60% release tags threshold\n");
}

#[test]
fn types_are_counted_over_javascript_functions() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/**\n * @param {number} a\n * @returns {number}\n */\nexport function f(a) {}\n/** @param b */\nexport function g(b) {}\n";
    fs::write(dir.path().join("a.js"), source).unwrap();
    fs::write(dir.path().join("b.ts"), "/** @param b */\nexport function h(b: string) {}\n").unwrap();
    let (code, out, _) = docov(dir.path(), &["--types"]);
    assert_eq!((code, out.lines().last()), (0, Some("  50.0%  1/2  types")));
    let (code, out, err) = docov(dir.path(), &["--format", "json", "--types-threshold", "75"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((code, &json["typed"], &json["files"][1]["typesPercent"]), (1, &1.into(), &100.0.into()));
    assert_eq!(err, "docov: 50.0% with types, under the 75% types threshold\n");
}

#[test]
fn the_public_scope_follows_the_package_entry_points() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), r#"{ "exports": { ".": "./dist/index.js" } }"#).unwrap();
    fs::write(dir.path().join("tsconfig.json"), r#"{ "compilerOptions": { "outDir": "dist", "rootDir": "src" } }"#)
        .unwrap();
    fs::write(dir.path().join("src/index.ts"), "export { a } from './a.js';\nexport * from './b';\n").unwrap();
    fs::write(dir.path().join("src/a.ts"), "/** A. */\nexport function a() {}\nexport function hidden() {}\n").unwrap();
    fs::write(dir.path().join("src/b.ts"), "export const b = 1;\n").unwrap();
    let (code, out, _) = docov(dir.path(), &["--scope", "public"]);
    let (a, b) =
        (Path::new("src").join("a.ts").display().to_string(), Path::new("src").join("b.ts").display().to_string());
    assert_eq!(
        (code, out),
        (0, format!(" 100.0%  1/1  {a}\n   0.0%  0/1  {b}\n         {b}:1:1  variable b\n  50.0%  1/2  total\n"))
    );

    fs::write(dir.path().join("package.json"), r#"{ "main": "./lib/main.js" }"#).unwrap();
    let (code, out, err) = docov(dir.path(), &["--scope", "public"]);
    let package = Path::new("package.json").display();
    assert_eq!(
        (code, out.as_str(), err),
        (2, "", format!("docov: {package}: entry point ./lib/main.js has no source file\n"))
    );
}

#[test]
fn explain_says_why_for_each_item_in_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = "/** A. */\nexport function a() {}\nexport class C {\n  constructor() {}\n}\n/** @internal */\nexport const i = 1;\n";
    fs::write(dir.path().join("a.ts"), source).unwrap();
    let (code, out, _) = docov(dir.path(), &["--explain", "a.ts", "--exclude-tag", "internal"]);
    let lines = [
        "a.ts:2:1  function a  documented by the block at 1:1",
        "a.ts:3:1  class C  undocumented",
        "a.ts:4:3  skipped C.constructor: a constructor, which its class's doc covers",
        "a.ts:7:1  variable i  left out: its JSDoc has @internal",
    ];
    assert_eq!((code, out), (0, lines.map(|l| format!("{l}\n")).concat()));
}

#[test]
fn doc_checks_print_what_they_find_and_fail_on_errors() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.ts"), "/**\n * A.\n * @param b B.\n */\nexport function a(c) {}\n").unwrap();
    let (code, out, err) = docov(dir.path(), &[]);
    assert_eq!((code, err.as_str()), (0, ""));
    let (_, plain, _) = docov(dir.path(), &["--rule", "require-param=warn", "--rule", "param-names=warn"]);
    assert_eq!(plain, out);

    let found = "docov: a.ts:1:1: require-param: missing @param for `c`\ndocov: a.ts:3:11: param-names: @param `b` doesn't match a parameter\n";
    let (code, _, err) = docov(dir.path(), &["--rule", "require-param=warn", "--rule", "param-names=warn"]);
    assert_eq!((code, err.as_str()), (0, found));
    fs::write(dir.path().join("docov.json"), r#"{ "rules": { "param-names": "error", "require-param": "warn" } }"#)
        .unwrap();
    let (code, _, err) = docov(dir.path(), &[]);
    assert_eq!((code, err), (1, format!("{found}docov: the doc checks found 1 error\n")));
    let (code, _, err) = docov(dir.path(), &["--rule", "param-names=off", "--rule", "require-param=off"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let (code, _, err) = docov(dir.path(), &["--rule", "nope=warn"]);
    assert_eq!(code, 2, "{err}");
}
