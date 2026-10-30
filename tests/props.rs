//! Properties over generated modules, projects and reports.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, UNIX_EPOCH},
};

use docov::feature::{
    Feature, access::Access, add_ignore::AddIgnore, commonjs_members::CommonjsMembers, complete::Complete,
    examples::Examples, exports::Exports, file_docs::FileDocs, inherit::Inherit, members::Members,
    release_tags::ReleaseTags, report_unused_ignores::ReportUnusedIgnores, rules::DocChecks, sort::Sort, types::Types,
    virtual_types::VirtualTypes,
};
use docov::{
    Context, FileReport, Item, Kind, Output, Rules, Scope, Totals, check, collect, collect::Facts, collect::Ignore,
    config, feature, report, rewrite, scan, scan_file,
};
use hegel::{TestCase, generators as gs};
use oxc_span::SourceType;

#[path = "props/combined.rs"]
mod combined;
#[path = "props/config_cli.rs"]
mod config_cli;
#[path = "props/fix.rs"]
mod fix;
#[path = "props/lsp.rs"]
mod lsp;

/// The docov binary, without the environment it reads to change its output: GitHub Actions'
/// annotations and color. Properties that want them set them again.
fn docov_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
    for var in ["GITHUB_ACTIONS", "NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE"] {
        command.env_remove(var);
    }
    command
}

/// A generated module, and the items a scan of it should find.
#[derive(Default)]
struct Module {
    source: String,
    /// Each item the module declares.
    expected: Vec<Expected>,
    specifiers: Vec<String>,
    has_default: bool,
    /// The `docov-ignore-next-line` just written, for the item on the line after it.
    next_ignore: Option<Ignore>,
    /// The module's `docov-ignore-file`.
    file_ignore: Option<Ignore>,
    /// Whether its first comment says it's `@generated`.
    generated: bool,
    /// How many declarations and blocks it has that a scan passes over.
    skipped: usize,
    /// Whether the comments just written end with a JSDoc block, empty or not, for the item after
    /// them, which then gets no stub.
    block: bool,
}

/// `docov-ignore-next-line` comments and comments that aren't quite, with the reason each one
/// gives, or `None` for those that aren't one.
const NEXT_LINE_IGNORES: [(&str, Option<Option<&str>>); 6] = [
    ("// docov-ignore-next-line", Some(None)),
    ("// docov-ignore-next-line: legacy API", Some(Some("legacy API"))),
    ("/* docov-ignore-next-line:   */", Some(None)),
    ("//docov-ignore-next-line : later", Some(Some("later"))),
    ("// docov-ignore-next-lines", None),
    ("// docov-ignore-next-line legacy", None),
];

/// The same for `docov-ignore-file`, which counts only outside every statement.
const FILE_IGNORES: [(&str, Option<Option<&str>>); 4] = [
    ("/* docov-ignore-file */", Some(None)),
    ("// docov-ignore-file: generated", Some(Some("generated"))),
    ("if (x) { /* docov-ignore-file */ }", None),
    ("/* docov-ignore-files */", None),
];

/// An item a generated module declares.
struct Expected {
    /// How the module exports it, if it does.
    via: Option<Via>,
    /// Whether its export declares it (`exports.a = 1`), rather than exporting a declaration.
    value: bool,
    /// The item, documented when it has a JSDoc block.
    item: Item,
    /// Its JSDoc block.
    doc: Option<Block>,
    /// Whether it's a TypeScript `protected` member.
    protected: bool,
    /// Whether it's a TypeScript `override` member.
    overrides: bool,
    /// For a function or method, its parameters' names.
    params: Option<&'static [&'static str]>,
    /// Whether a JSDoc block, empty or not, sits right before it.
    block: bool,
    /// Whether it's a function whose body returns a value.
    returns: bool,
}

impl Expected {
    /// The item, if it counts by `rules`.
    fn by(&self, rules: &Rules) -> Option<Item> {
        let counts = |via| match via {
            Via::Esm => rules.exports.esm,
            Via::Cjs => rules.exports.cjs,
            Via::Global => rules.exports.globals,
        };
        if self.value && self.via == Some(Via::Global) && !rules.exports.globals {
            return None;
        }
        if rules.scope == Scope::Exports && !self.via.is_some_and(counts) {
            return None;
        }
        let tags = self.doc.map_or(&[][..], |b| b.tags);
        if rules.ignore_deprecated && tags.contains(&"deprecated") {
            return None;
        }
        if tags.iter().any(|tag| rules.exclude_tags.iter().any(|t| t == tag)) {
            return None;
        }
        if rules.kinds.as_ref().is_some_and(|kinds| !kinds.contains(&self.item.kind)) {
            return None;
        }
        let member = matches!(self.item.kind, Kind::Method | Kind::Accessor | Kind::Property);
        let own_name = self.item.name.split_once('.').filter(|_| member).map(|(_, name)| name);
        let level =
            self.doc.and_then(|b| b.access).unwrap_or(if self.protected { Access::Protected } else { Access::Public });
        if rules.access.as_ref().is_some_and(|levels| !levels.contains(&level)) {
            return None;
        }
        if rules.ignore_names.iter().any(|re| re.is_match(&self.item.name) || own_name.is_some_and(|n| re.is_match(n)))
        {
            return None;
        }
        let described = |b: Block| if self.item.kind == Kind::Class { b.class_described } else { b.described };
        let inherits = rules.inherit == Inherit::Override && self.overrides;
        let documented = inherits || self.doc.is_some_and(|b| !rules.require_description || described(b));
        let doc = self.doc;
        let facts = Facts {
            has_description: doc.is_some_and(described),
            has_example: doc.is_some_and(|b| b.example),
            params_documented: self.params.map(|ps| ps.iter().all(|p| doc.is_some_and(|b| b.params.contains(p)))),
            deprecated: tags.contains(&"deprecated"),
            since: doc.and_then(|b| b.since).map(String::from),
            params: self.params.map_or(0, <[_]>::len),
            exported: self.via.is_some_and(counts),
            release_tagged: ["public", "beta", "alpha", "internal"].iter().filter(|t| tags.contains(t)).count() == 1,
            // The generated modules are TypeScript.
            typed: None,
            diagnostics: Vec::new(),
            stub: (!documented && self.item.ignore.is_none() && !self.block).then(|| self.stub()),
        };
        Some(Item { documented, facts, ..self.item.clone() })
    }

    /// The stub `--fix` writes above it: a `@param` for each parameter and `@returns` if it returns.
    fn stub(&self) -> collect::Stub {
        let params = self.params.unwrap_or_default().iter().map(|p| format!(" * @param {p}\n"));
        let tags: String = params.chain(self.returns.then(|| " * @returns\n".to_owned())).collect();
        let text = if tags.is_empty() { "/** TODO */".to_owned() } else { format!("/**\n * TODO\n{tags} */") };
        collect::Stub { line: self.item.line, text }
    }
}

/// A JSDoc block a generated module has, and what it says.
#[derive(Clone, Copy, Debug)]
struct Block {
    text: &'static str,
    /// Whether it describes its item: text before its first tag, or a tag that stands in for it.
    described: bool,
    /// Whether it describes a class, which `@classdesc` does too.
    class_described: bool,
    /// Its tags' names.
    tags: &'static [&'static str],
    /// The access level its tags give.
    access: Option<Access>,
    /// Whether it has an `@example`.
    example: bool,
    /// Its `@since` text.
    since: Option<&'static str>,
    /// The parameters its `@param` tags name, leaving out dotted names.
    params: &'static [&'static str],
}

const fn block(text: &'static str, described: bool, class_described: bool, tags: &'static [&'static str]) -> Block {
    Block { text, described, class_described, tags, access: None, example: false, since: None, params: &[] }
}

const fn access(text: &'static str, tags: &'static [&'static str], level: Access) -> Block {
    Block { access: Some(level), ..block(text, true, true, tags) }
}

const BLOCKS: [Block; 22] = [
    block("/** Docs.\n * @beta\n * @alpha\n */", true, true, &["beta", "alpha"]),
    block("/** Docs. */", true, true, &[]),
    Block { params: &["a"], ..block("/**\n * Docs.\n * @param a The a.\n */", true, true, &["param"]) },
    Block {
        example: true,
        since: Some("1.2"),
        params: &["a"],
        ..block(
            "/**\n * Docs.\n * @param {string} [a=1] The a.\n * @example\n * f()\n * @since 1.2\n */",
            true,
            true,
            &["param", "example", "since"],
        )
    },
    Block {
        params: &["b"],
        ..block("/** Docs.\n * @arg b Not a.\n * @param a.x Part of a.\n */", true, true, &["arg", "param"])
    },
    block("/** @since */", false, false, &["since"]),
    block("/** Mail me@example.com. */", true, true, &[]),
    block("/** @type {string} */", false, false, &["type"]),
    block("/** @deprecated */", false, false, &["deprecated"]),
    block("/** @internal */", false, false, &["internal"]),
    block("/** @description Docs. */", true, true, &["description"]),
    block("/**\n * @desc\n */", false, false, &["desc"]),
    block("/** @summary Docs. */", true, true, &["summary"]),
    block("/** @classdesc Docs. */", false, true, &["classdesc"]),
    block("/** @inheritDoc */", true, true, &["inheritDoc"]),
    block("/**\n * @INHERITDOC\n */", true, true, &["INHERITDOC"]),
    block("/**\n * Hidden.\n * @hidden\n * @ignore\n */", true, true, &["hidden", "ignore"]),
    access("/** Docs.\n * @private\n */", &["private"], Access::Private),
    access("/**\n * Docs.\n * @access protected\n */", &["access"], Access::Protected),
    access("/** Docs.\n * @package\n */", &["package"], Access::Package),
    access("/** Docs.\n * @public\n * @private\n */", &["public", "private"], Access::Public),
    block("/** Docs.\n * @access whoever\n */", true, true, &["access"]),
];

const LEVELS: [Access; 4] = [Access::Public, Access::Protected, Access::Package, Access::Private];

/// Tags `excludeTags` may name.
const TAGS: [&str; 6] = ["internal", "ignore", "hidden", "deprecated", "param", "inheritdoc"];

/// How a module exports an item.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Via {
    Esm,
    Cjs,
    Global,
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
    /// `window.name = name;` or `globalThis.name = name;`
    Global,
}

impl Module {
    fn write(&mut self, text: &str) {
        self.source.push_str(text);
        self.source.push('\n');
    }

    /// The line about to be written.
    fn line(&self) -> usize {
        self.source.matches('\n').count() + 1
    }

    /// Maybe writes a `docov-ignore-file` (or `-next-line`) comment, or one that isn't quite, and
    /// gives the ignore it is, if it's one.
    fn ignore(&mut self, tc: &TestCase, file: bool) -> Option<Ignore> {
        if !tc.draw(gs::weighted_booleans(0.3)) {
            return None;
        }
        let ignores = if file { FILE_IGNORES.to_vec() } else { NEXT_LINE_IGNORES.to_vec() };
        let (text, reason) = tc.draw(gs::sampled_from(ignores));
        let line = self.line();
        self.write(text);
        reason.map(|reason| Ignore { reason: reason.map(String::from), line, column: 1, file })
    }

    /// Writes ordinary comments and maybe a JSDoc block, and says which block it wrote.
    fn doc(&mut self, tc: &TestCase) -> Option<Block> {
        let noise = ["// note", "/* note */", "/** */", "/**\n *\n */", "/** @license MIT */"];
        self.block = false;
        if tc.draw(gs::booleans()) {
            let noise = tc.draw(gs::sampled_from(noise.to_vec()));
            self.skipped += usize::from(noise.contains("@license"));
            self.block = noise.starts_with("/**") && !noise.contains("@license");
            self.write(noise);
        }
        let doc = tc.draw(gs::booleans()).then(|| tc.draw(gs::sampled_from(BLOCKS.to_vec())));
        if let Some(block) = doc {
            self.block = true;
            self.write(block.text);
        }
        if tc.draw(gs::booleans()) {
            self.write("// after");
        }
        self.next_ignore = self.ignore(tc, false);
        doc
    }

    /// Expects an item on the line about to be written.
    fn expect(&mut self, via: Option<Via>, name: String, kind: Kind, doc: Option<Block>) {
        let line = self.line();
        let ignore = self.next_ignore.take().filter(|ignore| ignore.line + 1 == line);
        let item = Item { name, kind, line, column: 1, documented: doc.is_some(), facts: Facts::default(), ignore };
        let params = matches!(kind, Kind::Function | Kind::Method).then_some(&[][..]);
        let (block, returns) = (self.block, false);
        let (protected, overrides, value) = (false, false, false);
        self.expected.push(Expected { via, value, item, doc, protected, overrides, params, block, returns });
    }

    /// A statement that exports and declares nothing, and whether a scan passes over it.
    fn nothing(&mut self, tc: &TestCase) {
        let statements = vec![
            ("window.x.y = 1;", false),
            ("x = 1;", false),
            ("exports[key] = 1;", false),
            ("other.exports.y = 1;", false),
            ("z.w = a;", false),
            ("go();", false),
            ("export = { a: 1 };", false),
            ("if (x) {}", false),
            ("export * from './x';", true),
            ("export * as ns from './x';", true),
            ("export { y, z as w } from './y';", true),
            ("declare global { interface W {} }", true),
            ("import fs = require('fs');", true),
        ];
        let (statement, skipped) = tc.draw(gs::sampled_from(statements));
        self.skipped += usize::from(skipped);
        self.write(statement);
    }

    /// A CommonJS export of a value rather than a declaration.
    fn commonjs(&mut self, tc: &TestCase, i: usize) {
        let first = self.expected.len();
        let doc = self.doc(tc);
        match tc.draw(gs::integers::<u8>().max_value(7)) {
            6 => self.nothing(tc),
            7 => {
                let (global, kind, value) = tc.draw(gs::sampled_from(vec![
                    ("window", Kind::Function, "function () {}"),
                    ("globalThis", Kind::Variable, "1"),
                ]));
                self.expect(Some(Via::Global), format!("c{i}"), kind, doc);
                self.write(&format!("{global}.c{i} = {value};"));
            }
            0 => {
                self.expect(Some(Via::Cjs), format!("c{i}"), Kind::Function, doc);
                self.write(&format!("exports.c{i} = function () {{}};"));
            }
            1 => {
                self.expect(Some(Via::Cjs), format!("c{i}"), Kind::Variable, doc);
                self.write(&format!("module.exports.c{i} = 1;"));
            }
            2 => {
                self.expect(Some(Via::Cjs), format!("f{i}"), Kind::Function, doc);
                self.write(&format!("module.exports = function f{i}() {{}};"));
            }
            3 if tc.draw(gs::booleans()) => {
                self.expect(Some(Via::Cjs), format!("G{i}"), Kind::Class, doc);
                self.write(&format!("module.exports = class G{i} {{}};"));
            }
            3 => {
                self.expect(Some(Via::Cjs), "default".into(), Kind::Class, doc);
                self.write("module.exports = class {};");
            }
            4 => {
                self.expect(Some(Via::Cjs), "default".into(), Kind::Default, doc);
                self.write("module.exports = 1;");
            }
            _ => {
                self.write("module.exports = {");
                for j in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
                    let doc = self.doc(tc);
                    match tc.draw(gs::integers::<u8>().max_value(4)) {
                        4 => self.write("...rest,"),
                        0 => {
                            self.expect(Some(Via::Cjs), format!("p{i}_{j}"), Kind::Variable, doc);
                            self.write(&format!("p{i}_{j}: 1,"));
                        }
                        1 => {
                            self.expect(Some(Via::Cjs), format!("m{i}_{j}"), Kind::Function, doc);
                            self.write(&format!("m{i}_{j}() {{}},"));
                        }
                        2 => {
                            self.expect(Some(Via::Cjs), format!("a{i}_{j}"), Kind::Function, doc);
                            self.expected.last_mut().unwrap().returns = true;
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
            self.skipped += 1;
            self.write(&format!("const r{i} = require('r{i}');"));
        }
        for expected in &mut self.expected[first..] {
            expected.value = true;
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
        let mut exports = vec![Export::No, Export::Inline, Export::Specifier, Export::CommonJs, Export::Global];
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
        // The statement after the declaration that exports it, if one does.
        let after = match export {
            Export::CommonJs => vec![
                format!("exports.{name} = {name};"),
                format!("module.exports.{name} = {name};"),
                format!("module.exports = {name};"),
                format!("module.exports = {{ {name} }};"),
            ],
            Export::DefaultName => vec![
                format!("export default {name};"),
                format!("export = {name};"),
                format!("export {{ {name} as default }};"),
            ],
            Export::Global => vec![format!("window.{name} = {name};"), format!("globalThis.{name} = {name};")],
            _ => vec![String::new()],
        };
        let after = tc.draw(gs::sampled_from(after));
        let via = match export {
            Export::No => None,
            Export::CommonJs => Some(Via::Cjs),
            Export::Global => Some(Via::Global),
            Export::DefaultName if after.starts_with("export =") => Some(Via::Cjs),
            _ => Some(Via::Esm),
        };
        let prefix = match export {
            Export::Inline => "export ",
            Export::DefaultInline => "export default ",
            _ => "",
        };
        if export == Export::Specifier {
            self.specifiers.push(name.clone());
        }

        let doc = self.doc(tc);
        match kind {
            Kind::Function => {
                self.expect(via, name.clone(), kind, doc);
                self.expected.last_mut().unwrap().params = Some(&["a"]);
                if export != Export::DefaultInline && tc.draw(gs::booleans()) {
                    // Only the signature counts; the implementation after it doesn't.
                    self.skipped += 1;
                    self.write(&format!("{prefix}function {name}(a: string): void;"));
                }
                self.write(&format!("{prefix}function {name}(a: unknown) {{}}"));
            }
            Kind::Variable if tc.draw(gs::booleans()) => {
                // Nothing to name: destructuring declares no item.
                self.skipped += 1;
                self.write(&format!("{prefix}const {{ d{i} }} = {{ d{i}: 1 }};"));
            }
            Kind::Variable => {
                self.expect(via, name.clone(), kind, doc);
                self.write(&format!("{prefix}const {name} = 1;"));
            }
            Kind::Type | Kind::Enum | Kind::Namespace => {
                self.expect(via, name.clone(), kind, doc);
                let body = match kind {
                    Kind::Type => format!("type {name} = string;"),
                    Kind::Enum => format!("enum {name} {{ A }}"),
                    _ => format!("namespace {name} {{}}"),
                };
                self.write(&format!("{prefix}{body}"));
            }
            Kind::Default => {
                self.expect(Some(Via::Esm), "default".into(), kind, doc);
                self.write("export default { a: 1 };");
            }
            Kind::Class => {
                self.expect(via, name.clone(), kind, doc);
                if tc.draw(gs::booleans()) {
                    self.write("@sealed");
                }
                self.write(&format!("{prefix}class {name} {{"));
                self.class_members(tc, &name, via);
                self.write("}");
            }
            Kind::Interface => {
                self.expect(via, name.clone(), kind, doc);
                self.write(&format!("{prefix}interface {name} {{"));
                for j in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
                    let doc = self.doc(tc);
                    match tc.draw(gs::integers::<u8>().max_value(2)) {
                        0 => {
                            self.expect(via, format!("{name}.p{j}"), Kind::Property, doc);
                            self.write(&format!("p{j}: 1;"));
                        }
                        1 => {
                            self.expect(via, format!("{name}.m{j}"), Kind::Method, doc);
                            self.write(&format!("m{j}(): void;"));
                        }
                        _ => self.write("[key: string]: unknown;"),
                    }
                }
                self.write("}");
            }
            _ => unreachable!(),
        }
        if !after.is_empty() {
            self.write(&after);
        }
        if tc.draw(gs::weighted_booleans(0.2)) {
            self.skipped += 1;
            self.write(&format!("declare module 'm{i}' {{ export function f(): void; }}"));
        }
    }

    fn class_members(&mut self, tc: &TestCase, class: &str, via: Option<Via>) {
        let mut constructed = false;
        for j in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
            let doc = self.doc(tc);
            let item = |m: &mut Self, name: String, kind| m.expect(via, format!("{class}.{name}"), kind, doc);
            match tc.draw(gs::integers::<u8>().max_value(11)) {
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
                    self.skipped += 1;
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
                    self.expected.last_mut().unwrap().params = Some(&["a"]);
                    self.skipped += 1;
                    self.write(&format!("v{j}(a: string): void;\nv{j}(a: unknown) {{}}"));
                }
                8 if !constructed => {
                    constructed = true;
                    self.skipped += 1;
                    self.write("constructor() {}");
                }
                9 => {
                    item(self, format!("t{j}"), Kind::Property);
                    self.expected.last_mut().unwrap().protected = true;
                    self.write(&format!("protected t{j} = 1;"));
                }
                10 => {
                    item(self, format!("u{j}"), Kind::Method);
                    self.expected.last_mut().unwrap().overrides = true;
                    self.write(&format!("override u{j}() {{}}"));
                }
                _ => {
                    self.skipped += 4;
                    self.write(&format!(
                        "#h{j} = 1;\nprivate q{j} = 1;\nprivate r{j}() {{}}\nprivate accessor w{j} = 1;\nstatic {{}}"
                    ));
                }
            }
        }
    }

    fn generate(tc: &TestCase) -> Self {
        let mut module = Self::default();
        let header = tc.draw(gs::sampled_from(vec!["", "// @generated by a tool", "// header\n// @generated"]));
        if !header.is_empty() {
            module.write(header);
        }
        module.generated = header.starts_with("// @generated");
        let file_ignore_first = tc.draw(gs::booleans());
        if file_ignore_first {
            module.file_ignore = module.ignore(tc, true);
        }
        for i in 0..tc.draw(gs::integers::<usize>().max_value(10)) {
            module.declaration(tc, i);
        }
        if tc.draw(gs::booleans()) {
            module.nothing(tc);
        }
        let mut module = module.finish();
        if !file_ignore_first {
            module.file_ignore = module.ignore(tc, true);
        }
        for expected in &mut module.expected {
            expected.item.ignore = expected.item.ignore.take().or_else(|| module.file_ignore.clone());
        }
        module
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
        let items = scan(&source, SourceType::ts(), &scope.into()).unwrap();
        // Globals count only when asked, so by default they're nothing.
        let wanted: Vec<Item> = expected.iter().filter_map(|e| e.by(&scope.into())).collect();
        assert_eq!(items, wanted, "{source}");
    }

    let broken = format!("{source}export function (");
    let error = scan(&broken, SourceType::ts(), &Scope::All.into()).unwrap_err();
    assert!(error.offset >= source.len() && error.offset <= broken.len(), "{error:?}");
}

/// Rules with every filter drawn, for a generated module.
fn drawn_rules(tc: &TestCase) -> Rules {
    Rules {
        scope: tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::All])),
        require_description: tc.draw(gs::booleans()),
        exclude_tags: tc
            .draw(gs::vecs(gs::sampled_from(TAGS.to_vec())).max_size(3))
            .into_iter()
            .map(String::from)
            .collect(),
        kinds: tc.draw(gs::optional(gs::vecs(gs::sampled_from(ALL_KINDS.to_vec())).max_size(4))),
        inherit: tc.draw(gs::sampled_from(vec![Inherit::Tag, Inherit::Override])),
        ignore_deprecated: tc.draw(gs::booleans()),
        skip_generated: tc.draw(gs::booleans()),
        exports: Exports {
            esm: tc.draw(gs::booleans()),
            cjs: tc.draw(gs::booleans()),
            globals: tc.draw(gs::booleans()),
        },
        access: tc.draw(gs::optional(gs::vecs(gs::sampled_from(LEVELS.to_vec())).max_size(3))),
        ignore_names: tc
            .draw(
                gs::vecs(gs::sampled_from(vec!["^n1$", "^_", "^m", "^p\\d", "0$", "^n2\\.", "^default$"])).max_size(2),
            )
            .into_iter()
            .map(|re| regex::Regex::new(re).unwrap())
            .collect(),
        // The settings that add items of their own have properties of their own.
        ..Rules::default()
    }
}

/// The rules decide which of a generated module's items count and which are documented, the way
/// the model says.
#[hegel::test(test_cases = 300)]
fn rules_decide_what_counts_and_what_is_documented(tc: TestCase) {
    let Module { source, expected, generated, .. } = Module::generate(&tc);
    let rules = drawn_rules(&tc);
    // And each rule for what's documented with nothing left out, for the filters not to hide.
    let mut all = vec![rules];
    for (require_description, inherit) in [(false, Inherit::Tag), (true, Inherit::Tag), (true, Inherit::Override)] {
        all.push(Rules { scope: Scope::All, require_description, inherit, ..Rules::default() });
    }
    for rules in all {
        let items = scan(&source, SourceType::ts(), &rules).unwrap();
        let skipped = generated && rules.skip_generated;
        let wanted: Vec<Item> = expected.iter().filter_map(|e| e.by(&rules)).filter(|_| !skipped).collect();
        assert_eq!(items, wanted, "{source}");
    }
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

    let rules = Scope::All.into();
    match scan_file(&path, &rules) {
        Ok(report) => {
            assert!(tail.is_empty());
            assert_eq!((report.path, report.items), (path, scan(&source, SourceType::ts(), &rules).unwrap()));
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

/// Up to four files of up to six items, two to a line, with names and paths to escape, and
/// `docov-ignore` comments over some items when `ignores` is on.
fn reports(tc: &TestCase, ignores: bool) -> Vec<FileReport> {
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
                    facts: Facts {
                        has_example: drawn[2] / 2 % 2 == 0,
                        params: i % 3,
                        exported: drawn[1] / 2 % 2 == 0,
                        release_tagged: (drawn[0] + drawn[1]) % 3 == 0,
                        typed: (drawn[2] % 3 != 0).then_some(drawn[0] % 2 == 0),
                        ..Facts::default()
                    },
                    // The comment on the line before, the same for the items that share a line.
                    ignore: (ignores && drawn[2] % 2 == 0).then(|| Ignore {
                        reason: (i / 2 % 2 == 1).then(|| "why".into()),
                        line: i / 2,
                        column: 1,
                        file: i / 2 % 3 == 0,
                    }),
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
    text: report::Text,
}

impl Drawn {
    fn draw(tc: &TestCase) -> Self {
        let failures = tc.draw(gs::vecs(gs::sampled_from(FAILURES.to_vec())).max_size(2));
        let now_ms = tc.draw(gs::integers::<u64>().max_value(4_000_000_000_000));
        let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
        Self {
            files: reports(tc, false),
            failures: failures.into_iter().map(String::from).collect(),
            now_ms,
            threshold,
            text: report::Text::default(),
        }
    }

    fn context(&self) -> Context<'_> {
        Context {
            files: &self.files,
            failures: &self.failures,
            now: UNIX_EPOCH + Duration::from_millis(self.now_ms),
            threshold: self.threshold,
            text: self.text,
            base: None,
            packages: None,
            color: false,
            sort: Sort::Position,
            examples: false,
            metrics: &[],
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
    let flags = report::Flags {
        format: format.clone(),
        output: output.clone(),
        badge: badge.clone(),
        compare: None,
        text: drawn.text,
    };
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

/// With `--compare`, `markdown`'s total says the change from the base report's, and the table has
/// a row for each file with items in either: its percentage now and before and the change between
/// them, `new` for a file the base didn't have, and `gone`, after the rest, for one only the base
/// had. A base that can't be read or isn't a `json` report fails, naming its file.
#[hegel::test(test_cases = 100)]
fn markdown_shows_the_change_from_the_base(tc: TestCase) {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("base.json");
    let mut drawn = Drawn::draw(&tc);
    let old = reports(&tc, false);
    // Some files are in both.
    for (file, old) in drawn.files.iter_mut().zip(&old) {
        if tc.draw(gs::booleans()) {
            file.path.clone_from(&old.path);
        }
    }
    let json = report::format("json").unwrap().render(&Context { files: &old, ..drawn.context() });
    let text = tc.draw(gs::sampled_from(vec![Some(json), None, Some("not json".into()), Some("[]".into())]));
    if let Some(text) = &text {
        fs::write(&path, text).unwrap();
    }
    let base = match report::Base::read(&path) {
        Ok(base) => base,
        Err(e) => {
            assert!(text.is_none_or(|t| !t.starts_with('{')), "{e}");
            assert!(e.starts_with(&format!("{}: ", path.display())), "{e}");
            return;
        }
    };
    let out = report::format("markdown").unwrap().render(&Context { base: Some(&base), ..drawn.context() });

    // A change is a signed difference of the percentages it's between.
    let percent = |cell: &str| cell.strip_suffix('%').unwrap().parse::<f64>().unwrap();
    let is_change = |change: &str, now: &str, before: &str| {
        let sign = change.starts_with(['+', '-']);
        sign && (change.parse::<f64>().unwrap() - (percent(now) - percent(before))).abs() < 1e-9
    };
    let (now, before) = (Totals::of(&drawn.files), Totals::of(&old));
    let shown = format!("{:.1}%", now.percent());
    let heading = format!("**{shown}** documented ({}/{}), ", now.documented, now.total);
    let line = out.lines().find_map(|l| l.strip_prefix(&heading)).unwrap();
    let (change, from) = line.split_once(" from ").unwrap();
    assert!(from == format!("{:.1}%", before.percent()) && is_change(change, &shown, from), "{out}");

    let (mut rows, mut cells) = (Vec::new(), Vec::new());
    for event in Parser::new_ext(&out, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::TableCell) => cells.push(String::new()),
            Event::Text(text) | Event::Code(text) if !cells.is_empty() => cells.last_mut().unwrap().push_str(&text),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => rows.push(std::mem::take(&mut cells)),
            _ => {}
        }
    }
    let slash = |path: &Path| path.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    let with_items = |files: &[FileReport]| files.iter().filter(|f| !f.items.is_empty()).cloned().collect::<Vec<_>>();
    let (now, mut gone) = (with_items(&drawn.files), with_items(&old));
    let mut wanted =
        vec![vec!["Documented", "Before", "Change", "Items", "File"].into_iter().map(String::from).collect()];
    if now.is_empty() && gone.is_empty() {
        wanted.clear();
    }
    for file in &now {
        let totals = file.totals();
        let before = gone.iter().position(|f| f.path == file.path).map(|i| gone.remove(i).totals());
        let row = &rows[wanted.len()];
        let change = before.map_or("new".into(), |_| row[2].clone());
        let before = before.map_or(String::new(), |b| format!("{:.1}%", b.percent()));
        assert!(change == "new" || is_change(&change, &row[0], &before), "{out}");
        let (documented, total) = (totals.documented, totals.total);
        wanted.push(vec![
            format!("{:.1}%", totals.percent()),
            before,
            change,
            format!("{documented}/{total}"),
            slash(&file.path),
        ]);
    }
    gone.sort_by_key(|f| f.path.display().to_string());
    for file in gone {
        let before = format!("{:.1}%", file.totals().percent());
        wanted.push(vec![String::new(), before, "gone".into(), String::new(), slash(&file.path)]);
    }
    assert_eq!(rows, wanted, "{out}");
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

/// `text` lists each file with items and its undocumented ones; `--omit-covered` leaves out the
/// fully documented files, and `--quiet` everything but the total.
#[hegel::test(test_cases = 100)]
fn text_shows_what_its_flags_ask_for(tc: TestCase) {
    let mut drawn = Drawn::draw(&tc);
    drawn.text = report::Text { omit_covered: tc.draw(gs::booleans()), quiet: tc.draw(gs::booleans()), by_kind: false };
    let out = report::format("text").unwrap().render(&drawn.context());
    let totals = Totals::of(&drawn.files);
    let total = format!("{:>6.1}%  {}/{}  total", totals.percent(), totals.documented, totals.total);
    let shown: Vec<&FileReport> = drawn
        .files
        .iter()
        .filter(|f| !f.items.is_empty() && !drawn.text.quiet)
        .filter(|f| !drawn.text.omit_covered || f.items.iter().any(|i| !i.documented))
        .collect();
    let headers: Vec<&str> = out.lines().filter(|l| !l.starts_with("         ") && *l != total).collect();
    let wanted: Vec<String> = shown
        .iter()
        .map(|f| {
            format!(
                "{:>6.1}%  {}/{}  {}",
                f.totals().percent(),
                f.totals().documented,
                f.totals().total,
                f.path.display()
            )
        })
        .collect();
    assert_eq!(headers, wanted, "{out}");
    let listed = out.lines().filter(|l| l.starts_with("         ")).count();
    assert_eq!(listed, shown.iter().map(|f| f.items.iter().filter(|i| !i.documented).count()).sum::<usize>());
    assert!(out.ends_with(&format!("{total}\n")), "{out}");
}

/// `benchmark` is the total, to a decimal place, in github-action-benchmark's shape.
#[hegel::test(test_cases = 50)]
fn benchmark_is_the_total(tc: TestCase) {
    let drawn = Drawn::draw(&tc);
    let render = |name| report::format(name).unwrap().render(&drawn.context());
    let percent = (Totals::of(&drawn.files).percent() * 10.0).round() / 10.0;
    let benchmark: serde_json::Value = serde_json::from_str(&render("benchmark")).unwrap();
    assert_eq!(benchmark, serde_json::json!([{ "name": "docov", "unit": "%", "value": percent }]));
}

/// `unix` is a line per undocumented item, the way compilers write warnings.
#[hegel::test(test_cases = 50)]
fn unix_is_a_line_per_undocumented_item(tc: TestCase) {
    let drawn = Drawn::draw(&tc);
    let undocumented = drawn.files.iter().flat_map(|f| f.items.iter().filter(|i| !i.documented).map(move |i| (f, i)));
    let wanted: Vec<String> = undocumented
        .map(|(f, i)| {
            format!("{}:{}:{}: {} {} undocumented [docov]", f.path.display(), i.line, i.column, i.kind.as_str(), i.name)
        })
        .collect();
    assert_eq!(report::format("unix").unwrap().render(&drawn.context()).lines().collect::<Vec<_>>(), wanted);
}

/// `html` is one page that reads back as written: a table row per file with counted items, linking
/// to that file's source, a row per source line with the lines that have counted items marked,
/// missed when one is undocumented and hit otherwise. A file that's gone by the time it's written says so.
#[hegel::test(test_cases = 50)]
fn html_shows_each_files_source_with_its_items(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut drawn = Drawn::draw(&tc);
    // Some of the drawn files are real, and scanned.
    for f in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let Module { source, .. } = Module::generate(&tc);
        let path = dir.path().join(format!("m{f}.ts"));
        fs::write(&path, format!("{source}// <&> \"'\n")).unwrap();
        drawn.files.push(scan_file(&path, &Scope::All.into()).unwrap());
    }
    let page = report::format("html").unwrap().render(&drawn.context());
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let html = roxmltree::Document::parse_with_options(&page, options).unwrap();
    assert!(!html.descendants().any(|n| n.has_tag_name("script")));
    let class =
        |n: &roxmltree::Node, class: &str| n.attribute("class").is_some_and(|c| c.split(' ').any(|c| c == class));
    let with_items: Vec<&FileReport> = drawn.files.iter().filter(|f| f.counted().next().is_some()).collect();
    let table = html.descendants().find(|n| class(n, "files")).unwrap();
    let links: Vec<String> =
        table.descendants().filter(|n| n.has_tag_name("a")).map(|a| a.text().unwrap().to_owned()).collect();
    assert_eq!(links, with_items.iter().map(|f| f.path.display().to_string()).collect::<Vec<_>>());

    let headings: Vec<_> = html.descendants().filter(|n| n.has_tag_name("h2")).collect();
    assert_eq!(headings.len(), with_items.len());
    for (heading, file) in headings.iter().zip(&with_items) {
        assert!(heading.text().unwrap().starts_with(&file.path.display().to_string()));
        let after = heading.next_sibling_element().unwrap();
        let Ok(text) = fs::read_to_string(&file.path) else {
            assert!(class(&after, "missing"), "{}", file.path.display());
            continue;
        };
        let rows: Vec<_> = after.descendants().filter(|n| n.has_tag_name("tr")).collect();
        assert_eq!(rows.len(), text.lines().count());
        for (n, (row, line)) in rows.iter().zip(text.lines()).enumerate().map(|(i, r)| (i + 1, r)) {
            let cells: Vec<String> =
                row.children().filter(|c| c.has_tag_name("td")).map(|c| c.text().unwrap_or("").to_owned()).collect();
            let items: Vec<&Item> = file.counted().filter(|i| i.line == n).collect();
            let notes: Vec<String> =
                items.iter().filter(|i| !i.documented).map(|i| format!("{} {}", i.kind.as_str(), i.name)).collect();
            assert_eq!(cells, [n.to_string(), line.to_owned(), notes.join(", ")]);
            let mark = if items.is_empty() {
                None
            } else if notes.is_empty() {
                Some("hit")
            } else {
                Some("miss")
            };
            assert_eq!(row.attribute("class"), mark);
        }
    }
}

/// Every format reports the same items and totals, and documenting one more item never lowers
/// coverage.
#[hegel::test(test_cases = 100)]
fn formats_agree_on_totals(tc: TestCase) {
    let mut files = reports(&tc, false);
    let totals = Totals::of(&files);
    assert!((0.0..=100.0).contains(&totals.percent()));
    assert_eq!(totals.total, files.iter().map(|f| f.items.len()).sum::<usize>());

    // Every format fails when its output doesn't fit where it's going, and only then.
    for format in report::FORMATS {
        let context = Context {
            files: &files,
            failures: &[],
            now: UNIX_EPOCH,
            threshold: None,
            text: report::Text::default(),
            base: None,
            packages: None,
            color: false,
            sort: Sort::Position,
            examples: false,
            metrics: &[],
        };
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

/// What to report of `files`, with nothing else to report.
fn plain(files: &[FileReport]) -> Context<'_> {
    Context {
        files,
        failures: &[],
        now: UNIX_EPOCH,
        threshold: None,
        text: report::Text::default(),
        base: None,
        packages: None,
        color: false,
        sort: Sort::Position,
        examples: false,
        metrics: &[],
    }
}

/// `files` in the format named `name`, with nothing else to report.
fn render(name: &str, files: &[FileReport]) -> String {
    report::format(name).unwrap().render(&plain(files))
}

/// Why `feature`'s gate fails a scan of `files`, with the default settings and the current folder
/// as the config's.
fn gate(feature: &impl Feature, files: &[FileReport]) -> Vec<String> {
    let settings = settings(tempfile::tempdir().unwrap().path(), "{}", &[]);
    let root = docov::root::Root::new(Path::new(""));
    feature.gate(&docov::feature::Run { reports: files, root: &root, settings: &settings })
}

/// The text report's line right after its total, where a metric's goes.
fn metric_line(context: &Context) -> Option<String> {
    let text = report::format("text").unwrap().render(context);
    text.lines().skip_while(|line| !line.ends_with("  total")).nth(1).map(String::from)
}

/// `--sort priority` lists each file's undocumented items with exported functions and methods
/// first, more parameters before fewer, then classes, then the rest, each group in file order, and
/// changes nothing else. The JSON report has each item's facts.
#[hegel::test(test_cases = 100)]
fn priority_puts_exported_functions_with_more_parameters_first(tc: TestCase) {
    let mut files = reports(&tc, true);
    // The kinds that sort apart, so a file often has several of each.
    for item in files.iter_mut().flat_map(|f| &mut f.items) {
        let at = ALL_KINDS.iter().position(|k| *k == item.kind).unwrap();
        item.kind = [Kind::Function, Kind::Method, Kind::Class, Kind::Variable][at % 4];
    }
    let text = report::format("text").unwrap().render(&Context { sort: Sort::Priority, ..plain(&files) });
    let mut wanted = Vec::new();
    for file in &files {
        let undocumented = file.items.iter().filter(|i| !i.documented && i.ignore.is_none());
        let tier = |i: &&Item| match i.kind {
            Kind::Function | Kind::Method if i.facts.exported => i.facts.params,
            Kind::Class => 100,
            _ => 101,
        };
        let mut tiers: Vec<_> = undocumented.collect();
        // Fewer parameters sort later: a tier for each count, highest first.
        tiers.sort_by_key(|i| match tier(i) {
            n @ 0..100 => 99 - n,
            n => n,
        });
        wanted.extend(
            tiers
                .iter()
                .map(|i| format!("         {}:{}:1  {} {}", file.path.display(), i.line, i.kind.as_str(), i.name)),
        );
    }
    let listed: Vec<&str> = text.lines().filter(|l| l.starts_with("         ")).collect();
    assert_eq!(listed, wanted);
    let lines = |text: &str| text.lines().filter(|l| !l.starts_with("         ")).map(String::from).collect::<Vec<_>>();
    assert_eq!(lines(&text), lines(&render("text", &files)));

    let json: serde_json::Value = serde_json::from_str(&render("json", &files)).unwrap();
    let shown = json["files"].as_array().unwrap().iter().flat_map(|f| f["items"].as_array().unwrap().clone());
    let facts = files.iter().flat_map(|f| &f.items).map(|i| serde_json::json!([i.facts.has_example, i.facts.since]));
    assert!(shown.map(|i| serde_json::json!([i["hasExample"], i["since"]])).eq(facts));
}

/// Example coverage is the share of counted items with an `@example`. The JSON report has it for
/// each file and in all, `--examples` puts it on each of the text report's lines and changes
/// nothing else, and its threshold fails the scan under it.
#[hegel::test(test_cases = 100)]
fn examples_are_a_second_metric(tc: TestCase) {
    let files = reports(&tc, true);
    let of = |f: &FileReport| {
        let counted: Vec<_> = f.items.iter().filter(|i| i.documented || i.ignore.is_none()).collect();
        Totals { documented: counted.iter().filter(|i| i.facts.has_example).count(), total: counted.len() }
    };
    let all = files
        .iter()
        .map(of)
        .fold(Totals::default(), |a, b| Totals { documented: a.documented + b.documented, total: a.total + b.total });
    let json: serde_json::Value = serde_json::from_str(&render("json", &files)).unwrap();
    let shown = |v: &serde_json::Value| (v["withExamples"].as_u64(), v["examplesPercent"].as_f64());
    let wanted = |t: Totals| (Some(t.documented as u64), Some(t.percent()));
    assert_eq!(shown(&json), wanted(all));
    assert!(json["files"].as_array().unwrap().iter().map(shown).eq(files.iter().map(|f| wanted(of(f)))));

    let text = report::format("text").unwrap().render(&Context { examples: true, ..plain(&files) });
    let mut figures = files.iter().filter(|f| of(f).total > 0).map(of).chain([all]);
    let plain: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with("         ") {
                return line.to_owned();
            }
            let t = figures.next().unwrap();
            let figure = format!("  examples {}/{}  ", t.documented, t.total);
            assert!(line.contains(&figure), "{line}");
            line.replacen(&figure, "  ", 1)
        })
        .collect();
    assert_eq!(plain, render("text", &files).lines().collect::<Vec<_>>());

    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
    let percent = all.percent();
    let failure = threshold
        .filter(|&t| percent < t)
        .map(|t| format!("{percent:.1}% with examples, under the {t}% examples threshold"));
    assert_eq!(gate(&Examples { text: false, threshold }, &files), Vec::from_iter(failure));
}

/// A function's parameters are documented when each has an `@param` (or `@arg`, or `@argument`)
/// with its name, or for a destructured one, any at its place. Dotted names document parts.
#[hegel::test(test_cases = 100)]
fn parameters_are_documented_by_name_or_place(tc: TestCase) {
    let params = tc.draw(gs::vecs(gs::sampled_from(vec!["a", "b", "{ c }", "[d]"])).max_size(3));
    let rest = tc.draw(gs::sampled_from(vec!["", "...e", "...{ length }"]));
    let tags = tc.draw(gs::vecs(gs::sampled_from(vec!["a", "b", "e", "[b=1]", "a.x", "z"])).max_size(4));
    let tag = tc.draw(gs::sampled_from(vec!["param", "arg", "argument", "returns"]));
    let documented = tc.draw(gs::booleans());
    let doc: String = tags.iter().map(|name| format!(" * @{tag} {{string}} {name} Docs.\n")).collect();
    let mut list: Vec<&str> = params.clone();
    list.extend(Some(rest).filter(|r| !r.is_empty()));
    let source = format!(
        "{}export function f({}) {{}}\nexport class C {{ m({}) {{}} }}\n",
        if documented { format!("/**\n{doc} */\n") } else { String::new() },
        list.join(", "),
        list.join(", "),
    );
    let names: Vec<&str> = if documented && tag != "returns" {
        tags.iter()
            .filter(|n| !n.contains('.'))
            .map(|n| n.trim_matches(['[', ']']).split('=').next().unwrap())
            .collect()
    } else {
        Vec::new()
    };
    let wanted = list.iter().enumerate().all(|(i, p)| match p.trim_start_matches("...") {
        name if name.starts_with(['{', '[']) => i < names.len(),
        name => names.contains(&name),
    });
    let items = scan(&source, SourceType::ts(), &Rules::default()).unwrap();
    let facts: Vec<_> = items.iter().map(|i| (i.facts.params_documented, i.facts.params)).collect();
    // The class has no JSDoc, so its method's parameters are documented only when it has none.
    assert_eq!(facts, [(Some(wanted), list.len()), (None, 0), (Some(list.is_empty()), list.len())], "{source}");
}

/// With `"fileDocs": "require"`, each file is an item at its start, named by its `@module` or its
/// path, documented when its first JSDoc block is about the file and says something. That block
/// then documents nothing else, while a first block that isn't about the file still documents what
/// follows it. Off, files aren't items.
#[hegel::test(test_cases = 100)]
fn files_are_items_documented_by_a_block_about_them(tc: TestCase) {
    // A first block, whether it documents what follows it today, whether it's about the file and
    // says something, and its `@module` name.
    let (first, documents, about, module) = tc.draw(gs::sampled_from(vec![
        ("", false, None, None),
        ("/** @file Helpers. */", true, Some(true), None),
        ("/** @file */", true, Some(false), None),
        ("/**\n * Helpers.\n * @fileoverview\n */", true, Some(true), None),
        ("/** @overview Helpers. */", true, Some(true), None),
        ("/** @module util/strings The strings. */", true, Some(true), Some("util/strings")),
        ("/** @module */", true, Some(false), None),
        ("/** @packageDocumentation The package. */", true, Some(true), None),
        ("/**\n * @file Helpers.\n * @license MIT\n */", false, Some(true), None),
        ("/** Docs. */", true, None, None),
        ("/** @license MIT */", false, None, None),
    ]));
    let ignored = tc.draw(gs::booleans());
    let source = format!("{first}\nexport const a = 1;\n{}", if ignored { "/* docov-ignore-file */\n" } else { "" });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ts");
    fs::write(&path, &source).unwrap();
    let a = |documented| (String::from("a"), Kind::Variable, documented);
    let summary = |items: Vec<Item>| items.into_iter().map(|i| (i.name, i.kind, i.documented)).collect::<Vec<_>>();

    let off = Rules::default();
    assert_eq!(summary(scan(&source, SourceType::ts(), &off).unwrap()), [a(documents)]);
    let on = Rules { file_docs: FileDocs::Require, ..Rules::default() };
    let items = scan(&source, SourceType::ts(), &on).unwrap();
    let file = &items[0];
    assert_eq!((file.line, file.column, file.ignored()), (1, 1, ignored && about != Some(true)), "{source}");
    assert_eq!(file.kind.as_str(), "module");
    let file = (module.unwrap_or_default().to_owned(), Kind::Module, about == Some(true));
    assert_eq!(summary(items), [file.clone(), a(documents && about.is_none())], "{source}");
    let named = module.map_or(path.display().to_string(), String::from);
    assert_eq!(summary(scan_file(&path, &on).unwrap().items)[0], (named, Kind::Module, file.2));
}

/// With `"virtual": true`, a JavaScript file's `@typedef` and `@callback` blocks with a name are
/// items of kind `type`, where the block is, documented when they have a description, and they no
/// longer document the declaration after them. In TypeScript, or off, they're ordinary blocks.
#[hegel::test(test_cases = 100)]
fn comment_types_are_items_in_javascript(tc: TestCase) {
    let mut source = String::new();
    // Each item, by whether types count: (name, kind, line, documented).
    let (mut plain, mut typed) = (Vec::new(), Vec::new());
    for i in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        // A block, whether it names a type, and whether it describes it.
        let (block, declares, described) = tc.draw(gs::sampled_from(vec![
            ("/** @typedef {Object} T */", true, false),
            ("/**\n * A shape.\n * @typedef {Object} T\n * @property {number} x\n */", true, true),
            ("/** @typedef {string} T - An id. */", true, true),
            ("/** @callback T */", true, false),
            ("/**\n * @callback T\n * Called back.\n */", true, true),
            ("/** @typedef */", false, false),
            ("/** Docs. */", false, true),
        ]));
        let line = source.matches('\n').count() + 1;
        source.push_str(&block.replace(" T", &format!(" T{i}")));
        source.push('\n');
        if declares {
            typed.push((format!("T{i}"), Kind::Type, line, described));
        }
        if tc.draw(gs::booleans()) {
            let line = source.matches('\n').count() + 1;
            source.push_str(&format!("exports.v{i} = 1;\n"));
            plain.push((format!("v{i}"), Kind::Variable, line, true));
            typed.push((format!("v{i}"), Kind::Variable, line, !declares));
        }
    }
    let types = tc.draw(gs::booleans());
    let (name, source_type) = tc.draw(gs::sampled_from(vec![
        ("mjs", SourceType::mjs()),
        ("cjs", SourceType::cjs()),
        ("jsx", SourceType::jsx()),
        ("ts", SourceType::ts()),
    ]));
    let rules = Rules { virtual_types: types, ..Rules::default() };
    let items = scan(&source, source_type, &rules).unwrap();
    let found: Vec<_> = items.iter().map(|i| (i.name.clone(), i.kind, i.line, i.documented)).collect();
    let wanted = if types && name != "ts" { typed } else { plain };
    assert_eq!(found, wanted, "{name}: {source}");
    assert!(items.iter().all(|i| i.facts.exported));
}

/// With `"members": "all"`, enum members, the members of a type alias's object type and the
/// properties of an object literal tagged `@enum`, `@namespace` or `@lends X` count as members of
/// their parent (or of `X`), documented by a block of their own or a matching `@property` in the
/// parent's. They count where their parent does. By default only the parents count.
#[hegel::test(test_cases = 100)]
fn every_member_counts_when_asked(tc: TestCase) {
    let mut module = Module::default();
    // Each item's (name, kind, line, documented), whether it's a member, and whether it's exported.
    let mut wanted = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let exported = tc.draw(gs::booleans());
        let (shape, tag) = tc.draw(gs::sampled_from(vec![
            ("enum", None),
            ("type", None),
            ("const", None),
            ("const", Some("@enum {number}")),
            ("const", Some("@namespace")),
            ("const", Some("@lends Lent")),
            ("const", Some("@readonly")),
        ]));
        let listed = tc.draw(gs::vecs(gs::sampled_from(vec!["a", "b", "c"])).max_size(2));
        let parent_doc = tag.is_some() || tc.draw(gs::booleans());
        if parent_doc {
            module.write("/**\n * Parent.");
            if let Some(tag) = tag {
                module.write(&format!(" * {tag}"));
            }
            for name in &listed {
                module.write(&format!(" * @property {{number}} {name} Docs."));
            }
            module.write(" */");
        }
        let name = format!("P{i}");
        let kind = [("enum", Kind::Enum), ("type", Kind::Type), ("const", Kind::Variable)];
        let kind = kind.iter().find(|(s, _)| *s == shape).unwrap().1;
        wanted.push(((name.clone(), kind, module.line(), parent_doc), false, exported));
        let (open, close) = match shape {
            "type" => (" = {", "};"),
            "const" => (" = {", "};"),
            _ => (" {", "}"),
        };
        module.write(&format!("{}{shape} {name}{open}", if exported { "export " } else { "" }));
        let owner = if tag == Some("@lends Lent") { "Lent".to_owned() } else { name.clone() };
        let counts = shape != "const" || tag.is_some_and(|tag| tag != "@readonly");
        for (j, key) in ["a", "b", "c"].into_iter().enumerate().take(tc.draw(gs::integers::<usize>().max_value(3))) {
            let own = tc.draw(gs::booleans());
            if own {
                module.write("/** Docs. */");
            }
            let (text, kind) = match (shape, j) {
                ("enum", _) => (format!("{key},"), Kind::Property),
                ("type", 2) => (format!("{key}(): void;"), Kind::Method),
                ("type", _) => (format!("{key}: number;"), Kind::Property),
                (_, 2) => (format!("{key}() {{}},"), Kind::Method),
                _ => (format!("{key}: 1,"), Kind::Property),
            };
            let documented = own || (parent_doc && listed.contains(&key));
            if counts {
                wanted.push(((format!("{owner}.{key}"), kind, module.line(), documented), true, exported));
            }
            module.write(&text);
        }
        // A spread has no member to count.
        if shape == "const" && tc.draw(gs::booleans()) {
            module.write("...rest,");
        }
        module.write(close);
    }
    let source = module.source;
    for (members, scope) in
        [(Members::Classes, Scope::Exports), (Members::All, Scope::Exports), (Members::All, Scope::All)]
    {
        let rules = Rules { members, scope, ..Rules::default() };
        let items = scan(&source, SourceType::ts(), &rules).unwrap();
        let found: Vec<_> = items.into_iter().map(|i| (i.name, i.kind, i.line, i.documented)).collect();
        let counts =
            |member: bool, exported: bool| (members == Members::All || !member) && (scope == Scope::All || exported);
        let expected: Vec<_> =
            wanted.iter().filter(|(_, m, e)| counts(*m, *e)).map(|(item, _, _)| item.clone()).collect();
        assert_eq!(found, expected, "{members:?} {scope:?}: {source}");
    }
}

/// With `"commonjsMembers": true`, `X.prototype.m = …` and `X.m = …` at the top level are members
/// of `X` when it's a function or class declared there, counting where `X` does, and in a CommonJS
/// file `this.a = …` exports `a`. Off, none of them are items.
#[hegel::test(test_cases = 100)]
fn commonjs_assignments_add_members_when_asked(tc: TestCase) {
    let (ext, source_type) = tc.draw(gs::sampled_from(vec![
        ("cjs", SourceType::cjs()),
        ("js", SourceType::from_path("m.js").unwrap()),
        ("mjs", SourceType::mjs()),
    ]));
    let mut module = Module::default();
    // Each item's (name, kind, line, documented), whether it's new with the setting, and whether
    // it's exported.
    let mut wanted = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let (declaration, kind) = tc.draw(gs::sampled_from(vec![
            ("function X() {}", Kind::Function),
            ("class X {}", Kind::Class),
            ("const X = {};", Kind::Variable),
        ]));
        let name = format!("X{i}");
        let declaration = declaration.replace('X', &name);
        // How it's exported: not at all, inline (only in an ES module), or with `exports.X = X`.
        let export = tc.draw(gs::integers::<u8>().max_value(if ext == "mjs" { 2 } else { 1 }));
        let doc = module.doc(&tc).is_some();
        wanted.push(((name.clone(), kind, module.line(), doc), false, export > 0));
        module.write(&format!("{}{declaration}", if export == 2 { "export " } else { "" }));
        if export == 1 {
            module.write(&format!("exports.{name} = {name};"));
        }
        for j in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
            let (text, member_kind) = tc.draw(gs::sampled_from(vec![
                ("X.prototype.m = function (a) {};", Kind::Method),
                ("X.m = () => {};", Kind::Method),
                ("X.prototype.m = 1;", Kind::Property),
                ("X.m = other;", Kind::Property),
                ("X.prototype[k] = 1;", Kind::Property),
                ("X.prototype.constructor.m = 1;", Kind::Property),
                ("Y.m = 1;", Kind::Property),
            ]));
            let doc = module.doc(&tc).is_some();
            let member = kind != Kind::Variable && !text.contains(['[', 'Y']) && !text.contains("constructor");
            if member {
                wanted.push(((format!("{name}.m{j}"), member_kind, module.line(), doc), true, export > 0));
            }
            module.write(&text.replace('X', &name).replace(".m ", &format!(".m{j} ")));
        }
        if tc.draw(gs::booleans()) {
            let doc = module.doc(&tc).is_some();
            if ext != "mjs" {
                wanted.push(((format!("t{i}"), Kind::Variable, module.line(), doc), true, true));
            }
            module.write(&format!("this.t{i} = 1;"));
        }
    }
    let source = module.source;
    for (on, scope) in [(false, Scope::Exports), (true, Scope::Exports), (true, Scope::All)] {
        let rules = Rules { commonjs_members: on, scope, ..Rules::default() };
        let items = scan(&source, source_type, &rules).unwrap();
        let found: Vec<_> = items.into_iter().map(|i| (i.name, i.kind, i.line, i.documented)).collect();
        let counts = |new: bool, exported: bool| (on || !new) && (scope == Scope::All || exported);
        let expected: Vec<_> =
            wanted.iter().filter(|(_, n, e)| counts(*n, *e)).map(|(item, _, _)| item.clone()).collect();
        assert_eq!(found, expected, "{ext} {on} {scope:?}: {source}");
    }
}

/// Release-tag coverage is the share of counted top-level exports with exactly one of `@public`,
/// `@beta`, `@alpha` and `@internal`. With `--release-tags` (or a threshold) the JSON report has it
/// and the text report gets a line of it, and its threshold fails the scan under it.
#[hegel::test(test_cases = 100)]
fn release_tags_are_a_metric_when_asked(tc: TestCase) {
    let files = reports(&tc, true);
    let over = |i: &&Item| (i.documented || i.ignore.is_none()) && i.facts.exported && !i.kind.is_member();
    let of = |f: &FileReport| {
        let over: Vec<_> = f.items.iter().filter(over).collect();
        Totals { documented: over.iter().filter(|i| i.facts.release_tagged).count(), total: over.len() }
    };
    let all = files
        .iter()
        .map(of)
        .fold(Totals::default(), |a, b| Totals { documented: a.documented + b.documented, total: a.total + b.total });
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
    let on = threshold.is_some() || tc.draw(gs::booleans());
    let feature = ReleaseTags { on, threshold };

    let metrics = Vec::from_iter(feature.metric());
    let context = Context { metrics: &metrics, ..plain(&files) };
    let json: serde_json::Value = serde_json::from_str(&report::format("json").unwrap().render(&context)).unwrap();
    let shown = |v: &serde_json::Value| (v["releaseTagged"].as_u64(), v["releaseTagsPercent"].as_f64());
    let wanted = |t: Totals| if on { (Some(t.documented as u64), Some(t.percent())) } else { (None, None) };
    assert_eq!(shown(&json), wanted(all));
    assert!(json["files"].as_array().unwrap().iter().map(shown).eq(files.iter().map(|f| wanted(of(f)))));

    let line = format!("{:>6.1}%  {}/{}  release tags", all.percent(), all.documented, all.total);
    assert_eq!(metric_line(&context), on.then_some(line));
    assert_eq!(report::format("lcov").unwrap().render(&context), render("lcov", &files));

    let percent = all.percent();
    let failure = threshold
        .filter(|&t| percent < t)
        .map(|t| format!("{percent:.1}% with release tags, under the {t}% release tags threshold"));
    assert_eq!(gate(&feature, &files), Vec::from_iter(failure));
}

/// Type coverage is over JavaScript functions and methods: one is typed when its JSDoc gives each
/// `@param` (or `@arg`, or `@argument`) a `{type}`, and its `@returns` one when it has one. Other
/// items, and TypeScript, aren't counted for it. With `--types` (or a threshold) the JSON report
/// has it and the text report gets a line of it, and its threshold fails the scan under it.
#[hegel::test(test_cases = 100)]
fn javascript_functions_are_typed_by_their_tags(tc: TestCase) {
    let tags = tc.draw(
        gs::vecs(gs::sampled_from(vec![
            ("@param {string} a Docs.", true),
            ("@param a Docs.", false),
            ("@argument {number} b", true),
            ("@arg b", false),
            ("@returns {number} Docs.", true),
            ("@return Docs.", false),
            ("@since 1.2", true),
        ]))
        .max_size(3),
    );
    let documented = tc.draw(gs::booleans());
    let doc: String = tags.iter().map(|(tag, _)| format!(" * {tag}\n")).collect();
    let source = format!(
        "{}export function f(a, b) {{}}\nexport class C {{ m() {{}} }}\nexport const v = 1;\n",
        if documented { format!("/**\n * Docs.\n{doc} */\n") } else { String::new() }
    );
    let typed = documented && tags.iter().all(|(_, typed)| *typed);
    let (name, source_type) = tc.draw(gs::sampled_from(vec![
        ("mjs", SourceType::mjs()),
        ("jsx", SourceType::jsx()),
        ("cjs", SourceType::cjs()),
        ("ts", SourceType::ts()),
    ]));
    let source = if name == "cjs" { source.replace("export ", "") } else { source };
    let rules = Rules { scope: Scope::All, ..Rules::default() };
    let items = scan(&source, source_type, &rules).unwrap();
    let found: Vec<_> = items.iter().map(|i| i.facts.typed).collect();
    let wanted = if name == "ts" { vec![None; 4] } else { vec![Some(typed), None, Some(false), None] };
    assert_eq!(found, wanted, "{name}: {source}");

    let files = vec![FileReport { path: PathBuf::from(format!("m.{name}")), items }];
    let all = Totals {
        documented: wanted.iter().filter(|t| **t == Some(true)).count(),
        total: wanted.iter().filter(|t| t.is_some()).count(),
    };
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
    let on = threshold.is_some() || tc.draw(gs::booleans());
    let feature = Types { on, threshold };
    let metrics = Vec::from_iter(feature.metric());
    let context = Context { metrics: &metrics, ..plain(&files) };
    let json: serde_json::Value = serde_json::from_str(&report::format("json").unwrap().render(&context)).unwrap();
    let shown = (json["typed"].as_u64(), json["typesPercent"].as_f64());
    assert_eq!(shown, if on { (Some(all.documented as u64), Some(all.percent())) } else { (None, None) });

    let line = format!("{:>6.1}%  {}/{}  types", all.percent(), all.documented, all.total);
    assert_eq!(metric_line(&context), on.then_some(line));
    assert_eq!(report::format("lcov").unwrap().render(&context), render("lcov", &files));
    let percent = all.percent();
    let failure =
        threshold.filter(|&t| percent < t).map(|t| format!("{percent:.1}% with types, under the {t}% types threshold"));
    assert_eq!(gate(&feature, &files), Vec::from_iter(failure));
}

/// What a generated index re-exports from module `k`: nothing it can resolve, a name, or all of
/// the module's names (but `default`, through `export *`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Wanted {
    Name(String),
    All,
    AllButDefault,
}

/// The declarations a model of the generated package reaches: `(module, name)` for each, and
/// each module reached at all.
fn reach(
    stars: &[Option<usize>],
    defaults: &[Option<String>],
    start: Vec<(usize, Wanted)>,
) -> (Vec<(usize, String)>, Vec<usize>) {
    let (mut found, mut modules, mut seen, mut work) = (Vec::new(), Vec::new(), Vec::new(), start);
    while let Some((k, wanted)) = work.pop() {
        if seen.contains(&(k, wanted.clone())) {
            continue;
        }
        seen.push((k, wanted.clone()));
        modules.push(k);
        let own = [format!("f{k}_0"), format!("f{k}_1")];
        match wanted {
            Wanted::Name(name) if name == "default" => found.extend(defaults[k].clone().map(|d| (k, d))),
            Wanted::Name(name) if own.contains(&name) => found.push((k, name)),
            Wanted::Name(name) => work.extend(stars[k].map(|next| (next, Wanted::Name(name)))),
            every => {
                found.extend(own.map(|name| (k, name)));
                if every == Wanted::All {
                    found.extend(defaults[k].clone().map(|d| (k, d)));
                }
                work.extend(stars[k].map(|next| (next, Wanted::AllButDefault)));
            }
        }
    }
    found.sort();
    found.dedup();
    modules.sort();
    modules.dedup();
    (found, modules)
}

/// A generated package: modules that declare functions, a default and a CommonJS export, some
/// re-exporting the next with `export *`, and an index that re-exports them every way there is,
/// named in package.json directly, through tsconfig's `outDir`, or through a declaration file.
/// The public scope counts exactly what the model says its entry points reach, each in its own
/// file. An entry point with no source, a package.json that isn't JSON, or none at all fails the
/// load, naming the culprit.
#[hegel::test(test_cases = 60)]
fn the_public_scope_counts_what_entry_points_reach(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let (mut stars, mut defaults) = (vec![None; n], Vec::new());
    for (i, star) in stars.iter_mut().enumerate() {
        // Its default export, and the item that is.
        let (default, item) = tc.draw(gs::sampled_from(vec![
            ("export default function dI() {}", Some("dI")),
            ("export default class dI {}", Some("dI")),
            ("function dI() {}\nexport default dI;", Some("dI")),
            ("export default 1;", Some("default")),
            ("", None),
        ]));
        defaults.push(item.map(|d| d.replace('I', &i.to_string())));
        let mut text = format!(
            "export function f{i}_0() {{}}\nexport function f{i}_1() {{}}\n{}\nfunction hidden{i}() {{}}\nexports.c{i} = 1;\n",
            default.replace('I', &i.to_string())
        );
        if i + 1 < n && tc.draw(gs::booleans()) {
            text.push_str(&format!("export * from './m{}.js';\n", i + 1));
            *star = Some(i + 1);
        }
        write(root, &format!("src/m{i}.ts"), &text);
    }
    let mut index = String::from("export const own = 1;\nexports.cjs = 1;\n");
    let mut wanted = Vec::new();
    let mut written = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let (way, k, j) = (
            tc.draw(gs::integers::<u8>().max_value(8)),
            tc.draw(gs::integers::<usize>().max_value(n - 1)),
            tc.draw(gs::integers::<usize>().max_value(2)),
        );
        if written.contains(&(way, k, j)) {
            continue;
        }
        written.push((way, k, j));
        // A name from module k, or from one after it, which only `export *` there finds. f{m}_2
        // is no module's.
        let m = (k + tc.draw(gs::integers::<usize>().max_value(1))).min(n - 1);
        let f = format!("f{m}_{j}");
        let (text, want) = match way {
            0 => (format!("export * from './m{k}';"), Some(Wanted::AllButDefault)),
            1 => (format!("export {{ {f} as e{k}_{j} }} from './m{k}.js';"), Some(Wanted::Name(f))),
            2 => (format!("export {{ default as d{k} }} from './m{k}';"), Some(Wanted::Name("default".into()))),
            3 => (format!("export * as ns{k} from './m{k}';"), Some(Wanted::All)),
            4 => (
                format!("import {{ {f} as g{k}_{j} }} from './m{k}.js';\nexport {{ g{k}_{j} }};"),
                Some(Wanted::Name(f)),
            ),
            5 => (format!("import dd{k} from './m{k}';\nexport {{ dd{k} }};"), Some(Wanted::Name("default".into()))),
            6 => (format!("import * as all{k} from './m{k}';\nexport {{ all{k} }};"), Some(Wanted::All)),
            // What resolves to nothing, or to what isn't a module it can read.
            7 => (
                format!(
                    "export * from './missing{k}';\nexport {{ x{k} }} from './missing{k}';\nimport {{ z{k} }} from './missing{k}';\nexport * from './data.json';\nexport * from './latin1.js';\nexport * from '\\uD800';"
                ),
                None,
            ),
            _ => (format!("import {{ f{k}_0 as unused{k} }} from './m{k}';"), None),
        };
        index.push_str(&text);
        index.push('\n');
        wanted.extend(want.map(|w| (k, w)));
    }
    write(root, "src/index.ts", &index);
    write(root, "src/data.json", "{}");
    fs::write(root.join("src/latin1.js"), b"// caf\xe9\n").unwrap();

    // How package.json names the index, the tsconfig.json that goes with it, and whether the index
    // is found.
    let (manifest, tsconfig, found) = tc.draw(gs::sampled_from(vec![
        (r#"{ "main": "./src/index.ts" }"#, None, true),
        (r#"{ "exports": ["./src/index.ts", null] }"#, Some("{"), true),
        (
            r#"{ "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" }, "./*": "./dist/*.js" } }"#,
            Some(r#"{ "compilerOptions": { "outDir": "dist", "rootDir": "src" } }"#),
            true,
        ),
        (r#"{ "module": "lib/index.js" }"#, Some("// built\n{ \"compilerOptions\": { \"outDir\": \"./lib\", }, }"), true),
        (r#"{ "typings": "./types/index.d.ts", "main": "./src/index.ts" }"#, None, false),
    ]));
    write(root, "package.json", manifest);
    if let Some(tsconfig) = tsconfig {
        write(root, "tsconfig.json", tsconfig);
    }
    // From the package's folder, or from a path under it.
    let from_path = tc.draw(gs::booleans());
    let (start, paths) =
        if from_path { (root.join("nowhere"), vec![root.join("src")]) } else { (root.to_owned(), vec![]) };
    let api = docov::public::Api::load(&start, &paths);
    if !found {
        let errors = api.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("entry point ./types/index.d.ts has no source file"), "{errors:?}");
        return;
    }
    let rules = Rules { scope: Scope::Public, public: Some(std::sync::Arc::new(api.unwrap())), ..Rules::default() };
    let mut counted = Vec::new();
    for i in 0..n {
        let report = scan_file(&root.join(format!("src/m{i}.ts")), &rules).unwrap();
        counted.extend(report.items.into_iter().map(|item| (i, item.name)));
    }
    let index_items = scan_file(&root.join("src/index.ts"), &rules).unwrap().items;
    assert_eq!(index_items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["own", "cjs"]);
    let explained = collect::explain(&root.join("src/index.ts"), &rules).unwrap();
    assert!(explained[..2].iter().all(|line| line.ends_with("  undocumented")), "{explained:?}");
    let (mut names, modules) = reach(&stars, &defaults, wanted);
    names.extend(modules.iter().map(|&k| (k, format!("c{k}"))));
    names.sort();
    counted.sort();
    assert_eq!(counted, names, "{index}");
    // Scanned without its path, a file has nothing reached.
    let source = fs::read_to_string(root.join("src/index.ts")).unwrap();
    assert_eq!(scan(&source, SourceType::ts(), &rules).unwrap(), []);
}

/// The public scope's load fails without a package.json, or with one that isn't JSON, and from
/// the command line its errors exit 2, while a package it can load is counted.
#[hegel::test(test_cases = 5)]
fn the_public_scope_needs_a_package(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/index.ts", "/** A. */\nexport const a = 1;\nexport const b = 1;\n");
    write(root, "src/other.ts", "export const c = 1;\n");
    let docov = |args: &[&str]| docov_command().args(args).current_dir(root).output().unwrap();
    let broken = tc.draw(gs::booleans());
    if broken {
        let errors = docov::public::Api::load(&root.join("src"), &[root.join("src")]).unwrap_err();
        assert_eq!(errors, ["the public scope needs a package.json"]);
        write(root, "package.json", "{ main: 1 }");
        let errors = docov::public::Api::load(root, &[]).unwrap_err();
        assert!(
            errors[0].starts_with(&format!("{}: key must be a string", root.join("package.json").display())),
            "{errors:?}"
        );
        let out = docov(&["--scope", "public"]);
        assert_eq!((out.status.code(), out.stdout.is_empty()), (Some(2), true));
        fs::write(root.join("package.json"), b"{ \"main\": \"caf\xe9\" }").unwrap();
        let errors = docov::public::Api::load(root, &[]).unwrap_err();
        assert!(errors[0].contains("stream did not contain valid UTF-8"), "{errors:?}");
        return;
    }
    write(root, "package.json", r#"{ "main": "./src/index.ts" }"#);
    let out = docov(&["--scope", "public", "--format", "json"]);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((out.status.code(), &json["documented"], &json["total"]), (Some(0), &1.into(), &2.into()));
}

/// `--explain` has a line for every item a generated module declares, in its order and at its
/// place, saying whether it counts and is documented as the rules decide, and a line for each
/// declaration and block the scan passed over. A file skipped as generated has one line, saying
/// so, and a file that can't be read fails, naming it.
#[hegel::test(test_cases = 100)]
fn explain_says_why_each_item_counts_and_what_was_passed_over(tc: TestCase) {
    let module = Module::generate(&tc);
    let mut rules = drawn_rules(&tc);
    // Without the package's entry points, nothing is reached.
    if tc.draw(gs::weighted_booleans(0.1)) {
        rules.scope = Scope::Public;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ts");
    fs::write(&path, &module.source).unwrap();
    let lines = collect::explain(&path, &rules).unwrap();
    let at = |line: usize, column: usize| format!("{}:{line}:{column}  ", path.display());
    if module.generated && rules.skip_generated {
        assert_eq!(lines, [format!("{}skipped the file: its first comment says it's @generated", at(1, 1))]);
        return;
    }
    let (skips, items): (Vec<_>, Vec<_>) = lines.iter().partition(|line| line.contains("  skipped "));
    assert_eq!(skips.len(), module.skipped, "{}", lines.join("\n"));
    assert_eq!(items.len(), module.expected.len(), "{}", lines.join("\n"));
    for (line, expected) in items.iter().zip(&module.expected) {
        let item = &expected.item;
        let what = format!("{}{} {}  ", at(item.line, item.column), item.kind.as_str(), item.name);
        let status = line.strip_prefix(&what).unwrap_or_else(|| panic!("{line} isn't {what}"));
        let wanted = match expected.by(&rules) {
            None => "left out: ",
            _ if rules.scope == Scope::Public => "left out: not reached",
            Some(item) if item.documented => "documented",
            Some(item) if item.ignored() => "ignored by the docov-ignore",
            Some(_) => "undocumented",
        };
        assert!(status.starts_with(wanted), "{line}: {wanted}");
    }
    assert!(matches!(collect::explain(&dir.path().join("gone.ts"), &rules), Err(collect::Error::Read { .. })));
    fs::write(&path, format!("{}export function (", module.source)).unwrap();
    assert!(matches!(collect::explain(&path, &rules), Err(collect::Error::Syntax { .. })));

    // What only a file's own item and the members of `"members": "all"` say.
    let source = "/** @file Helpers. */\n\n/**\n * Colors.\n * @property Red The red one.\n */\nexport enum Color {\n  Red,\n}\n";
    fs::write(&path, source).unwrap();
    let rules =
        Rules { file_docs: FileDocs::Require, members: Members::All, require_description: true, ..Rules::default() };
    assert_eq!(
        collect::explain(&path, &rules).unwrap(),
        [
            format!("{}module {}  documented by the block at 1:1", at(1, 1), path.display()),
            format!("{}enum Color  documented by the block at 3:1", at(7, 1)),
            format!("{}property Color.Red  documented by a matching @property in its parent's block", at(8, 3)),
        ]
    );
}

/// From the command line, `--explain FILE` writes its lines in place of a report and exits 0, and
/// a file it can't read, or output it can't write, exits 2.
#[hegel::test(test_cases = 3)]
fn explain_runs_from_the_command_line(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let documented = tc.draw(gs::booleans());
    let doc = if documented { "/** A. */\n" } else { "" };
    write(dir.path(), "a.ts", &format!("{doc}export const a = 1;\nexport * from './b';\n"));
    let docov = |args: &[&str]| {
        let mut command = docov_command();
        command.args(args).current_dir(dir.path());
        command
    };
    let out = docov(&["--explain", "a.ts", "--threshold", "100"]).output().unwrap();
    let line = if documented {
        "a.ts:2:1  variable a  documented by the block at 1:1"
    } else {
        "a.ts:1:1  variable a  undocumented"
    };
    let skipped =
        format!("a.ts:{}:1  skipped *: a re-export, which counts in its own file", if documented { 3 } else { 2 });
    assert_eq!((out.status.code(), String::from_utf8(out.stdout).unwrap()), (Some(0), format!("{line}\n{skipped}\n")));
    let out = docov(&["--explain", "gone.ts"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().contains("gone.ts"));
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    assert_eq!(docov(&["--explain", "a.ts"]).stdout(writer).output().unwrap().status.code(), Some(2));
}

/// An undocumented item under a `docov-ignore` is out of every format's totals and of the text
/// report, and the JSON report marks it, with its reason. A documented one counts as documented.
#[hegel::test(test_cases = 100)]
fn ignored_items_are_out_of_the_totals(tc: TestCase) {
    let files = reports(&tc, true);
    let counted = |f: &FileReport| f.items.iter().filter(|i| i.documented || i.ignore.is_none()).count();
    let totals = Totals::of(&files);
    assert_eq!(totals.total, files.iter().map(counted).sum::<usize>());
    assert_eq!(totals.documented, files.iter().flat_map(|f| &f.items).filter(|i| i.documented).count());

    let text = render("text", &files);
    let listed = text.lines().filter(|l| l.starts_with("         ")).count();
    let undocumented = files.iter().flat_map(|f| &f.items).filter(|i| !i.documented && i.ignore.is_none()).count();
    assert_eq!(listed, undocumented, "{text}");
    // A file with nothing counted has no line.
    let headed = text.lines().filter(|l| l.ends_with(".ts") && !l.starts_with("         ")).count();
    assert_eq!(headed, files.iter().filter(|f| counted(f) > 0).count(), "{text}");

    let json: serde_json::Value = serde_json::from_str(&render("json", &files)).unwrap();
    let marks = json["files"].as_array().unwrap().iter().flat_map(|f| f["items"].as_array().unwrap().clone());
    let wanted = files.iter().flat_map(|f| &f.items).map(|i| {
        let ignore = i.ignore.as_ref().filter(|_| !i.documented);
        (ignore.map(|_| true), ignore.and_then(|i| i.reason.clone()))
    });
    let marks = marks.map(|m| (m["ignored"].as_bool(), m["reason"].as_str().map(String::from)));
    assert!(marks.eq(wanted));

    let lcov = render("lcov", &files);
    let found: usize = lcov.lines().filter_map(|l| l.strip_prefix("LF:")).map(|n| n.parse::<usize>().unwrap()).sum();
    let mut lines: Vec<(usize, usize)> = Vec::new();
    for (f, file) in files.iter().enumerate() {
        lines.extend(file.items.iter().filter(|i| i.documented || i.ignore.is_none()).map(|i| (f, i.line)));
    }
    lines.dedup();
    assert_eq!(found, lines.len(), "{lcov}");
}

/// Every registered format but json reports ignored items as if they weren't there, leaving them
/// out of what it lists, the lines it counts, its totals and its metrics, whatever else it reports
/// and however it orders them, and so does a `--compare` base's json report. json lists them too,
/// marked, and is otherwise the same.
#[hegel::test(test_cases = 100)]
fn every_format_leaves_ignored_items_out(tc: TestCase) {
    let mut drawn = Drawn::draw(&tc);
    drawn.files = reports(&tc, true);
    let flag = || tc.draw(gs::booleans());
    drawn.text = report::Text { omit_covered: flag(), quiet: flag(), by_kind: flag() };
    let sort = tc.draw(gs::sampled_from(vec![Sort::Position, Sort::Priority]));
    let metrics: Vec<_> =
        [ReleaseTags { on: flag(), threshold: None }.metric(), Types { on: flag(), threshold: None }.metric()]
            .into_iter()
            .flatten()
            .collect();
    let examples = flag();
    let counted = |files: &[FileReport]| -> Vec<FileReport> {
        files.iter().map(|f| FileReport { path: f.path.clone(), items: f.counted().cloned().collect() }).collect()
    };
    let without_ignored = counted(&drawn.files);
    assert_eq!(Totals::of(&without_ignored), Totals::of(&drawn.files));
    // The base, read back from its json report with its ignored items and without them.
    let old = reports(&tc, true);
    let base = |files: &[FileReport]| {
        let json = report::format("json").unwrap().render(&Context { files, ..drawn.context() });
        serde_json::from_str::<report::Base>(&json).unwrap()
    };
    let compared = flag();
    let bases = compared.then(|| (base(&old), base(&counted(&old))));
    let base = bases.as_ref().map(|(all, _)| all);
    let context = Context { base, sort, examples, metrics: &metrics, ..drawn.context() };
    let without = Context { files: &without_ignored, base: bases.as_ref().map(|(_, shown)| shown), ..context };
    for format in report::FORMATS {
        let (all, shown) = (format.render(&context), format.render(&without));
        if format.name != "json" {
            assert_eq!(all, shown, "{}", format.name);
            continue;
        }
        let mut json: serde_json::Value = serde_json::from_str(&all).unwrap();
        for file in json["files"].as_array_mut().unwrap() {
            file["items"].as_array_mut().unwrap().retain(|i| i.get("ignored").is_none());
        }
        assert_eq!(json, serde_json::from_str::<serde_json::Value>(&shown).unwrap());
    }
}

/// With `--report-unused-ignores`, each `docov-ignore-next-line` whose items are all documented
/// fails the scan, once, at the comment. Off, or with none, nothing does.
#[hegel::test(test_cases = 100)]
fn unused_ignores_fail_the_scan(tc: TestCase) {
    let files = reports(&tc, true);
    let mut wanted = Vec::new();
    for file in &files {
        // Each comment's line, and whether all its items are documented.
        let mut comments: Vec<(usize, bool)> = Vec::new();
        for item in &file.items {
            let Some(ignore) = item.ignore.as_ref().filter(|i| !i.file) else { continue };
            match comments.last_mut() {
                Some((line, all)) if *line == ignore.line => *all &= item.documented,
                _ => comments.push((ignore.line, item.documented)),
            }
        }
        let unused = comments.iter().filter(|(_, all)| *all);
        wanted.extend(unused.map(|(line, _)| format!("{}:{line}:1: unused docov-ignore", file.path.display())));
    }
    let settings = settings(tempfile::tempdir().unwrap().path(), "{}", &[]);
    let root = docov::root::Root::new(Path::new(""));
    let run = docov::feature::Run { reports: &files, root: &root, settings: &settings };
    assert_eq!(ReportUnusedIgnores(false).gate(&run), Vec::<String>::new());
    assert_eq!(ReportUnusedIgnores(true).gate(&run), wanted);
}

/// The totals by kind add up to the total, one per kind there is, in order. The JSON report has
/// them, and with `--by-kind` the text report gets a line of them.
#[hegel::test(test_cases = 100)]
fn totals_by_kind_add_up(tc: TestCase) {
    let files = reports(&tc, true);
    let kinds = Totals::by_kind(&files);
    let sum = kinds.iter().fold(Totals::default(), |a, (_, b)| Totals {
        documented: a.documented + b.documented,
        total: a.total + b.total,
    });
    assert_eq!(sum, Totals::of(&files));
    assert!(kinds.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert!(kinds.iter().all(|(_, totals)| totals.total > 0));

    let json: serde_json::Value = serde_json::from_str(&render("json", &files)).unwrap();
    let by_kind = json["kinds"].as_object().unwrap();
    assert_eq!(by_kind.len(), kinds.len());
    for (kind, totals) in &kinds {
        let shown = &by_kind[kind.as_str()];
        assert_eq!(
            (shown["documented"].as_u64(), shown["total"].as_u64()),
            (Some(totals.documented as u64), Some(totals.total as u64))
        );
        assert_eq!(shown["percent"].as_f64(), Some(totals.percent()));
    }

    let by_kind = |name| {
        let text = report::Text { by_kind: true, ..report::Text::default() };
        report::format(name).unwrap().render(&Context {
            files: &files,
            failures: &[],
            now: UNIX_EPOCH,
            threshold: None,
            text,
            base: None,
            packages: None,
            color: false,
            sort: Sort::Position,
            examples: false,
            metrics: &[],
        })
    };
    let line: Vec<String> = kinds.iter().map(|(k, t)| format!("{} {}/{}", k.as_str(), t.documented, t.total)).collect();
    assert_eq!(by_kind("text"), render("text", &files) + &format!("by kind: {}\n", line.join(", ")));
    assert_eq!(by_kind("json"), render("json", &files));
}

/// Name patterns load as regexes, the same patterns loading the same. One that isn't a regex, or an
/// `ignore` key docov doesn't know, fails to load, naming it.
#[hegel::test(test_cases = 10)]
fn name_patterns_load_as_regexes(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let load = |names: &[&str]| {
        let path = dir.path().join("names.json");
        fs::write(&path, serde_json::json!({ "ignore": { "names": names } }).to_string()).unwrap();
        config::Config::load(Some(&path)).unwrap()
    };
    let (a, b) = (tc.draw(gs::sampled_from(vec!["^_", "a|b"])), tc.draw(gs::sampled_from(vec!["^_", "a|b"])));
    assert_eq!(load(&[a]) == load(&[b]), a == b);

    let (keys, culprit) = tc.draw(gs::sampled_from(vec![
        (r#"{ "ignore": { "names": ["("] } }"#, "unclosed group"),
        (r#"{ "ignore": { "names": ["^_", "[z"] } }"#, "unclosed character class"),
        (r#"{ "ignore": { "nmes": [] } }"#, "unknown field `nmes`"),
        (r#"{ "ignore": { "names": "^_" } }"#, "invalid type: string"),
    ]));
    let path = dir.path().join("docov.json");
    fs::write(&path, keys).unwrap();
    let error = config::Config::load(Some(&path)).unwrap_err().to_string();
    assert!(error.contains(culprit), "{error}");
}

fn write(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// An exclude glob globset reads but can't build: its alternates nest deeper than the regex parser
/// allows (it fails from 125 deep). A glob too big to build fails at the same step, but takes about
/// a second under coverage, enough over ten cases to trip Hegel's TooSlow check on a loaded machine.
fn unbuildable_glob() -> String {
    format!("{}b{}", "{a,".repeat(200), "}".repeat(200))
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
    /// docov.json with an exclude glob that doesn't parse, or can't be built.
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
        let out = docov_command().arg("--print-schema").output().unwrap();
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
    let unbuildable = settings == Settings::BadGlob && tc.draw(gs::weighted_booleans(0.2));
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
            let glob = if unbuildable { unbuildable_glob() } else { "src/[z".into() };
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
    let mut command = docov_command();
    command.args(&args).current_dir(cwd);
    if github_actions {
        command.env("GITHUB_ACTIONS", "true");
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
        Settings::BadGlob => Some(if unbuildable { "parsing pattern" } else { "[z" }),
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
        let out = docov_command().args(args).current_dir(dir.path()).output().unwrap();
        let (stdout, stderr) = (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap());
        assert_eq!((out.status.code(), stdout.as_str()), (Some(2), ""), "{args:?} {stderr}");
        assert!(stderr.contains(culprit), "{culprit}: {stderr}");
    };
    // Bad arguments, and what the error names.
    let bad_args: [(&[&str], &str); 8] = [
        (&["--config", "missing.json"], "missing.json"),
        (&["--output", "o.txt", "--format", "lcov=x.lcov"], "--output"),
        (&["--format", "json=README.md/x.json"], "README.md"),
        (&["--threshold", "150"], "150"),
        (&["gone"], "gone"),
        (&["README.md"], "README.md"),
        (&["--compare", "missing.json"], "missing.json"),
        (&["--compare", "README.md"], "README.md"),
    ];
    for (args, culprit) in bad_args {
        fails_naming(args, culprit);
    }
    // Bad files, each removed after its run, and what the error names.
    let config = |value: serde_json::Value| value.to_string().into_bytes();
    let bad_files = [
        ("docov.json", br#"{ "treshold": 1 }"#.to_vec(), "treshold"),
        ("docov.json", br#"{ "threshold": "80" }"#.to_vec(), "invalid type"),
        ("docov.json", br#"{ "threshold": 150 }"#.to_vec(), "expected a number from 0 to 100"),
        ("docov.jsonc", br#"{ "threshold": 5 / }"#.to_vec(), "doesn't start a comment"),
        ("docov.json", config(serde_json::json!({ "exclude": ["src/[z"] })), "[z"),
        ("docov.json", config(serde_json::json!({ "exclude": [unbuildable_glob()] })), "parsing pattern"),
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
        let out = docov_command().args(["--format", format]).current_dir(dir.path()).stdout(writer).output().unwrap();
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
        (r#"{ "scope": "some" }"#, "unknown variant `some`, expected one of `exports`, `all`, `public`", 17),
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
        (r#"{ "typesThreshold": true }"#, "invalid type: boolean `true`, expected f64", 24),
    ]));
    let path = dir.path().join("docov.json");
    // A threshold's glob that globset reads but can't build, like the walk's.
    if tc.draw(gs::weighted_booleans(0.1)) {
        fs::write(&path, serde_json::json!({ "threshold": { unbuildable_glob(): 1 } }).to_string()).unwrap();
        let error = config::Config::load(Some(&path)).unwrap_err().to_string();
        assert!(error.contains("parsing pattern"), "{error}");
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

/// The flags and docov.json keys a generated run sets.
#[derive(Default)]
struct Run {
    args: Vec<String>,
    keys: serde_json::Map<String, serde_json::Value>,
}

impl Run {
    /// Sets a setting by its flag, its key, both or neither, and gives the value the flag or else
    /// the key set, if either did. `flag` turns a value into the flag's arguments.
    fn set<T: Clone + std::fmt::Debug + Send + Sync + serde::Serialize + 'static>(
        &mut self,
        tc: &TestCase,
        key: &str,
        values: Vec<T>,
        flag: impl Fn(&T) -> Vec<String>,
    ) -> Option<T> {
        let from_key = tc.draw(gs::optional(gs::sampled_from(values.clone())));
        let from_flag = tc.draw(gs::optional(gs::sampled_from(values)));
        if let Some(value) = &from_key {
            self.keys.insert(key.into(), serde_json::to_value(value).unwrap());
        }
        // A value that makes no arguments is no flag.
        let from_flag = from_flag.filter(|value| !flag(value).is_empty());
        if let Some(value) = &from_flag {
            self.args.extend(flag(value));
        }
        from_flag.or(from_key)
    }

    /// Sets an on-or-off setting by its flag, its key, both or neither, and says whether it's on: a
    /// flag can only turn it on.
    fn switch(&mut self, tc: &TestCase, key: &str, flag: &str) -> bool {
        let on = tc.draw(gs::optional(gs::booleans()));
        if let Some(on) = on {
            self.keys.insert(key.into(), on.into());
        }
        let flagged = tc.draw(gs::booleans());
        if flagged {
            self.args.push(flag.into());
        }
        on.unwrap_or_default() || flagged
    }

    /// The settings the run's flags and docov.json (in `dir`) make, loaded in-process.
    fn settings(&self, dir: &Path) -> docov::Settings {
        let path = dir.join("docov.json");
        fs::write(&path, serde_json::Value::Object(self.keys.clone()).to_string()).unwrap();
        let keys = config::Config::load(Some(&path)).unwrap().keys;
        let args = std::iter::once("docov".to_owned()).chain(self.args.iter().cloned());
        let matches = feature::args(clap::Command::new("docov")).try_get_matches_from(args).unwrap();
        docov::Settings::new(&matches, keys)
    }
}

/// Each rule comes from its flag when there is one, else its docov.json key, else its default.
#[hegel::test(test_cases = 50)]
fn rules_come_from_flags_over_the_config(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut run = Run::default();
    let scope = run.set(&tc, "scope", vec![Scope::Exports, Scope::All], |s| {
        vec!["--scope".into(), serde_json::to_value(s).unwrap().as_str().unwrap().into()]
    });
    let require_description = run.switch(&tc, "requireDescription", "--require-description");
    let tag_lists = vec![vec![], vec!["internal".to_owned()], vec!["ignore".into(), "hidden".into()]];
    let exclude_tags = run.set(&tc, "excludeTags", tag_lists, |tags| {
        tags.iter().flat_map(|tag| ["--exclude-tag".to_owned(), tag.clone()]).collect()
    });

    let kind_lists = vec![vec![Kind::Function], vec![Kind::Class, Kind::Method]];
    let kinds = run.set(&tc, "kinds", kind_lists, |kinds| {
        vec!["--kind".into(), kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(",")]
    });
    let name_lists = vec![vec!["^_".to_owned()], vec!["^a".into(), "b$".into()]];
    let names = run.set(&tc, "ignore", name_lists, |names| {
        names.iter().flat_map(|name| ["--ignore-name".to_owned(), name.clone()]).collect()
    });
    // The key is an object.
    if let Some(names) = run.keys.get_mut("ignore") {
        *names = serde_json::json!({ "names": names.take() });
    }

    let level_lists = vec![vec![Access::Public], vec![Access::Protected, Access::Private]];
    let access = run.set(&tc, "access", level_lists, |levels| {
        let names: Vec<_> =
            levels.iter().map(|l| serde_json::to_value(l).unwrap().as_str().unwrap().to_owned()).collect();
        vec!["--access".into(), names.join(",")]
    });

    let inherit = run.set(&tc, "inherit", vec![Inherit::Tag, Inherit::Override], |_| vec![]);

    let exports = tc.draw(gs::optional(gs::sampled_from(vec![
        (serde_json::json!({}), Exports::default()),
        (serde_json::json!({ "esm": false }), Exports { esm: false, ..Exports::default() }),
        (serde_json::json!({ "cjs": false, "globals": true }), Exports { esm: true, cjs: false, globals: true }),
    ])));
    if let Some((keys, _)) = &exports {
        run.keys.insert("exports".into(), keys.clone());
    }

    let examples = tc.draw(gs::booleans());
    if examples {
        run.args.push("--examples".into());
    }
    let examples_threshold =
        run.set(&tc, "examplesThreshold", vec![0.0, 50.5], |t| vec!["--examples-threshold".into(), t.to_string()]);
    let release_tags = tc.draw(gs::booleans());
    if release_tags {
        run.args.push("--release-tags".into());
    }
    let release_threshold = run
        .set(&tc, "releaseTagsThreshold", vec![10.0, 90.0], |t| vec!["--release-tags-threshold".into(), t.to_string()]);
    let types = tc.draw(gs::booleans());
    if types {
        run.args.push("--types".into());
    }
    let types_threshold =
        run.set(&tc, "typesThreshold", vec![20.0], |t| vec!["--types-threshold".into(), t.to_string()]);
    let sort = tc.draw(gs::sampled_from(vec![Sort::Position, Sort::Priority]));
    run.args.extend(["--sort".into(), if sort == Sort::Priority { "priority" } else { "position" }.into()]);
    let ignore_deprecated = run.set(&tc, "ignoreDeprecated", vec![false, true], |_| vec![]);
    let skip_generated = run.set(&tc, "skipGenerated", vec![false, true], |_| vec![]);
    let file_docs = run.set(&tc, "fileDocs", vec![FileDocs::Off, FileDocs::Require], |_| vec![]);
    let virtual_types = run.set(&tc, "virtual", vec![false, true], |_| vec![]);
    let members = run.set(&tc, "members", vec![Members::Classes, Members::All], |_| vec![]);
    let commonjs_members = run.set(&tc, "commonjsMembers", vec![false, true], |_| vec![]);

    let settings = run.settings(dir.path());
    let rules = settings.rules();
    assert_eq!(
        (rules.ignore_deprecated, rules.skip_generated),
        (ignore_deprecated.unwrap_or_default(), skip_generated.unwrap_or_default())
    );
    assert_eq!(rules.exports, exports.map_or(Exports::default(), |(_, e)| e));
    assert_eq!(rules.file_docs, file_docs.unwrap_or_default());
    assert_eq!(*settings.get::<VirtualTypes>(), VirtualTypes(virtual_types.unwrap_or_default()));
    assert_eq!(rules.virtual_types, virtual_types.unwrap_or_default());
    assert_eq!(rules.members, members.unwrap_or_default());
    assert_eq!(*settings.get::<CommonjsMembers>(), CommonjsMembers(commonjs_members.unwrap_or_default()));
    assert_eq!(rules.commonjs_members, commonjs_members.unwrap_or_default());
    assert_eq!(rules.inherit, inherit.unwrap_or_default());
    assert_eq!(rules.access, access);
    let patterns: Vec<&str> = rules.ignore_names.iter().map(regex::Regex::as_str).collect();
    assert_eq!(patterns, names.unwrap_or_default());
    assert_eq!(*settings.get::<Sort>(), sort);
    assert_eq!(settings.get::<Examples>().text, examples);
    let release_tags = ReleaseTags { on: release_tags || release_threshold.is_some(), threshold: release_threshold };
    assert_eq!(*settings.get::<ReleaseTags>(), release_tags);
    let types = Types { on: types || types_threshold.is_some(), threshold: types_threshold };
    assert_eq!(*settings.get::<Types>(), types);
    assert_eq!(settings.metrics().len(), usize::from(release_tags.on) + usize::from(types.on));
    assert_eq!(*settings.get::<Examples>(), Examples { text: examples, threshold: examples_threshold });
    assert_eq!(rules.kinds, kinds);
    assert_eq!(*settings.get::<Scope>(), scope.unwrap_or_default());
    assert_eq!(rules.scope, scope.unwrap_or_default());
    assert_eq!(rules.require_description, require_description, "{:?}", run.args);
    assert_eq!(rules.exclude_tags, exclude_tags.unwrap_or_default());
}

/// Lines put above others are indented and ended like them (a last line with no ending takes the
/// file's first one), and every other line stays as it was.
#[hegel::test(test_cases = 200)]
fn inserted_lines_match_the_line_below(tc: TestCase) {
    let (indents, endings) = (vec!["", "  ", "\t", " \t "], vec!["\n", "\r\n"]);
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let mut lines = Vec::new();
    for i in 0..count {
        let ending =
            if i + 1 == count && tc.draw(gs::booleans()) { "" } else { tc.draw(gs::sampled_from(endings.clone())) };
        lines.push((tc.draw(gs::sampled_from(indents.clone())), format!("x{i}"), ending));
    }
    let source: String = lines.iter().map(|(indent, text, ending)| format!("{indent}{text}{ending}")).collect();
    let mut inserts = Vec::new();
    for line in 1..=count {
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
            inserts.push((line, tc.draw(gs::sampled_from(vec!["// a", "/**\n * b\n */"]))));
        }
    }
    let first_ending = if lines[0].2.is_empty() { "\n" } else { lines[0].2 };
    let mut wanted = String::new();
    for (i, (indent, text, ending)) in lines.iter().enumerate() {
        let inserted_ending = if ending.is_empty() { first_ending } else { ending };
        for (_, insert) in inserts.iter().filter(|(line, _)| *line == i + 1) {
            for insert_line in insert.lines() {
                wanted.push_str(&format!("{indent}{insert_line}{inserted_ending}"));
            }
        }
        wanted.push_str(&format!("{indent}{text}{ending}"));
    }
    assert_eq!(rewrite::insert_above(&source, &inserts), wanted);
}

/// `--add-ignore` puts a `docov-ignore-next-line` (with the reason given) above each line with an
/// undocumented item, after which each item is documented or ignored, and running it again adds
/// none. A file it can't read or write fails, naming it.
#[hegel::test(test_cases = 50)]
fn adding_ignores_covers_every_undocumented_item(tc: TestCase) {
    let Module { source, .. } = Module::generate(&tc);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ts");
    fs::write(&path, &source).unwrap();
    let rules = Rules::from(tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::All])));
    let reason = tc.draw(gs::optional(gs::sampled_from(vec!["", "legacy", " later "])));
    let mut run = Run::default();
    run.args.push(reason.map_or("--add-ignore".into(), |r| format!("--add-ignore={r}")));
    let settings = run.settings(dir.path());

    let before = scan_file(&path, &rules).unwrap();
    let mut lines: Vec<usize> = before.counted().filter(|i| !i.documented).map(|i| i.line).collect();
    lines.dedup();
    let added =
        |n: usize| Some(Ok(format!("added {n} docov-ignore-next-line comment{}", if n == 1 { "" } else { "s" })));
    assert_eq!(settings.edit(std::slice::from_ref(&before)), added(lines.len()));
    let after = scan_file(&path, &rules).unwrap();
    assert!(after.items.iter().all(|i| i.documented || i.ignored()), "{}", fs::read_to_string(&path).unwrap());
    let summary = |r: &FileReport| r.items.iter().map(|i| (i.name.clone(), i.documented)).collect::<Vec<_>>();
    assert_eq!(summary(&after), summary(&before));
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    let newly: Vec<_> = before.items.iter().zip(&after.items).filter(|(b, _)| !b.documented && !b.ignored()).collect();
    assert!(newly.iter().all(|(_, a)| a.ignore.as_ref().unwrap().reason.as_deref() == reason));
    assert_eq!(settings.edit(std::slice::from_ref(&after)), added(0));
    assert_eq!(AddIgnore(None).edit(std::slice::from_ref(&after)), None);

    // An undocumented item in a file that's gone, or can't be written.
    let item = Item {
        name: "a".into(),
        kind: Kind::Variable,
        line: 1,
        column: 1,
        documented: false,
        facts: Facts::default(),
        ignore: None,
    };
    let gone = FileReport { path: dir.path().join("gone.ts"), items: vec![item.clone()] };
    let locked = FileReport { path: dir.path().join("locked.ts"), items: vec![item] };
    fs::write(&locked.path, "export const a = 1;\n").unwrap();
    let mut permissions = fs::metadata(&locked.path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&locked.path, permissions).unwrap();
    for report in [gone, locked] {
        let error = settings.edit(std::slice::from_ref(&report)).unwrap().unwrap_err();
        assert!(error.starts_with(&format!("{}: ", report.path.display())), "{error}");
    }
}

/// From the command line, `--add-ignore` writes the comments and reports nothing else, and a file
/// it can't write fails the run.
#[hegel::test(test_cases = 5)]
fn add_ignore_runs_from_the_command_line(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    for f in 0..count {
        write(dir.path(), &format!("f{f}.ts"), "export const a = 1;\n");
    }
    let docov = |args: &[&str]| docov_command().args(args).current_dir(dir.path()).output().unwrap();
    let out = docov(&["--add-ignore=legacy"]);
    let message = format!("docov: added {count} docov-ignore-next-line comment{}\n", if count == 1 { "" } else { "s" });
    assert_eq!(
        (out.status.code(), out.stdout.is_empty(), String::from_utf8(out.stderr).unwrap()),
        (Some(0), true, message)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("f0.ts")).unwrap(),
        "// docov-ignore-next-line: legacy\nexport const a = 1;\n"
    );

    write(dir.path(), "locked.ts", "export const b = 1;\n");
    let mut permissions = fs::metadata(dir.path().join("locked.ts")).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(dir.path().join("locked.ts"), permissions).unwrap();
    let out = docov(&["--add-ignore"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.starts_with("docov: locked.ts: "), "{stderr}");
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

/// [`reports`] with `docov-ignore` comments, which the gates leave out as the report does, as
/// files `f<i>.ts` for the globs to match.
fn gated_reports(tc: &TestCase) -> Vec<FileReport> {
    let mut files = reports(tc, true);
    for (i, file) in files.iter_mut().enumerate() {
        file.path = PathBuf::from(format!("f{i}.ts"));
    }
    files
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
struct DrawnThresholds {
    global: Option<i64>,
    per_file: Option<i64>,
    globs: Vec<(usize, i64)>,
}

impl DrawnThresholds {
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
    let files = gated_reports(&tc);
    let mut top = DrawnThresholds::draw(&tc);
    let overrides: Vec<(usize, Option<DrawnThresholds>)> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|_| {
            (
                tc.draw(gs::integers::<usize>().max_value(GLOBS.len() - 1)),
                tc.draw(gs::optional(gs::just(()))).map(|()| DrawnThresholds::draw(&tc)),
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
    let top =
        docov::walk::Exclude { root: dir.path().to_owned(), globs: docov::glob::Globs::new(text(&exclude)).unwrap() };
    let excluder = overrides_setting.excluder(vec![top], &root);
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
    let files = gated_reports(&tc);
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
    let mut files = gated_reports(&tc);
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
            f.counted()
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
        for item in file.items.iter_mut().filter(|i| !i.documented && !i.ignored()) {
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
    assert!(excused.iter().flat_map(|f| &f.items).all(|i| i.documented || i.ignored()));

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

/// The gates judge the items the report shows. Leaving the ignored items out changes no
/// threshold's verdict (overall, per file or per glob) and no `maxUndocumented` one, and a
/// baseline that names ignored items lets the same items through either way: ignored ones stay
/// ignored instead of counting as documented.
#[hegel::test(test_cases = 100)]
fn gates_leave_ignored_items_out(tc: TestCase) {
    use docov::feature::baseline::Baseline;
    let dir = tempfile::tempdir().unwrap();
    let root = docov::root::Root::new(dir.path());
    let mut files = gated_reports(&tc);
    for file in &mut files {
        file.path = dir.path().join(&file.path);
        // Few names, so an ignored item and a counted one can share their path, kind and name.
        for item in &mut file.items {
            item.name = format!("n{}", tc.draw(gs::integers::<u8>().max_value(1)));
        }
    }
    let shown = |files: &[FileReport]| -> Vec<FileReport> {
        files.iter().map(|f| FileReport { path: f.path.clone(), items: f.counted().cloned().collect() }).collect()
    };
    // Baseline entries for any of the items, ignored or documented ones too.
    let all: Vec<(&FileReport, &Item)> = files.iter().flat_map(|f| f.items.iter().map(move |i| (f, i))).collect();
    let picked = tc.draw(gs::vecs(gs::integers::<usize>().max_value(all.len().max(1) - 1)).max_size(6));
    let entries: Vec<serde_json::Value> = picked
        .into_iter()
        .filter(|&p| p < all.len())
        .map(|p| {
            let (file, item) = all[p];
            let path = file.path.file_name().unwrap().to_str().unwrap();
            serde_json::json!({ "path": path, "kind": item.kind.as_str(), "name": item.name })
        })
        .collect();
    fs::write(dir.path().join("known.json"), serde_json::json!({ "items": entries }).to_string()).unwrap();
    let most = tc.draw(gs::integers::<usize>().max_value(6));
    let threshold = DrawnThresholds::draw(&tc).json(&tc);
    let config = format!(r#"{{ "threshold": {threshold}, "maxUndocumented": {most}, "baseline": "known.json" }}"#);
    let settings = settings(dir.path(), &config, &[]);

    let excuse =
        |files: Vec<FileReport>| settings.get::<Baseline>().read(dir.path()).unwrap().unwrap().apply(&root, files);
    let (excused, excused_shown) = (excuse(files.clone()), excuse(shown(&files)));
    assert_eq!(shown(&excused), excused_shown);
    assert_eq!(settings.gates(&excused, &root), settings.gates(&excused_shown, &root), "{config}");
}

/// Through the binary: --write-baseline writes every undocumented item and passes, and with that
/// baseline the gates pass until a new undocumented item comes along, though the report still
/// lists the old ones. `--print-schema baseline` prints the baseline file's schema.
#[hegel::test(test_cases = 5)]
fn the_cli_writes_a_baseline_and_gates_by_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = docov_command().args(args).current_dir(dir.path()).output().unwrap();
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

/// Runs git in `dir`, a throwaway test repository, with an identity and no signing.
fn git(dir: &Path, args: &[&str]) {
    let config =
        ["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"];
    let out = Command::new("git").args(config).args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// What happens to a file in a generated change.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Change {
    Untouched,
    Committed,
    Staged,
    Uncommitted,
    Untracked,
}

/// In a real repository with a branch off main: --changed scans the files the branch changed,
/// committed or not, and untracked ones, from main or from origin/HEAD in a clone, and --staged
/// the staged ones. --diff-threshold counts the items on the lines they added. Outside a repository git's error says so, and without git, that it's missing.
#[hegel::test(test_cases = 15)]
fn git_narrows_to_changed_files_and_lines(tc: TestCase) {
    use docov::feature::{changed::Changed, diff::Diff};
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q"]);
    let changes: Vec<Change> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
        .map(|_| {
            tc.draw(gs::sampled_from(vec![
                Change::Untouched,
                Change::Committed,
                Change::Staged,
                Change::Uncommitted,
                Change::Untracked,
            ]))
        })
        .collect();
    // Each file has a comment, a documented function and another comment. The change takes out the
    // first comment (a hunk that adds no lines), adds a line before the second that the diff shows
    // as `+++ x;`, and adds a function, documented or not, after it.
    let kept = |f: usize| format!("/** Docs. */\nexport function a{f}() {{}}\n");
    let base = |f: usize| format!("// old\n{}// mid\n", kept(f));
    for (f, change) in changes.iter().enumerate() {
        if *change != Change::Untracked {
            write(&origin, &format!("src/f{f}.ts"), &base(f));
        }
    }
    write(&origin, "README.md", "readme");
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-q", "-m", "base"]);
    let cloned = tc.draw(gs::booleans());
    let repo = if cloned {
        git(dir.path(), &["clone", "-q", "origin", "clone"]);
        dir.path().join("clone")
    } else {
        origin.clone()
    };
    git(&repo, &["checkout", "-q", "-b", "change"]);
    let documented: Vec<bool> = changes.iter().map(|_| tc.draw(gs::booleans())).collect();
    let edit = |f: usize| {
        let doc = if documented[f] { "/** Docs. */\n" } else { "" };
        write(
            &repo,
            &format!("src/f{f}.ts"),
            &format!(
                "{}++ x;\n// mid\n// changed\n{doc}export function b{f}() {{}}\n// docov-ignore-next-line\nexport function c{f}() {{}}\n",
                kept(f)
            ),
        );
    };
    for (f, change) in changes.iter().enumerate() {
        if *change == Change::Committed {
            edit(f);
        }
    }
    git(&repo, &["commit", "-q", "--allow-empty", "-am", "change"]);
    for (f, change) in changes.iter().enumerate() {
        match change {
            Change::Staged => {
                edit(f);
                git(&repo, &["add", &format!("src/f{f}.ts")]);
            }
            Change::Uncommitted | Change::Untracked => edit(f),
            _ => {}
        }
    }

    // From the repository's src/, so git's paths from the top don't match the scan's by accident.
    std::env::set_current_dir(repo.join("src")).unwrap();
    let root = docov::root::Root::new(Path::new(".."));
    let files: Vec<PathBuf> = (0..changes.len()).map(|f| PathBuf::from(format!("f{f}.ts"))).collect();
    // Without --since, the default branch is origin/HEAD's in the clone, and main otherwise.
    let since = if tc.draw(gs::booleans()) { vec![] } else { vec!["--since".to_owned(), "main".to_owned()] };
    let changed = settings(&repo, "{}", &[vec!["--changed".to_owned()], since.clone()].concat());
    let staged = settings(&repo, "{}", &["--staged".to_owned()]);
    let all = settings(&repo, "{}", &[]);
    let pick = |wanted: &[Change]| -> Vec<PathBuf> {
        files.iter().zip(&changes).filter(|(_, c)| wanted.contains(c)).map(|(f, _)| f.clone()).collect()
    };
    assert_eq!(all.get::<Changed>().filter(files.clone(), &root).unwrap(), files);
    let since_main = pick(&[Change::Committed, Change::Staged, Change::Uncommitted, Change::Untracked]);
    assert_eq!(changed.get::<Changed>().filter(files.clone(), &root).unwrap(), since_main);
    assert_eq!(staged.get::<Changed>().filter(files.clone(), &root).unwrap(), pick(&[Change::Staged]));

    // The changed lines' items are the functions the change added.
    let reports: Vec<FileReport> = files.iter().map(|f| scan_file(f, &Scope::Exports.into()).unwrap()).collect();
    let threshold = tc.draw(gs::integers::<u8>().max_value(100));
    let diff = settings(
        &repo,
        "{}",
        &[
            vec!["--diff-threshold".to_owned(), threshold.to_string()],
            since.iter().map(|a| a.replace("since", "diff-base")).collect(),
        ]
        .concat(),
    );
    let diff = diff.get::<Diff>();
    let added = diff.added(&root).unwrap();
    let changed_items = Totals {
        documented: changes.iter().zip(&documented).filter(|(c, d)| **c != Change::Untouched && **d).count(),
        total: changes.iter().filter(|c| **c != Change::Untouched).count(),
    };
    // An untracked file's every line is new, its first function too.
    let untracked = changes.iter().filter(|c| **c == Change::Untracked).count();
    let changed_items =
        Totals { documented: changed_items.documented + untracked, total: changed_items.total + untracked };
    let wanted = (changed_items.percent() < f64::from(threshold)).then(|| {
        format!("changed lines: {:.1}% documented, under the {threshold}% diff threshold", changed_items.percent())
    });
    assert_eq!(diff.gate(added.as_ref(), &reports, &root), wanted);
    assert!(all.get::<Diff>().added(&root).unwrap().is_none());

    // Outside a repository, git's error says so.
    std::env::set_current_dir(dir.path()).unwrap();
    for settings in [&changed, &staged] {
        let error = settings.get::<Changed>().filter(files.clone(), &root).unwrap_err();
        assert!(error.starts_with("git rev-parse --show-cdup: ") && error.contains("not a git repository"), "{error}");
    }
    assert!(diff.added(&root).unwrap_err().contains("not a git repository"));
    let out = docov_command().args(["--diff-threshold", "100"]).current_dir(dir.path()).output().unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("not a git repository"), "{stderr}");
    let out = docov_command().arg("--staged").env("PATH", "").current_dir(&repo).output().unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.starts_with("docov: git rev-parse --show-cdup: "), "{stderr}");
}

/// What the doc checks `on` (each rule id and its docov.json setting) find in `source`, a
/// TypeScript module scanned by `rules`: each finding's line, column, rule and message, and whether
/// it's an error.
fn diagnose(
    source: &str,
    on: &[(&str, serde_json::Value)],
    mut rules: Rules,
) -> Vec<(usize, usize, &'static str, String, bool)> {
    let settings: Vec<(&str, check::Setting)> =
        on.iter().map(|(id, value)| (*id, check::Setting::parse(id, value).unwrap())).collect();
    let Some(checks) = check::Checks::new(settings.iter().map(|(id, setting)| (*id, setting))) else {
        assert!(settings.iter().all(|(_, setting)| setting.level == check::Level::Off));
        return Vec::new();
    };
    rules.checks = Some(std::sync::Arc::new(checks));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ts");
    fs::write(&path, source).unwrap();
    let mut found: Vec<_> =
        scan_file(&path, &rules).unwrap().items.into_iter().flat_map(|i| i.facts.diagnostics).collect();
    found.sort();
    found
        .into_iter()
        .map(|d| {
            assert_ne!(d.severity, check::Level::Off);
            (d.line, d.column, d.rule, d.message, d.severity == check::Level::Error)
        })
        .collect()
}

/// A level for a rule, drawn, as docov.json writes it, and whether it's on and an error.
fn level(tc: &TestCase) -> (serde_json::Value, Option<bool>) {
    let (name, on) = tc.draw(gs::sampled_from(vec![("off", None), ("warn", Some(false)), ("error", Some(true))]));
    let value = if tc.draw(gs::booleans()) { serde_json::json!(name) } else { serde_json::json!([name]) };
    (value, on)
}

/// `findings`, each at `level`, or none when it's off, in the order diagnostics sort in.
fn at_level(
    findings: Vec<(usize, usize, &'static str, String)>,
    level: Option<bool>,
) -> Vec<(usize, usize, &'static str, String, bool)> {
    let Some(error) = level else { return Vec::new() };
    let mut all: Vec<_> = findings.into_iter().map(|(l, c, r, m)| (l, c, r, m, error)).collect();
    all.sort();
    all
}

/// `require-param` finds each parameter with no matching `@param` (by name, or for a destructured
/// one, by place), unless the block has `@inheritdoc`, and `param-names` each `@param` whose name,
/// or a dotted name's root, is no parameter's, nor the name of the `@param` at a destructured
/// one's place. Only counted functions and methods with a block are checked.
#[hegel::test(test_cases = 100)]
fn parameters_and_their_tags_match(tc: TestCase) {
    let mut params: Vec<&str> = Vec::new();
    for param in tc.draw(gs::vecs(gs::sampled_from(vec!["a", "b", "{ c }", "[d]"])).max_size(3)) {
        if !params.contains(&param) {
            params.push(param);
        }
    }
    params.extend(tc.draw(gs::optional(gs::sampled_from(vec!["...e", "...{ length }"]))));
    let tags = tc.draw(
        gs::vecs(gs::sampled_from(vec!["a", "b", "c", "e", "z", "[b=1]", "a.x", "z.y", "opts", "opts.v"])).max_size(4),
    );
    let kind = tc.draw(gs::sampled_from(vec!["param", "arg", "argument"]));
    let inherits = tc.draw(gs::booleans());
    let ((require, require_on), (names, names_on)) = (level(&tc), level(&tc));
    let doc: String = tags.iter().map(|name| format!(" * @{kind} {{string}} {name} Docs.\n")).collect();
    let source = format!(
        "/**\n * Docs.\n{doc}{} */\nexport function f({}) {{}}\n/** Docs. */\nfunction hidden(z) {{}}\nexport function bare(q) {{}}\n/** @param a Not a function's. */\nexport const v = 1;\n",
        if inherits { " * @inheritDoc\n" } else { "" },
        params.join(", "),
    );

    let name = |tag: &str| tag.trim_matches(['[', ']']).split('=').next().unwrap().to_owned();
    let tops: Vec<String> = tags.iter().filter(|t| !t.contains('.')).map(|t| name(t)).collect();
    let mut wanted = Vec::new();
    let destructured = |p: &str| p.trim_start_matches("...").starts_with(['{', '[']);
    for (i, param) in params.iter().enumerate() {
        let own = param.trim_start_matches("...");
        let documented = if destructured(param) { i < tops.len() } else { tops.iter().any(|t| t == own) };
        if !documented && !inherits {
            let what = if destructured(param) { format!("parameter {}", i + 1) } else { format!("`{own}`") };
            wanted.push((1, 1, "require-param", format!("missing @param for {what}")));
        }
    }
    let mut found = at_level(wanted, require_on);
    let roots: Vec<String> = params
        .iter()
        .filter(|p| !destructured(p))
        .map(|p| p.trim_start_matches("...").to_owned())
        .chain(
            tops.iter()
                .enumerate()
                .filter(|(i, _)| params.get(*i).is_some_and(|p| destructured(p)))
                .map(|(_, t)| t.clone()),
        )
        .collect();
    let stale =
        tags.iter().enumerate().filter(|(_, t)| !roots.contains(&name(t).split('.').next().unwrap().to_owned()));
    let stale = stale.map(|(i, t)| {
        (i + 3, 15 + kind.len(), "param-names", format!("@param `{}` doesn't match a parameter", name(t)))
    });
    found.extend(at_level(stale.collect(), names_on));
    found.sort();
    let on = [("require-param", require), ("param-names", names)];
    assert_eq!(diagnose(&source, &on, Rules::default()), found, "{source}");
}

/// Each rule's level comes from `--rule ID=LEVEL` over docov.json's `rules`, with none on unless
/// one of them turns it on. The gate fails the scan when a finding is at `error`, and passes with
/// warnings alone.
#[hegel::test(test_cases = 50)]
fn rule_levels_come_from_flags_over_the_config(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut run = Run::default();
    let mut keys = serde_json::Map::new();
    let mut wanted: Vec<(&str, check::Level)> = Vec::new();
    let levels = vec![check::Level::Off, check::Level::Warn, check::Level::Error];
    let named = |level: check::Level| serde_json::to_value(level).unwrap().as_str().unwrap().to_owned();
    for id in check::ids() {
        let from_key = tc.draw(gs::optional(gs::sampled_from(levels.clone())));
        let from_flag = tc.draw(gs::optional(gs::sampled_from(levels.clone())));
        if let Some(level) = from_key {
            let value =
                if tc.draw(gs::booleans()) { named(level).into() } else { serde_json::json!([named(level), {}]) };
            keys.insert(id.to_owned(), value);
        }
        if let Some(level) = from_flag {
            run.args.extend(["--rule".to_owned(), format!("{id}={}", named(level))]);
        }
        wanted.extend(from_flag.or(from_key).filter(|&level| level != check::Level::Off).map(|level| (id, level)));
    }
    if !keys.is_empty() || tc.draw(gs::booleans()) {
        run.keys.insert("rules".into(), keys.into());
    }
    let settings = run.settings(dir.path());
    let checks = settings.get::<DocChecks>().0.clone();
    let mut levels: Vec<_> = checks.iter().flat_map(|c| c.levels()).collect();
    levels.sort();
    wanted.sort();
    assert_eq!(levels, wanted);
    let rules = settings.rules();
    assert_eq!(rules.checks.is_some(), !wanted.is_empty());
    assert_eq!(format!("{:?}", rules.checks), format!("{:?}", checks));

    let path = dir.path().join("m.ts");
    fs::write(&path, "/** Docs. */\nexport function f(a) {}\n").unwrap();
    let reports = vec![scan_file(&path, &rules).unwrap()];
    let found: Vec<_> = report::diagnostics(&reports).map(|(_, d)| (d.rule, d.severity)).collect();
    assert_eq!(found, wanted.iter().filter(|(id, _)| *id == "require-param").copied().collect::<Vec<_>>());
    let failure =
        wanted.contains(&("require-param", check::Level::Error)).then(|| "the doc checks found 1 error".to_owned());
    assert_eq!(settings.gates(&reports, &docov::root::Root::new(dir.path())), Vec::from_iter(failure));
}

/// A `rules` key or `--rule` flag that names no rule, or gives a rule a level or options it
/// doesn't have, fails to load, naming what's wrong.
#[hegel::test(test_cases = 30)]
fn bad_rule_settings_fail_naming_the_rule(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let known = check::ids().collect::<Vec<_>>().join(", ");
    let unknown = format!("unknown rule `nope`, expected one of {known}");
    let level = "param-names: expected \"off\", \"warn\" or \"error\", or [level, options]";
    let (text, message) = tc.draw(gs::sampled_from(vec![
        (r#"{ "rules": { "nope": "warn" } }"#, unknown.clone()),
        (r#"{ "rules": { "param-names": "loud" } }"#, level.to_owned()),
        (r#"{ "rules": { "param-names": [] } }"#, level.to_owned()),
        (r#"{ "rules": { "param-names": ["warn", {}, 1] } }"#, level.to_owned()),
        (
            r#"{ "rules": { "param-names": ["warn", { "x": 1 }] } }"#,
            "param-names: unknown field `x`, there are no fields".to_owned(),
        ),
        (r#"{ "rules": [] }"#, "invalid type: sequence, expected a map".to_owned()),
    ]));
    let path = dir.path().join("docov.json");
    fs::write(&path, text).unwrap();
    match config::Config::load(Some(&path)) {
        Err(config::Error::Parse { source, .. }) => {
            assert!(source.to_string().starts_with(&format!("{message} at line 1 column ")), "{source}");
        }
        other => panic!("{other:?}"),
    }
    let (flag, message) = tc.draw(gs::sampled_from(vec![
        ("param-names", "expected ID=LEVEL".to_owned()),
        ("nope=warn", unknown),
        ("param-names=loud", "param-names: expected off, warn or error".to_owned()),
    ]));
    assert_eq!(check::flag(flag), Err(message));
    assert_eq!(check::flag("param-names=warn"), Ok(("param-names".to_owned(), check::Level::Warn)));
}

/// Complete coverage is over every counted item: one is complete when it's documented and, for a
/// function or method, each parameter has a matching `@param`. With `--complete` (or a threshold)
/// the JSON report has it and the text report gets a line of it, and its threshold fails the scan
/// under it.
#[hegel::test(test_cases = 100)]
fn complete_items_are_a_metric_when_asked(tc: TestCase) {
    let mut files = reports(&tc, true);
    for item in files.iter_mut().flat_map(|f| f.items.iter_mut()) {
        item.facts.params_documented = tc.draw(gs::optional(gs::booleans()));
    }
    let of = |f: &FileReport| {
        let over: Vec<_> = f.items.iter().filter(|i| i.documented || i.ignore.is_none()).collect();
        let complete = over.iter().filter(|i| i.documented && i.facts.params_documented != Some(false)).count();
        Totals { documented: complete, total: over.len() }
    };
    let all = files
        .iter()
        .map(of)
        .fold(Totals::default(), |a, b| Totals { documented: a.documented + b.documented, total: a.total + b.total });
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
    let on = threshold.is_some() || tc.draw(gs::booleans());
    let feature = Complete { on, threshold };

    let metrics = Vec::from_iter(feature.metric());
    let context = Context { metrics: &metrics, ..plain(&files) };
    let json: serde_json::Value = serde_json::from_str(&report::format("json").unwrap().render(&context)).unwrap();
    let shown = |v: &serde_json::Value| (v["complete"].as_u64(), v["completePercent"].as_f64());
    let wanted = |t: Totals| if on { (Some(t.documented as u64), Some(t.percent())) } else { (None, None) };
    assert_eq!(shown(&json), wanted(all));
    assert!(json["files"].as_array().unwrap().iter().map(shown).eq(files.iter().map(|f| wanted(of(f)))));

    let line = format!("{:>6.1}%  {}/{}  complete", all.percent(), all.documented, all.total);
    assert_eq!(metric_line(&context), on.then_some(line));
    assert_eq!(report::format("lcov").unwrap().render(&context), render("lcov", &files));

    let percent = all.percent();
    let failure = threshold
        .filter(|&t| percent < t)
        .map(|t| format!("{percent:.1}% with complete, under the {t}% complete threshold"));
    assert_eq!(gate(&feature, &files), Vec::from_iter(failure));
}

/// Tag lines a generated block has, past its description, and the tags they give.
const RETURN_TAGS: [(&str, &str); 6] = [
    ("", ""),
    (" * @returns The thing.\n", "returns"),
    (" * @return The thing.\n", "returns"),
    (" * @yields A thing.\n", "yields"),
    (" * @yield A thing.\n", "yields"),
    (" * @inheritDoc\n", "inherits"),
];

/// `require-returns` finds a function or method whose own body returns a value (`return x;`, or an
/// arrow function's expression body, past the functions and classes inside it) with no `@returns`
/// or `@return`, unless its return type is `void`, `undefined` or `Promise<void>` or its block has
/// `@inheritdoc`. `require-yields` finds a generator that yields a value with no `@yields` or
/// `@yield`.
#[hegel::test(test_cases = 100)]
fn returned_and_yielded_values_are_documented(tc: TestCase) {
    let ((returns_level, returns_on), (yields_level, yields_on)) = (level(&tc), level(&tc));
    // Each statement, whether it returns a value, and whether it yields one.
    let statements = vec![
        ("return 1;", true, false),
        ("return;", false, false),
        ("return undefined;", false, false),
        ("return void 0;", false, false),
        ("if (x) { return x; }", true, false),
        ("const g = () => 1;", false, false),
        ("function inner() { return 1; }", false, false),
        ("class K { m() { return 1; } }", false, false),
        ("yield 1;", false, true),
        ("yield;", false, false),
        ("const h = function* () { yield 1; };", false, false),
    ];
    let annotations = vec![
        ("", false),
        (": void", true),
        (": undefined", true),
        (": Promise<void>", true),
        (": number", false),
        (": Promise<number>", false),
        (": Array<void>", false),
    ];
    let mut source = String::new();
    let mut wanted = Vec::new();
    for owner in ["function", "method", "arrow"] {
        let generator = owner != "arrow" && tc.draw(gs::booleans());
        let body = tc.draw(gs::vecs(gs::sampled_from(statements.clone())).max_size(3));
        let body: Vec<_> = body.into_iter().filter(|(text, _, _)| generator || !text.starts_with("yield")).collect();
        let (annotation, void) = tc.draw(gs::sampled_from(annotations.clone()));
        let (tag, gives) = tc.draw(gs::sampled_from(RETURN_TAGS.to_vec()));
        let indent = if owner == "method" { "  " } else { "" };
        let line = source.matches('\n').count() + usize::from(owner == "method") + 1;
        if owner == "method" {
            source += "export class C {\n";
        }
        source += &format!("{indent}/**\n{indent} * Docs.\n{tag}{indent} */\n");
        let star = if generator { "*" } else { "" };
        let text = body.iter().map(|(t, _, _)| *t).collect::<Vec<_>>().join(" ");
        let returns = match owner {
            "function" => {
                source += &format!("export function{star} f(x){annotation} {{ {text} }}\n");
                body.iter().any(|(_, r, _)| *r)
            }
            "method" => {
                source += &format!("  {star}m(x){annotation} {{ {text} }}\n}}\n");
                body.iter().any(|(_, r, _)| *r)
            }
            _ => {
                let expressions =
                    vec![("1", true), ("undefined", false), ("void 0", false), ("{ return 2; }", true), ("{}", false)];
                let (expression, returns) = tc.draw(gs::sampled_from(expressions));
                source += &format!("exports.g = (x){annotation} => {expression};\n");
                returns
            }
        };
        let column = indent.len() + 1;
        if returns && !void && !matches!(gives, "returns" | "inherits") {
            wanted.extend(returns_on.map(|e| (line, column, "require-returns", "missing @returns".to_owned(), e)));
        }
        if body.iter().any(|(_, _, y)| *y) && !matches!(gives, "yields" | "inherits") {
            wanted.extend(yields_on.map(|e| (line, column, "require-yields", "missing @yields".to_owned(), e)));
        }
    }
    wanted.sort();
    let on = [("require-returns", returns_level), ("require-yields", yields_level)];
    assert_eq!(diagnose(&source, &on, Rules::default()), wanted, "{source}");
}

/// `require-throws` finds an exported function or method whose own body throws (outside a `try`
/// with a `catch`, and past the functions inside it) with no `@throws` or `@exception`, unless its
/// block has `@inheritdoc`.
#[hegel::test(test_cases = 100)]
fn thrown_errors_are_documented(tc: TestCase) {
    let (throws_level, throws_on) = level(&tc);
    // Each statement, and whether it throws.
    let statements = vec![
        ("throw new Error('no');", true),
        ("if (x) { throw x; }", true),
        ("try { throw x; } catch { }", false),
        ("try { throw x; } finally { }", true),
        ("try { } catch (e) { throw e; }", true),
        ("try { } finally { throw x; }", true),
        ("try { try { throw x; } finally { } } catch { }", false),
        ("function inner() { throw x; }", false),
        ("const g = () => { throw x; };", false),
        ("return 1;", false),
    ];
    let tags = vec![
        ("", false),
        (" * @throws {Error} When it fails.\n", true),
        (" * @exception When it fails.\n", true),
        (" * @inheritdoc\n", true),
    ];
    let mut source = String::new();
    let mut wanted = Vec::new();
    for (owner, exported) in [("f", true), ("g", false), ("C", true), ("h", true)] {
        let body = tc.draw(gs::vecs(gs::sampled_from(statements.clone())).max_size(3));
        let (tag, documented) = tc.draw(gs::sampled_from(tags.clone()));
        let text = body.iter().map(|(t, _)| *t).collect::<Vec<_>>().join(" ");
        let (mut line, mut column) = (source.matches('\n').count() + 1, 1);
        match owner {
            "C" => {
                source += &format!("export class C {{\n  /**\n   * Docs.\n{tag}   */\n  m(x) {{ {text} }}\n}}\n");
                (line, column) = (line + 1, 3);
            }
            "h" => source += &format!("/**\n * Docs.\n{tag} */\nexports.h = (x) => {{ {text} }};\n"),
            name => {
                let export = if exported { "export " } else { "" };
                source += &format!("/**\n * Docs.\n{tag} */\n{export}function {name}(x) {{ {text} }}\n");
            }
        }
        if exported && !documented && body.iter().any(|(_, throws)| *throws) {
            wanted.push((line, column, "require-throws", "missing @throws".to_owned()));
        }
    }
    let on = [("require-throws", throws_level)];
    assert_eq!(diagnose(&source, &on, Rules::from(Scope::All)), at_level(wanted, throws_on), "{source}");
}

/// `require-template` finds each type parameter of a function, method, class, interface, interface
/// method or type alias with no `@template` naming it, where one tag can name several
/// (`@template T, U`) after a constraint (`@template {string} K`), or with a default
/// (`@template [T=string]`), unless its block has `@inheritdoc`.
#[hegel::test(test_cases = 100)]
fn type_parameters_are_documented(tc: TestCase) {
    let (template_level, template_on) = level(&tc);
    let names = ["T", "U", "K"];
    // Each tag line, and the names it gives.
    let tags = vec![
        (" * @template T\n", vec!["T"]),
        (" * @template T, U Two of them.\n", vec!["T", "U"]),
        (" * @template T,U\n", vec!["T", "U"]),
        (" * @template {string} K The key.\n", vec!["K"]),
        (" * @template [T=string]\n", vec!["T"]),
        (" * @template\n", vec![]),
        (" * @inheritDoc\n", names.to_vec()),
    ];
    let mut source = String::new();
    let mut wanted = Vec::new();
    for owner in ["function", "class", "method", "interface", "signature", "type"] {
        let mut unique: Vec<&str> = Vec::new();
        for param in tc.draw(gs::vecs(gs::sampled_from(names.to_vec())).max_size(3)) {
            if !unique.contains(&param) {
                unique.push(param);
            }
        }
        let given = tc.draw(gs::vecs(gs::sampled_from(tags.clone())).max_size(2));
        let generics = if unique.is_empty() { String::new() } else { format!("<{}>", unique.join(", ")) };
        let block = format!("/**\n * Docs.\n{} */\n", given.iter().map(|(text, _)| *text).collect::<String>());
        let member = matches!(owner, "method" | "signature");
        let indented: String = block.lines().map(|l| format!("  {l}\n")).collect();
        let line = source.matches('\n').count() + 1 + usize::from(member);
        source += &match owner {
            "function" => format!("{block}export function f{generics}() {{}}\n"),
            "class" => format!("{block}export class C{generics} {{}}\n"),
            "method" => format!("export class D {{\n{indented}  m{generics}() {{}}\n}}\n"),
            "interface" => format!("{block}export interface I{generics} {{}}\n"),
            "signature" => format!("export interface J {{\n{indented}  m{generics}(): void;\n}}\n"),
            _ => format!("{block}export type A{generics} = 1;\n"),
        };
        let column = if member { 3 } else { 1 };
        for param in unique.iter().filter(|p| !given.iter().any(|(_, names)| names.contains(p))) {
            wanted.push((line, column, "require-template", format!("missing @template for `{param}`")));
        }
    }
    let on = [("require-template", template_level)];
    assert_eq!(diagnose(&source, &on, Rules::default()), at_level(wanted, template_on), "{source}");
}

/// `empty-tag-description` finds each `@param`, `@returns`, `@throws` and `@yields` tag (or one of
/// their other names) that says nothing past its type and name, except a `@returns` of a function
/// that returns nothing, by its return type or the tag's.
#[hegel::test(test_cases = 100)]
fn tags_have_descriptions(tc: TestCase) {
    let (empty_level, empty_on) = level(&tc);
    let (annotation, void) = tc.draw(gs::sampled_from(vec![("", false), (": void", true), (": number", false)]));
    // Each tag line, its name, what it names, whether it says nothing, and whether it's a
    // `@returns` of nothing by its type.
    let tags = vec![
        ("@param {string} a The a.", "param", None, false, false),
        ("@param {string} a", "param", Some("a"), true, false),
        ("@arg b", "arg", Some("b"), true, false),
        ("@argument", "argument", None, true, false),
        ("@returns {number} The count.", "returns", None, false, false),
        ("@returns {number}", "returns", None, true, false),
        ("@return", "return", None, true, false),
        ("@returns {void}", "returns", None, true, true),
        ("@returns {Promise<void>}", "returns", None, true, true),
        ("@throws {Error}", "throws", None, true, false),
        ("@exception When it fails.", "exception", None, false, false),
        ("@yields", "yields", None, true, false),
        ("@yield {number} A number.", "yield", None, false, false),
        ("@since", "since", None, true, false),
    ];
    let drawn = tc.draw(gs::vecs(gs::sampled_from(tags)).max_size(4));
    let lines: String = drawn.iter().map(|(text, ..)| format!(" * {text}\n")).collect();
    let source = format!("/**\n * Docs.\n{lines} */\nexport function f(a, b){annotation} {{}}\n");
    let wanted = drawn.iter().enumerate().filter_map(|(i, (_, kind, name, empty, nothing))| {
        let counts = *kind != "since" && !(matches!(*kind, "returns" | "return") && (void || *nothing));
        let name = name.map(|n| format!(" `{n}`")).unwrap_or_default();
        (counts && *empty).then(|| (i + 3, 4, "empty-tag-description", format!("@{kind}{name} has no description")))
    });
    let on = [("empty-tag-description", empty_level)];
    assert_eq!(diagnose(&source, &on, Rules::default()), at_level(wanted.collect(), empty_on), "{source}");
}

/// `informative-docs` finds a description whose words, without case and past filler words like
/// "the" and "gets", are all words of its item's own name, split at camelCase, digits and
/// separators.
#[hegel::test(test_cases = 100)]
fn descriptions_say_more_than_the_name(tc: TestCase) {
    let (informative_level, informative_on) = level(&tc);
    let (name, words) = tc.draw(gs::sampled_from(vec![
        ("userName", vec!["user", "name", "User", "NAME", "user-name", "userName", "user_name"]),
        ("HTTPServer", vec!["http", "server", "HTTP", "Server"]),
        ("getValue2", vec!["get", "value", "2", "value2"]),
        ("id", vec!["id", "ID"]),
    ]));
    let filler = ["the", "a", "an", "this", "gets", "sets", "returns", "The"];
    let pool: Vec<&str> = words.iter().copied().chain(filler).collect();
    let mut description = tc.draw(gs::vecs(gs::sampled_from(pool)).max_size(4));
    let extra = tc.draw(gs::booleans());
    if extra {
        description.push("cached");
    }
    let text = description.join(" ");
    let member = tc.draw(gs::booleans());
    // A block with no description has nothing to check.
    let described = tc.draw(gs::weighted_booleans(0.9));
    let source = if member {
        format!("export class C {{\n  /** {text}. */\n  {name}() {{}}\n}}\n")
    } else if described {
        format!("/**\n * {text}.\n * @since 1\n */\nexport function {name}() {{}}\n")
    } else {
        format!("/**\n * @since 1\n */\nexport function {name}() {{}}\n")
    };
    // A description of punctuation alone says no more than the name, too.
    let (line, column) = if member { (2, 7) } else { (2, 4) };
    let message = format!("the description only restates the name `{name}`");
    let wanted =
        Vec::from_iter((!extra && (member || described)).then_some((line, column, "informative-docs", message)));
    let on = [("informative-docs", informative_level)];
    assert_eq!(diagnose(&source, &on, Rules::default()), at_level(wanted, informative_on), "{source}");
}

/// `valid-link` finds each `{@link}`, `{@linkcode}` or `{@linkplain}` that isn't closed, has an
/// unescaped `{` inside or no target, or whose target is a plain name or namepath whose first part
/// the file doesn't declare or import (nor a block declare as a type, nor a JavaScript, browser or
/// Node.js global define), and each `@see` with no text or whose lone word is such a name. Escaped
/// links and links in code spans are left be, as are URLs and other targets.
#[hegel::test(test_cases = 100)]
fn links_are_well_formed_and_resolve(tc: TestCase) {
    let (link_level, link_on) = level(&tc);
    let (default, named) = tc.draw(gs::sampled_from(vec![
        ("export default function main() {}", Some("main")),
        ("export default class Main {}", Some("Main")),
        ("export default 1;", None),
    ]));
    let prelude = format!(
        "import {{ Thing }} from './thing';\nimport * as ns from './ns';\nimport req = require('req');\n\
         const {{ dest }} = ns;\nexport interface Shape {{}}\nexport type Alias = 1;\nexport enum Color {{}}\n\
         export namespace Space {{}}\ndeclare module 'm' {{}}\n{default}\nns.call();\n\
         /** @typedef {{object}} Opt */\nexport function f() {{}}\nexport class K {{}}\n"
    );
    let unresolved = |name: &str| format!("`{name}` isn't declared or imported in this file");
    let undeclared = |name: &str| (named != Some(name)).then(|| (0, unresolved(name)));
    // Each link, and where in it the finding is and what it says, if it has one.
    let links: Vec<(&str, Option<(usize, String)>)> = vec![
        ("{@link Thing}", None),
        ("{@linkcode ns.x}", None),
        ("{@linkplain dest}", None),
        ("{@link Shape#area}", None),
        ("{@link Alias}", None),
        ("{@link Color~red}", None),
        ("{@link Space}", None),
        ("{@link req}", None),
        ("{@link Opt}", None),
        ("{@link K.m()}", None),
        ("{@link Array}", None),
        ("{@link document}", None),
        ("{@link process}", None),
        ("{@link f|the function}", None),
        ("{@link Thing more text}", None),
        ("{@link https://x.dev}", None),
        ("{@link module:a/b}", None),
        ("{@link main}", undeclared("main")),
        ("{@link Main}", undeclared("Main")),
        ("{@link Nope}", Some((0, unresolved("Nope")))),
        ("{@linkcode Nope.x|text}", Some((0, unresolved("Nope.x")))),
        ("{@link}", Some((0, "{@link} has no target".into()))),
        ("{@linkplain  }", Some((0, "{@linkplain} has no target".into()))),
        ("{@link Thing", Some((0, "{@link} isn't closed".into()))),
        ("{@link {a}}", Some((7, "an unescaped `{` in {@link}".into()))),
        ("{@link a\\{b}", None),
        ("\\{@link Nope}", None),
        ("`{@link Nope}`", None),
    ];
    let sees: Vec<(&str, Option<(usize, String)>)> = vec![
        ("@see Thing", None),
        ("@see https://x.dev", None),
        ("@see the docs for more", None),
        ("@see Nope", Some((0, unresolved("Nope")))),
        ("@see", Some((0, "@see has no text".into()))),
        ("@see {@link Nope}", Some((5, unresolved("Nope")))),
    ];
    let drawn = tc.draw(gs::vecs(gs::sampled_from(links)).max_size(4));
    let see = tc.draw(gs::optional(gs::sampled_from(sees)));
    let mut source = prelude;
    let mut wanted = Vec::new();
    for (i, (link, finding)) in drawn.iter().enumerate() {
        let line = source.matches('\n').count() + 2;
        source += &format!("/**\n * See {link}.\n */\nexport const v{i} = 1;\n");
        wanted.extend(finding.clone().map(|(at, message)| (line, 8 + at, "valid-link", message)));
    }
    if let Some((tag, finding)) = see {
        let line = source.matches('\n').count() + 3;
        source += &format!("/**\n * Docs.\n * {tag}\n */\nexport const seen = 1;\n");
        wanted.extend(finding.map(|(at, message)| (line, 4 + at, "valid-link", message)));
    }
    let on = [("valid-link", link_level)];
    assert_eq!(diagnose(&source, &on, Rules::from(Scope::All)), at_level(wanted, link_on), "{source}");
}

/// `summary-sentence` finds a summary, a description's first sentence (up to a `.`, `!` or `?`
/// with a space after it, or else its first paragraph), that's longer than `maxLength` characters
/// (120 by default) or, with `period` on (the default), doesn't end with one of those.
#[hegel::test(test_cases = 100)]
fn summaries_are_short_sentences(tc: TestCase) {
    let (summary_level, summary_on) = level(&tc);
    let length = tc.draw(gs::optional(gs::sampled_from(vec![10, 50, 120])));
    let period = tc.draw(gs::optional(gs::booleans()));
    let mut options = serde_json::Map::new();
    options.extend(length.map(|l| ("maxLength".to_owned(), l.into())));
    options.extend(period.map(|p| ("period".to_owned(), p.into())));
    let level = summary_level.as_str().or_else(|| summary_level[0].as_str()).unwrap().to_owned();
    let setting =
        if options.is_empty() && tc.draw(gs::booleans()) { level.into() } else { serde_json::json!([level, options]) };

    let words = tc.draw(gs::integers::<usize>().min_value(1).max_value(30));
    let wrap = tc.draw(gs::integers::<usize>().max_value(words - 1));
    let end = tc.draw(gs::sampled_from(vec![".", "!", "?", "", ":"]));
    let second = tc.draw(gs::sampled_from(vec!["", " More here."]));
    let paragraph = tc.draw(gs::sampled_from(vec!["", "\n *\n * Another paragraph."]));
    let described = tc.draw(gs::weighted_booleans(0.9));
    let first = vec!["word"; words];
    let text = format!("{}\n * {}{end}{second}{paragraph}", first[..wrap].join(" "), first[wrap..].join(" "));
    let source = if described {
        format!("/**\n * {text}\n * @since 1\n */\nexport const a = 1;\n")
    } else {
        "/**\n * @since 1\n */\nexport const a = 1;\n".to_owned()
    };
    let summary = if [".", "!", "?"].contains(&end) || second.is_empty() {
        format!("{}{end}", first.join(" "))
    } else {
        format!("{}{end}{second}", first.join(" "))
    };
    let (max, period) = (length.unwrap_or(120), period.unwrap_or(true));
    let (line, column) = if wrap == 0 { (3, 4) } else { (2, 4) };
    let mut wanted = Vec::new();
    if described && summary.len() > max {
        wanted.push((
            line,
            column,
            "summary-sentence",
            format!("the summary is {} characters, over {max}", summary.len()),
        ));
    }
    if described && period && !summary.ends_with(['.', '!', '?']) {
        wanted.push((line, column, "summary-sentence", "the summary doesn't end with a period".to_owned()));
    }
    let on = [("summary-sentence", setting)];
    assert_eq!(diagnose(&source, &on, Rules::default()), at_level(wanted, summary_on), "{source}");
}

/// `doc-markdown` finds each word of a description that looks like code (`snake_case`, `a::b`,
/// `camelCase`, `PascalCase`) outside code spans, code blocks and inline tags, unless it's a URL,
/// path or address or in the allowlist (a default one, and the option's), and each lazy
/// continuation line: one in a list item that's indented less than the item's text.
#[hegel::test(test_cases = 100)]
fn descriptions_are_markdown(tc: TestCase) {
    let (markdown_level, markdown_on) = level(&tc);
    let allow = tc.draw(gs::booleans());
    let level = markdown_level.as_str().or_else(|| markdown_level[0].as_str()).unwrap().to_owned();
    let setting = if allow { serde_json::json!([level, { "allowlist": ["MyThing"] }]) } else { markdown_level };
    // Each word, and where in it a word that looks like code starts, if it has one.
    let words: Vec<(&str, Option<usize>)> = vec![
        ("plain", None),
        ("fooBar", Some(0)),
        ("snake_case", Some(0)),
        ("a::b", Some(0)),
        ("`fooBar`", None),
        ("`a b`", None),
        ("JavaScript", None),
        ("MyThing", (!allow).then_some(0)),
        ("https://x.dev/aB", None),
        ("src/fooBar.ts", None),
        ("me@fooBar.dev", None),
        ("{@link fooBar}", None),
        ("(fooBar).", Some(1)),
        ("**boldWord**", Some(2)),
        ("_emphasis_", None),
        ("2D", None),
    ];
    // Each line's list marker, if it's a list item, its indentation if it isn't, its word, and
    // whether it's blank (0), a fence (1) or text.
    let count = tc.draw(gs::integers::<usize>().max_value(6));
    let drawn: Vec<_> = (0..count)
        .map(|_| {
            let marker = tc.draw(gs::sampled_from(vec![Some("- "), Some("1. "), Some("* "), None, None, None]));
            let indent = tc.draw(gs::sampled_from(vec![0, 2, 4]));
            (marker, indent, tc.draw(gs::sampled_from(words.clone())), tc.draw(gs::integers::<u8>().max_value(9)))
        })
        .collect();
    let mut lines = vec!["Docs.".to_owned()];
    let mut wanted = Vec::new();
    let (mut item, mut fenced) = (None::<usize>, false);
    for (marker, indent, (word, code), shape) in drawn {
        // The block's first line is `/**`, and its description starts on the next.
        let line = lines.len() + 2;
        let text = match (marker, shape) {
            (_, 0) => {
                item = None;
                lines.push(String::new());
                continue;
            }
            (_, 1) => {
                fenced = !fenced;
                lines.push("```".into());
                continue;
            }
            (Some(marker), _) => format!("{marker}{word}"),
            (None, _) => format!("{}{word}", " ".repeat(indent)),
        };
        lines.push(text.clone());
        if fenced {
            continue;
        }
        match marker {
            Some(marker) => item = Some(marker.len()),
            None if item.is_some_and(|text| indent < text) => {
                wanted.push((line, 4, "doc-markdown", "a lazy continuation line in a list item: indent it".to_owned()));
            }
            None => {}
        }
        if let Some(at) = code {
            let start = text.len() - word.len() + at;
            let code = word[at..].trim_end_matches(['*', ')', '.']);
            wanted.push((line, 4 + start, "doc-markdown", format!("`{code}` is code, so put it in backticks")));
        }
    }
    let block: String =
        lines.iter().map(|l| if l.is_empty() { " *\n".to_owned() } else { format!(" * {l}\n") }).collect();
    let source = format!("/**\n{block} * @since 1\n */\nexport const a = 1;\n");
    let on = [("doc-markdown", setting)];
    assert_eq!(diagnose(&source, &on, Rules::default()), at_level(wanted, markdown_on), "{source}");
}

/// Doc check findings on counted items add to what `unix`, `github`, `markdown` and `sarif` write,
/// after the rest, and change nothing else: a line each in `unix` and an annotation at its level in
/// `github` (both read back whole), a bullet each folded away at the end of `markdown`, and in
/// `sarif` a result each at its level and a rule for each check with a finding. An ignored item's
/// findings are left out.
#[hegel::test(test_cases = 100)]
fn formats_list_each_doc_check_finding(tc: TestCase) {
    use pulldown_cmark::{Event, Parser, Tag};
    let plain_files = reports(&tc, true);
    let mut files = plain_files.clone();
    let ids: Vec<&'static str> = check::ids().collect();
    let messages = vec!["missing @param for `a`", "50% of it, then\nmore", "**bold** <b> & |pipe|"];
    for item in files.iter_mut().flat_map(|f| &mut f.items) {
        let count = tc.draw(gs::integers::<usize>().max_value(2));
        item.facts.diagnostics = (0..count)
            .map(|_| check::Diagnostic {
                line: item.line,
                column: tc.draw(gs::integers::<usize>().min_value(1).max_value(9)),
                rule: tc.draw(gs::sampled_from(ids.clone())),
                severity: tc.draw(gs::sampled_from(vec![check::Level::Warn, check::Level::Error])),
                message: tc.draw(gs::sampled_from(messages.clone())).to_owned(),
            })
            .collect();
    }
    let found: Vec<(&FileReport, &check::Diagnostic)> = files
        .iter()
        .flat_map(|f| {
            f.items.iter().filter(|i| !i.ignored()).flat_map(move |i| i.facts.diagnostics.iter().map(move |d| (f, d)))
        })
        .collect();
    let slash = |path: &Path| path.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    let level = |d: &check::Diagnostic| if d.severity == check::Level::Error { "error" } else { "warning" };
    let both = |name: &str| (render(name, &plain_files), render(name, &files));

    let (before, after) = both("unix");
    let lines: Vec<String> = found
        .iter()
        .map(|(f, d)| format!("{}:{}:{}: {}: {}\n", f.path.display(), d.line, d.column, d.rule, d.message))
        .collect();
    assert_eq!(after, before + &lines.concat());

    let (before, after) = both("github");
    let added = after.strip_prefix(&before).unwrap();
    let read: Vec<String> = added
        .lines()
        .map(|line| {
            let (command, message) = line.strip_prefix("::").unwrap().split_once("::").unwrap();
            let (name, properties) = command.split_once(' ').unwrap();
            let properties: Vec<String> = properties.split(',').map(unescape_github).collect();
            format!("{name} {} {}", properties.join(" "), unescape_github(message))
        })
        .collect();
    let wanted: Vec<String> = found
        .iter()
        .map(|(f, d)| {
            let path = slash(&f.path);
            format!("{} file={path} line={} col={} title=docov/{} {}", level(d), d.line, d.column, d.rule, d.message)
        })
        .collect();
    assert_eq!(read, wanted, "{after}");

    let (before, after) = both("markdown");
    let added = after.strip_prefix(&before).unwrap();
    assert_eq!(added.is_empty(), found.is_empty());
    let (mut bullets, mut html) = (Vec::new(), String::new());
    for event in Parser::new(added) {
        match event {
            Event::Html(text) => html.push_str(&text),
            Event::Start(Tag::Item) => bullets.push(String::new()),
            Event::Code(text) | Event::Text(text) => bullets.last_mut().unwrap().push_str(&text),
            Event::Start(_) | Event::End(_) => {}
            other => panic!("{other:?}"),
        }
    }
    let summary = format!("<details>\n<summary>Doc checks: {} found</summary>\n</details>\n", found.len());
    assert_eq!(html, if found.is_empty() { String::new() } else { summary });
    let wanted: Vec<String> = found
        .iter()
        .map(|(f, d)| {
            let message = d.message.replace('\n', " ");
            format!("{}:{}:{} {} {}: {message}", slash(&f.path), d.line, d.column, level(d), d.rule)
        })
        .collect();
    assert_eq!(bullets, wanted, "{added}");

    let (before, after) = both("sarif");
    let (before, after): (serde_json::Value, serde_json::Value) =
        (serde_json::from_str(&before).unwrap(), serde_json::from_str(&after).unwrap());
    let rules = after["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
    let mut with: Vec<&str> = ids.iter().copied().filter(|id| found.iter().any(|(_, d)| d.rule == *id)).collect();
    with.dedup();
    let rule_ids: Vec<String> = rules.iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect();
    let wanted_ids: Vec<String> =
        std::iter::once("docov/undocumented".to_owned()).chain(with.iter().map(|id| format!("docov/{id}"))).collect();
    assert_eq!(rule_ids, wanted_ids);
    assert_eq!(&rules[0], &before["runs"][0]["tool"]["driver"]["rules"][0]);
    let results = after["runs"][0]["results"].as_array().unwrap();
    let old = before["runs"][0]["results"].as_array().unwrap();
    assert_eq!(&results[..old.len()], &old[..]);
    assert_eq!(results.len() - old.len(), found.len());
    for (result, (f, d)) in results[old.len()..].iter().zip(&found) {
        let index = result["ruleIndex"].as_u64().unwrap() as usize;
        assert_eq!(rules[index]["id"], result["ruleId"]);
        assert_eq!(result["ruleId"], format!("docov/{}", d.rule));
        assert!(!rules[index]["shortDescription"]["text"].as_str().unwrap().is_empty());
        assert_eq!((result["level"].as_str(), result["message"]["text"].as_str()), (Some(level(d)), Some(&*d.message)));
        let location = &result["locations"][0]["physicalLocation"];
        let uri = location["artifactLocation"]["uri"].as_str().unwrap();
        assert_eq!(percent_encoding::percent_decode_str(uri).decode_utf8().unwrap(), slash(&f.path));
        assert_eq!(
            (&location["region"]["startLine"], &location["region"]["startColumn"]),
            (&d.line.into(), &d.column.into())
        );
    }
}
