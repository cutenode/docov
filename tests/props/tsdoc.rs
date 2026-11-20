//! Properties of the `tsdoc-*` checks: TSDoc's grammar as `@microsoft/tsdoc` reads it, the tags a
//! tsdoc.json defines, and which tsdoc.json each file is read with.

use std::{fs, path::Path, sync::Arc};

use docov::{
    Rules,
    tsdoc::{
        config::{self, Configs},
        jsdoc_types,
    },
};
use hegel::{TestCase, generators as gs};
use oxc_jsdoc::JSDoc;
use oxc_span::Span;
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
/// and what `@microsoft/tsdoc` 0.17.1 finds in it, as recorded before the parser moved to tsdocrs, whose
/// own corpus and oracle (tools/oracle/) take it from there.
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

/// A tsdoc.json that can't be used fails the scan with tsdocrs's error, which names the file and
/// TSDoc's message id for why (tsdocrs's own properties go through every why), whether a config
/// names it or it's found for a file beside a package.json.
#[hegel::test(test_cases = 3)]
fn tsdoc_json_errors_say_why(tc: TestCase) {
    let schema = json!(config::SCHEMA);
    let cases: Vec<(Vec<(&str, String)>, &str)> = vec![
        (vec![], "tsdoc-config-file-not-found"),
        (vec![("tsdoc.json", "{".into())], "tsdoc-config-invalid-json"),
        (
            vec![("tsdoc.json", json!({ "$schema": schema, "extends": ["./missing.json"] }).to_string())],
            "tsdoc-config-unresolved-extends",
        ),
    ];
    let folder = tc.draw(gs::sampled_from(vec!["", "a/", "a/b/"]));
    for (files, id) in cases {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in &files {
            write(dir.path(), &format!("{folder}{name}"), text);
        }
        let path = dir.path().join(format!("{folder}tsdoc.json"));
        let error = Configs::file(&path).unwrap_err();
        assert!(error.contains(id) && error.contains("json: "), "{error}");
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
        // A path as docov prints it, with the OS's separator.
        let path = file.replace('/', std::path::MAIN_SEPARATOR_STR);
        let finding = format!("{path}:3:4: tsdoc-tags: tsdoc-undefined-tag");
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

/// The checks `tsdocJsdocTypes` is for.
const JSDOC_RULES: [&str; 3] = ["tsdoc-escapes", "tsdoc-html", "tsdoc-params"];

/// JSDoc `{type}`s for [`jsdoc_types_read_as_if_blank`].
const JSDOC_TYPES: [&str; 6] =
    ["{string}", "{Array<string>}", "{{ a?: string }}", "{import('x').Y}", "{Map<string, number>}", "{T}"];

/// `@param` names for [`jsdoc_types_read_as_if_blank`], each with the same-width name TSDoc should
/// read it as: an optional name with its brackets and default blanked, and a dotted one with `_`
/// for each dot.
const JSDOC_NAMES: [(&str, &str); 6] = [
    ("name", "name"),
    ("[name]", " name "),
    ("[name='a b']", " name       "),
    ("[options={}]", " options    "),
    ("options.flag", "options_flag"),
    ("[options.a.b=1]", " options_a_b   "),
];

/// The tags besides `@param` that [`jsdoc_types_read_as_if_blank`] writes a JSDoc `{type}` after.
const TYPED_TAGS: [&str; 12] = [
    "@returns",
    "@return",
    "@throws",
    "@type",
    "@typedef",
    "@template",
    "@property",
    "@yields",
    "@satisfies",
    "@this",
    "@enum",
    "@arg",
];

/// Lines with TSDoc's own mistakes where JSDoc's syntax could be: an inline tag where a type
/// would go, and a `{` after a tag that takes no type.
const MISTAKEN: [&str; 2] = [" * @returns {@link Foo..bar} the thing\n", " * @remarks {x} the thing\n"];

/// What follows each block in a generated file: a function for it to document.
const DOCUMENTED: &str = "\nexport function f(name: string, options: object) {}\n";

/// A `@param` line with type `ty`, if any, and `name`, and ` - ` before its description if
/// `hyphen`.
fn param_line(ty: Option<&str>, name: &str, hyphen: bool) -> String {
    let ty = ty.map_or(String::new(), |ty| format!("{ty} "));
    let description = if hyphen { " - The value." } else { " The value." };
    format!(" * @param {ty}{name}{description}\n")
}

/// A `tag` line with type `ty`, if any.
fn typed_line(tag: &str, ty: Option<&str>) -> String {
    format!(" * {tag} {}Folder the thing\n", ty.unwrap_or_default())
}

/// A block with a summary and then `lines`.
fn summarized(lines: &str) -> String {
    format!("/**\n * Summary.\n *\n{lines} */")
}

/// With `tsdocJsdocTypes` on, the `tsdoc-*` checks read a block the way TSDoc reads it with each
/// JSDoc `{type}` after a tag that takes one blanked out, and each `@param` name written as a plain
/// identifier of the same width. So an inline tag where a type would go, and a `{` after a tag
/// that takes no type, are still reported, and `@param` still wants ` - ` before its description.
/// Without it, each JSDoc type and optional or dotted name is reported.
#[hegel::test(test_cases = 100)]
fn jsdoc_types_read_as_if_blank(tc: TestCase) {
    let on: Vec<(&str, Value)> = RULES.iter().map(|rule| (*rule, json!("error"))).collect();
    let (mut jsdoc, mut plain) = (String::new(), String::new());
    // The lines with JSDoc's syntax on them, the `@param` lines without ` - ` and with it, and the
    // lines with TSDoc's own mistakes, each after the block's first three.
    let (mut typed, mut hyphenless, mut hyphened, mut mistaken) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let line = jsdoc.matches('\n').count() + 4;
        let ty = tc.draw(gs::optional(gs::sampled_from(JSDOC_TYPES.to_vec())));
        let blank = ty.map(|ty| " ".repeat(ty.len()));
        let (written, read_as) = match tc.draw(gs::sampled_from(vec!["param", "typed", "mistaken"])) {
            "param" => {
                let (name, plain_name) = tc.draw(gs::sampled_from(JSDOC_NAMES.to_vec()));
                let hyphen = tc.draw(gs::booleans());
                if ty.is_some() || name != plain_name {
                    typed.push(line);
                }
                (if hyphen { &mut hyphened } else { &mut hyphenless }).push(line);
                (param_line(ty, name, hyphen), param_line(blank.as_deref(), plain_name, hyphen))
            }
            "typed" => {
                let tag = tc.draw(gs::sampled_from(TYPED_TAGS.to_vec()));
                typed.extend(ty.map(|_| line));
                (typed_line(tag, ty), typed_line(tag, blank.as_deref()))
            }
            _ => {
                mistaken.push(line);
                let text = tc.draw(gs::sampled_from(MISTAKEN.to_vec())).to_owned();
                (text.clone(), text)
            }
        };
        jsdoc += &written;
        plain += &read_as;
    }
    let (jsdoc, plain) = (summarized(&jsdoc) + DOCUMENTED, summarized(&plain) + DOCUMENTED);
    let read = diagnose(&jsdoc, &on, Rules { tsdoc_jsdoc_types: true, ..Rules::default() });
    assert_eq!(read, diagnose(&plain, &on, Rules::default()), "{jsdoc}");
    let on_line = |found: &[(usize, usize, &str, String, bool)], line: usize| -> Vec<String> {
        found.iter().filter(|finding| finding.0 == line).map(|finding| finding.3.clone()).collect()
    };
    for &line in &hyphened {
        assert_eq!(on_line(&read, line), Vec::<String>::new(), "{line}: {jsdoc}");
    }
    for &line in &hyphenless {
        let messages = on_line(&read, line);
        assert!(messages.iter().any(|m| m.starts_with("tsdoc-param-tag-missing-hyphen: ")), "{line}: {jsdoc}");
    }
    for line in mistaken {
        assert!(!on_line(&read, line).is_empty(), "{line}: {jsdoc}");
    }
    let without = diagnose(&jsdoc, &on, Rules::default());
    for line in typed {
        assert!(!on_line(&without, line).is_empty(), "{line}: {jsdoc}");
    }
}

/// An entry of tests/fixtures/tsdoc/jsdoc-types-corpus.json: a block, the tsdoc.json keys it's
/// read with, the text TSDoc reads with `tsdocJsdocTypes` on, for a generated block the same block
/// written without JSDoc's syntax, and what the `tsdoc-*` checks find in it with the setting off and
/// on, as TSDoc's message ids by line and column.
#[derive(serde::Serialize)]
struct OracleEntry<'a> {
    comment: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<&'a Map<String, Value>>,
    blanked: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    plain: Option<&'a str>,
    off: Vec<(usize, usize, String)>,
    on: Vec<(usize, usize, String)>,
}

/// A block for tests/fixtures/tsdoc/jsdoc-types-corpus.json, the tsdoc.json keys it's read with, and
/// for a generated one, the block written without JSDoc's syntax.
type OracleBlock = (String, Option<Map<String, Value>>, Option<String>);

/// tests/fixtures/tsdoc/jsdoc-types-corpus.json, which tests/tsdoc-oracle.test.mjs checks against
/// `@microsoft/tsdoc`, is current. It has each block in the tsdoc fixtures, each comment in
/// corpus.json but the one TSDoc's parser throws on, and a block for each line
/// [`jsdoc_types_read_as_if_blank`] draws, with how it reads written without JSDoc's syntax.
/// `DOCOV_BLESS=1` writes it anew.
#[test]
fn jsdoc_types_corpus_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut blocks: Vec<OracleBlock> = Vec::new();
    let mut fixtures =
        vec![root.join("tests/fixtures/tsdoc/parser.ts"), root.join("tests/fixtures/tsdoc/jsdoc-types.js")];
    for rule in RULES {
        fixtures.extend(["fail.ts", "pass.ts"].map(|name| root.join("tests/fixtures/checks").join(rule).join(name)));
    }
    for file in fixtures {
        let text = fs::read_to_string(file).unwrap();
        let mut rest = text.as_str();
        while let Some(start) = rest.find("/**") {
            let end = start + rest[start..].find("*/").unwrap() + 2;
            blocks.push((rest[start..end].to_owned(), None, None));
            rest = &rest[end..];
        }
    }
    let entries: Vec<Entry> = serde_json::from_str(include_str!("../fixtures/tsdoc/corpus.json")).unwrap();
    let kept = entries.into_iter().filter(|entry| entry.throws.is_none());
    blocks.extend(kept.map(|entry| (entry.comment, entry.config, None)));
    let generated = |written: String, plain: String| (summarized(&written), None, Some(summarized(&plain)));
    for ty in std::iter::once(None).chain(JSDOC_TYPES.map(Some)) {
        let blank = ty.map(|ty| " ".repeat(ty.len()));
        for (name, plain_name) in JSDOC_NAMES {
            blocks.extend([false, true].map(|hyphen| {
                generated(param_line(ty, name, hyphen), param_line(blank.as_deref(), plain_name, hyphen))
            }));
        }
        blocks.extend(TYPED_TAGS.map(|tag| generated(typed_line(tag, ty), typed_line(tag, blank.as_deref()))));
    }
    blocks.extend(MISTAKEN.map(|line| generated(line.to_owned(), line.to_owned())));
    let dir = tempfile::tempdir().unwrap();
    let on: Vec<(&str, Value)> = RULES.iter().map(|rule| (*rule, json!("error"))).collect();
    let mut lines = Vec::new();
    for (comment, config, plain) in &blocks {
        // A block with nothing in it documents nothing, so it isn't checked.
        if comment[3..comment.len() - 2].chars().all(|c| c.is_whitespace() || c == '*') {
            continue;
        }
        let tsdoc = config.as_ref().map(|keys| Arc::new(Configs::file(&tsdoc_json(dir.path(), keys)).unwrap()));
        let source = comment.clone() + DOCUMENTED;
        let found = |tsdoc_jsdoc_types| {
            let rules = Rules { tsdoc: tsdoc.clone(), tsdoc_jsdoc_types, ..Rules::default() };
            diagnose(&source, &on, rules)
                .into_iter()
                .filter(|(.., message, _)| !message.starts_with("docov-"))
                .map(|(line, column, _, message, _)| (line, column, message.split(": ").next().unwrap().to_owned()))
                .collect()
        };
        // A block's span runs from past its `/**` to before its `*/`.
        let end = comment.len() - 2;
        let blanked = jsdoc_types::blank(comment, 0, &JSDoc::new(&comment[3..end], Span::new(3, end as u32)));
        let (config, plain) = (config.as_ref(), plain.as_deref());
        let entry = OracleEntry { comment, config, blanked, plain, off: found(false), on: found(true) };
        lines.push(serde_json::to_string(&entry).unwrap());
    }
    let text = format!("[\n{}\n]\n", lines.join(",\n"));
    let path = root.join("tests/fixtures/tsdoc/jsdoc-types-corpus.json");
    if std::env::var_os("DOCOV_BLESS").is_some() {
        fs::write(&path, &text).unwrap();
    }
    let checked_in = fs::read_to_string(&path).unwrap_or_default();
    assert!(checked_in == text, "{} is stale: run `DOCOV_BLESS=1 cargo nextest run --test props`", path.display());
}

/// With `"tsdocJsdocTypes": true` in docov.json, `tsdoc-escapes`, `tsdoc-html` and `tsdoc-params`
/// pass a JavaScript file typed in JSDoc (tests/fixtures/tsdoc/jsdoc-types.js) that each of them
/// fails without it, `false` or unset.
#[hegel::test(test_cases = 4)]
fn the_cli_reads_jsdoc_types_when_asked(tc: TestCase) {
    let on = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.js", include_str!("../fixtures/tsdoc/jsdoc-types.js"));
    let rules: Map<String, Value> = JSDOC_RULES.iter().map(|rule| ((*rule).to_owned(), json!("error"))).collect();
    let mut config = json!({ "rules": rules });
    if on || tc.draw(gs::booleans()) {
        config["tsdocJsdocTypes"] = json!(on);
    }
    write(dir.path(), "docov.json", &config.to_string());
    let (code, _, stderr) = docov(dir.path(), &["--threshold", "0"], false);
    for rule in JSDOC_RULES {
        assert_eq!(stderr.contains(&format!(": {rule}: ")), !on, "{rule}\n{stderr}");
    }
    assert_eq!(code, i32::from(!on), "{stderr}");
}
