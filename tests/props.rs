//! Properties over generated modules, projects and reports.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, UNIX_EPOCH},
};

use docov::{Context, FileReport, Item, Kind, Output, Scope, Totals, collect, config, report, scan, scan_file};
use hegel::{TestCase, generators as gs};
use oxc_span::SourceType;

/// A generated module, and the items a scan of it should find.
#[derive(Default)]
struct Module {
    source: String,
    /// Each item, and whether the module exports it.
    expected: Vec<(bool, Item)>,
    specifiers: Vec<String>,
    has_default: bool,
}

/// How a generated declaration is exported.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Export {
    No,
    Inline,
    Specifier,
    /// `export default name;`
    DefaultName,
    /// `export default function name() {}`
    DefaultInline,
    /// `exports.name = name;` and the like.
    CommonJs,
}

impl Module {
    fn write(&mut self, text: &str) {
        self.source.push_str(text);
        self.source.push('\n');
    }

    /// Writes ordinary comments and maybe a JSDoc block, and says whether it wrote the block.
    fn doc(&mut self, tc: &TestCase) -> bool {
        let noise = ["// note", "/* note */", "/** */", "/**\n *\n */", "/** @license MIT */"];
        if tc.draw(gs::booleans()) {
            self.write(tc.draw(gs::sampled_from(noise.to_vec())));
        }
        let documented = tc.draw(gs::booleans());
        if documented {
            self.write("/** Docs. */");
        }
        if tc.draw(gs::booleans()) {
            self.write("// after");
        }
        documented
    }

    /// Expects an item on the line about to be written.
    fn expect(&mut self, exported: bool, name: String, kind: Kind, documented: bool) {
        let line = self.source.matches('\n').count() + 1;
        self.expected.push((exported, Item { name, kind, line, column: 1, documented }));
    }

    /// A statement that exports and declares nothing.
    fn nothing(&mut self, tc: &TestCase) {
        let statements = vec![
            "window.x = 1;",
            "x = 1;",
            "exports[key] = 1;",
            "other.exports.y = 1;",
            "z.w = a;",
            "go();",
            "export = { a: 1 };",
            "if (x) {}",
        ];
        self.write(tc.draw(gs::sampled_from(statements)));
    }

    /// A CommonJS export of a value rather than a declaration.
    fn commonjs(&mut self, tc: &TestCase, i: usize) {
        let documented = self.doc(tc);
        match tc.draw(gs::integers::<u8>().max_value(6)) {
            6 => self.nothing(tc),
            0 => {
                self.expect(true, format!("c{i}"), Kind::Function, documented);
                self.write(&format!("exports.c{i} = function () {{}};"));
            }
            1 => {
                self.expect(true, format!("c{i}"), Kind::Variable, documented);
                self.write(&format!("module.exports.c{i} = 1;"));
            }
            2 => {
                self.expect(true, format!("f{i}"), Kind::Function, documented);
                self.write(&format!("module.exports = function f{i}() {{}};"));
            }
            3 if tc.draw(gs::booleans()) => {
                self.expect(true, format!("G{i}"), Kind::Class, documented);
                self.write(&format!("module.exports = class G{i} {{}};"));
            }
            3 => {
                self.expect(true, "default".into(), Kind::Class, documented);
                self.write("module.exports = class {};");
            }
            4 => {
                self.expect(true, "default".into(), Kind::Default, documented);
                self.write("module.exports = 1;");
            }
            _ => {
                self.write("module.exports = {");
                for j in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
                    let documented = self.doc(tc);
                    match tc.draw(gs::integers::<u8>().max_value(4)) {
                        4 => self.write("...rest,"),
                        0 => {
                            self.expect(true, format!("p{i}_{j}"), Kind::Variable, documented);
                            self.write(&format!("p{i}_{j}: 1,"));
                        }
                        1 => {
                            self.expect(true, format!("m{i}_{j}"), Kind::Function, documented);
                            self.write(&format!("m{i}_{j}() {{}},"));
                        }
                        2 => {
                            self.expect(true, format!("a{i}_{j}"), Kind::Function, documented);
                            self.write(&format!("a{i}_{j}: () => 1,"));
                        }
                        // Exports a name nothing here declares.
                        _ => self.write("undeclared,"),
                    }
                }
                self.write("};");
            }
        }
        if tc.draw(gs::booleans()) {
            self.write(&format!("const r{i} = require('r{i}');"));
        }
    }

    fn declaration(&mut self, tc: &TestCase, i: usize) {
        if tc.draw(gs::weighted_booleans(0.2)) {
            return self.commonjs(tc, i);
        }
        let name = format!("n{i}");
        let kinds = vec![
            Kind::Function,
            Kind::Class,
            Kind::Variable,
            Kind::Interface,
            Kind::Type,
            Kind::Enum,
            Kind::Namespace,
            Kind::Default,
        ];
        let kind = tc.draw(gs::sampled_from(kinds));
        let mut exports = vec![Export::No, Export::Inline, Export::Specifier, Export::CommonJs];
        if !self.has_default {
            exports.push(Export::DefaultName);
            if matches!(kind, Kind::Function | Kind::Class | Kind::Interface) {
                exports.push(Export::DefaultInline);
            }
        }
        let export = if kind == Kind::Default { Export::DefaultInline } else { tc.draw(gs::sampled_from(exports)) };
        if kind == Kind::Default && self.has_default {
            return;
        }
        self.has_default |= matches!(export, Export::DefaultName | Export::DefaultInline);
        let exported = export != Export::No;
        let prefix = match export {
            Export::Inline => "export ",
            Export::DefaultInline => "export default ",
            _ => "",
        };
        if export == Export::Specifier {
            self.specifiers.push(name.clone());
        }

        let documented = self.doc(tc);
        match kind {
            Kind::Function => {
                self.expect(exported, name.clone(), kind, documented);
                if export != Export::DefaultInline && tc.draw(gs::booleans()) {
                    // Only the signature counts; the implementation after it doesn't.
                    self.write(&format!("{prefix}function {name}(a: string): void;"));
                }
                self.write(&format!("{prefix}function {name}(a: unknown) {{}}"));
            }
            Kind::Variable if tc.draw(gs::booleans()) => {
                // Nothing to name: destructuring declares no item.
                self.write(&format!("{prefix}const {{ d{i} }} = {{ d{i}: 1 }};"));
            }
            Kind::Variable => {
                self.expect(exported, name.clone(), kind, documented);
                self.write(&format!("{prefix}const {name} = 1;"));
            }
            Kind::Type | Kind::Enum | Kind::Namespace => {
                self.expect(exported, name.clone(), kind, documented);
                let body = match kind {
                    Kind::Type => format!("type {name} = string;"),
                    Kind::Enum => format!("enum {name} {{ A }}"),
                    _ => format!("namespace {name} {{}}"),
                };
                self.write(&format!("{prefix}{body}"));
            }
            Kind::Default => {
                self.expect(true, "default".into(), kind, documented);
                self.write("export default { a: 1 };");
            }
            Kind::Class => {
                self.expect(exported, name.clone(), kind, documented);
                if tc.draw(gs::booleans()) {
                    self.write("@sealed");
                }
                self.write(&format!("{prefix}class {name} {{"));
                self.class_members(tc, &name, exported);
                self.write("}");
            }
            Kind::Interface => {
                self.expect(exported, name.clone(), kind, documented);
                self.write(&format!("{prefix}interface {name} {{"));
                for j in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
                    let documented = self.doc(tc);
                    match tc.draw(gs::integers::<u8>().max_value(2)) {
                        0 => {
                            self.expect(exported, format!("{name}.p{j}"), Kind::Property, documented);
                            self.write(&format!("p{j}: 1;"));
                        }
                        1 => {
                            self.expect(exported, format!("{name}.m{j}"), Kind::Method, documented);
                            self.write(&format!("m{j}(): void;"));
                        }
                        _ => self.write("[key: string]: unknown;"),
                    }
                }
                self.write("}");
            }
            _ => unreachable!(),
        }
        if export == Export::CommonJs {
            let statements = vec![
                format!("exports.{name} = {name};"),
                format!("module.exports.{name} = {name};"),
                format!("module.exports = {name};"),
                format!("module.exports = {{ {name} }};"),
            ];
            self.write(&tc.draw(gs::sampled_from(statements)));
        }
        if export == Export::DefaultName {
            let statements = vec![
                format!("export default {name};"),
                format!("export = {name};"),
                format!("export {{ {name} as default }};"),
            ];
            self.write(&tc.draw(gs::sampled_from(statements)));
        }
        if tc.draw(gs::weighted_booleans(0.2)) {
            self.write(&format!("declare module 'm{i}' {{ export function f(): void; }}"));
        }
    }

    fn class_members(&mut self, tc: &TestCase, class: &str, exported: bool) {
        let mut constructed = false;
        for j in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
            let documented = self.doc(tc);
            let item =
                |m: &mut Self, name: String, kind| m.expect(exported, format!("{class}.{name}"), kind, documented);
            match tc.draw(gs::integers::<u8>().max_value(9)) {
                0 => {
                    item(self, format!("m{j}"), Kind::Method);
                    if tc.draw(gs::booleans()) {
                        self.write("@log");
                    }
                    self.write(&format!("m{j}() {{}}"));
                }
                1 => {
                    item(self, format!("p{j}"), Kind::Property);
                    self.write(&format!("p{j} = 1;"));
                }
                2 => {
                    item(self, format!("s{j}"), Kind::Property);
                    self.write(&format!("static s{j} = 1;"));
                }
                3 => {
                    item(self, format!("g{j}"), Kind::Accessor);
                    self.write(&format!("get g{j}() {{ return 1; }}\nset g{j}(v) {{}}"));
                }
                4 => {
                    item(self, format!("o{j}"), Kind::Accessor);
                    self.write(&format!("set o{j}(v) {{}}"));
                }
                5 => {
                    item(self, format!("a{j}"), Kind::Accessor);
                    self.write(&format!("accessor a{j} = 1;"));
                }
                6 => {
                    item(self, format!("[k{j}]"), Kind::Method);
                    self.write(&format!("[k{j}]() {{}}"));
                }
                7 => {
                    item(self, format!("v{j}"), Kind::Method);
                    self.write(&format!("v{j}(a: string): void;\nv{j}(a: unknown) {{}}"));
                }
                8 if !constructed => {
                    constructed = true;
                    self.write("constructor() {}");
                }
                _ => self.write(&format!("#h{j} = 1;\nprivate q{j} = 1;\nprivate r{j}() {{}}")),
            }
        }
    }

    fn generate(tc: &TestCase) -> Self {
        let mut module = Self::default();
        for i in 0..tc.draw(gs::integers::<usize>().max_value(10)) {
            module.declaration(tc, i);
        }
        if tc.draw(gs::booleans()) {
            module.nothing(tc);
        }
        module.finish()
    }

    fn finish(mut self) -> Self {
        if !self.specifiers.is_empty() {
            let list = self.specifiers.join(", ");
            self.write(&format!("export {{ {list} }};"));
        }
        self
    }
}

/// A module of generated declarations, each with or without JSDoc, stray ordinary comments,
/// members, decorators, overloads and every way of exporting. Scanning it finds exactly the items
/// the model says it should, in order, with their docs and lines. Breaking its syntax fails the
/// scan at a place inside it.
#[hegel::test(test_cases = 300)]
fn scans_find_what_modules_declare(tc: TestCase) {
    let Module { source, expected, .. } = Module::generate(&tc);

    for scope in [Scope::Exports, Scope::All] {
        let items = scan(&source, SourceType::ts(), scope).unwrap();
        let wanted: Vec<&Item> =
            expected.iter().filter(|(exported, _)| scope == Scope::All || *exported).map(|(_, item)| item).collect();
        assert_eq!(items.iter().collect::<Vec<_>>(), wanted, "{source}");
    }

    let broken = format!("{source}export function (");
    let error = scan(&broken, SourceType::ts(), Scope::All).unwrap_err();
    assert!(error.offset >= source.len() && error.offset <= broken.len(), "{error:?}");
}

/// A module read from a file has the items its text does. One that doesn't parse fails with its
/// path and the line of the error, and one that isn't UTF-8 with its path.
#[hegel::test(test_cases = 100)]
fn files_scan_as_their_text(tc: TestCase) {
    const SYNTAX_ERROR: &[u8] = b"export function (";
    let Module { source, .. } = Module::generate(&tc);
    let tail = tc.draw(gs::sampled_from(vec![&b""[..], SYNTAX_ERROR, b"// caf\xe9"]));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("m.{}", tc.draw(gs::sampled_from(vec!["ts", "mts", "cts"]))));
    fs::write(&path, [source.as_bytes(), tail].concat()).unwrap();

    match scan_file(&path, Scope::All) {
        Ok(report) => {
            assert!(tail.is_empty());
            assert_eq!((report.path, report.items), (path, scan(&source, SourceType::ts(), Scope::All).unwrap()));
        }
        Err(collect::Error::Syntax { path: at, line, .. }) => {
            assert_eq!(tail, SYNTAX_ERROR);
            assert_eq!((at, line), (path, source.matches('\n').count() + 1));
        }
        Err(collect::Error::Read { path: at, source }) => {
            assert!(std::str::from_utf8(tail).is_err());
            assert_eq!((at, source.kind()), (path, std::io::ErrorKind::InvalidData));
        }
        Err(e) => panic!("{e}"),
    }
}

const ALL_KINDS: [Kind; 11] = [
    Kind::Function,
    Kind::Class,
    Kind::Method,
    Kind::Accessor,
    Kind::Property,
    Kind::Variable,
    Kind::Interface,
    Kind::Type,
    Kind::Enum,
    Kind::Namespace,
    Kind::Default,
];

/// Names and paths with what each format has to escape: separators, markup and quotes.
const NAMES: [&str; 5] = ["n", "C.m", "`k``", "[a, b]", "p|q*_<&>%:\"'"];
const FOLDERS: [&str; 4] = ["", "src/", "src/deep/", "a b/"];
const FILE_NAMES: [&str; 4] = ["", ",%:", "<&>'\"", "`|`"];

/// Up to four files of up to six items, two to a line, with names and paths to escape.
fn reports(tc: &TestCase) -> Vec<FileReport> {
    let item = gs::vecs(gs::integers::<usize>().max_value(ALL_KINDS.len() - 1)).min_size(3).max_size(3);
    let files = tc.draw(gs::vecs(gs::vecs(item).max_size(6)).max_size(4));
    files
        .into_iter()
        .enumerate()
        .map(|(f, items)| FileReport {
            path: PathBuf::from(format!(
                "{}f{f}{}.ts",
                tc.draw(gs::sampled_from(FOLDERS.to_vec())),
                tc.draw(gs::sampled_from(FILE_NAMES.to_vec()))
            )),
            items: items
                .into_iter()
                .enumerate()
                .map(|(i, drawn)| Item {
                    name: format!("{}{i}", NAMES[drawn[2] % NAMES.len()]),
                    kind: ALL_KINDS[drawn[0]],
                    line: i / 2 + 1,
                    column: 1,
                    documented: drawn[1] % 2 == 0,
                })
                .collect(),
        })
        .collect()
}

/// Messages from failed gates, with what formats have to escape.
const FAILURES: [&str; 4] = [
    "66.7% documented, under the 80% threshold",
    "two\nlines\r\n",
    "100%: a, b",
    "*bold* _it_ <b> | `c` [l](u) & # ~ \\",
];

/// What a format reports from, drawn.
struct Drawn {
    files: Vec<FileReport>,
    failures: Vec<String>,
    /// When the scan ran, in milliseconds since 1970.
    now_ms: u64,
    threshold: Option<f64>,
}

impl Drawn {
    fn draw(tc: &TestCase) -> Self {
        let failures = tc.draw(gs::vecs(gs::sampled_from(FAILURES.to_vec())).max_size(2));
        let now_ms = tc.draw(gs::integers::<u64>().max_value(4_000_000_000_000));
        let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
        Self { files: reports(tc), failures: failures.into_iter().map(String::from).collect(), now_ms, threshold }
    }

    fn context(&self) -> Context<'_> {
        Context {
            files: &self.files,
            failures: &self.failures,
            now: UNIX_EPOCH + Duration::from_millis(self.now_ms),
            threshold: self.threshold,
        }
    }
}

/// A writer with room for this many bytes, failing a write that doesn't fit.
struct Room(usize);

impl std::io::Write for Room {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.checked_sub(buf.len()).ok_or(std::io::ErrorKind::StorageFull)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A threshold is any number from 0 to 100, written any way Rust reads a float, and nothing else.
#[hegel::test(test_cases = 200)]
fn thresholds_are_percentages(tc: TestCase) {
    let edges = vec!["0", "-0", "100", "100.0", "1e2", "100.00001", "-0.00001", "NaN", "inf", ""];
    let text = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => tc.draw(gs::floats::<f64>()).to_string(),
        1 => tc.draw(gs::sampled_from(edges)).to_owned(),
        _ => tc.draw(gs::text()),
    };
    match text.parse::<f64>() {
        Ok(p) if (0.0..=100.0).contains(&p) => assert_eq!(config::percent(&text), Ok(p)),
        _ => assert_eq!(config::percent(&text), Err("expected a number from 0 to 100".into())),
    }
}

/// `--format` takes each format's name, and nothing else, listing the names when it fails. No two
/// formats share one.
#[hegel::test(test_cases = 50)]
fn formats_are_found_by_name(tc: TestCase) {
    let names: Vec<&str> = report::FORMATS.iter().map(|f| f.name).collect();
    assert!(names.iter().enumerate().all(|(i, name)| !names[..i].contains(name)), "{names:?}");
    let name =
        if tc.draw(gs::booleans()) { tc.draw(gs::sampled_from(names.clone())).to_owned() } else { tc.draw(gs::text()) };
    match report::format(&name) {
        Ok(format) => assert_eq!(format.name, name),
        Err(e) => {
            assert!(!names.contains(&name.as_str()), "{name}");
            assert_eq!(e, format!("unknown format `{name}`, expected one of: {}", names.join(", ")));
        }
    }
}

/// `--format` takes a format's name, then maybe `=FILE`, and nothing else.
#[hegel::test(test_cases = 100)]
fn format_flags_name_a_format_and_maybe_a_file(tc: TestCase) {
    let names: Vec<&str> = report::FORMATS.iter().map(|f| f.name).collect();
    let name =
        if tc.draw(gs::booleans()) { tc.draw(gs::sampled_from(names.clone())).to_owned() } else { tc.draw(gs::text()) };
    let file = tc.draw(gs::optional(gs::sampled_from(vec!["", "x.lcov", "a=b", "deep/x.json"])));
    let value = file.map_or(name.clone(), |file| format!("{name}={file}"));
    let (name, file) = value.split_once('=').map_or((&*value, None), |(name, file)| (name, Some(file)));
    match Output::parse(&value) {
        Ok(output) => assert_eq!((output.format.name, output.path), (name, file.map(PathBuf::from))),
        Err(e) if file == Some("") => assert_eq!(e, format!("expected a file after `{name}=`")),
        Err(e) => assert_eq!(Err(e), report::format(name).map(|f| f.name)),
    }
}

/// Each format goes to its file, folders made, or else to stdout, one after another; `--output` is
/// the file for the one format without its own, and an error without exactly one. With no
/// `--format`, it's text, and GitHub annotations too in GitHub Actions. A file that can't be written fails, naming it.
#[hegel::test(test_cases = 50)]
fn outputs_go_where_the_flags_send_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let drawn = Drawn::draw(&tc);
    let context = drawn.context();
    let names: Vec<&str> = report::FORMATS.iter().map(|f| f.name).collect();
    let mut format = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let name = tc.draw(gs::sampled_from(names.clone()));
        let file = tc.draw(gs::optional(gs::sampled_from(vec![format!("{i}.out"), format!("deep/er/{i}.out")])));
        format.push(Output { format: report::format(name).unwrap(), path: file.map(|f| dir.path().join(f)) });
    }
    let output = tc.draw(gs::booleans()).then(|| dir.path().join("out/o.txt"));
    let unnamed = if format.is_empty() { 1 } else { format.iter().filter(|o| o.path.is_none()).count() };
    let github_actions = tc.draw(gs::booleans());
    let badge = tc.draw(gs::booleans()).then(|| dir.path().join("badge.svg"));
    let flags = report::Flags { format: format.clone(), output: output.clone(), badge: badge.clone() };
    let outputs = match flags.outputs(github_actions) {
        Err(e) => {
            assert!(output.is_some() && unnamed != 1, "{e}");
            assert_eq!(e, "--output needs exactly one --format without a file");
            return;
        }
        Ok(outputs) => outputs,
    };
    assert!(output.is_none() || unnamed == 1);
    let mut named: Vec<&str> = outputs.iter().map(|o| o.format.name).collect();
    // `--badge` adds a badge, at the end.
    if badge.is_some() {
        assert_eq!((named.pop(), &outputs.last().unwrap().path), (Some("badge"), &badge));
    }
    if format.is_empty() {
        assert_eq!(named, if github_actions { &["text", "github"][..] } else { &["text"] });
    } else {
        assert_eq!(named, format.iter().map(|o| o.format.name).collect::<Vec<_>>());
    }
    let sent_to_output = outputs.iter().filter(|o| o.path.is_some() && o.path == output).count();
    assert_eq!(sent_to_output, usize::from(output.is_some()));
    // With `--output`, only GitHub's annotations still go to stdout, where the runner reads them.
    let left = outputs.iter().filter(|o| o.path.is_none()).map(|o| o.format.name).collect::<Vec<_>>();
    assert!(output.is_none() || left.is_empty() || (format.is_empty() && left == ["github"]), "{left:?}");

    let mut stdout = Vec::new();
    report::write(&outputs, &context, &mut stdout).unwrap();
    let to_stdout: String = outputs.iter().filter(|o| o.path.is_none()).map(|o| o.format.render(&context)).collect();
    assert_eq!(String::from_utf8(stdout).unwrap(), to_stdout);
    for output in &outputs {
        if let Some(path) = &output.path {
            assert_eq!(fs::read_to_string(path).unwrap(), output.format.render(&context), "{}", path.display());
        }
    }

    // A file in the way of the folder, and a folder in the way of the file.
    write(dir.path(), "in-the-way", "");
    let format = outputs[0].format;
    for path in [dir.path().join("in-the-way/x.out"), dir.path().to_owned()] {
        let e = report::write(&[Output { format, path: Some(path.clone()) }], &context, &mut Vec::new()).unwrap_err();
        assert!(e.starts_with(&format!("{}: ", path.display())), "{e}");
    }
}

/// Undoes GitHub's workflow command escaping.
fn unescape_github(text: &str) -> String {
    text.replace("%2C", ",").replace("%3A", ":").replace("%0A", "\n").replace("%0D", "\r").replace("%25", "%")
}

/// `github` writes one command per line: a warning at each undocumented item, then an error for
/// each failed gate. Its properties and messages come back as they were once unescaped.
#[hegel::test(test_cases = 100)]
fn github_annotates_each_undocumented_item_and_failure(tc: TestCase) {
    let drawn = Drawn::draw(&tc);
    let out = report::format("github").unwrap().render(&drawn.context());
    let undocumented = drawn.files.iter().flat_map(|f| f.items.iter().filter(|i| !i.documented).map(move |i| (f, i)));
    let mut wanted: Vec<String> = undocumented
        .map(|(f, i)| {
            let path = f.path.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
            format!(
                "warning file={path} line={} col={} title=docov {} {} is undocumented",
                i.line,
                i.column,
                i.kind.as_str(),
                i.name
            )
        })
        .collect();
    wanted.extend(drawn.failures.iter().map(|f| format!("error title=docov {f}")));
    let read: Vec<String> = out
        .lines()
        .map(|line| {
            let (command, message) = line.strip_prefix("::").unwrap().split_once("::").unwrap();
            let (name, properties) = command.split_once(' ').unwrap();
            let properties: Vec<String> = properties.split(',').map(unescape_github).collect();
            format!("{name} {} {}", properties.join(" "), unescape_github(message))
        })
        .collect();
    assert_eq!(read, wanted, "{out}");
}

/// `markdown` reads back as written: the total, each failure as a line of text, a table row per
/// file with items with its path as code, and each undocumented item's name as code, folded away
/// under its file. Names and paths come back whole however many backticks, pipes or spaces they
/// have, and failures as plain text whatever Markdown they look like.
#[hegel::test(test_cases = 100)]
fn markdown_reads_back_as_written(tc: TestCase) {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let drawn = Drawn::draw(&tc);
    let out = report::format("markdown").unwrap().render(&drawn.context());
    let totals = Totals::of(&drawn.files);
    let heading =
        format!("## docov\n\n**{:.1}%** documented ({}/{})\n", totals.percent(), totals.documented, totals.total);
    assert!(out.starts_with(&heading), "{out}");

    let (mut code, mut paragraphs, mut rows, mut details) = (Vec::new(), Vec::new(), 0, 0);
    let mut in_paragraph = false;
    for event in Parser::new_ext(&out, Options::ENABLE_TABLES) {
        match event {
            Event::Code(text) => code.push(text.into_string()),
            Event::Start(Tag::Paragraph) => {
                in_paragraph = true;
                paragraphs.push(String::new());
            }
            Event::End(TagEnd::Paragraph) => in_paragraph = false,
            Event::Text(text) if in_paragraph => paragraphs.last_mut().unwrap().push_str(&text),
            Event::End(TagEnd::TableRow) => rows += 1,
            Event::Html(html) if html.starts_with("<details>") => details += 1,
            _ => {}
        }
    }
    let slash = |path: &Path| path.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    let with_items: Vec<_> = drawn.files.iter().filter(|f| !f.items.is_empty()).collect();
    let mut wanted: Vec<String> = with_items.iter().map(|f| slash(&f.path)).collect();
    wanted.extend(drawn.files.iter().flat_map(|f| f.items.iter().filter(|i| !i.documented).map(|i| i.name.clone())));
    assert_eq!(code, wanted, "{out}");
    let failed: Vec<String> =
        paragraphs.into_iter().filter_map(|p| p.strip_prefix("Failed: ").map(String::from)).collect();
    let failures: Vec<String> =
        drawn.failures.iter().map(|f| f.replace(['\r', '\n'], " ").trim_end().to_owned()).collect();
    assert_eq!(failed.iter().map(|f| f.trim_end()).collect::<Vec<_>>(), failures, "{out}");
    assert_eq!(rows, with_items.len());
    assert_eq!(details, drawn.files.iter().filter(|f| f.items.iter().any(|i| !i.documented)).count());
}

/// Each undocumented item and its file, with the file's path as CI services want it.
fn undocumented(files: &[FileReport]) -> Vec<(String, &Item)> {
    let slash = |path: &Path| path.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    files.iter().flat_map(|f| f.items.iter().filter(|i| !i.documented).map(move |i| (slash(&f.path), i))).collect()
}

/// `sarif` has the fields SARIF 2.1.0 requires, one rule, and a result at each undocumented item
/// whose location is a URI reference that decodes to the item's file.
#[hegel::test(test_cases = 100)]
fn sarif_has_a_result_at_each_undocumented_item(tc: TestCase) {
    let drawn = Drawn::draw(&tc);
    let out = report::format("sarif").unwrap().render(&drawn.context());
    let sarif: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(sarif["version"], "2.1.0");
    let run = &sarif["runs"][0];
    assert_eq!(sarif["runs"].as_array().unwrap().len(), 1);
    assert_eq!(run["tool"]["driver"]["name"], "docov");
    assert_eq!(run["tool"]["driver"]["rules"][0]["id"], "docov/undocumented");
    let results = run["results"].as_array().unwrap();
    let wanted = undocumented(&drawn.files);
    assert_eq!(results.len(), wanted.len());
    for (result, (path, item)) in results.iter().zip(wanted) {
        assert_eq!(result["ruleId"], "docov/undocumented");
        assert_eq!(result["message"]["text"], format!("{} {} is undocumented", item.kind.as_str(), item.name));
        let location = &result["locations"][0]["physicalLocation"];
        let uri = location["artifactLocation"]["uri"].as_str().unwrap();
        assert!(uri.chars().all(|c| c.is_ascii_alphanumeric() || "-._~/%".contains(c)), "{uri}");
        assert_eq!(percent_encoding::percent_decode_str(uri).decode_utf8().unwrap(), path);
        assert_eq!(
            (&location["region"]["startLine"], &location["region"]["startColumn"]),
            (&item.line.into(), &item.column.into())
        );
    }
}

/// `gitlab`, `rdjson`, `checkstyle` and `junit` each report every undocumented item at its file,
/// line and message, which come back whole from their JSON or XML; `junit` passes the documented
/// items too. GitLab's fingerprints are the same for the same file, kind and name, and only then.
#[hegel::test(test_cases = 100)]
fn ci_formats_report_each_undocumented_item(tc: TestCase) {
    let drawn = Drawn::draw(&tc);
    let render = |name| report::format(name).unwrap().render(&drawn.context());
    let wanted = undocumented(&drawn.files);
    let message = |item: &Item| format!("{} {} is undocumented", item.kind.as_str(), item.name);
    let slash = |path: &Path| path.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");

    let gitlab: serde_json::Value = serde_json::from_str(&render("gitlab")).unwrap();
    let issues = gitlab.as_array().unwrap();
    assert_eq!(issues.len(), wanted.len());
    for (issue, (path, item)) in issues.iter().zip(&wanted) {
        let read = (&issue["description"], &issue["check_name"], &issue["severity"]);
        assert_eq!(read, (&message(item).into(), &"docov/undocumented".into(), &"minor".into()));
        assert_eq!(
            (&issue["location"]["path"], &issue["location"]["lines"]["begin"]),
            (&path.as_str().into(), &item.line.into())
        );
        let fingerprint = issue["fingerprint"].as_str().unwrap();
        assert!(fingerprint.len() == 16 && fingerprint.chars().all(|c| c.is_ascii_hexdigit()), "{fingerprint}");
    }
    for (a, (a_path, a_item)) in issues.iter().zip(&wanted) {
        for (b, (b_path, b_item)) in issues.iter().zip(&wanted) {
            let same = (a_path, a_item.kind, &a_item.name) == (b_path, b_item.kind, &b_item.name);
            assert_eq!(a["fingerprint"] == b["fingerprint"], same);
        }
    }

    let rdjson: serde_json::Value = serde_json::from_str(&render("rdjson")).unwrap();
    assert_eq!(rdjson["source"]["name"], "docov");
    let diagnostics = rdjson["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), wanted.len());
    for (diagnostic, (path, item)) in diagnostics.iter().zip(&wanted) {
        let start = &diagnostic["location"]["range"]["start"];
        assert_eq!(
            (&diagnostic["message"], &diagnostic["location"]["path"]),
            (&message(item).into(), &path.as_str().into())
        );
        assert_eq!((&start["line"], &start["column"]), (&item.line.into(), &item.column.into()));
    }

    let checkstyle = render("checkstyle");
    let checkstyle = roxmltree::Document::parse(&checkstyle).unwrap();
    let files: Vec<_> = checkstyle.root_element().children().filter(|n| n.has_tag_name("file")).collect();
    assert_eq!(
        files.iter().map(|f| f.attribute("name").unwrap().to_owned()).collect::<Vec<_>>(),
        drawn.files.iter().map(|f| slash(&f.path)).collect::<Vec<_>>()
    );
    let errors = files
        .iter()
        .flat_map(|f| f.children().filter(|n| n.has_tag_name("error")).map(move |e| (f.attribute("name").unwrap(), e)));
    let read: Vec<_> = errors
        .map(|(path, e)| {
            (path.to_owned(), e.attribute("line").unwrap().parse().unwrap(), e.attribute("message").unwrap().to_owned())
        })
        .collect();
    assert_eq!(
        read,
        wanted
            .iter()
            .map(|(path, item)| (path.clone(), item.line, message(item)))
            .collect::<Vec<(String, usize, String)>>()
    );

    let junit = render("junit");
    let junit = roxmltree::Document::parse(&junit).unwrap();
    let suites = junit.root_element();
    let totals = Totals::of(&drawn.files);
    let count = |node: roxmltree::Node, name| node.attribute(name).unwrap().parse::<usize>().unwrap();
    assert_eq!((count(suites, "tests"), count(suites, "failures")), (totals.total, totals.total - totals.documented));
    let suites: Vec<_> = suites.children().filter(|n| n.has_tag_name("testsuite")).collect();
    assert_eq!(suites.len(), drawn.files.len());
    for (suite, file) in suites.iter().zip(&drawn.files) {
        assert_eq!(suite.attribute("name"), Some(slash(&file.path).as_str()));
        let cases: Vec<_> = suite.children().filter(|n| n.has_tag_name("testcase")).collect();
        assert_eq!(cases.len(), file.items.len());
        for (case, item) in cases.iter().zip(&file.items) {
            assert_eq!(case.attribute("name").unwrap(), format!("{} {}", item.kind.as_str(), item.name));
            let failure = case.children().find(|n| n.has_tag_name("failure"));
            assert_eq!(
                failure.map(|f| f.attribute("message").unwrap().to_owned()),
                (!item.documented).then(|| message(item))
            );
        }
    }
}

/// `lcov` and `cobertura` agree on the lines: each line with items is a record, hit when they're
/// all documented. `lcov` names each item as a function, hit when it's documented, and
/// `cobertura` puts each folder's files in one package, with line rates from 0 to 1.
#[hegel::test(test_cases = 100)]
fn coverage_formats_agree_on_lines(tc: TestCase) {
    let drawn = Drawn::draw(&tc);
    let render = |name| report::format(name).unwrap().render(&drawn.context());
    let slash = |path: &Path| path.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    // Each file's lines with items, and whether all of them are documented.
    let lines = |file: &FileReport| {
        let mut lines: Vec<(usize, bool)> = Vec::new();
        for line in file.items.iter().map(|i| i.line) {
            if lines.last().is_none_or(|(at, _)| *at != line) {
                lines.push((line, file.items.iter().filter(|i| i.line == line).all(|i| i.documented)));
            }
        }
        lines
    };

    let lcov = render("lcov");
    let records: Vec<&str> = lcov.split_terminator("end_of_record\n").collect();
    assert_eq!(records.len(), drawn.files.len());
    for (record, file) in records.iter().zip(&drawn.files) {
        let functions: Vec<String> = file.items.iter().map(|i| format!("FN:{},{}", i.line, i.name)).collect();
        let hits: Vec<String> =
            file.items.iter().map(|i| format!("FNDA:{},{}", u8::from(i.documented), i.name)).collect();
        let records: Vec<String> =
            lines(file).iter().map(|(line, hit)| format!("DA:{line},{}", u8::from(*hit))).collect();
        let (totals, hit) = (file.totals(), lines(file).iter().filter(|(_, hit)| *hit).count());
        let wanted = [
            vec!["TN:docov".to_owned(), format!("SF:{}", file.path.display())],
            functions,
            hits,
            vec![format!("FNF:{}", totals.total), format!("FNH:{}", totals.documented)],
            records,
            vec![format!("LH:{hit}"), format!("LF:{}", lines(file).len())],
        ];
        assert_eq!(record.lines().collect::<Vec<_>>(), wanted.concat(), "{lcov}");
    }

    let cobertura = render("cobertura");
    let cobertura = roxmltree::Document::parse_with_options(
        &cobertura,
        roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
    )
    .unwrap();
    let coverage = cobertura.root_element();
    let number = |node: roxmltree::Node, name| node.attribute(name).unwrap().parse::<f64>().unwrap();
    let all: Vec<(usize, bool)> = drawn.files.iter().flat_map(lines).collect();
    let hit = all.iter().filter(|(_, hit)| *hit).count();
    assert_eq!((number(coverage, "lines-covered"), number(coverage, "lines-valid")), (hit as f64, all.len() as f64));
    assert_eq!(number(coverage, "line-rate"), Totals { documented: hit, total: all.len() }.percent() / 100.0);
    assert_eq!(number(coverage, "timestamp"), drawn.now_ms as f64);
    let packages: Vec<_> = coverage.descendants().filter(|n| n.has_tag_name("package")).collect();
    let names: Vec<_> = packages.iter().map(|p| p.attribute("name").unwrap()).collect();
    assert!(names.iter().enumerate().all(|(i, name)| !names[..i].contains(name)), "{names:?}");
    let mut classes = Vec::new();
    for package in &packages {
        assert!((0.0..=1.0).contains(&number(*package, "line-rate")));
        for class in package.descendants().filter(|n| n.has_tag_name("class")) {
            let filename = class.attribute("filename").unwrap().to_owned();
            let folder = filename.rsplit_once('/').map_or(".", |(folder, _)| folder).to_owned();
            assert_eq!(package.attribute("name"), Some(folder.as_str()));
            let lines: Vec<(usize, bool)> = class
                .descendants()
                .filter(|n| n.has_tag_name("line"))
                .map(|l| (number(l, "number") as usize, number(l, "hits") == 1.0))
                .collect();
            classes.push((filename, lines));
        }
    }
    let mut wanted: Vec<(String, Vec<(usize, bool)>)> =
        drawn.files.iter().map(|f| (slash(&f.path), lines(f))).collect();
    classes.sort();
    wanted.sort();
    assert_eq!(classes, wanted);
}

/// `badge` and `shields` show the total to one decimal place, red under the threshold, yellow up
/// to 10 points over it, green past that, and blue with no threshold to compare with.
#[hegel::test(test_cases = 100)]
fn badges_are_colored_by_the_threshold(tc: TestCase) {
    let mut drawn = Drawn::draw(&tc);
    let percent = Totals::of(&drawn.files).percent();
    let near = tc.draw(gs::sampled_from(vec![-10.0, -0.5, 0.0, 5.0, 10.0, 10.5]));
    drawn.threshold = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => None,
        1 => Some(f64::from(tc.draw(gs::integers::<u8>().max_value(100)))),
        _ => Some(percent - near),
    };
    let (name, hex) = match drawn.threshold {
        None => ("blue", "#007ec6"),
        Some(t) if percent < t => ("red", "#e05d44"),
        Some(t) if percent <= t + 10.0 => ("yellow", "#dfb317"),
        Some(_) => ("green", "#97ca00"),
    };
    let message = format!("{percent:.1}%");

    let shields: serde_json::Value =
        serde_json::from_str(&report::format("shields").unwrap().render(&drawn.context())).unwrap();
    assert_eq!(shields, serde_json::json!({ "schemaVersion": 1, "label": "docs", "message": message, "color": name }));

    let svg = report::format("badge").unwrap().render(&drawn.context());
    let svg = roxmltree::Document::parse(&svg).unwrap();
    let root = svg.root_element();
    assert_eq!(root.attribute("aria-label"), Some(format!("docs: {message}").as_str()));
    assert!(svg.descendants().any(|n| n.attribute("fill") == Some(hex)), "{name}");
    let texts: Vec<_> = svg.descendants().filter(|n| n.has_tag_name("text")).filter_map(|n| n.text()).collect();
    assert_eq!(texts, ["docs", "docs", message.as_str(), message.as_str()]);
    // Wide enough for the message, at about 7 pixels a character.
    let width: usize = root.attribute("width").unwrap().parse().unwrap();
    assert!(width >= 37 + 7 * message.len(), "{width}");
}

/// Every format reports the same items and totals, and documenting one more item never lowers
/// coverage.
#[hegel::test(test_cases = 100)]
fn formats_agree_on_totals(tc: TestCase) {
    let mut files = reports(&tc);
    let totals = Totals::of(&files);
    assert!((0.0..=100.0).contains(&totals.percent()));
    assert_eq!(totals.total, files.iter().map(|f| f.items.len()).sum::<usize>());

    let render = |name, files: &[FileReport]| {
        report::format(name).unwrap().render(&Context { files, failures: &[], now: UNIX_EPOCH, threshold: None })
    };
    // Every format fails when its output doesn't fit where it's going, and only then.
    for format in report::FORMATS {
        let context = Context { files: &files, failures: &[], now: UNIX_EPOCH, threshold: None };
        let length = format.render(&context).len();
        let room = tc.draw(gs::integers::<usize>().max_value(length + 1));
        let result = report::write(&[Output { format, path: None }], &context, &mut Room(room));
        assert_eq!(result.is_err(), room < length, "{} in {room} of {length} bytes", format.name);
    }
    let json: serde_json::Value = serde_json::from_str(&render("json", &files)).unwrap();
    let json_totals = (json["documented"].as_u64(), json["total"].as_u64());
    assert_eq!(json_totals, (Some(totals.documented as u64), Some(totals.total as u64)));
    let kinds = json["files"].as_array().unwrap().iter().flat_map(|f| f["items"].as_array().unwrap());
    assert!(kinds.map(|i| i["kind"].as_str()).eq(files.iter().flat_map(|f| &f.items).map(|i| Some(i.kind.as_str()))));

    let text = render("text", &files);
    let last = format!("{:>6.1}%  {}/{}  total\n", totals.percent(), totals.documented, totals.total);
    assert!(text.ends_with(&last), "{text}");
    // Each undocumented item gets a line, with its location, kind and name.
    let listed: Vec<&str> = text.lines().filter(|l| l.starts_with("         ")).collect();
    let undocumented = files.iter().flat_map(|f| f.items.iter().filter(|i| !i.documented).map(move |i| (f, i)));
    let wanted: Vec<String> = undocumented
        .map(|(f, i)| format!("         {}:{}:1  {} {}", f.path.display(), i.line, i.kind.as_str(), i.name))
        .collect();
    assert_eq!(listed, wanted);

    // lcov counts lines: a line is hit when everything on it is documented.
    let lcov = render("lcov", &files);
    let sum =
        |prefix: &str| lcov.lines().filter_map(|l| l.strip_prefix(prefix)).map(|n| n.parse::<usize>().unwrap()).sum();
    let mut lines = 0;
    let mut hit = 0;
    for file in &files {
        for line in 1..=file.items.iter().map(|i| i.line).max().unwrap_or(0) {
            lines += 1;
            hit += usize::from(file.items.iter().filter(|i| i.line == line).all(|i| i.documented));
        }
    }
    assert_eq!((sum("LH:"), sum("LF:")), (hit, lines));

    if let Some(item) = files.iter_mut().flat_map(|f| &mut f.items).find(|i| !i.documented) {
        item.documented = true;
        assert!(Totals::of(&files).percent() > totals.percent());
    }
}

fn write(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Where a generated project's settings come from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Settings {
    /// docov.json, docov.jsonc or .docovrc, found without a flag, sometimes from a subfolder.
    Default,
    /// Another file, named by `--config`.
    Named,
    /// Flags alone.
    Flags,
    /// `--config` naming a file that isn't there.
    Missing,
    /// docov.json with a misspelled key, or a threshold that isn't a number or is over 100.
    Invalid,
    /// docov.json with an exclude glob that doesn't parse, or is too big to build.
    BadGlob,
}

/// A generated project, with files docov scans and files .gitignore and the config's excludes
/// leave out, settings from a config file or flags (or broken ones), and sometimes a file that
/// doesn't parse. The binary reports the files' totals in every format, and exits 0, 1 or 2 by
/// them.
#[hegel::test(test_cases = 60)]
fn the_cli_counts_the_project_and_fails_by_the_threshold(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    if tc.draw(gs::weighted_booleans(0.05)) {
        let out = Command::new(env!("CARGO_BIN_EXE_docov")).arg("--print-schema").output().unwrap();
        let schema: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!((out.status.code(), schema["title"].as_str()), (Some(0), Some("Config")));
        return;
    }
    let scope = tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::All]));
    let settings = tc.draw(gs::sampled_from(vec![
        Settings::Default,
        Settings::Named,
        Settings::Flags,
        Settings::Missing,
        Settings::Invalid,
        Settings::BadGlob,
    ]));
    let huge_glob = settings == Settings::BadGlob && tc.draw(gs::weighted_booleans(0.2));
    // An invalid docov.json, and what its error names.
    let invalid = (settings == Settings::Invalid).then(|| {
        tc.draw(gs::sampled_from(vec![
            (r#"{ "treshold": 1 }"#, "treshold"),
            (r#"{ "threshold": "80" }"#, "invalid type"),
            (r#"{ "threshold": 150 }"#, "expected a number from 0 to 100"),
        ]))
    });
    // A found config's name, whether it has comments and a trailing comma, and whether docov runs
    // from a subfolder, finding it one folder up.
    let name = tc.draw(gs::sampled_from(config::FILE_NAMES.to_vec()));
    let commented = tc.draw(gs::booleans());
    let from_sub = settings == Settings::Default && tc.draw(gs::booleans());
    let up = if from_sub { "../" } else { "" };
    // Each file's path, documented and total items, and whether .gitignore (0) or the config's
    // excludes (1) leave it out.
    let mut files = Vec::new();
    for f in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let folder = tc.draw(gs::sampled_from(vec!["", "src/", "lib/deep/"]));
        let ext = tc.draw(gs::sampled_from(vec!["ts", "js", "mjs", "cts", "tsx", "jsx"]));
        let path = format!("{folder}f{f}.{ext}");
        let mut text = String::from("function local() {}\n");
        let docs = tc.draw(gs::vecs(gs::booleans()).max_size(4));
        for (i, doc) in docs.iter().enumerate() {
            text.push_str(&format!("{}export function e{i}() {{}}\n", if *doc { "/** Docs. */\n" } else { "" }));
        }
        write(dir.path(), &path, &text);
        let total = docs.len() + usize::from(scope == Scope::All);
        files.push((path, docs.iter().filter(|d| **d).count(), total, tc.draw(gs::integers::<u8>().max_value(3))));
    }
    let left_out = |how: u8| files.iter().filter(|f| f.3 == how).map(|f| f.0.clone()).collect::<Vec<_>>();
    let (ignored, excluded) = (left_out(0), left_out(1));
    // Without a config file there are no excludes.
    let scanned: Vec<_> = files.iter().filter(|f| f.3 >= 2 || (f.3 == 1 && settings == Settings::Flags)).collect();
    write(dir.path(), "README.md", "not code");
    write(dir.path(), ".gitignore", &ignored.join("\n"));
    // A file that can't be scanned: one that doesn't parse, one that isn't UTF-8, or (named on the
    // command line) one that isn't JavaScript.
    let broken = if tc.draw(gs::weighted_booleans(0.2)) {
        Some(tc.draw(gs::sampled_from(vec!["src/broken.ts", "src/latin1.js", "README.md"])))
    } else {
        None
    };
    match broken {
        Some("src/broken.ts") => write(dir.path(), "src/broken.ts", "export function ("),
        Some("src/latin1.js") => {
            fs::create_dir_all(dir.path().join("src")).unwrap();
            fs::write(dir.path().join("src/latin1.js"), b"// caf\xe9\n").unwrap();
        }
        _ => {}
    }

    // Half the thresholds are the project's own percentage, which passes.
    let totals = Totals { documented: scanned.iter().map(|f| f.1).sum(), total: scanned.iter().map(|f| f.2).sum() };
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
    let threshold = threshold.map(|t| if tc.draw(gs::booleans()) { totals.percent() } else { t });
    // No `--format` is text, with GitHub annotations after it in GitHub Actions.
    let format = tc.draw(gs::sampled_from(vec!["text", "json", "lcov", "default"]));
    let github_actions = format == "default" && tc.draw(gs::booleans());
    let mut args = if format == "default" { vec![] } else { vec!["--format".to_owned(), format.into()] };
    let scope_name = if scope == Scope::All { "all" } else { "exports" };
    // `paths` is the config's own folder, the project's, from wherever docov runs.
    let config =
        serde_json::json!({ "paths": ["."], "exclude": excluded, "scope": scope_name, "threshold": threshold });
    let config = config.to_string();
    let config =
        if commented { format!("// docov\n{}, /* end */ }}\n", config.strip_suffix('}').unwrap()) } else { config };
    match settings {
        Settings::Default => write(dir.path(), name, &config),
        Settings::Named => {
            write(dir.path(), "custom.json", &config);
            args.extend(["--config".into(), "custom.json".into()]);
        }
        Settings::Missing => args.extend(["--config".into(), "missing.json".into()]),
        Settings::Invalid => write(dir.path(), name, invalid.unwrap().0),
        Settings::BadGlob => {
            // Big enough that globset can't build it (it fails from about 30,000 alternatives).
            let glob = if huge_glob {
                format!("{{{}}}", (0..40_000).map(|i| format!("x{i}[a-z][0-9]?")).collect::<Vec<_>>().join(","))
            } else {
                "src/[z".into()
            };
            write(dir.path(), "docov.json", &serde_json::json!({ "exclude": [glob] }).to_string());
        }
        Settings::Flags => {
            args.extend(["--scope".into(), scope_name.into()]);
            if let Some(t) = threshold {
                args.extend(["--threshold".into(), t.to_string()]);
            }
        }
    }
    let out_of_range = tc.draw(gs::weighted_booleans(0.1)) && !args.iter().any(|a| a == "--threshold");
    if out_of_range {
        args.extend(["--threshold".into(), "150".into()]);
    }
    // Naming the files scans the same ones, the broken one too.
    let named = tc.draw(gs::booleans());
    if named {
        args.extend(scanned.iter().map(|f| format!("{up}{}", f.0)));
    }
    if let Some(path) = broken.filter(|p| named || *p == "README.md") {
        args.push(format!("{up}{path}"));
    }
    let gone = tc.draw(gs::weighted_booleans(0.1));
    if gone {
        args.push("gone".into());
    }

    let cwd = dir.path().join(if from_sub { "sub" } else { "" });
    fs::create_dir_all(&cwd).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
    command.args(&args).current_dir(cwd);
    if github_actions {
        command.env("GITHUB_ACTIONS", "true");
    } else {
        command.env_remove("GITHUB_ACTIONS");
    }
    // A pipe whose reader has gone (`docov | head` after head exits): writes to it fail.
    let unwritable = tc.draw(gs::weighted_booleans(0.1));
    if unwritable {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        command.stdout(writer);
    }
    let out = command.output().unwrap();
    let (stdout, stderr) = (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap());
    let culprit = match settings {
        _ if out_of_range => Some("150"),
        Settings::Missing => Some("missing.json"),
        Settings::Invalid => invalid.map(|(_, culprit)| culprit),
        Settings::BadGlob => Some(if huge_glob { "NFA" } else { "[z" }),
        _ if gone => Some("gone"),
        _ => broken.map(|p| Path::new(p).file_name().unwrap().to_str().unwrap()),
    };
    if let Some(culprit) = culprit {
        assert_eq!((out.status.code(), stdout.as_str()), (Some(2), ""), "{args:?} {stderr}");
        assert!(stderr.contains(culprit), "{stderr}");
        return;
    }
    // lcov has nothing to write for no files.
    if unwritable && !(format == "lcov" && scanned.is_empty()) {
        assert_eq!(out.status.code(), Some(2), "{stderr}");
        assert!(stderr.starts_with("docov: "), "{stderr}");
        return;
    }
    let (documented, total) = (totals.documented, totals.total);
    let passed = threshold.is_none_or(|t| totals.percent() >= t);
    assert_eq!(out.status.code(), Some(i32::from(!passed)), "{args:?} {stderr}");
    match format {
        "json" => {
            let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            assert_eq!(
                (json["documented"].as_u64(), json["total"].as_u64()),
                (Some(documented as u64), Some(total as u64))
            );
        }
        "lcov" => assert_eq!(stdout.matches("SF:").count(), scanned.len()),
        _ => {
            let (text, annotations) = stdout.split_at(stdout.find("::").unwrap_or(stdout.len()));
            assert!(text.ends_with(&format!("{documented}/{total}  total\n")), "{stdout}");
            let wanted = if github_actions { total - documented + usize::from(!passed) } else { 0 };
            assert_eq!(annotations.lines().count(), wanted, "{stdout}");
        }
    }
}

/// Each kind of bad input on its own, in a generated project that's fine otherwise: docov exits 2
/// without a report, and its error names the culprit. The CLI property above draws these too, but
/// each only now and then, so a derandomized run could miss one; this one reaches them all.
#[hegel::test(test_cases = 10)]
fn each_bad_input_fails_naming_itself(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    for f in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let doc = if tc.draw(gs::booleans()) { "/** Docs. */\n" } else { "" };
        write(dir.path(), &format!("src/f{f}.ts"), &format!("{doc}export const a = 1;\n"));
    }
    write(dir.path(), "README.md", "not code");
    let fails_naming = |args: &[&str], culprit: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_docov")).args(args).current_dir(dir.path()).output().unwrap();
        let (stdout, stderr) = (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap());
        assert_eq!((out.status.code(), stdout.as_str()), (Some(2), ""), "{args:?} {stderr}");
        assert!(stderr.contains(culprit), "{culprit}: {stderr}");
    };
    // Bad arguments, and what the error names.
    let bad_args: [(&[&str], &str); 6] = [
        (&["--config", "missing.json"], "missing.json"),
        (&["--output", "o.txt", "--format", "lcov=x.lcov"], "--output"),
        (&["--format", "json=README.md/x.json"], "README.md"),
        (&["--threshold", "150"], "150"),
        (&["gone"], "gone"),
        (&["README.md"], "README.md"),
    ];
    for (args, culprit) in bad_args {
        fails_naming(args, culprit);
    }
    // Bad files, each removed after its run, and what the error names.
    let config = |value: serde_json::Value| value.to_string().into_bytes();
    // Big enough that globset can't build it (it fails from about 30,000 alternatives).
    let huge_glob = format!("{{{}}}", (0..40_000).map(|i| format!("x{i}[a-z][0-9]?")).collect::<Vec<_>>().join(","));
    let bad_files = [
        ("docov.json", br#"{ "treshold": 1 }"#.to_vec(), "treshold"),
        ("docov.json", br#"{ "threshold": "80" }"#.to_vec(), "invalid type"),
        ("docov.json", br#"{ "threshold": 150 }"#.to_vec(), "expected a number from 0 to 100"),
        ("docov.jsonc", br#"{ "threshold": 5 / }"#.to_vec(), "doesn't start a comment"),
        ("docov.json", config(serde_json::json!({ "exclude": ["src/[z"] })), "[z"),
        ("docov.json", config(serde_json::json!({ "exclude": [huge_glob] })), "NFA"),
        ("src/broken.ts", b"export function (".to_vec(), "broken.ts"),
        ("src/latin1.js", b"// caf\xe9\n".to_vec(), "latin1.js"),
    ];
    for (path, text, culprit) in bad_files {
        fs::write(dir.path().join(path), text).unwrap();
        fails_naming(&[], culprit);
        fs::remove_file(dir.path().join(path)).unwrap();
    }

    // A pipe whose reader has gone (`docov | head` after head exits): every format's write fails.
    for format in ["text", "json", "lcov"] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let out = Command::new(env!("CARGO_BIN_EXE_docov"))
            .args(["--format", format])
            .current_dir(dir.path())
            .stdout(writer)
            .output()
            .unwrap();
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert_eq!(out.status.code(), Some(2), "{format}: {stderr}");
        assert!(stderr.starts_with("docov: "), "{format}: {stderr}");
    }
}

/// A config file that isn't an object, or has a key that isn't a string, a value that isn't JSON or
/// a known key of the wrong type, fails to load, and says where. Loaded in-process: the binary's
/// properties above already run it on bad files, and each run is slow under coverage.
#[hegel::test(test_cases = 20)]
fn malformed_configs_fail_to_load_where_they_break(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let padding = " ".repeat(tc.draw(gs::integers::<usize>().max_value(3)));
    let (text, message, column) = tc.draw(gs::sampled_from(vec![
        ("[]", "invalid type: sequence, expected an object", 0),
        ("{ 1: 2 }", "key must be a string", 3),
        (r#"{ "paths": [1 }"#, "expected `,` or `]`", 15),
        (r#"{ "paths": 1 }"#, "invalid type: integer `1`, expected a sequence", 12),
        (r#"{ "scope": "some" }"#, "unknown variant `some`, expected `exports` or `all`", 17),
    ]));
    let path = dir.path().join("docov.json");
    fs::write(&path, format!("{padding}{text}")).unwrap();
    let column = column + padding.len();
    match config::Config::load(Some(&path)) {
        Err(config::Error::Parse { source, .. }) => {
            assert_eq!(source.to_string(), format!("{message} at line 1 column {column}"));
        }
        other => panic!("{other:?}"),
    }
}

/// A config file loads the same with comments and trailing commas added as without them, strings
/// that look like comments included, and fails to load with a `/` that starts no comment.
#[hegel::test(test_cases = 50)]
fn comments_in_config_files_change_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let exclude = tc.draw(gs::vecs(gs::sampled_from(vec!["*.js", "src/*/x.ts", "a//b", "c/**"])).max_size(3));
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100)));
    let scope = tc.draw(gs::sampled_from(vec!["exports", "all"]));
    let json = serde_json::json!({ "exclude": exclude, "threshold": threshold, "scope": scope });
    let plain = serde_json::to_string_pretty(&json).unwrap();
    let load = |text: &str| {
        let path = dir.path().join("docov.jsonc");
        fs::write(&path, text).unwrap();
        config::Config::load(Some(&path))
    };

    let lines: Vec<_> = plain.lines().collect();
    let mut commented = String::new();
    for (i, line) in lines.iter().enumerate() {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => commented.push_str("// a note\n"),
            1 => commented.push_str("/* a note */ "),
            _ => {}
        }
        commented.push_str(line);
        let closes_next = lines.get(i + 1).is_some_and(|next| next.trim_start().starts_with(['}', ']']));
        if closes_next && !line.ends_with(['{', '[']) && tc.draw(gs::booleans()) {
            commented.push(',');
        }
        commented.push('\n');
    }
    assert_eq!(load(&commented).unwrap(), load(&plain).unwrap(), "{commented}");

    // A `/` before a line, where nothing is in a string.
    let at = tc.draw(gs::integers::<usize>().max_value(lines.len() - 1));
    let stray: Vec<_> =
        lines.iter().enumerate().map(|(i, l)| if i == at { format!("/ {l}") } else { (*l).into() }).collect();
    assert!(matches!(load(&stray.join("\n")), Err(config::Error::Slash { .. })));
}
