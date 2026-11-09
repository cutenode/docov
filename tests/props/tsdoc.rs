//! Properties of the `tsdoc-*` checks: TSDoc's grammar as `@microsoft/tsdoc` reads it, the tags a
//! tsdoc.json defines, and which tsdoc.json each file is read with.

use std::{fs, path::Path, sync::Arc};

use docov::{
    Rules,
    tsdoc::{
        Configuration, SyntaxKind, TagDefinition,
        config::{self, Configs},
    },
};
use hegel::{TestCase, generators as gs};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{at_level, config_cli::docov, diagnose, level, write};

/// The `tsdoc-*` checks.
const RULES: [&str; 7] =
    ["tsdoc-tags", "tsdoc-params", "tsdoc-inline-tags", "tsdoc-escapes", "tsdoc-code", "tsdoc-html", "tsdoc-links"];

/// The check that reports TSDoc's message `id`.
fn rule_of(id: &str) -> &'static str {
    let tags = [
        "tsdoc-undefined-tag",
        "tsdoc-unsupported-tag",
        "tsdoc-tag-should-not-have-braces",
        "tsdoc-inline-tag-missing-braces",
        "tsdoc-missing-deprecation-message",
        "tsdoc-extra-inheritdoc-tag",
        "tsdoc-inheritdoc-incompatible-tag",
        "tsdoc-inheritdoc-incompatible-summary",
    ];
    let escapes = [
        "tsdoc-escape-right-brace",
        "tsdoc-escape-greater-than",
        "tsdoc-unnecessary-backslash",
        "tsdoc-at-sign-in-word",
        "tsdoc-at-sign-without-tag-name",
        "tsdoc-malformed-inline-tag",
    ];
    let inline = [
        "tsdoc-malformed-tag-name",
        "tsdoc-characters-after-block-tag",
        "tsdoc-characters-after-inline-tag",
        "tsdoc-inline-tag-missing-right-brace",
        "tsdoc-inline-tag-unescaped-brace",
        "tsdoc-inheritdoc-tag-syntax",
    ];
    let html = [
        "tsdoc-html-tag-missing-greater-than",
        "tsdoc-html-tag-missing-equals",
        "tsdoc-html-tag-missing-string",
        "tsdoc-html-string-missing-quote",
        "tsdoc-text-after-html-string",
        "tsdoc-malformed-html-name",
        "tsdoc-unsupported-html-name",
    ];
    match id {
        _ if tags.contains(&id) => "tsdoc-tags",
        _ if escapes.contains(&id) => "tsdoc-escapes",
        _ if inline.contains(&id) => "tsdoc-inline-tags",
        _ if html.contains(&id) => "tsdoc-html",
        _ if id.starts_with("tsdoc-param-") => "tsdoc-params",
        _ if id.starts_with("tsdoc-code-") => "tsdoc-code",
        _ => "tsdoc-links",
    }
}

/// An entry of tests/fixtures/tsdoc/corpus.json: a comment, the tsdoc.json keys it's read with,
/// and what tools/tsdoc-oracle.sh recorded `@microsoft/tsdoc` finding in it.
#[derive(Deserialize)]
struct Entry {
    comment: String,
    #[serde(default)]
    config: Option<Map<String, Value>>,
    #[serde(default)]
    messages: Vec<Expected>,
    #[serde(default)]
    throws: Option<String>,
}

#[derive(Deserialize)]
struct Expected {
    id: String,
    text: String,
    at: usize,
}

/// Writes a tsdoc.json with `keys` and its `$schema` in `dir`, and gives its path.
fn tsdoc_json(dir: &Path, keys: &Map<String, Value>) -> std::path::PathBuf {
    let mut file = keys.clone();
    file.insert("$schema".into(), json!(config::SCHEMA));
    let path = dir.join(config::FILE_NAME);
    fs::write(&path, Value::Object(file).to_string()).unwrap();
    path
}

/// The 1-based line and column, in characters, of byte `at` in `source`.
fn position(source: &str, at: usize) -> (usize, usize) {
    let before = &source[..at];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (before.matches('\n').count() + 1, before[line_start..].chars().count() + 1)
}

/// The `tsdoc-*` checks report, for each comment `@microsoft/tsdoc`'s own tests and docov's
/// corpus have, what that parser reports, by its message ids and texts and where, each under the
/// check for its part of the grammar, wherever the comment is in the file, and with the tsdoc.json
/// the entry has. A comment with nothing in it documents nothing, so it isn't checked. Where
/// `@microsoft/tsdoc` throws (a selector cut off by the end of a tag, `{@link (Foo:}`), docov stops
/// at the message it reports before that.
#[hegel::test(test_cases = 5)]
fn tsdoc_checks_report_what_microsoft_tsdoc_does(tc: TestCase) {
    let levels: Vec<(&str, (Value, Option<bool>))> = RULES.iter().map(|rule| (*rule, level(&tc))).collect();
    let on: Vec<(&str, Value)> = levels.iter().map(|(rule, (value, _))| (*rule, value.clone())).collect();
    let before = "export const before = 1;\n".repeat(tc.draw(gs::integers::<usize>().max_value(2)));
    let entries: Vec<Entry> = serde_json::from_str(include_str!("../fixtures/tsdoc/corpus.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for entry in &entries {
        let source = format!("{before}{}\nexport const v = 1;\n", entry.comment);
        let tsdoc = entry.config.as_ref().map(|keys| Arc::new(Configs::file(&tsdoc_json(dir.path(), keys)).unwrap()));
        let rules = Rules { tsdoc, ..Rules::default() };
        let empty = entry.comment[3..entry.comment.len() - 2].chars().all(|c| c.is_whitespace() || c == '*');
        let label = || Expected {
            id: "tsdoc-reference-missing-label".into(),
            text: "Expecting a selector label after the colon".into(),
            at: entry.comment.find(':').unwrap(),
        };
        let messages = if entry.throws.is_some() { &vec![label()] } else { &entry.messages };
        let mut wanted = Vec::new();
        for message in messages.iter().filter(|_| !empty) {
            let rule = rule_of(&message.id);
            let error = levels.iter().find(|(id, _)| *id == rule).unwrap().1.1;
            let (line, column) = position(&source, before.len() + message.at);
            let finding = (line, column, rule, format!("{}: {}", message.id, message.text));
            wanted.extend(at_level(vec![finding], error));
        }
        wanted.sort();
        let mut found = diagnose(&source, &on, rules);
        found.retain(|(.., message, _)| !message.starts_with("docov-"));
        assert_eq!(found, wanted, "{}", entry.comment);
    }
}

/// A tag for [`tags_are_used_once_unless_they_allow_more`]: how it's written, whether a comment
/// may have more than one, and whether it's a modifier tag.
const TAGS: [(&str, bool, bool); 8] = [
    ("@remarks text", false, false),
    ("@example text", true, false),
    ("@see text", false, false),
    ("@beta", false, true),
    ("@public", false, true),
    ("@virtual", false, true),
    ("see {@label L}", false, false),
    ("see {@link Foo}", true, false),
];

/// `tsdoc-tags` reports each use of a tag after its first, in any case, unless its definition
/// has `allowMultiple` (`docov-repeated-tag`), and a modifier tag with text after it on its line
/// (`docov-modifier-tag-text`), but not one followed by another tag.
#[hegel::test(test_cases = 50)]
fn tags_are_used_once_unless_they_allow_more(tc: TestCase) {
    let (value, error) = level(&tc);
    let mut source = "/**\n * Summary.\n".to_owned();
    let (mut seen, mut wanted) = (Vec::new(), Vec::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let mut line = " * ".to_owned();
        let pair = tc.draw(gs::booleans());
        let picked: Vec<usize> =
            if pair { vec![4, 3] } else { vec![tc.draw(gs::integers::<usize>().max_value(TAGS.len() - 1))] };
        let trailing = tc.draw(gs::booleans());
        let upper = tc.draw(gs::booleans());
        let line_number = source.matches('\n').count() + 1;
        for (i, &tag) in picked.iter().enumerate() {
            let (written, multiple, modifier) = TAGS[tag];
            let at = written.find('@').unwrap();
            let name_end = written[at..].find([' ', '}']).map_or(written.len(), |end| at + end);
            let name = &written[at..name_end];
            let name = if upper { name.to_uppercase() } else { name.to_owned() };
            if i > 0 {
                line.push(' ');
            }
            let column = line.chars().count() + at + 1;
            line += &written.replacen(&written[at..name_end], &name, 1);
            let upper_name = name.to_uppercase();
            if !multiple && seen.contains(&upper_name) {
                let text =
                    format!("docov-repeated-tag: The TSDoc tag \"{name}\" may only appear once in a doc comment");
                wanted.push((line_number, column, "tsdoc-tags", text));
            }
            seen.push(upper_name);
            if modifier && trailing && i == picked.len() - 1 {
                let text = format!(
                    "docov-modifier-tag-text: The TSDoc tag \"{name}\" is a modifier tag; text after it on its line belongs to the section before it"
                );
                wanted.push((line_number, column, "tsdoc-tags", text));
            }
        }
        if trailing {
            line += " more";
        }
        source += &line;
        source.push('\n');
    }
    source += " */\nexport const v = 1;\n";
    assert_eq!(diagnose(&source, &[("tsdoc-tags", value)], Rules::default()), at_level(wanted, error), "{source}");
}

/// A tag definition as a tsdoc.json writes it, and as docov reads it.
fn definition(tc: &TestCase, name: &str) -> (Value, TagDefinition) {
    let (kind, word) = tc.draw(gs::sampled_from(vec![
        (SyntaxKind::Block, "block"),
        (SyntaxKind::Inline, "inline"),
        (SyntaxKind::Modifier, "modifier"),
    ]));
    let allow_multiple = tc.draw(gs::booleans());
    let json = json!({ "tagName": name, "syntaxKind": word, "allowMultiple": allow_multiple });
    (json, TagDefinition { name: name.into(), kind, allow_multiple })
}

/// A tsdoc.json sets up TSDoc's parser the way `@microsoft/tsdoc-config` does: the files it
/// extends (a path, or a file in a package) first, then its own tag definitions,
/// `supportForTags` and HTML elements over theirs, and its `noStandardTags` or else theirs. Comments
/// are fine in it. The files it read come back too.
#[hegel::test(test_cases = 50)]
fn tsdoc_json_sets_up_the_parser(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut base = Map::new();
    let mut top = Map::new();
    let base_standard = tc.draw(gs::optional(gs::booleans()));
    let top_standard = tc.draw(gs::optional(gs::booleans()));
    let standard = !top_standard.or(base_standard).unwrap_or(false);
    let mut wanted = Configuration::standard(standard);
    let (base_file, specifier) = if tc.draw(gs::booleans()) {
        ("base/tsdoc.json", "./base/tsdoc.json")
    } else {
        ("node_modules/shared/tsdoc.json", "shared/tsdoc.json")
    };
    let extends = tc.draw(gs::booleans());
    if extends {
        if let Some(no) = base_standard {
            base.insert("noStandardTags".into(), json!(no));
        }
        let (json, custom) = definition(&tc, "@custom");
        base.insert("tagDefinitions".into(), json!([json]));
        wanted.tags.push(custom);
        if tc.draw(gs::booleans()) {
            base.insert("supportedHtmlElements".into(), json!(["b", "my-el"]));
            wanted.html = vec!["b".into(), "my-el".into()];
            wanted.report_unsupported_html = true;
        }
        top.insert("extends".into(), json!([specifier]));
    }
    if let Some(no) = top_standard {
        top.insert("noStandardTags".into(), json!(no));
    }
    let (json, mine) = definition(&tc, "@mine");
    top.insert("tagDefinitions".into(), json!([json]));
    wanted.tags.push(mine);
    let mut support = Map::new();
    let mut candidates = vec!["@MINE"];
    if extends {
        candidates.push("@custom");
    }
    if standard {
        candidates.push("@remarks");
    }
    for name in tc.draw(gs::vecs(gs::sampled_from(candidates)).max_size(4)) {
        let supported = tc.draw(gs::booleans());
        support.insert(name.into(), json!(supported));
    }
    if !support.is_empty() {
        let mut supported = Vec::new();
        for (name, value) in &support {
            let upper = name.to_uppercase();
            supported.retain(|tag| *tag != upper);
            if value == &json!(true) {
                supported.push(upper);
            }
        }
        wanted.supported = Some(supported);
        top.insert("supportForTags".into(), Value::Object(support));
    }
    if tc.draw(gs::booleans()) {
        top.insert("supportedHtmlElements".into(), json!(["i"]));
        wanted.html = vec!["i".into()];
        wanted.report_unsupported_html = true;
    }
    if let Some(report) = tc.draw(gs::optional(gs::booleans())) {
        top.insert("reportUnsupportedHtmlElements".into(), json!(report));
        wanted.report_unsupported_html = report;
    }
    base.insert("$schema".into(), json!(config::SCHEMA));
    write(dir.path(), base_file, &format!("// shared\n{}", Value::Object(base)));
    let path = tsdoc_json(dir.path(), &top);
    let (configuration, sources) = config::load(&path).unwrap();
    assert_eq!(configuration, wanted);
    let mut read = vec![path];
    if extends {
        read.push(fs::canonicalize(dir.path().join(base_file)).unwrap());
    }
    assert_eq!(sources, read);
}

/// A tsdoc.json that can't be used fails with TSDoc's message id for why, naming the file: it
/// isn't there or isn't a file, isn't JSON (a stray `/` included), has another `$schema`, a key
/// tsdoc-config doesn't know, an HTML element name that isn't one, a tag name that isn't one, a
/// tag defined twice (in the file, over a file it extends, or over a standard tag), a supported tag
/// nobody defines, or extends a file that isn't there, one that can't be used, or itself.
#[hegel::test(test_cases = 3)]
fn tsdoc_json_errors_say_why(tc: TestCase) {
    let schema = json!(config::SCHEMA);
    let tag = |name: &str| json!({ "tagName": name, "syntaxKind": "block" });
    let cases: Vec<(Vec<(&str, String)>, &str)> = vec![
        (vec![], "tsdoc-config-file-not-found"),
        (vec![("tsdoc.json/x", String::new())], "tsdoc-config-file-not-found"),
        (vec![("tsdoc.json", "{ / }".into())], "tsdoc-config-invalid-json"),
        (vec![("tsdoc.json", "{".into())], "tsdoc-config-invalid-json"),
        (vec![("tsdoc.json", "{}".into())], "tsdoc-config-unsupported-schema"),
        (vec![("tsdoc.json", json!({ "$schema": schema, "x": 1 }).to_string())], "tsdoc-config-schema-error"),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "supportedHtmlElements": ["b c"] }).to_string())],
            "tsdoc-config-schema-error",
        ),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "supportedHtmlElements": [""] }).to_string())],
            "tsdoc-config-schema-error",
        ),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "tagDefinitions": [tag("custom")] }).to_string())],
            "tsdoc-config-invalid-tag-name",
        ),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "tagDefinitions": [tag("@a"), tag("@A")] }).to_string())],
            "tsdoc-config-duplicate-tag-name",
        ),
        (
            vec![
                ("base.json", json!({ "$schema": schema, "tagDefinitions": [tag("@a")] }).to_string()),
                (
                    "tsdoc.json",
                    json!({ "$schema": schema, "extends": ["./base.json"], "tagDefinitions": [tag("@a")] }).to_string(),
                ),
            ],
            "tsdoc-config-duplicate-tag-name",
        ),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "tagDefinitions": [tag("@remarks")] }).to_string())],
            "tsdoc-config-duplicate-tag-name",
        ),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "supportForTags": { "@nope": true } }).to_string())],
            "tsdoc-config-undefined-tag",
        ),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "extends": ["./missing.json"] }).to_string())],
            "tsdoc-config-unresolved-extends",
        ),
        (
            vec![
                ("base.json", "{}".into()),
                ("tsdoc.json", json!({ "$schema": schema, "extends": ["./base.json"] }).to_string()),
            ],
            "tsdoc-config-unsupported-schema",
        ),
        (
            vec![
                ("base.json", json!({ "$schema": schema, "supportForTags": { "@nope": true } }).to_string()),
                ("tsdoc.json", json!({ "$schema": schema, "extends": ["./base.json"] }).to_string()),
            ],
            "tsdoc-config-undefined-tag",
        ),
        (
            vec![
                ("base.json", json!({ "$schema": schema, "extends": ["./tsdoc.json"] }).to_string()),
                ("tsdoc.json", json!({ "$schema": schema, "extends": ["./base.json"] }).to_string()),
            ],
            "tsdoc-config-cyclic-extends",
        ),
    ];
    let folder = tc.draw(gs::sampled_from(vec!["", "a/", "a/b/"]));
    for (files, id) in cases {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in &files {
            write(dir.path(), &format!("{folder}{name}"), text);
        }
        let path = dir.path().join(format!("{folder}tsdoc.json"));
        let error = config::load(&path).unwrap_err();
        assert!(error.contains(id) && error.contains("json: "), "{error}");
        assert_eq!(Configs::file(&path).unwrap_err(), error);
        // Found for a file beside a package.json, it fails the same, once it's there to find.
        write(dir.path(), &format!("{folder}package.json"), "{}");
        let file = dir.path().join(format!("{folder}a.ts"));
        if path.is_file() {
            assert!(Configs::find([file.as_path()]).unwrap_err().contains(id));
        }
    }
}

/// With `tsdoc-tags` on, each file's blocks are read with the tsdoc.json `--tsdoc-config` names,
/// else the one `tsdocConfig` names (relative to the config file that has the key, an extended one
/// included), else with `"syntax": "tsdoc"` the one in the nearest folder with a tsconfig.json or
/// a package.json, if that folder has one (so a package with none, or a tsconfig.json below the
/// root's, gets TSDoc's standard tags), and otherwise with TSDoc's standard tags. One that can't be
/// used fails the scan (exit 2), but only when it's read.
#[hegel::test(test_cases = 30)]
fn the_cli_reads_each_files_tsdoc_json(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let custom =
        json!({ "$schema": config::SCHEMA, "tagDefinitions": [{ "tagName": "@custom", "syntaxKind": "modifier" }] });
    write(root, "package.json", "{}");
    write(root, "a/package.json", "{}");
    write(root, "a/tsdoc.json", &custom.to_string());
    write(root, "b/tsconfig.json", "{}");
    write(root, "cfg/tsdoc.json", &custom.to_string());
    write(root, "tsdoc.json", &custom.to_string());
    write(root, "d/package.json", "{}");
    let block = "/**\n * Summary.\n * @custom\n */\nexport const v = 1;\n";
    let files = ["a/x.ts", "a/x2.ts", "a/sub/w.ts", "b/inner/y.ts", "c/z.ts", "d/v.ts"];
    for file in files {
        write(root, file, block);
    }
    let on = tc.draw(gs::weighted_booleans(0.8));
    // Where the tsdoc.json comes from.
    let from = tc.draw(gs::sampled_from(vec!["flag", "key", "nearest", "standard"]));
    let syntax = match from {
        "nearest" => "tsdoc",
        "standard" => "jsdoc",
        _ => tc.draw(gs::sampled_from(vec!["jsdoc", "tsdoc"])),
    };
    let (key, flag, lookup) = (from == "key", from == "flag", from == "nearest");
    let mut config = json!({ "rules": { "tsdoc-tags": if on { "error" } else { "off" } }, "syntax": syntax });
    if key {
        write(root, "cfg/shared.json", r#"{ "tsdocConfig": "tsdoc.json" }"#);
        config["extends"] = json!("cfg/shared.json");
    }
    write(root, "docov.json", &config.to_string());
    let mut args = vec!["--threshold", "0"];
    if flag {
        args.extend(["--tsdoc-config", "cfg/tsdoc.json"]);
    }
    let (code, _, stderr) = docov(root, &args, false);
    let defined = |file: &str| match () {
        () if flag || key => true,
        () if lookup => file.starts_with("a/") || file.starts_with("c/"),
        () => false,
    };
    let undefined: Vec<&str> = files.iter().copied().filter(|file| on && !defined(file)).collect();
    for file in files {
        let finding = format!("{file}:3:4: tsdoc-tags: tsdoc-undefined-tag");
        assert_eq!(stderr.contains(&finding), undefined.contains(&file), "{file}\n{stderr}");
    }
    assert_eq!(code, i32::from(!undefined.is_empty()), "{stderr}");
    // Run inside a folder with neither file, it looks above where it runs.
    if lookup {
        let (code, _, stderr) = docov(&root.join("c"), &[], false);
        assert_eq!((code, stderr.as_str()), (0, ""));
    }
    // One that can't be used, when it's read, fails the scan.
    write(root, "a/tsdoc.json", "{");
    write(root, "docov.json", r#"{ "rules": { "tsdoc-tags": "warn" }, "syntax": "tsdoc" }"#);
    let (code, _, stderr) = docov(root, &[], false);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("tsdoc.json: tsdoc-config-invalid-json"), "{stderr}");
}

/// With no tsconfig.json or package.json in a folder or above it, there's no tsdoc.json to read,
/// and docov says it looked for both in each folder.
#[hegel::test(test_cases = 3)]
fn no_package_no_tsdoc_json(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(tc.draw(gs::sampled_from(vec!["", "a", "a/b"])));
    fs::create_dir_all(&folder).unwrap();
    let (found, looked) = config::find(&folder);
    assert_eq!(found, None);
    assert_eq!(looked.len(), 2 * folder.ancestors().count());
}
