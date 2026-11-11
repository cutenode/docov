//! docs/checks.md, from the check registry: each check's module docs, its options from the schema,
//! and the failing and passing examples in tests/fixtures/checks/<rule-id>/, run through docov.

use std::{fs, path::Path};

use serde_json::Value;

use crate::{code_block, config::kind, current, docov, generated, module_docs};

const INTRO: &str = "
# Doc checks

Checks on what each counted item's JSDoc block says, past whether it has one. Each is off until docov.json's \
`rules` or `--rule ID=LEVEL` (repeatable) turns it on, at `warn` or `error`, with its options if it has some:

```jsonc
\"rules\": {
  \"require-param\": \"error\",
  \"param-names\": \"warn\",
  \"summary-sentence\": [\"warn\", { \"maxLength\": 80 }],
}
```

Each finding prints to stderr as `docov: path:line:column: rule-id: message`, and the `json`, `github`, \
`markdown`, `sarif`, `gitlab`, `junit`, `checkstyle`, `rdjson`, `unix` and `html` [formats](formats.md) list it \
too. A finding at `error` fails the scan (exit 1). A flag's level wins \
over the config's, which keeps its options. A config's `rules` merges by rule id over those of a config it extends \
or nests under, the way ESLint's flat config does: its setting for a rule replaces that one's, keeping its options \
when it gives a level alone, and that one's other rules stay.

An item a `docov-ignore` comment takes out of the total isn't checked. The checks that want a tag, and \
`summary-sentence`, leave a block that inherits its docs (`@inheritdoc` or `{@inheritDoc}`) alone. \
`const f = () => …` is a variable to docov, so the checks on parameters, returns and throws don't look at it.

Each example below is a file docov scans with that one check at `error`, and what it prints.
";

#[test]
fn the_checks_reference_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let schema: Value = serde_json::from_str(&fs::read_to_string(root.join("docov.schema.json")).unwrap()).unwrap();
    let mut page =
        generated("src/check/ and tests/fixtures/checks/") + INTRO + "\n| Rule | What it wants |\n| --- | --- |\n";
    for (id, about) in docov::check::catalog() {
        page += &format!("| [`{id}`](#{id}) | {about} |\n");
    }
    for (id, _) in docov::check::catalog() {
        let docs = module_docs(&format!("src/check/{}.rs", id.replace('-', "_")));
        page += &format!("\n## `{id}`\n\n{docs}\n{}", options(&schema, id));
        let [fails, passes] = ["fail", "pass"].map(|name| example(root, id, name));
        page += &format!("\nFails:\n\n{}\n{}\nPasses:\n\n{}", fails.0, code_block("text", &fails.1), passes.0);
    }
    current("docs/checks.md", &page);
}

/// A table of check `id`'s options, from the schema, or nothing when it has none.
fn options(schema: &Value, id: &str) -> String {
    let with_options = &schema["$defs"]["Rules"]["properties"][id]["anyOf"][1]["prefixItems"][1]["$ref"];
    let name = with_options.as_str().unwrap().trim_start_matches("#/$defs/");
    let Some(options) = schema["$defs"][name]["properties"].as_object() else { return String::new() };
    let rows: String = options
        .iter()
        .map(|(key, option)| {
            let description = option["description"].as_str().unwrap();
            format!("| `{key}` | {} | `{}` | {description} |\n", kind(option, &schema["$defs"]), option["default"])
        })
        .collect();
    format!("\n| Option | Type | Default | Meaning |\n| --- | --- | --- | --- |\n{rows}")
}

/// Check `id`'s `name` example as a code block, and what docov prints to stderr with only that check
/// on, at `error`. A failing example has findings from that check alone, and a passing one none.
fn example(root: &Path, id: &str, name: &str) -> (String, String) {
    let folder = root.join("tests/fixtures/checks").join(id);
    let file = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_stem().is_some_and(|stem| stem == name))
        .unwrap_or_else(|| panic!("add tests/fixtures/checks/{id}/{name}.ts"));
    let extension = file.extension().unwrap().to_str().unwrap();
    let code = fs::read_to_string(&file).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let scanned = format!("example.{extension}");
    fs::write(dir.path().join(&scanned), &code).unwrap();
    let rule = format!("{id}=error");
    let out = docov(dir.path(), &["--quiet", "--rule", &rule, &scanned]).output().unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let findings: Vec<&str> = stderr.lines().filter(|line| line.starts_with("docov: example.")).collect();
    if name == "fail" {
        assert_eq!(out.status.code(), Some(1), "{id}: {stderr}");
        assert!(!findings.is_empty() && findings.iter().all(|f| f.contains(&format!(": {id}: "))), "{id}: {stderr}");
    } else {
        assert_eq!((out.status.code(), stderr.as_str()), (Some(0), ""), "{id}");
    }
    (code_block(extension, &code), stderr)
}
