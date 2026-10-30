//! Properties of doc stubs: `--fix` and the blocks it writes.

use std::fs;

use docov::{Rules, Scope, check, collect, feature::Feature, feature::fix::Fix, rewrite, scan_file};
use hegel::{TestCase, generators as gs};

use super::{config_cli::docov, write};

/// A generated declaration that `--fix` may write a stub for.
struct Declared {
    /// Its name as the scan reports it.
    name: String,
    /// The stub it should get, or `None` when it shouldn't get one.
    stub: Option<String>,
    /// The indentation of its line.
    indent: &'static str,
}

/// A module of functions, methods and variables with drawn parameters (named, defaulted,
/// destructured and rest), type parameters and bodies, some with a block before them already
/// (empty, a stub, tags alone, or docs) and some sharing a line or not starting one, and what
/// each should get.
fn module(tc: &TestCase) -> (String, Vec<Declared>) {
    let mut source = String::new();
    let mut declared = Vec::new();
    let class = tc.draw(gs::booleans());
    let indent = if class { tc.draw(gs::sampled_from(vec!["  ", "\t"])) } else { "" };
    if class {
        source.push_str("export class K {\n");
        declared.push(Declared { name: "K".into(), stub: Some("/** TODO */".into()), indent: "" });
    }
    for i in 0..tc.draw(gs::integers::<usize>().max_value(5)) {
        let block =
            tc.draw(gs::sampled_from(vec![None, Some("/** */"), Some("/** TODO */"), Some("/** @type {x} */")]));
        if let Some(block) = block {
            source.push_str(&format!("{indent}{block}\n"));
        }
        if !class && tc.draw(gs::weighted_booleans(0.2)) {
            // Two items that start on one line, and one that starts after another on its line.
            source.push_str(&format!("const v{i} = 1, w{i} = 2; const x{i} = 3;\n"));
            let stub = block.is_none().then(|| "/** TODO */".to_owned());
            declared.push(Declared { name: format!("v{i}"), stub: stub.clone(), indent });
            declared.push(Declared { name: format!("w{i}"), stub, indent });
            declared.push(Declared { name: format!("x{i}"), stub: None, indent });
            continue;
        }
        let mut params: Vec<&str> = Vec::new();
        for param in tc.draw(gs::vecs(gs::sampled_from(vec!["a", "b = 1", "{ c }", "[d]"])).max_size(3)) {
            if !params.contains(&param) {
                params.push(param);
            }
        }
        params.extend(tc.draw(gs::optional(gs::sampled_from(vec!["...e", "...{ length }"]))));
        let generics = tc.draw(gs::sampled_from(vec![vec![], vec!["T"], vec!["T", "U"]]));
        let (body, returns) = tc.draw(gs::sampled_from(vec![("", false), ("return;", false), ("return 1;", true)]));
        let generic = if generics.is_empty() { String::new() } else { format!("<{}>", generics.join(", ")) };
        let (keyword, name) = if class { ("", format!("K.f{i}")) } else { ("export function ", format!("f{i}")) };
        source.push_str(&format!("{indent}{keyword}f{i}{generic}({}) {{ {body} }}\n", params.join(", ")));
        let mut tags: Vec<String> = generics.iter().map(|t| format!("@template {t}")).collect();
        for (place, param) in params.iter().enumerate() {
            let own = param.trim_start_matches("...").split(' ').next().unwrap();
            let named = own.chars().all(|c| c.is_ascii_alphabetic());
            tags.push(format!("@param {}", if named { own.to_owned() } else { format!("param{place}") }));
        }
        if returns {
            tags.push("@returns".into());
        }
        let text = if tags.is_empty() {
            "/** TODO */".to_owned()
        } else {
            format!("/**\n * TODO\n{} */", tags.iter().map(|tag| format!(" * {tag}\n")).collect::<String>())
        };
        declared.push(Declared { name, stub: block.is_none().then_some(text), indent });
    }
    if class {
        source.push_str("}\n");
    }
    (source, declared)
}

/// The checks the stubs' tags are for, at `error`.
fn checks() -> Rules {
    let on = ["require-param", "param-names", "require-returns", "require-template"];
    let settings: Vec<_> =
        on.iter().map(|id| (*id, check::Setting::parse(id, &serde_json::json!("error")).unwrap())).collect();
    let checks = check::Checks::new(settings.iter().map(|(id, setting)| (*id, setting))).unwrap();
    Rules { checks: Some(std::sync::Arc::new(checks)), ..Rules::from(Scope::All) }
}

/// `--fix` writes each undocumented item's stub above it at its indentation, the same for items
/// that start on one line and none for one that starts after something else on its line or has a
/// block already, and changes no other line. The stubs leave the coverage as it was, since a
/// `TODO` description documents nothing, and running it again writes none. Once each `TODO` is a
/// description, each stubbed item is documented, with a tag for each parameter, type parameter
/// and returned value that the checks want.
#[hegel::test(test_cases = 100)]
fn stubs_go_above_each_undocumented_item(tc: TestCase) {
    let (source, declared) = module(&tc);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ts");
    fs::write(&path, &source).unwrap();
    let rules = Rules::from(Scope::All);
    let before = scan_file(&path, &rules).unwrap();
    let names: Vec<_> = before.items.iter().map(|i| i.name.clone()).collect();
    assert_eq!(names, declared.iter().map(|d| d.name.clone()).collect::<Vec<_>>(), "{source}");
    let stubs: Vec<_> = before.items.iter().map(|i| i.facts.stub.as_ref().map(|s| s.text.clone())).collect();
    assert_eq!(stubs, declared.iter().map(|d| d.stub.clone()).collect::<Vec<_>>(), "{source}");

    let mut wanted = source.clone();
    let mut lines: Vec<_> = before.items.iter().filter_map(|i| i.facts.stub.as_ref().map(|s| (s.line, i))).collect();
    lines.dedup_by_key(|(line, _)| *line);
    for &(line, item) in lines.iter().rev() {
        let at: usize = source.split_inclusive('\n').take(line - 1).map(str::len).sum();
        let indent = declared.iter().find(|d| d.name == item.name).unwrap().indent;
        let stub = item.facts.stub.as_ref().unwrap().text.lines().map(|l| format!("{indent}{l}\n")).collect::<String>();
        wanted.insert_str(at, &stub);
    }
    let added = |n: usize| Some(Ok(format!("added {n} doc stub{}", if n == 1 { "" } else { "s" })));
    assert_eq!(Fix(true).edit(std::slice::from_ref(&before)), added(lines.len()));
    assert_eq!(fs::read_to_string(&path).unwrap(), wanted);
    let after = scan_file(&path, &rules).unwrap();
    assert_eq!(after.totals(), before.totals());
    assert!(after.items.iter().all(|i| i.facts.stub.is_none() || lines.iter().all(|(_, s)| s.name != i.name)));
    // A buffer with stubs scans as its file does.
    assert_eq!(docov::scan(&wanted, oxc_span::SourceType::ts(), &rules).unwrap(), after.items);
    assert_eq!(Fix(true).edit(std::slice::from_ref(&after)), added(0));
    assert_eq!(Fix(false).edit(std::slice::from_ref(&after)), None);
    let explained = collect::explain(&path, &rules).unwrap();
    let stub =
        explained.iter().filter(|line| line.ends_with("skipped a block: a stub, whose description is still TODO"));
    assert_eq!(stub.count(), wanted.matches("TODO").count(), "{explained:#?}");

    fs::write(&path, wanted.replace("TODO", "Docs.")).unwrap();
    let filled = scan_file(&path, &checks()).unwrap();
    for (item, declared) in filled.items.iter().zip(&declared) {
        if declared.stub.is_some() {
            assert!(item.documented && item.facts.diagnostics.is_empty(), "{item:?}\n{wanted}");
        }
    }
}

/// Lines put past the end of a file go there unindented, after a line ending if its last line has
/// none: the file's first one, or `\n`.
#[hegel::test(test_cases = 50)]
fn inserts_past_the_end_go_at_the_end(tc: TestCase) {
    let source = tc.draw(gs::sampled_from(vec!["", "#!/usr/bin/env node", "  a\r\n", "a\r\n  b", "a\n"]));
    let count = source.split_inclusive('\n').count();
    let past = tc.draw(gs::integers::<usize>().min_value(count + 1).max_value(count + 3));
    let text = tc.draw(gs::sampled_from(vec!["// a", "/**\n * b\n */"]));
    let ending = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let separator = if source.is_empty() || source.ends_with('\n') { "" } else { ending };
    let inserted: String = text.lines().map(|line| format!("{line}{ending}")).collect();
    assert_eq!(rewrite::insert_above(source, &[(past, text)]), format!("{source}{separator}{inserted}"));
}

/// A file's own item, with `fileDocs`, gets a stub with `@file` at the top, below a shebang, even in
/// an empty file, above the stub of an item on its first line, and it counts as undocumented until
/// its `TODO` is a description.
#[hegel::test(test_cases = 20)]
fn files_get_a_stub_at_the_top(tc: TestCase) {
    let shebang = tc.draw(gs::sampled_from(vec!["", "#!/usr/bin/env node\n", "#!/usr/bin/env node"]));
    let bodies = vec![
        ("", None),
        ("/** Docs. */\nexport const a = 1;\n", None),
        ("export const b = 1;\n", Some("/** TODO */\n")),
    ];
    let (body, stub) = if shebang.ends_with("node") { ("", None) } else { tc.draw(gs::sampled_from(bodies)) };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ts");
    fs::write(&path, format!("{shebang}{body}")).unwrap();
    let rules = Rules { file_docs: docov::feature::file_docs::FileDocs::Require, ..Rules::default() };
    let before = scan_file(&path, &rules).unwrap();
    let edited = Fix(true).edit(std::slice::from_ref(&before));
    let added = 1 + usize::from(stub.is_some());
    assert_eq!(edited, Some(Ok(format!("added {added} doc stub{}", if added == 1 { "" } else { "s" }))));
    let first = if shebang.is_empty() { "" } else { "#!/usr/bin/env node\n" };
    let wanted = format!("{first}/**\n * TODO\n * @file\n */\n{}{body}", stub.unwrap_or_default());
    assert_eq!(fs::read_to_string(&path).unwrap(), wanted);
    let after = scan_file(&path, &rules).unwrap();
    assert_eq!(after.totals(), before.totals());
    assert!(after.items.iter().all(|i| i.facts.stub.is_none()));
    fs::write(&path, wanted.replace("TODO", "Docs.")).unwrap();
    assert_eq!(scan_file(&path, &rules).unwrap().totals().documented, before.totals().documented + added);
}

/// From the command line, `--fix` writes the stubs and reports how many, under each file's own
/// config (a nested one can exclude a file, which it leaves alone), leaving out ignored items. It
/// can't go with `--add-ignore` or `--stdin-filename`.
#[hegel::test(test_cases = 3)]
fn fix_runs_from_the_command_line(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    for f in 0..count {
        write(
            dir.path(),
            &format!("f{f}.ts"),
            "export function f(a) { return a; }\n// docov-ignore-next-line\nexport const b = 1;\n",
        );
    }
    write(dir.path(), "docov.json", r#"{ "nested": true }"#);
    write(dir.path(), "pkg/docov.json", r#"{ "exclude": ["x.ts"] }"#);
    write(dir.path(), "pkg/x.ts", "export const x = 1;\n");
    let (code, out, err) = docov(dir.path(), &["--fix"], false);
    let message = format!("docov: added {count} doc stub{}\n", if count == 1 { "" } else { "s" });
    assert_eq!((code, out.as_str(), err), (0, "", message));
    assert_eq!(
        fs::read_to_string(dir.path().join("f0.ts")).unwrap(),
        "/**\n * TODO\n * @param a\n * @returns\n */\nexport function f(a) { return a; }\n// docov-ignore-next-line\nexport const b = 1;\n"
    );
    assert_eq!(fs::read_to_string(dir.path().join("pkg/x.ts")).unwrap(), "export const x = 1;\n");
    for other in [&["--fix", "--add-ignore"][..], &["--fix", "--stdin-filename", "a.ts"]] {
        let (code, _, err) = docov(dir.path(), other, false);
        assert_eq!(code, 2, "{err}");
        assert!(err.contains("cannot be used with"), "{err}");
    }
}
