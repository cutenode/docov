//! Properties over generated modules, projects and reports.

use std::{fs, path::Path, path::PathBuf, process::Command};

use docov::feature::{
    Feature, access::Access, add_ignore::AddIgnore, commonjs_members::CommonjsMembers, complete::Complete,
    examples::Examples, exports::Exports, file_docs::FileDocs, inherit::Inherit, kinds::Kinds, members::Members,
    release_tags::ReleaseTags, report_unused_ignores::ReportUnusedIgnores, rules::DocChecks, sort::Sort, types::Types,
    virtual_types::VirtualTypes,
};
use docov::{
    FileReport, Format, Item, Kind, Rules, Scope, Totals, check, collect, collect::Facts, collect::Ignore, config,
    feature, report, rewrite, scan, scan_file,
};
use hegel::{TestCase, generators as gs};
use oxc_span::SourceType;

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
        };
        Some(Item { documented, facts, ..self.item.clone() })
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
        if tc.draw(gs::booleans()) {
            let noise = tc.draw(gs::sampled_from(noise.to_vec()));
            self.skipped += usize::from(noise.contains("@license"));
            self.write(noise);
        }
        let doc = tc.draw(gs::booleans()).then(|| tc.draw(gs::sampled_from(BLOCKS.to_vec())));
        if let Some(block) = doc {
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
        self.expected.push(Expected { via, value: false, item, doc, protected: false, overrides: false, params });
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

/// Generated reports, with `docov-ignore` comments over some items when `ignores` is on.
fn reports(tc: &TestCase, ignores: bool) -> Vec<FileReport> {
    let item = gs::vecs(gs::integers::<usize>().max_value(ALL_KINDS.len() - 1)).min_size(3).max_size(3);
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
                    facts: Facts {
                        has_example: drawn[2] / 2 % 2 == 0,
                        params: i % 3,
                        exported: drawn[1] / 2 % 2 == 0,
                        release_tagged: (drawn[0] + drawn[1]) % 3 == 0,
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
    let mut files = reports(&tc, false);
    let totals = Totals::of(&files);
    assert!((0.0..=100.0).contains(&totals.percent()));
    assert_eq!(totals.total, files.iter().map(|f| f.items.len()).sum::<usize>());

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
    let options = report::Options { sort: Sort::Priority, ..report::Options::default() };
    let mut out = Vec::new();
    report::write_with(Format::Text, &files, &options, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
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
    assert_eq!(lines(&text), lines(&render(Format::Text, &files)));

    let json: serde_json::Value = serde_json::from_str(&render(Format::Json, &files)).unwrap();
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
    let json: serde_json::Value = serde_json::from_str(&render(Format::Json, &files)).unwrap();
    let shown = |v: &serde_json::Value| (v["withExamples"].as_u64(), v["examplesPercent"].as_f64());
    let wanted = |t: Totals| (Some(t.documented as u64), Some(t.percent()));
    assert_eq!(shown(&json), wanted(all));
    assert!(json["files"].as_array().unwrap().iter().map(shown).eq(files.iter().map(|f| wanted(of(f)))));

    let options = report::Options { examples: true, ..report::Options::default() };
    let mut out = Vec::new();
    report::write_with(Format::Text, &files, &options, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
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
    assert_eq!(plain, render(Format::Text, &files).lines().collect::<Vec<_>>());

    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100))).map(f64::from);
    let percent = all.percent();
    let failure = threshold
        .filter(|&t| percent < t)
        .map(|t| format!("{percent:.1}% with examples, under the {t}% examples threshold"));
    assert_eq!(Examples { text: false, threshold }.gate(&files), failure);
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

    let mut options = report::Options::default();
    feature.report(&mut options);
    let mut out = Vec::new();
    report::write_with(Format::Json, &files, &options, &mut out).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let shown = |v: &serde_json::Value| (v["releaseTagged"].as_u64(), v["releaseTagsPercent"].as_f64());
    let wanted = |t: Totals| if on { (Some(t.documented as u64), Some(t.percent())) } else { (None, None) };
    assert_eq!(shown(&json), wanted(all));
    assert!(json["files"].as_array().unwrap().iter().map(shown).eq(files.iter().map(|f| wanted(of(f)))));

    let append = |format| {
        let mut out = Vec::new();
        feature.append(format, &files, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let line = format!("{:>6.1}%  {}/{}  release tags\n", all.percent(), all.documented, all.total);
    assert_eq!(append(Format::Text), if on { line } else { String::new() });
    assert_eq!(append(Format::Json), "");

    let percent = all.percent();
    let failure = threshold
        .filter(|&t| percent < t)
        .map(|t| format!("{percent:.1}% with release tags, under the {t}% release tags threshold"));
    assert_eq!(feature.gate(&files), failure);
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
    let mut options = report::Options::default();
    feature.report(&mut options);
    let mut out = Vec::new();
    report::write_with(Format::Json, &files, &options, &mut out).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let shown = (json["typed"].as_u64(), json["typesPercent"].as_f64());
    assert_eq!(shown, if on { (Some(all.documented as u64), Some(all.percent())) } else { (None, None) });

    let append = |format| {
        let mut out = Vec::new();
        feature.append(format, &files, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let line = format!("{:>6.1}%  {}/{}  types\n", all.percent(), all.documented, all.total);
    assert_eq!(append(Format::Text), if on { line } else { String::new() });
    assert_eq!(append(Format::Lcov), "");
    let percent = all.percent();
    let failure =
        threshold.filter(|&t| percent < t).map(|t| format!("{percent:.1}% with types, under the {t}% types threshold"));
    assert_eq!(feature.gate(&files), failure);
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
    let docov =
        |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_docov")).args(args).current_dir(root).output().unwrap();
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
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

fn render(format: Format, files: &[FileReport]) -> String {
    let mut out = Vec::new();
    report::write(format, files, &mut out).unwrap();
    String::from_utf8(out).unwrap()
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

    let text = render(Format::Text, &files);
    let listed = text.lines().filter(|l| l.starts_with("         ")).count();
    let undocumented = files.iter().flat_map(|f| &f.items).filter(|i| !i.documented && i.ignore.is_none()).count();
    assert_eq!(listed, undocumented, "{text}");
    // A file with nothing counted has no line.
    let headed = text.lines().filter(|l| l.ends_with(".ts") && !l.starts_with("         ")).count();
    assert_eq!(headed, files.iter().filter(|f| counted(f) > 0).count(), "{text}");

    let json: serde_json::Value = serde_json::from_str(&render(Format::Json, &files)).unwrap();
    let marks = json["files"].as_array().unwrap().iter().flat_map(|f| f["items"].as_array().unwrap().clone());
    let wanted = files.iter().flat_map(|f| &f.items).map(|i| {
        let ignore = i.ignore.as_ref().filter(|_| !i.documented);
        (ignore.map(|_| true), ignore.and_then(|i| i.reason.clone()))
    });
    let marks = marks.map(|m| (m["ignored"].as_bool(), m["reason"].as_str().map(String::from)));
    assert!(marks.eq(wanted));

    let lcov = render(Format::Lcov, &files);
    let found: usize = lcov.lines().filter_map(|l| l.strip_prefix("LF:")).map(|n| n.parse::<usize>().unwrap()).sum();
    let mut lines: Vec<(usize, usize)> = Vec::new();
    for (f, file) in files.iter().enumerate() {
        lines.extend(file.items.iter().filter(|i| i.documented || i.ignore.is_none()).map(|i| (f, i.line)));
    }
    lines.dedup();
    assert_eq!(found, lines.len(), "{lcov}");
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
    assert_eq!(ReportUnusedIgnores(false).gate(&files), None);
    let gate = ReportUnusedIgnores(true).gate(&files);
    assert_eq!(gate, (!wanted.is_empty()).then(|| wanted.join("\n")));
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

    let json: serde_json::Value = serde_json::from_str(&render(Format::Json, &files)).unwrap();
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

    let append = |by_kind: bool, format| {
        let mut out = Vec::new();
        Kinds { only: None, by_kind }.append(format, &files, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let line: Vec<String> = kinds.iter().map(|(k, t)| format!("{} {}/{}", k.as_str(), t.documented, t.total)).collect();
    assert_eq!(append(true, Format::Text), format!("by kind: {}\n", line.join(", ")));
    assert_eq!(append(false, Format::Text), "");
    assert_eq!(append(true, Format::Json), "");
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
#[hegel::test(test_cases = 20)]
fn malformed_configs_fail_to_load_where_they_break(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let padding = " ".repeat(tc.draw(gs::integers::<usize>().max_value(3)));
    let (text, message, column) = tc.draw(gs::sampled_from(vec![
        ("[]", "invalid type: sequence, expected an object", 0),
        ("{ 1: 2 }", "key must be a string", 3),
        (r#"{ "paths": [1 }"#, "expected `,` or `]`", 15),
        (r#"{ "paths": 1 }"#, "invalid type: integer `1`, expected a sequence", 12),
        (r#"{ "scope": "some" }"#, "unknown variant `some`, expected one of `exports`, `all`, `public`", 17),
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
    let by_kind = tc.draw(gs::booleans());
    if by_kind {
        run.args.push("--by-kind".into());
    }

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
    assert_eq!(settings.get::<Kinds>().by_kind, by_kind);
    assert_eq!(settings.report().sort, sort);
    assert_eq!(settings.report().examples, examples);
    let release_tags = ReleaseTags { on: release_tags || release_threshold.is_some(), threshold: release_threshold };
    assert_eq!(*settings.get::<ReleaseTags>(), release_tags);
    let types = Types { on: types || types_threshold.is_some(), threshold: types_threshold };
    assert_eq!(*settings.get::<Types>(), types);
    assert_eq!(settings.report().metrics.len(), usize::from(release_tags.on) + usize::from(types.on));
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
    let docov =
        |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_docov")).args(args).current_dir(dir.path()).output().unwrap();
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
    let checks = std::sync::Arc::new(checks);
    rules.checks = Some(checks.clone());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.ts");
    fs::write(&path, source).unwrap();
    scan_file(&path, &rules).unwrap();
    let found = checks.take();
    assert!(checks.take().is_empty());
    found
        .into_iter()
        .map(|d| {
            assert_eq!(d.path, path);
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
/// one of them turns it on. The gate fails the scan with each error-level finding, and passes with
/// warnings alone, which it prints.
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
    let failure = wanted
        .contains(&("require-param", check::Level::Error))
        .then(|| format!("{}:1:1: require-param: missing @param for `a`", path.display()));
    assert_eq!(settings.gates(&reports), Vec::from_iter(failure));
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

    let mut options = report::Options::default();
    feature.report(&mut options);
    let mut out = Vec::new();
    report::write_with(Format::Json, &files, &options, &mut out).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let shown = |v: &serde_json::Value| (v["complete"].as_u64(), v["completePercent"].as_f64());
    let wanted = |t: Totals| if on { (Some(t.documented as u64), Some(t.percent())) } else { (None, None) };
    assert_eq!(shown(&json), wanted(all));
    assert!(json["files"].as_array().unwrap().iter().map(shown).eq(files.iter().map(|f| wanted(of(f)))));

    let append = |format| {
        let mut out = Vec::new();
        feature.append(format, &files, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let line = format!("{:>6.1}%  {}/{}  complete\n", all.percent(), all.documented, all.total);
    assert_eq!(append(Format::Text), if on { line } else { String::new() });
    assert_eq!(append(Format::Json), "");

    let percent = all.percent();
    let failure = threshold
        .filter(|&t| percent < t)
        .map(|t| format!("{percent:.1}% with complete, under the {t}% complete threshold"));
    assert_eq!(feature.gate(&files), failure);
}
