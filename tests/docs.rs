//! The references in docs/, generated from the code, and the README's `console` examples, which are
//! docov's real output. Each test fails when its file is stale, and `DOCOV_BLESS=1 cargo test --test
//! docs` writes it anew.

use std::{
    env, fs,
    io::{Read, Seek},
    iter::Peekable,
    path::Path,
    process::{Command, Stdio},
};

#[path = "docs/checks.rs"]
mod checks;
#[path = "docs/config.rs"]
mod config;
#[path = "docs/formats.rs"]
mod formats;

/// The note at the top of each generated page.
fn generated(from: &str) -> String {
    format!("<!-- Generated from {from} by tests/docs.rs: run `DOCOV_BLESS=1 cargo test --test docs`. -->\n")
}

/// Fails unless the file at `path`, relative to the crate, holds `text`. With `DOCOV_BLESS` set, writes
/// `text` there instead.
fn current(path: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    if env::var_os("DOCOV_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        return;
    }
    let checked_in = fs::read_to_string(&path).unwrap_or_default();
    let stale = checked_in.lines().zip(text.lines()).position(|(old, new)| old != new);
    let line = stale.unwrap_or(checked_in.lines().count().min(text.lines().count()));
    assert!(
        checked_in == text,
        "{} is stale from line {}: run `DOCOV_BLESS=1 cargo test --test docs`\nchecked in: {:?}\ngenerated:  {:?}",
        path.display(),
        line + 1,
        checked_in.lines().nth(line),
        text.lines().nth(line)
    );
}

/// docov's `output` as it reads on macOS and Linux, which the docs show. On Windows, the reports
/// meant for people and editors write native paths (`src\cart.ts`, and `src\\cart.ts` in JSON),
/// and clap names the binary `docov.exe`. Every path in the fixtures is under `src/`.
fn as_on_unix(output: String) -> String {
    if !cfg!(windows) {
        return output;
    }
    output.replace(r"src\\", "src/").replace(r"src\", "src/").replace("docov.exe", "docov")
}

/// docov in `dir`, without the environment variables that change its output: GitHub Actions' annotations,
/// color, and the terminal width `--help` wraps at.
fn docov(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
    for var in ["GITHUB_ACTIONS", "NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "COLUMNS"] {
        command.env_remove(var);
    }
    command.args(args).current_dir(dir).stdin(Stdio::null());
    command
}

/// A temporary copy of the folder at `fixture`, relative to tests/fixtures.
fn fixture(fixture: &str) -> tempfile::TempDir {
    fn copy(from: &Path, to: &Path) {
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                fs::create_dir(&target).unwrap();
                copy(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture), dir.path());
    dir
}

/// Each `console` block in the README runs in a fresh copy of tests/fixtures/quickstart: its
/// `$ docov …` lines run there in turn, and the lines after each are what it printed to stdout and
/// stderr, as a terminal shows them.
#[test]
fn the_readme_examples_are_real_output() {
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")).unwrap();
    let mut out = String::new();
    let mut lines = readme.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        out += line;
        if line.trim_end() == "```console" {
            console(&mut lines, &mut out);
        }
    }
    current("README.md", &out);
}

/// Runs the rest of a `console` block from `lines`, writing it to `out` with each command's output
/// in place of what followed it.
fn console<'a>(lines: &mut Peekable<impl Iterator<Item = &'a str>>, out: &mut String) {
    let dir = fixture("quickstart");
    while let Some(line) = lines.next() {
        *out += line;
        if line.starts_with("```") {
            return;
        }
        let command = line.strip_prefix("$ docov").expect("each line of a console block's commands runs docov");
        assert!(!command.contains(['"', '\'', '|', '>']), "{line}: no quotes or shell syntax");
        let args: Vec<&str> = command.split_whitespace().collect();
        *out += &as_on_unix(interleaved(docov(dir.path(), &args)));
        while lines.next_if(|next| !next.starts_with("$ ") && !next.starts_with("```")).is_some() {}
    }
    panic!("a console block isn't closed");
}

/// What `command` printed to stdout and stderr, in the order it printed them.
fn interleaved(mut command: Command) -> String {
    let mut log = tempfile::tempfile().unwrap();
    command.stdout(log.try_clone().unwrap()).stderr(log.try_clone().unwrap()).status().unwrap();
    let mut text = String::new();
    log.rewind().unwrap();
    log.read_to_string(&mut text).unwrap();
    text
}

/// docs/cli.md is `docov --help`.
#[test]
fn the_command_line_reference_is_current() {
    let dir = tempfile::tempdir().unwrap();
    let help = docov(dir.path(), &["--help"]).output().unwrap();
    let help = as_on_unix(String::from_utf8(help.stdout).unwrap());
    let page = format!(
        "{}\n# Command line\n\n`docov --help` prints this, and `docov -h` a summary. Flags win over the config file's \
         settings, which [config.md](config.md) lists.\n\n```text\n{help}```\n",
        generated("`docov --help`")
    );
    current("docs/cli.md", &page);
}

/// The `//!` docs of the module at `path`, relative to the crate, as Markdown paragraphs, past the
/// "`name`: " they start with, which a page's heading says.
fn module_docs(path: &str) -> String {
    let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
    let lines: Vec<&str> = source.lines().map_while(|line| line.strip_prefix("//!")).map(str::trim).collect();
    let docs =
        lines.split(|line| line.is_empty()).map(|paragraph| paragraph.join(" ")).collect::<Vec<_>>().join("\n\n");
    let (_, docs) = docs.split_once(": ").unwrap_or_else(|| panic!("{path}'s docs start with \"`name`: \""));
    let mut chars = docs.chars();
    chars.next().unwrap().to_uppercase().chain(chars).collect()
}

/// `text` in a code block, fenced with more backticks than it has in a row.
fn code_block(language: &str, text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{language}\n{text}{fence}\n")
}
