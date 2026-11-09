//! docs/formats.md, from the format registry: each format's module docs and its real output on
//! tests/fixtures/quickstart.

use crate::{as_on_unix, code_block, current, docov, fixture, generated, module_docs};

/// The flags each sample runs with, past `--format`: enough for undocumented items, a doc check
/// finding and a failed gate.
const FLAGS: [&str; 4] = ["--threshold", "80", "--rule", "require-param=warn"];

const INTRO: &str = "
# Output formats

`--format` picks one, and can be given more than once: one scan feeds them all. `FORMAT=FILE` writes that one to \
`FILE`, making its folder if needed, and the rest go to stdout one after another. `--output FILE` writes the one \
format without a file of its own to `FILE`.

```sh
docov --format text --format lcov=coverage/docov.lcov
```

Each sample is the real output of `docov --threshold 80 --rule require-param=warn --format NAME` on \
[tests/fixtures/quickstart](../tests/fixtures/quickstart), a project with undocumented items, a doc check finding \
and a failed gate. docov's version is shown as `X.Y.Z`, and cobertura's timestamp as `TIMESTAMP`.
";

#[test]
fn the_formats_reference_is_current() {
    let mut page = generated("src/report/") + INTRO + "\n| Format | What it writes |\n| --- | --- |\n";
    for format in docov::report::FORMATS {
        page += &format!("| [`{0}`](#{0}) | {1} |\n", format.name, format.help);
    }
    for format in docov::report::FORMATS {
        let docs = module_docs(&format!("src/report/{}.rs", format.name));
        let dir = fixture("quickstart");
        let out = docov(dir.path(), &[&FLAGS[..], &["--format", format.name]].concat()).output().unwrap();
        // docov's version changes with each release, and cobertura's timestamp with each run.
        let sample = as_on_unix(String::from_utf8(out.stdout).unwrap()).replace(env!("CARGO_PKG_VERSION"), "X.Y.Z");
        let sample = match sample.split_once("timestamp=\"") {
            Some((before, after)) => {
                format!("{before}timestamp=\"TIMESTAMP{}", after.trim_start_matches(|c: char| c.is_ascii_digit()))
            }
            None => sample,
        };
        assert!(
            !sample.contains(&*dir.path().to_string_lossy()),
            "{}: a sample names its temporary folder",
            format.name
        );
        let block = code_block(language(&sample), &sample);
        let block = if sample.lines().count() > 30 {
            format!("<details>\n<summary>Sample</summary>\n\n{block}\n</details>\n")
        } else {
            block
        };
        page += &format!("\n## `{}`\n\n{docs}\n\n{block}", format.name);
    }
    current("docs/formats.md", &page);
}

/// The code block language that highlights `sample`, by how it starts.
fn language(sample: &str) -> &'static str {
    match sample.trim_start().as_bytes() {
        [b'{' | b'[', ..] => "json",
        s if s.to_ascii_lowercase().starts_with(b"<!doctype html") => "html",
        [b'<', ..] => "xml",
        [b'#', ..] => "markdown",
        _ => "text",
    }
}
