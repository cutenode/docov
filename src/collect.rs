//! Finds the declarations in a file that should have JSDoc, and whether they do.

use std::{fs, io, path::Path, path::PathBuf};

mod doc;
mod ignore;
mod rules;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    AssignmentExpression, Class, ClassElement, CommentContent, Declaration, ExportDefaultDeclarationKind, Expression,
    Function, MemberExpression, MethodDefinitionKind, ObjectPropertyKind, PropertyKey, Statement, TSAccessibility,
    TSInterfaceDeclaration, TSSignature,
};
use oxc_jsdoc::JSDoc;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;

use self::ignore::Ignores;
pub use self::{ignore::Ignore, rules::Rules};
use crate::Totals;

/// What kind of declaration an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `function f() {}`, or a function assigned to an export.
    Function,
    /// `class C {}`, or a class assigned to an export.
    Class,
    /// A class method or an interface method signature.
    Method,
    /// A getter (with its setter, if any), a lone setter, or an `accessor` property.
    Accessor,
    /// A class field or an interface property signature.
    Property,
    /// `const`, `let` or `var`, or another value CommonJS exports by name.
    Variable,
    /// `interface I {}`.
    Interface,
    /// `type T = …`.
    Type,
    /// `enum E {}`.
    Enum,
    /// `namespace N {}`.
    Namespace,
    /// `export default` or `module.exports =` of an expression.
    Default,
}

impl Kind {
    /// The kind's name in reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Class => "class",
            Self::Method => "method",
            Self::Accessor => "accessor",
            Self::Property => "property",
            Self::Variable => "variable",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::Enum => "enum",
            Self::Namespace => "namespace",
            Self::Default => "default",
        }
    }
}

impl Serialize for Kind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A declaration that should have JSDoc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    /// The declared name, `Class.member` for members, `default` for an unnamed default export.
    pub name: String,
    /// What kind of declaration it is.
    pub kind: Kind,
    /// Where the JSDoc goes: the start of the declaration, at its `export` or first decorator if
    /// it has them. 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
    /// Whether a JSDoc block with something in it comes right before it (and, by the rules, says
    /// enough).
    pub documented: bool,
    /// The `docov-ignore` comment that covers it, if one does. The JSON report shapes it.
    #[serde(skip)]
    pub ignore: Option<Ignore>,
}

impl Item {
    /// Whether it's out of the total: undocumented, and covered by a `docov-ignore` comment. A
    /// documented item counts as documented, ignore or not.
    pub fn ignored(&self) -> bool {
        !self.documented && self.ignore.is_some()
    }
}

/// One file's items, in source order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileReport {
    /// The file, as the walk or the command line named it.
    pub path: PathBuf,
    /// Its items.
    pub items: Vec<Item>,
}

impl FileReport {
    /// Its items that aren't [ignored](Item::ignored).
    pub fn counted(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| !i.ignored())
    }

    /// How many of its counted items are documented.
    pub fn totals(&self) -> Totals {
        Totals { documented: self.items.iter().filter(|i| i.documented).count(), total: self.counted().count() }
    }
}

/// Why a file couldn't be scanned. Each variant's `path` is the file's.
#[derive(Debug, thiserror::Error)]
#[expect(missing_docs, reason = "the variants document their fields")]
pub enum Error {
    /// The file couldn't be read, or isn't UTF-8.
    #[error("{}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    /// The file's extension isn't one oxc parses.
    #[error("{}: not a JavaScript or TypeScript file", path.display())]
    Language { path: PathBuf },
    /// The file doesn't parse, at this 1-based line and column.
    #[error("{}:{line}:{column}: {message}", path.display())]
    Syntax { path: PathBuf, line: usize, column: usize, message: String },
}

/// A syntax error, at a byte offset into the source.
#[derive(Debug, PartialEq, Eq)]
pub struct SyntaxError {
    /// Where it is, in bytes.
    pub offset: usize,
    /// The parser's message.
    pub message: String,
}

/// Reads and scans one file.
pub fn scan_file(path: &Path, rules: &Rules) -> Result<FileReport, Error> {
    let source_type = SourceType::from_path(path).map_err(|_| Error::Language { path: path.to_owned() })?;
    let source = fs::read_to_string(path).map_err(|source| Error::Read { path: path.to_owned(), source })?;
    match scan(&source, source_type, rules) {
        Ok(items) => Ok(FileReport { path: path.to_owned(), items }),
        Err(e) => {
            let (line, column) = Lines::new(&source).position(e.offset);
            Err(Error::Syntax { path: path.to_owned(), line, column, message: e.message })
        }
    }
}

/// The items in `source` that count by `rules`. Any syntax error fails the scan: a partial AST
/// would undercount.
pub fn scan(source: &str, source_type: SourceType, rules: &Rules) -> Result<Vec<Item>, SyntaxError> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if let Some(error) = parsed.diagnostics.errors().next() {
        let offset = error.labels.first().map_or(0, |label| label.offset() as usize);
        return Err(SyntaxError { offset, message: error.message.to_string() });
    }

    // The JSDoc block with something in it attached to each position: the first token after it.
    // With several, the nearest is the item's, as in jsdoc. Legal comments (`/** @license */`) are
    // file headers, not docs.
    let docs = parsed
        .program
        .comments
        .iter()
        .filter(|c| c.content == CommentContent::Jsdoc && c.is_leading())
        .filter_map(|c| {
            // Past the `/**`, as oxc_jsdoc reads it.
            let span = Span::new(c.content_span().start + 1, c.content_span().end);
            let text = span.source_text(source);
            text.chars().any(|ch| !ch.is_whitespace() && ch != '*').then(|| (c.attached_to, JSDoc::new(text, span)))
        })
        .collect();
    let lines = Lines::new(source);
    let ignores = Ignores::find(&parsed.program, &lines);
    let mut collector = Collector { source, docs, found: Vec::new(), exported: FxHashSet::default(), overload: None };
    for statement in &parsed.program.body {
        collector.statement(statement);
    }

    let Collector { found, exported, .. } = collector;
    Ok(found
        .into_iter()
        .filter(|f| rules.counts(f, &exported))
        .map(|f| {
            let (line, column) = lines.position(f.at as usize);
            let documented = rules.documents(&f);
            Item { name: f.name, kind: f.kind, line, column, documented, ignore: ignores.covering(line) }
        })
        .collect())
}

struct Found<'s> {
    name: String,
    kind: Kind,
    at: u32,
    /// Its JSDoc block, if it has one with something in it.
    doc: Option<JSDoc<'s>>,
    /// The module-level name that exports it (a member's class), when `export { name }` can.
    binding: Option<&'s str>,
    exported: bool,
}

struct Collector<'s> {
    source: &'s str,
    docs: FxHashMap<u32, JSDoc<'s>>,
    found: Vec<Found<'s>>,
    /// Names exported apart from their declarations: `export { a }`, `export default a`,
    /// `export = a`, `exports.a = a`.
    exported: FxHashSet<&'s str>,
    /// The name of the function the last statement declared without a body. An overload's
    /// implementation follows its signatures, and only the signatures are documented.
    overload: Option<&'s str>,
}

/// The owner of a declaration being collected: where its docs go and how it's exported.
#[derive(Clone, Copy)]
struct Site<'s> {
    /// The start of the statement, where JSDoc attaches when the declaration is exported.
    at: u32,
    binding: Option<&'s str>,
    exported: bool,
}

impl<'s> Collector<'s> {
    fn push(&mut self, site: Site<'s>, start: u32, name: String, kind: Kind) {
        let doc = self.docs.get(&site.at).or_else(|| self.docs.get(&start)).cloned();
        // A decorator before `export` starts the declaration, and is where its doc goes.
        let at = site.at.min(start);
        self.found.push(Found { name, kind, at, doc, binding: site.binding, exported: site.exported });
    }

    fn statement(&mut self, statement: &Statement<'s>) {
        let overload = self.overload.take();
        let at = statement.span().start;
        match statement {
            Statement::ExportDeclaration(e) => self.declaration(&e.declaration, at, true, overload),
            Statement::ExportNamedDeclaration(e) => {
                self.exported.extend(e.specifiers.iter().map(|s| s.local.name().as_str()));
            }
            Statement::ExportDefaultDeclaration(e) => {
                let site = Site { at, binding: None, exported: true };
                match &e.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => self.function(f, site, overload),
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => self.class(c, site),
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(i) => self.interface(i, site),
                    ExportDefaultDeclarationKind::Identifier(id) => {
                        self.exported.insert(id.name.as_str());
                    }
                    expression => self.push(site, expression.span().start, "default".into(), Kind::Default),
                }
            }
            Statement::TSExportAssignment(e) => {
                if let Expression::Identifier(id) = &e.expression {
                    self.exported.insert(id.name.as_str());
                }
            }
            Statement::ExpressionStatement(e) => {
                if let Expression::AssignmentExpression(assignment) = &e.expression {
                    self.commonjs(assignment, at);
                }
            }
            _ => {
                if let Some(declaration) = statement.as_declaration() {
                    self.declaration(declaration, at, false, overload);
                }
            }
        }
    }

    fn declaration(&mut self, declaration: &Declaration<'s>, at: u32, exported: bool, overload: Option<&'s str>) {
        let site =
            |binding: &oxc_ast::ast::BindingIdentifier<'s>| Site { at, binding: Some(binding.name.as_str()), exported };
        match declaration {
            Declaration::VariableDeclaration(v) => {
                // Destructured bindings (`const { a } = b`) have nowhere for a doc of their own, and
                // `require()` is an import.
                for d in &v.declarations {
                    if d.init.as_ref().is_some_and(Expression::is_require_call) {
                        continue;
                    }
                    if let Some(id) = d.id.get_binding_identifier() {
                        self.push(site(id), d.span.start, id.name.to_string(), Kind::Variable);
                    }
                }
            }
            Declaration::FunctionDeclaration(f) => {
                let site = Site { at, binding: f.id.as_ref().map(|id| id.name.as_str()), exported };
                self.function(f, site, overload);
            }
            Declaration::ClassDeclaration(c) => {
                self.class(c, Site { at, binding: c.id.as_ref().map(|id| id.name.as_str()), exported });
            }
            Declaration::TSInterfaceDeclaration(i) => self.interface(i, site(&i.id)),
            Declaration::TSTypeAliasDeclaration(t) => {
                self.push(site(&t.id), t.span.start, t.id.name.to_string(), Kind::Type);
            }
            Declaration::TSEnumDeclaration(e) => {
                self.push(site(&e.id), e.span.start, e.id.name.to_string(), Kind::Enum)
            }
            Declaration::TSNamespaceDeclaration(n) => {
                self.push(site(&n.id), n.span.start, n.id.name.to_string(), Kind::Namespace);
            }
            // `declare module 'x'`, `declare global` and `import x = require()` declare nothing of
            // this module's own.
            Declaration::TSExternalModuleDeclaration(_)
            | Declaration::TSGlobalDeclaration(_)
            | Declaration::TSImportEqualsDeclaration(_) => {}
        }
    }

    /// CommonJS exports: `module.exports = …`, `module.exports.name = …` and `exports.name = …`.
    /// An identifier exports its declaration, which carries the doc; anything else is an item
    /// here. `module.exports = { … }` exports each property.
    fn commonjs(&mut self, assignment: &AssignmentExpression<'s>, at: u32) {
        let Some(target) = assignment.left.as_member_expression() else { return };
        let site = Site { at, binding: None, exported: true };
        let value = &assignment.right;
        let is_module_exports = target.is_specific_member_access("module", "exports");
        if let Expression::Identifier(id) = value {
            if is_module_exports || is_commonjs_export(target) {
                self.exported.insert(id.name.as_str());
            }
            return;
        }
        if !is_module_exports {
            let name = target.static_property_name().and_then(|name| name.as_str());
            if let Some(name) = name.filter(|_| is_commonjs_export(target)) {
                self.push(site, at, name.to_owned(), value_kind(value));
            }
            return;
        }
        let Expression::ObjectExpression(object) = value else {
            let name = match value {
                Expression::FunctionExpression(f) => f.id.as_ref().map(|id| id.name.as_str()),
                Expression::ClassExpression(c) => c.id.as_ref().map(|id| id.name.as_str()),
                _ => None,
            };
            let kind = match value_kind(value) {
                Kind::Variable => Kind::Default,
                kind => kind,
            };
            self.push(site, at, name.unwrap_or("default").to_owned(), kind);
            return;
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(p) = property else { continue };
            if let Expression::Identifier(id) = &p.value {
                self.exported.insert(id.name.as_str());
                continue;
            }
            let start = p.span.start;
            let key = self.key(&p.key);
            self.push(Site { at: start, ..site }, start, key, value_kind(&p.value));
        }
    }

    fn function(&mut self, f: &Function<'s>, site: Site<'s>, overload: Option<&'s str>) {
        let name = f.id.as_ref().map_or("default", |id| id.name.as_str());
        if f.body.is_none() {
            self.overload = Some(name);
        } else if overload == Some(name) {
            return;
        }
        self.push(site, f.span.start, name.to_owned(), Kind::Function);
    }

    fn class(&mut self, class: &Class<'s>, site: Site<'s>) {
        let class_name = class.id.as_ref().map_or("default", |id| id.name.as_str());
        // Decorators can come before `export`, and a doc before them attaches to the first one.
        let start = class.decorators.first().map_or(class.span.start, |d| d.span.start);
        self.push(site, start, class_name.to_owned(), Kind::Class);

        // A getter and its setter are one property: the getter carries the doc.
        let getters: FxHashSet<String> = class
            .body
            .body
            .iter()
            .filter_map(|e| match e {
                ClassElement::MethodDefinition(m) if m.kind == MethodDefinitionKind::Get => Some(self.key(&m.key)),
                _ => None,
            })
            .collect();
        let mut overload = None;
        for element in &class.body.body {
            let previous = overload.take();
            let (start, key, kind) = match element {
                ClassElement::MethodDefinition(m) => {
                    if m.kind.is_constructor() || is_private(&m.key, m.accessibility) {
                        continue;
                    }
                    let key = self.key(&m.key);
                    if m.kind == MethodDefinitionKind::Set && getters.contains(&key) {
                        continue;
                    }
                    if m.value.body.is_none() {
                        overload = Some(key.clone());
                    } else if previous.as_ref() == Some(&key) {
                        continue;
                    }
                    let kind = if m.kind == MethodDefinitionKind::Method { Kind::Method } else { Kind::Accessor };
                    (m.span.start, key, kind)
                }
                ClassElement::PropertyDefinition(p) if !is_private(&p.key, p.accessibility) => {
                    (p.span.start, self.key(&p.key), Kind::Property)
                }
                ClassElement::AccessorProperty(a) if !is_private(&a.key, a.accessibility) => {
                    (a.span.start, self.key(&a.key), Kind::Accessor)
                }
                _ => continue,
            };
            self.push(Site { at: start, ..site }, start, format!("{class_name}.{key}"), kind);
        }
    }

    fn interface(&mut self, interface: &TSInterfaceDeclaration<'s>, site: Site<'s>) {
        let name = interface.id.name.as_str();
        self.push(site, interface.span.start, name.to_owned(), Kind::Interface);
        for signature in &interface.body.body {
            let (start, key, kind) = match signature {
                TSSignature::TSPropertySignature(p) => (p.span.start, self.key(&p.key), Kind::Property),
                TSSignature::TSMethodSignature(m) => (m.span.start, self.key(&m.key), Kind::Method),
                _ => continue,
            };
            self.push(Site { at: start, ..site }, start, format!("{name}.{key}"), kind);
        }
    }

    /// A member's name, or for a computed key without one, the key as written (`[Symbol.iterator]`).
    fn key(&self, key: &PropertyKey<'s>) -> String {
        match key.static_name().as_ref().and_then(|name| name.as_str()) {
            Some(name) => name.to_owned(),
            None => format!("[{}]", key.span().source_text(self.source)),
        }
    }
}

/// `exports.name` or `module.exports.name`.
fn is_commonjs_export(target: &MemberExpression) -> bool {
    let object = target.object();
    object.is_specific_id("exports") || object.is_specific_member_access("module", "exports")
}

fn value_kind(value: &Expression) -> Kind {
    match value {
        _ if value.is_function() => Kind::Function,
        Expression::ClassExpression(_) => Kind::Class,
        _ => Kind::Variable,
    }
}

fn is_private(key: &PropertyKey, accessibility: Option<TSAccessibility>) -> bool {
    key.is_private_identifier() || accessibility == Some(TSAccessibility::Private)
}

/// Turns byte offsets into 1-based lines and columns.
struct Lines<'s> {
    source: &'s str,
    starts: Vec<usize>,
}

impl<'s> Lines<'s> {
    fn new(source: &'s str) -> Self {
        let starts = std::iter::once(0).chain(source.match_indices('\n').map(|(i, _)| i + 1)).collect();
        Self { source, starts }
    }

    fn position(&self, offset: usize) -> (usize, usize) {
        let line = self.starts.partition_point(|&start| start <= offset);
        let start = self.starts[line - 1];
        (line, self.source[start..offset].chars().count() + 1)
    }
}

#[cfg(test)]
mod tests;
