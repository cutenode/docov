//! Properties over generated modules, projects and reports.

use std::{fs, path::Path, path::PathBuf, process::Command};

use docov::{FileReport, Format, Item, Kind, Scope, Totals, collect, config, report, scan, scan_file};
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

fn reports(tc: &TestCase) -> Vec<FileReport> {
    let item = gs::vecs(gs::integers::<usize>().max_value(ALL_KINDS.len() - 1)).min_size(2).max_size(2);
    let files = tc.draw(gs::vecs(gs::vecs(item).max_size(6)).max_size(4));
    files
        .into_iter()
        .enumerate()
        .map(|(f, items)| FileReport {
            path: PathBuf::from(format!("f{f}.ts")),
            items: items
                .into_iter()
                .enumerate()
                .map(|(i, drawn)| Item {
                    name: format!("n{i}"),
                    kind: ALL_KINDS[drawn[0]],
                    line: i / 2 + 1,
                    column: 1,
                    documented: drawn[1] % 2 == 0,
                })
                .collect(),
        })
        .collect()
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

/// Every format reports the same items and totals, and documenting one more item never lowers
/// coverage.
#[hegel::test(test_cases = 100)]
fn formats_agree_on_totals(tc: TestCase) {
    let mut files = reports(&tc);
    let totals = Totals::of(&files);
    assert!((0.0..=100.0).contains(&totals.percent()));
    assert_eq!(totals.total, files.iter().map(|f| f.items.len()).sum::<usize>());

    let render = |format, files: &[FileReport]| {
        let mut out = Vec::new();
        report::write(format, files, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    // Every format fails when its output doesn't fit where it's going, and only then.
    for format in [Format::Text, Format::Json, Format::Lcov] {
        let length = render(format, &files).len();
        let room = tc.draw(gs::integers::<usize>().max_value(length + 1));
        let result = report::write(format, &files, &mut Room(room));
        assert_eq!(result.is_err(), room < length, "{format:?} in {room} of {length} bytes");
    }
    let json: serde_json::Value = serde_json::from_str(&render(Format::Json, &files)).unwrap();
    let json_totals = (json["documented"].as_u64(), json["total"].as_u64());
    assert_eq!(json_totals, (Some(totals.documented as u64), Some(totals.total as u64)));
    let kinds = json["files"].as_array().unwrap().iter().flat_map(|f| f["items"].as_array().unwrap());
    assert!(kinds.map(|i| i["kind"].as_str()).eq(files.iter().flat_map(|f| &f.items).map(|i| Some(i.kind.as_str()))));

    let text = render(Format::Text, &files);
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
    let lcov = render(Format::Lcov, &files);
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
    let format = tc.draw(gs::sampled_from(vec!["text", "json", "lcov"]));
    let mut args = vec!["--format".to_owned(), format.into()];
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
        _ => assert!(stdout.ends_with(&format!("{documented}/{total}  total\n")), "{stdout}"),
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
    let bad_args: [(&[&str], &str); 4] = [
        (&["--config", "missing.json"], "missing.json"),
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
#[hegel::test(test_cases = 60)]
fn malformed_configs_fail_to_load_where_they_break(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let padding = " ".repeat(tc.draw(gs::integers::<usize>().max_value(3)));
    let (text, message, column) = tc.draw(gs::sampled_from(vec![
        ("[]", "invalid type: sequence, expected an object", 0),
        ("{ 1: 2 }", "key must be a string", 3),
        (r#"{ "paths": [1 }"#, "expected `,` or `]`", 15),
        (r#"{ "paths": 1 }"#, "invalid type: integer `1`, expected a sequence", 12),
        (r#"{ "scope": "some" }"#, "unknown variant `some`, expected `exports` or `all`", 17),
        (r#"{ "threshold": { "global": "80" } }"#, "invalid type: string \"80\", expected f64", 31),
        (
            r#"{ "overrides": [{ "files": ["a"], "scope": "all" }] }"#,
            "unknown field `scope`, expected one of `files`, `exclude`, `threshold`",
            41,
        ),
        (r#"{ "overrides": [{ "exclude": [] }] }"#, "missing field `files`", 33),
        (
            r#"{ "overrides": [{ "files": ["[z"] }] }"#,
            "error parsing glob '[z': unclosed character class; missing ']'",
            35,
        ),
        (r#"{ "threshold": true }"#, "invalid type: boolean `true`, expected a number, or an object of them", 19),
        (r#"{ "threshold": -0.5 }"#, "expected a number from 0 to 100, or a negative whole number", 19),
        (r#"{ "threshold": { "perFile": 101 } }"#, "expected a number from 0 to 100, or a negative whole number", 33),
        (r#"{ "threshold": { "[z": 1 } }"#, "error parsing glob '[z': unclosed character class; missing ']'", 26),
    ]));
    let path = dir.path().join("docov.json");
    // A threshold's glob too big to build, like the walk's (globset fails from about 30,000
    // alternatives).
    if tc.draw(gs::weighted_booleans(0.1)) {
        let huge = format!("{{{}}}", (0..40_000).map(|i| format!("x{i}[a-z][0-9]?")).collect::<Vec<_>>().join(","));
        fs::write(&path, serde_json::json!({ "threshold": { huge: 1 } }).to_string()).unwrap();
        let error = config::Config::load(Some(&path)).unwrap_err().to_string();
        assert!(error.contains("NFA"), "{error}");
    }
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
    // A threshold by path, with a glob that compares by its text.
    let threshold = match threshold {
        Some(t) if tc.draw(gs::booleans()) => serde_json::json!({ "src/**": t, "perFile": -1 }),
        t => serde_json::json!(t),
    };
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

/// Settings from `config` (docov.json's text) and `args`, made in-process.
fn settings(dir: &Path, config: &str, args: &[String]) -> docov::Settings {
    let path = dir.join("docov.json");
    fs::write(&path, config).unwrap();
    load_settings(&path, args)
}

/// Settings from the config file at `path` and `args`, made in-process.
fn load_settings(path: &Path, args: &[String]) -> docov::Settings {
    let config = config::Config::load(Some(path)).unwrap();
    let matches = docov::feature::args(clap::Command::new("docov"))
        .try_get_matches_from(std::iter::once("docov".to_owned()).chain(args.iter().cloned()));
    docov::Settings::new(&matches.unwrap(), config.keys)
}

/// A threshold from docov.json: a percentage, or a negative number for the most undocumented items.
fn limit(tc: &TestCase) -> i64 {
    if tc.draw(gs::booleans()) {
        tc.draw(gs::integers::<i64>().min_value(0).max_value(100))
    } else {
        -tc.draw(gs::integers::<i64>().min_value(0).max_value(4))
    }
}

/// Why `totals` fails `limit`, as the model has it.
fn fails(limit: i64, totals: Totals, what: &str) -> Option<String> {
    let undocumented = totals.total - totals.documented;
    if limit >= 0 {
        let percent = totals.percent();
        (percent < limit as f64).then(|| format!("{percent:.1}% documented, under the {limit}% {what}"))
    } else {
        let most = (-limit) as usize;
        (undocumented > most).then(|| format!("{undocumented} undocumented, over the {what} of {most}"))
    }
}

/// Which of `reports`' files (`f<i>.ts`, by `i`) a glob matches.
type Matches = fn(usize) -> bool;

/// Globs a threshold can name, and which of `reports`' files each matches.
const GLOBS: [(&str, Matches); 5] =
    [("f0.ts", |i| i == 0), ("f[01].ts", |i| i < 2), ("*.ts", |_| true), ("/f3*", |i| i == 3), ("nothing/", |_| false)];

/// Drawn thresholds: global, per file, and by glob (an index into [`GLOBS`]).
struct Drawn {
    global: Option<i64>,
    per_file: Option<i64>,
    globs: Vec<(usize, i64)>,
}

impl Drawn {
    fn draw(tc: &TestCase) -> Self {
        let global = tc.draw(gs::optional(gs::just(()))).map(|()| limit(tc));
        let per_file = tc.draw(gs::optional(gs::just(()))).map(|()| limit(tc));
        let globs = tc.draw(gs::vecs(gs::integers::<usize>().max_value(GLOBS.len() - 1)).max_size(3).unique(true));
        Self { global, per_file, globs: globs.into_iter().map(|g| (g, limit(tc))).collect() }
    }

    /// As docov.json has it, written by hand: serde_json's maps sort their keys, and the globs'
    /// order is the file's. Numbers are sometimes floats, which JSON readers see apart from
    /// integers, and a lone global threshold is sometimes a plain number.
    fn json(&self, tc: &TestCase) -> String {
        let number = |n: i64| if tc.draw(gs::booleans()) { format!("{n}.0") } else { n.to_string() };
        let mut entries: Vec<String> = Vec::new();
        entries.extend(self.global.map(|g| format!(r#""global": {}"#, number(g))));
        entries.extend(self.per_file.map(|p| format!(r#""perFile": {}"#, number(p))));
        entries.extend(self.globs.iter().map(|(g, l)| format!("{}: {}", serde_json::json!(GLOBS[*g].0), number(*l))));
        match self.global {
            Some(g) if entries.len() == 1 && tc.draw(gs::booleans()) => number(g),
            _ => format!("{{ {} }}", entries.join(", ")),
        }
    }

    /// Why the `members` of `files` fail them, as the model has it, naming the group `name`.
    fn fails(&self, files: &[FileReport], members: &[usize], name: Option<&str>) -> Vec<String> {
        let sum = |matched: &dyn Fn(usize) -> bool| -> Totals {
            members.iter().filter(|i| matched(**i)).map(|i| files[*i].totals()).sum()
        };
        let mut wanted = Vec::new();
        let global = self.global.and_then(|g| fails(g, sum(&|_| true), "threshold"));
        wanted.extend(global.map(|m| name.map_or(m.clone(), |name| format!("{name}: {m}"))));
        for i in self.per_file.iter().flat_map(|_| members) {
            let failure = fails(self.per_file.unwrap(), files[*i].totals(), "per-file threshold");
            wanted.extend(failure.map(|m| format!("{}: {m}", files[*i].path.display())));
        }
        for (g, l) in &self.globs {
            wanted.extend(fails(*l, sum(&GLOBS[*g].1), "threshold").map(|m| format!("{}: {m}", GLOBS[*g].0)));
        }
        wanted
    }
}

/// Thresholds by file and by glob, overall or a most undocumented items: each fails on its own
/// line exactly when the files it covers don't meet it, the global one first, then each file's,
/// then each glob's. `--threshold` replaces the global one. An override's thresholds are for the
/// files it's the last override with a threshold for, and the top-level ones for the rest.
#[hegel::test(test_cases = 100)]
fn thresholds_fail_by_file_and_glob(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let files = reports(&tc);
    let mut top = Drawn::draw(&tc);
    let overrides: Vec<(usize, Option<Drawn>)> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|_| {
            (
                tc.draw(gs::integers::<usize>().max_value(GLOBS.len() - 1)),
                tc.draw(gs::optional(gs::just(()))).map(|()| Drawn::draw(&tc)),
            )
        })
        .collect();
    let flag = tc.draw(gs::optional(gs::integers::<u8>().max_value(100)));

    let overrides_json: Vec<String> = overrides
        .iter()
        .map(|(files, t)| {
            let threshold = t.as_ref().map(|t| format!(r#", "threshold": {}"#, t.json(&tc))).unwrap_or_default();
            format!(r#"{{ "files": [{}]{threshold} }}"#, serde_json::json!(GLOBS[*files].0))
        })
        .collect();
    let config = format!(r#"{{ "threshold": {}, "overrides": [{}] }}"#, top.json(&tc), overrides_json.join(", "));
    let args: Vec<String> = flag.map(|f| vec!["--threshold".into(), f.to_string()]).unwrap_or_default();
    let settings = settings(dir.path(), &config, &args);

    top.global = flag.map(i64::from).or(top.global);
    let owner = |i: usize| overrides.iter().rposition(|(files, t)| t.is_some() && GLOBS[*files].1(i));
    let members = |o: Option<usize>| (0..files.len()).filter(|i| owner(*i) == o).collect::<Vec<_>>();
    let mut wanted = top.fails(&files, &members(None), None);
    for (o, (globs, t)) in overrides.iter().enumerate() {
        wanted.extend(t.iter().flat_map(|t| t.fails(&files, &members(Some(o)), Some(GLOBS[*globs].0))));
    }
    assert_eq!(settings.gates(&files, &docov::root::Root::new(Path::new(""))), wanted, "{config}");
}

/// Whether a glob matches a path, a folder when the flag says so.
type MatchesPath = fn(&str, bool) -> bool;

/// Globs the walk's excludes and overrides can have, and which paths each matches (a folder when
/// the flag says so), as gitignore has it.
const WALK_GLOBS: [(&str, MatchesPath); 7] = [
    ("*.js", |p, _| p.ends_with(".js")),
    ("*.ts", |p, _| p.ends_with(".ts")),
    ("a", |p, _| p == "a"),
    ("a/**", |p, _| p.starts_with("a/")),
    ("a/b/", |p, dir| dir && p == "a/b"),
    ("a/b/**", |p, _| p.starts_with("a/b/")),
    ("c/x.ts", |p, _| p == "c/x.ts"),
];

/// The walk leaves out what the top-level `exclude` does, except where the last override for a
/// file or folder with an `exclude` of its own says otherwise, and never goes into a folder it
/// leaves out.
#[hegel::test(test_cases = 100)]
fn overrides_exclude_by_the_last_match(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let paths = ["a/x.ts", "a/y.js", "a/b/x.ts", "a/b/y.js", "c/x.ts", "c/y.js", "x.ts", "y.js"];
    for path in paths {
        write(dir.path(), path, "");
    }
    let globs = |tc: &TestCase| tc.draw(gs::vecs(gs::integers::<usize>().max_value(WALK_GLOBS.len() - 1)).max_size(2));
    let exclude = globs(&tc);
    let overrides: Vec<(Vec<usize>, Option<Vec<usize>>)> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|_| (globs(&tc), tc.draw(gs::optional(gs::just(()))).map(|()| globs(&tc))))
        .collect();

    let text = |globs: &[usize]| globs.iter().map(|&g| WALK_GLOBS[g].0.to_owned()).collect::<Vec<_>>();
    let json = serde_json::json!({
        "overrides": overrides.iter().map(|(files, exclude)| match exclude {
            Some(exclude) => serde_json::json!({ "files": text(files), "exclude": text(exclude) }),
            None => serde_json::json!({ "files": text(files) }),
        }).collect::<Vec<_>>(),
    });
    let settings = settings(dir.path(), &json.to_string(), &[]);
    let root = docov::root::Root::new(dir.path());
    let overrides_setting = settings.get::<docov::feature::overrides::Overrides>().clone();
    let excluder = overrides_setting.excluder(docov::glob::Globs::new(text(&exclude)).unwrap(), root);
    let found = docov::walk::files(&[dir.path().to_path_buf()], excluder).unwrap();
    let found: Vec<String> =
        found.iter().map(|p| p.strip_prefix(dir.path()).unwrap().to_str().unwrap().replace('\\', "/")).collect();

    let matches = |globs: &[usize], path: &str, is_dir: bool| globs.iter().any(|&g| WALK_GLOBS[g].1(path, is_dir));
    // An override is for a path when its globs match the path or a folder it's in.
    let contain = |globs: &[usize], path: &str, is_dir: bool| {
        matches(globs, path, is_dir) || path.match_indices('/').any(|(i, _)| matches(globs, &path[..i], true))
    };
    let excluded = |path: &str, is_dir: bool| {
        let own = overrides.iter().rev().find_map(|(files, e)| e.as_ref().filter(|_| contain(files, path, is_dir)));
        matches(own.unwrap_or(&exclude), path, is_dir)
    };
    let mut wanted: Vec<String> = paths
        .iter()
        .filter(|path| {
            let folders = path.match_indices('/').map(|(i, _)| &path[..i]);
            !folders.into_iter().any(|f| excluded(f, true)) && !excluded(path, false)
        })
        .map(|p| (*p).to_owned())
        .collect();
    wanted.sort();
    assert_eq!(found, wanted, "{json} {exclude:?}");
}

/// `--max-undocumented` and `maxUndocumented` fail a scan with more undocumented items than they
/// allow, the flag winning, on a line of their own.
#[hegel::test(test_cases = 50)]
fn max_undocumented_caps_the_undocumented_items(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let files = reports(&tc);
    let key = tc.draw(gs::optional(gs::integers::<usize>().max_value(6)));
    let flag = tc.draw(gs::optional(gs::integers::<usize>().max_value(6)));
    let config = serde_json::json!({ "maxUndocumented": key }).to_string();
    let args: Vec<String> = flag.map(|f| vec!["--max-undocumented".into(), f.to_string()]).unwrap_or_default();
    let settings = settings(dir.path(), &config, &args);
    let totals = Totals::of(&files);
    let undocumented = totals.total - totals.documented;
    let wanted: Vec<String> = flag
        .or(key)
        .filter(|most| undocumented > *most)
        .map(|most| format!("{undocumented} undocumented, over the limit of {most}"))
        .into_iter()
        .collect();
    assert_eq!(settings.gates(&files, &docov::root::Root::new(Path::new(""))), wanted);
}

/// A baseline lets through the undocumented items it lists, each entry once, wherever their lines
/// moved to; what --write-baseline writes lets through every undocumented item there was. A
/// baseline that's missing or isn't one fails to load, naming it.
#[hegel::test(test_cases = 100)]
fn baselines_let_their_items_through(tc: TestCase) {
    use docov::feature::baseline::Baseline;
    let dir = tempfile::tempdir().unwrap();
    let root = docov::root::Root::new(dir.path());
    let mut files = reports(&tc);
    for file in &mut files {
        file.path = dir.path().join("src").join(&file.path);
        // Few names, so some items share their path, kind and name (`default` can).
        for item in &mut file.items {
            item.name = format!("n{}", tc.draw(gs::integers::<u8>().max_value(1)));
        }
    }
    let undocumented: Vec<(String, &Item)> = files
        .iter()
        .flat_map(|f| {
            f.items
                .iter()
                .filter(|i| !i.documented)
                .map(move |i| (format!("src/{}", f.path.file_name().unwrap().to_str().unwrap()), i))
        })
        .collect();
    // Entries for some of them, some twice, and one for an item that isn't there.
    let picked = tc.draw(gs::vecs(gs::integers::<usize>().max_value(undocumented.len().max(1) - 1)).max_size(6));
    let picked: Vec<usize> = picked.into_iter().filter(|&i| i < undocumented.len()).collect();
    let mut items: Vec<serde_json::Value> = picked
        .iter()
        .map(|&i| serde_json::json!({ "path": undocumented[i].0, "kind": undocumented[i].1.kind.as_str(), "name": undocumented[i].1.name }))
        .collect();
    items.push(serde_json::json!({ "path": "src/gone.ts", "kind": "function", "name": "gone" }));
    fs::write(dir.path().join("known.json"), serde_json::json!({ "items": items }).to_string()).unwrap();

    let from_key = tc.draw(gs::booleans());
    let (config, args) = if from_key {
        (r#"{ "baseline": "known.json" }"#.to_owned(), vec![])
    } else {
        ("{}".to_owned(), vec!["--baseline".to_owned(), dir.path().join("known.json").display().to_string()])
    };
    let named = settings(dir.path(), &config, &args);
    let excused = named.get::<Baseline>().read(dir.path()).unwrap().unwrap().apply(&root, files.clone());

    // Each entry lets one item through: the first undocumented one with its path, kind and name.
    let mut wanted = files.clone();
    let mut left: Vec<usize> = picked.clone();
    for file in &mut wanted {
        let path = format!("src/{}", file.path.file_name().unwrap().to_str().unwrap());
        for item in file.items.iter_mut().filter(|i| !i.documented) {
            let same = |&i: &usize| {
                undocumented[i].0 == path && undocumented[i].1.kind == item.kind && undocumented[i].1.name == item.name
            };
            if let Some(at) = left.iter().position(same) {
                left.remove(at);
                item.documented = true;
            }
        }
    }
    assert_eq!(excused, wanted);

    // Writing one, then reading it back, lets every undocumented item through, lines moved or not.
    let write = settings(dir.path(), "{}", &["--write-baseline".to_owned()]);
    let written = write.get::<Baseline>();
    assert!(written.read(dir.path()).unwrap().is_none());
    let path = written.save(dir.path(), &root, &files).unwrap();
    assert_eq!(path, dir.path().join("docov-baseline.json"));
    let read = settings(dir.path(), r#"{ "baseline": "docov-baseline.json" }"#, &[]);
    let mut moved = files.clone();
    for item in moved.iter_mut().flat_map(|f| &mut f.items) {
        item.line += tc.draw(gs::integers::<usize>().max_value(3));
    }
    let excused = read.get::<Baseline>().read(dir.path()).unwrap().unwrap().apply(&root, moved);
    assert!(excused.iter().flat_map(|f| &f.items).all(|i| i.documented));

    // A missing or malformed baseline names itself.
    let broken = tc.draw(gs::sampled_from(vec![None, Some("[]"), Some(r#"{ "items": [{ "path": "a" }] }"#)]));
    match broken {
        Some(text) => fs::write(dir.path().join("docov-baseline.json"), text).unwrap(),
        None => fs::remove_file(dir.path().join("docov-baseline.json")).unwrap(),
    }
    let error = read.get::<Baseline>().read(dir.path()).unwrap_err();
    assert!(error.starts_with(&format!("{}: ", dir.path().join("docov-baseline.json").display())), "{error}");
    // Somewhere it can't be written fails the same way.
    let unwritable = settings(
        dir.path(),
        "{}",
        &["--write-baseline".to_owned(), "--baseline".to_owned(), dir.path().join("gone/b.json").display().to_string()],
    );
    let error = unwritable.get::<Baseline>().save(dir.path(), &root, &files).unwrap_err();
    assert!(error.contains("b.json: "), "{error}");
}

/// Through the binary: --write-baseline writes every undocumented item and passes, and with that
/// baseline the gates pass until a new undocumented item comes along, though the report still
/// lists the old ones. `--print-schema baseline` prints the baseline file's schema.
#[hegel::test(test_cases = 5)]
fn the_cli_writes_a_baseline_and_gates_by_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_docov")).args(args).current_dir(dir.path()).output().unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };
    let undocumented = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let text: String = (0..undocumented).map(|i| format!("export const u{i} = 1;\n")).collect();
    write(dir.path(), "src/a.ts", &format!("/** Docs. */\nexport const d = 1;\n{text}"));
    let in_config = tc.draw(gs::booleans());
    if in_config {
        write(dir.path(), "docov.json", r#"{ "baseline": "known.json", "maxUndocumented": 0 }"#);
    }
    let named: &[&str] = if in_config { &[] } else { &["--baseline", "known.json", "--max-undocumented", "0"] };

    let (code, out, err) = run(&[&["--write-baseline"], named].concat());
    assert_eq!((code, err.as_str()), (0, "docov: wrote the undocumented items to known.json\n"));
    let known: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("known.json")).unwrap()).unwrap();
    assert_eq!(known["items"].as_array().unwrap().len(), undocumented);
    let (code, again, err) = run(named);
    assert_eq!((code, err.as_str(), again), (0, "", out));

    // A line above everything moves the items, and one more undocumented item fails.
    write(
        dir.path(),
        "src/a.ts",
        &format!("// moved\n/** Docs. */\nexport const d = 1;\n{text}export const fresh = 1;\n"),
    );
    let (code, _, err) = run(named);
    assert_eq!((code, err.as_str()), (1, "docov: 1 undocumented, over the limit of 0\n"));

    // --update-threshold writes the documented percentage into docov.json, or a new one.
    let (code, _, err) = run(&[&["--update-threshold"], named].concat());
    let tenths = 1000 / (undocumented + 2);
    let percent = (tenths as f64 / 10.0).to_string();
    let raised = format!("docov: raised the threshold to {percent}% in docov.json\n");
    assert_eq!((code, err), (1, format!("{raised}docov: 1 undocumented, over the limit of 0\n")));
    let config = fs::read_to_string(dir.path().join("docov.json")).unwrap();
    assert!(config.contains(&format!(r#""threshold": {percent}"#)), "{config}");

    let (code, schema, _) = run(&["--print-schema", "baseline"]);
    let schema: serde_json::Value = serde_json::from_str(&schema).unwrap();
    assert_eq!((code, schema["title"].as_str()), (0, Some("Baseline")));
    // Somewhere it can't be written fails after the report.
    let (code, _, err) = run(&["--write-baseline", "--baseline", "gone/known.json"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: gone/known.json: "), "{err}");
    // One that can't be read fails before the scan.
    fs::remove_file(dir.path().join("known.json")).unwrap();
    let (code, out, err) = run(named);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.starts_with("docov: known.json: "), "{err}");

    // A config it can't write fails naming it.
    let config = dir.path().join("docov.json");
    fs::write(&config, "{}").unwrap();
    let mut permissions = fs::metadata(&config).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&config, permissions).unwrap();
    let (code, _, err) = run(&["--update-threshold"]);
    unlock(&config);
    assert_eq!(code, 2);
    assert!(err.starts_with("docov: docov.json: "), "{err}");
}

/// Makes a read-only file writable again.
fn unlock(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    #[expect(clippy::permissions_set_readonly_false, reason = "only to delete it")]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}

/// A threshold already in docov.json, before --update-threshold.
#[derive(Clone, Copy, Debug)]
enum Current {
    Missing,
    Null,
    Percent(u16),
    Undocumented(u8),
    /// An object of thresholds, with or without `global`.
    Object(Option<u16>),
}

/// --update-threshold raises the global threshold, a number or an object's `global`, to the
/// documented percentage rounded down to a tenth, only when that's higher, leaving the rest of the
/// file as written. A count of undocumented items stays. Without a config it writes docov.json
/// where docov runs, and a config it can't write fails naming it.
#[hegel::test(test_cases = 100)]
fn update_threshold_only_raises(tc: TestCase) {
    use docov::feature::{
        threshold::{Limit, Threshold},
        update_threshold::UpdateThreshold,
    };
    let dir = tempfile::tempdir().unwrap();
    let total = tc.draw(gs::integers::<usize>().max_value(20));
    let totals = Totals { documented: tc.draw(gs::integers::<usize>().max_value(total)), total };
    let tenths = (totals.documented * 1000).checked_div(total).unwrap_or(1000);
    let percent = tenths as f64 / 10.0;
    // In tenths of a percent, near the scan's half the time.
    let tenth = |tc: &TestCase| {
        if tc.draw(gs::booleans()) {
            tc.draw(gs::integers::<u16>().max_value(1000))
        } else {
            tenths.saturating_sub(1).max(1) as u16 + tc.draw(gs::integers::<u16>().max_value(1))
        }
    };
    let current = match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => Current::Missing,
        1 => Current::Null,
        2 => Current::Percent(tenth(&tc).min(1000)),
        3 => Current::Undocumented(tc.draw(gs::integers::<u8>().min_value(1).max_value(5))),
        _ => Current::Object(tc.draw(gs::optional(gs::just(()))).map(|()| tenth(&tc).min(1000))),
    };
    let number = |t: u16| format!("{}", f64::from(t) / 10.0);
    let threshold = match current {
        Current::Missing => String::new(),
        Current::Null => r#""threshold": null, "#.into(),
        Current::Percent(t) => format!(r#""threshold": {}, "#, number(t)),
        Current::Undocumented(n) => format!(r#""threshold": -{n}, "#),
        Current::Object(global) => {
            let global = global.map(|t| format!(r#""global": {}, "#, number(t))).unwrap_or_default();
            format!(r#""threshold": {{ {global}"src/**": 50 }}, "#)
        }
    };
    let text = format!("// keep\n{{\n  {threshold}/* note */ \"scope\": \"all\",\n}}\n");
    let path = dir.path().join("docov.jsonc");
    fs::write(&path, &text).unwrap();
    let update = load_settings(&path, &["--update-threshold".to_owned()]);
    let update = *update.get::<UpdateThreshold>();
    let raised = update.update(Some(&path), totals).unwrap();

    let before = match current {
        Current::Percent(t) | Current::Object(Some(t)) => Some(f64::from(t) / 10.0),
        Current::Undocumented(_) => Some(f64::INFINITY),
        _ => None,
    };
    let raises = before.is_none_or(|before| before < percent);
    assert_eq!(raised.is_some(), raises, "{text}");
    let after = fs::read_to_string(&path).unwrap();
    assert!(
        after.contains("// keep") && after.contains("/* note */") && after.contains("\"scope\": \"all\""),
        "{after}"
    );
    let global = load_settings(&path, &[]).get::<Threshold>().0.global;
    let wanted = match current {
        _ if raises => Some(Limit::Percent(percent)),
        Current::Undocumented(n) => Some(Limit::Undocumented(n.into())),
        _ => before.map(Limit::Percent),
    };
    assert_eq!(global, wanted, "{after}");
    // Asking again changes nothing.
    assert_eq!(update.update(Some(&path), totals).unwrap(), None);
    assert_eq!(*load_settings(&path, &[]).get::<UpdateThreshold>(), UpdateThreshold(false));
    assert_eq!(UpdateThreshold(false).update(Some(&path), Totals::default()).unwrap(), None);

    // A read-only config fails naming it.
    let locked = dir.path().join("locked.json");
    fs::write(&locked, "{}").unwrap();
    let mut permissions = fs::metadata(&locked).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&locked, permissions).unwrap();
    let error = update.update(Some(&locked), totals).unwrap_err();
    assert!(error.starts_with(&format!("{}: ", locked.display())), "{error}");
    // So Windows can delete it.
    unlock(&locked);

    // Without a config, a new docov.json where docov runs.
    std::env::set_current_dir(dir.path()).unwrap();
    let message = update.update(None, totals).unwrap().unwrap();
    assert_eq!(message, format!("raised the threshold to {percent}% in docov.json"));
    assert_eq!(
        fs::read_to_string(dir.path().join("docov.json")).unwrap(),
        format!("{{\n  \"threshold\": {percent}\n}}\n")
    );
}
