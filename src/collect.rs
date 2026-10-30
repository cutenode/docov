//! Finds the declarations in a file that should have JSDoc, and whether they do.

use std::{fs, io, path::Path, path::PathBuf};

mod doc;
mod facts;
mod ignore;
mod rules;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    AssignmentExpression, Class, ClassElement, Comment, CommentContent, Declaration, ExportDefaultDeclarationKind,
    Expression, FormalParameters, Function, MemberExpression, MethodDefinitionKind, ObjectExpression,
    ObjectPropertyKind, PropertyKey, Statement, TSAccessibility, TSInterfaceDeclaration, TSSignature, TSType,
};
use oxc_jsdoc::JSDoc;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use self::ignore::Ignores;
pub use self::{facts::Facts, ignore::Ignore, rules::Rules};
use crate::{
    Totals,
    feature::{file_docs::FileDocs, members::Members},
};

/// What kind of declaration an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
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
    /// A file, with `fileDocs`.
    Module,
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
            Self::Module => "module",
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
    /// The declared name, `Class.member` for members, `default` for an unnamed default export. A
    /// file's own item is named by its `@module`, or else its path (empty from [`scan`], which has
    /// no path).
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
    /// What its JSDoc says past that.
    #[serde(flatten)]
    pub facts: Facts,
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
        Ok(mut items) => {
            if let Some(file) = items.first_mut().filter(|i| i.kind == Kind::Module && i.name.is_empty()) {
                file.name = path.display().to_string();
            }
            Ok(FileReport { path: path.to_owned(), items })
        }
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
        .map(|c| (c.attached_to, jsdoc(c, source)))
        .filter(|(_, doc)| doc.span.source_text(source).chars().any(|ch| !ch.is_whitespace() && ch != '*'))
        .collect();
    let generated = parsed.program.comments.first().is_some_and(|c| c.span.source_text(source).contains("@generated"));
    if rules.skip_generated && generated {
        return Ok(Vec::new());
    }

    let lines = Lines::new(source);
    let ignores = Ignores::find(&parsed.program, &lines);
    let (found, exported, overload, declared) = (Vec::new(), FxHashSet::default(), None, FxHashMap::default());
    // At the top of a CommonJS file, `this` is `module.exports`.
    let this_exports = rules.commonjs_members && !parsed.program.source_type.is_module();
    let mut collector = Collector { source, rules, docs, found, exported, overload, declared, this_exports };
    if rules.file_docs == FileDocs::Require {
        collector.file(&parsed.program.comments);
    }
    let types = rules.virtual_types && source_type.is_javascript();
    if types {
        collector.types(&parsed.program.comments);
    }
    for statement in &parsed.program.body {
        collector.statement(statement);
    }
    // The types are where their blocks are, among the declarations.
    if types {
        collector.found.sort_by_key(|f| f.at);
    }

    let Collector { found, exported, .. } = collector;
    Ok(found
        .into_iter()
        .filter(|f| rules.counts(f, &exported))
        .map(|f| {
            let (line, column) = lines.position(f.at as usize);
            let (documented, facts) = (rules.documents(&f), Facts::of(&f, rules.exported(&f, &exported)));
            Item { name: f.name, kind: f.kind, line, column, documented, facts, ignore: ignores.covering(line) }
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
    /// How it's exported, where it's declared.
    via: Option<Via>,
    /// Whether it's a TypeScript `protected` member.
    protected: bool,
    /// Whether it's a TypeScript `override` member.
    overrides: bool,
    /// For a function or method, each parameter's name, or `None` for a destructured one.
    params: Option<Vec<Option<&'s str>>>,
    /// Whether it's documented, for an item whose own rule says: a file's, or a comment's type.
    documented: Option<bool>,
    /// Whether it counts under any scope, as a file's own item and a comment's type do.
    standalone: bool,
    /// Whether its parent's block has a matching `@property`, which documents it too.
    listed: bool,
}

impl<'s> Found<'s> {
    fn new(name: String, kind: Kind, at: u32, doc: Option<JSDoc<'s>>) -> Self {
        let (binding, via, protected, overrides, params, documented) = (None, None, false, false, None, None);
        let (standalone, listed) = (false, false);
        Self { name, kind, at, doc, binding, via, protected, overrides, params, documented, standalone, listed }
    }
}

struct Collector<'s> {
    source: &'s str,
    rules: &'s Rules,
    docs: FxHashMap<u32, JSDoc<'s>>,
    found: Vec<Found<'s>>,
    /// Names exported apart from their declarations, and how: `export { a }`, `export default a`,
    /// `export = a`, `exports.a = a`, `window.a = a`.
    exported: FxHashSet<(&'s str, Via)>,
    /// The name of the function the last statement declared without a body. An overload's
    /// implementation follows its signatures, and only the signatures are documented.
    overload: Option<&'s str>,
    /// The functions and classes declared by name so far, and how each is exported where it's
    /// declared, which `X.m = …` adds members to.
    declared: FxHashMap<&'s str, Option<Via>>,
    /// Whether `this.a = …` exports `a`.
    this_exports: bool,
}

/// How a module exports something.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Via {
    /// `export`.
    Esm,
    /// `exports.a =`, `module.exports =` or TypeScript's `export =`.
    Cjs,
    /// `window.a =` or `globalThis.a =`.
    Global,
}

/// The owner of a declaration being collected: where its docs go and how it's exported.
#[derive(Clone, Copy)]
struct Site<'s> {
    /// The start of the statement, where JSDoc attaches when the declaration is exported.
    at: u32,
    binding: Option<&'s str>,
    via: Option<Via>,
}

impl<'s> Collector<'s> {
    fn push(&mut self, site: Site<'s>, start: u32, name: String, kind: Kind) -> &mut Found<'s> {
        let doc = self.docs.get(&site.at).or_else(|| self.docs.get(&start)).cloned();
        // A decorator before `export` starts the declaration, and is where its doc goes.
        let at = site.at.min(start);
        self.found.push(Found { binding: site.binding, via: site.via, ..Found::new(name, kind, at, doc) });
        self.found.last_mut().expect("just pushed")
    }

    /// Pushes `owner`'s member `key`, at `start`, which a matching `@property` in `parent`
    /// documents too.
    fn member(
        &mut self,
        site: Site<'s>,
        start: u32,
        (owner, key): (&str, &str),
        kind: Kind,
        parent: Option<&JSDoc>,
    ) -> &mut Found<'s> {
        let listed = parent.is_some_and(|parent| doc::lists(parent, key));
        let found = self.push(Site { at: start, ..site }, start, format!("{owner}.{key}"), kind);
        found.listed = listed;
        found
    }

    /// Stops `comment`'s block from documenting the declaration after it.
    fn detach(&mut self, comment: &Comment, block: &JSDoc) {
        if self.docs.get(&comment.attached_to).is_some_and(|doc| doc.span == block.span) {
            self.docs.remove(&comment.attached_to);
        }
    }

    /// A type for each block that declares one (`@typedef` or `@callback`, with a name), at the
    /// block, documented when it has a description. The block documents nothing else.
    fn types(&mut self, comments: &[Comment]) {
        for comment in comments.iter().filter(|c| c.content == CommentContent::Jsdoc) {
            let block = jsdoc(comment, self.source);
            let Some((name, documented)) = doc::declared_type(&block) else { continue };
            self.detach(comment, &block);
            let found = Found::new(name, Kind::Type, comment.span.start, Some(block));
            self.found.push(Found { documented: Some(documented), standalone: true, ..found });
        }
    }

    /// The file's own item, at its start. Its first JSDoc block documents it when the block is about
    /// the file and says something, and is its doc, and no other item's, when it's about the file.
    fn file(&mut self, comments: &[Comment]) {
        let first = comments.iter().find(|c| matches!(c.content, CommentContent::Jsdoc | CommentContent::JsdocLegal));
        let block = first.map(|c| jsdoc(c, self.source)).filter(|block| doc::file(block).is_some());
        if let (Some(comment), Some(block)) = (first, &block) {
            self.detach(comment, block);
        }
        let documented = Some(block.as_ref().and_then(doc::file) == Some(true));
        let name = block.as_ref().and_then(|b| doc::text(b, "module"));
        let name = name.and_then(|name| name.split_whitespace().next().map(str::to_owned)).unwrap_or_default();
        self.found.push(Found { documented, standalone: true, ..Found::new(name, Kind::Module, 0, block) });
    }

    fn statement(&mut self, statement: &Statement<'s>) {
        let overload = self.overload.take();
        let at = statement.span().start;
        match statement {
            Statement::ExportDeclaration(e) => self.declaration(&e.declaration, at, Some(Via::Esm), overload),
            Statement::ExportNamedDeclaration(e) => {
                self.exported.extend(e.specifiers.iter().map(|s| (s.local.name().as_str(), Via::Esm)));
            }
            Statement::ExportDefaultDeclaration(e) => {
                let site = Site { at, binding: None, via: Some(Via::Esm) };
                match &e.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => self.function(f, site, overload),
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => self.class(c, site),
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(i) => self.interface(i, site),
                    ExportDefaultDeclarationKind::Identifier(id) => {
                        self.exported.insert((id.name.as_str(), Via::Esm));
                    }
                    expression => {
                        self.push(site, expression.span().start, "default".into(), Kind::Default);
                    }
                }
            }
            Statement::TSExportAssignment(e) => {
                if let Expression::Identifier(id) = &e.expression {
                    self.exported.insert((id.name.as_str(), Via::Cjs));
                }
            }
            Statement::ExpressionStatement(e) => {
                if let Expression::AssignmentExpression(assignment) = &e.expression {
                    self.commonjs(assignment, at);
                }
            }
            _ => {
                if let Some(declaration) = statement.as_declaration() {
                    self.declaration(declaration, at, None, overload);
                }
            }
        }
    }

    fn declaration(&mut self, declaration: &Declaration<'s>, at: u32, via: Option<Via>, overload: Option<&'s str>) {
        let site =
            |binding: &oxc_ast::ast::BindingIdentifier<'s>| Site { at, binding: Some(binding.name.as_str()), via };
        match declaration {
            Declaration::VariableDeclaration(v) => {
                // Destructured bindings (`const { a } = b`) have nowhere for a doc of their own, and
                // `require()` is an import.
                for d in &v.declarations {
                    if d.init.as_ref().is_some_and(Expression::is_require_call) {
                        continue;
                    }
                    if let Some(id) = d.id.get_binding_identifier() {
                        let doc = self.push(site(id), d.span.start, id.name.to_string(), Kind::Variable).doc.clone();
                        if let Some(Expression::ObjectExpression(object)) = &d.init
                            && self.rules.members == Members::All
                        {
                            self.object(object, id.name.as_str(), doc, site(id));
                        }
                    }
                }
            }
            Declaration::FunctionDeclaration(f) => {
                let site = Site { at, binding: f.id.as_ref().map(|id| id.name.as_str()), via };
                self.declared.extend(site.binding.map(|name| (name, via)));
                self.function(f, site, overload);
            }
            Declaration::ClassDeclaration(c) => {
                let site = Site { at, binding: c.id.as_ref().map(|id| id.name.as_str()), via };
                self.declared.extend(site.binding.map(|name| (name, via)));
                self.class(c, site);
            }
            Declaration::TSInterfaceDeclaration(i) => self.interface(i, site(&i.id)),
            Declaration::TSTypeAliasDeclaration(t) => {
                let doc = self.push(site(&t.id), t.span.start, t.id.name.to_string(), Kind::Type).doc.clone();
                if let TSType::TSTypeLiteral(literal) = &t.type_annotation
                    && self.rules.members == Members::All
                {
                    self.signatures(&literal.members, t.id.name.as_str(), site(&t.id), doc.as_ref());
                }
            }
            Declaration::TSEnumDeclaration(e) => {
                let (name, site) = (e.id.name.as_str(), site(&e.id));
                let doc = self.push(site, e.span.start, name.to_owned(), Kind::Enum).doc.clone();
                for member in e.body.members.iter().filter(|_| self.rules.members == Members::All) {
                    let key = member.id.static_name().display().to_string();
                    self.member(site, member.span.start, (name, &key), Kind::Property, doc.as_ref());
                }
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

    /// CommonJS exports: `module.exports = …`, `module.exports.name = …` and `exports.name = …`,
    /// and globals: `window.name = …` and `globalThis.name = …`. An identifier exports its
    /// declaration, which carries the doc; anything else is an item here. `module.exports = { … }`
    /// exports each property.
    fn commonjs(&mut self, assignment: &AssignmentExpression<'s>, at: u32) {
        let Some(target) = assignment.left.as_member_expression() else { return };
        let value = &assignment.right;
        let is_module_exports = target.is_specific_member_access("module", "exports");
        let this = self.this_exports && matches!(target.object(), Expression::ThisExpression(_));
        let via = if is_module_exports || this || is_commonjs_export(target) {
            Via::Cjs
        } else if target.object().is_specific_id("window") || target.object().is_specific_id("globalThis") {
            Via::Global
        } else {
            return self.assigned_member(target, value, at);
        };
        let site = Site { at, binding: None, via: Some(via) };
        if let Expression::Identifier(id) = value {
            self.exported.insert((id.name.as_str(), via));
            return;
        }
        if !is_module_exports {
            if let Some(name) = target.static_property_name().and_then(|name| name.as_str()) {
                self.push(site, at, name.to_owned(), value_kind(value)).params = value_params(value);
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
            self.push(site, at, name.unwrap_or("default").to_owned(), kind).params = value_params(value);
            return;
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(p) = property else { continue };
            if let Expression::Identifier(id) = &p.value {
                self.exported.insert((id.name.as_str(), Via::Cjs));
                continue;
            }
            let start = p.span.start;
            let key = self.key(&p.key);
            self.push(Site { at: start, ..site }, start, key, value_kind(&p.value)).params = value_params(&p.value);
        }
    }

    /// With CommonJS members on, `X.prototype.m = …` and `X.m = …`, for a function or class `X`
    /// declared here, as `X`'s member `m`, counting where `X` does.
    fn assigned_member(&mut self, target: &MemberExpression<'s>, value: &Expression<'s>, at: u32) {
        if !self.rules.commonjs_members {
            return;
        }
        let object = match target.object() {
            Expression::StaticMemberExpression(m) if m.property.name == "prototype" => &m.object,
            object => object,
        };
        let Expression::Identifier(owner) = object else { return };
        let owner = owner.name.as_str();
        let key = target.static_property_name().and_then(|name| name.as_str());
        let (Some(&via), Some(key)) = (self.declared.get(owner), key) else { return };
        let kind = if value.is_function() { Kind::Method } else { Kind::Property };
        let site = Site { at, binding: Some(owner), via };
        self.member(site, at, (owner, key), kind, None).params = value_params(value);
    }

    fn function(&mut self, f: &Function<'s>, site: Site<'s>, overload: Option<&'s str>) {
        let name = f.id.as_ref().map_or("default", |id| id.name.as_str());
        if f.body.is_none() {
            self.overload = Some(name);
        } else if overload == Some(name) {
            return;
        }
        self.push(site, f.span.start, name.to_owned(), Kind::Function).params = Some(params(&f.params));
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
            let (start, key, kind, accessibility, overrides, method_params) = match element {
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
                    let method = m.kind == MethodDefinitionKind::Method;
                    let kind = if method { Kind::Method } else { Kind::Accessor };
                    (m.span.start, key, kind, m.accessibility, m.r#override, method.then(|| params(&m.value.params)))
                }
                ClassElement::PropertyDefinition(p) if !is_private(&p.key, p.accessibility) => {
                    (p.span.start, self.key(&p.key), Kind::Property, p.accessibility, p.r#override, None)
                }
                ClassElement::AccessorProperty(a) if !is_private(&a.key, a.accessibility) => {
                    (a.span.start, self.key(&a.key), Kind::Accessor, a.accessibility, a.r#override, None)
                }
                _ => continue,
            };
            let found = self.push(Site { at: start, ..site }, start, format!("{class_name}.{key}"), kind);
            found.protected = accessibility == Some(TSAccessibility::Protected);
            found.overrides = overrides;
            found.params = method_params;
        }
    }

    fn interface(&mut self, interface: &TSInterfaceDeclaration<'s>, site: Site<'s>) {
        let name = interface.id.name.as_str();
        self.push(site, interface.span.start, name.to_owned(), Kind::Interface);
        self.signatures(&interface.body.body, name, site, None);
    }

    /// The property and method signatures of an interface or object type, as members of `owner`,
    /// which a matching `@property` in `parent` documents too.
    fn signatures(&mut self, signatures: &[TSSignature<'s>], owner: &str, site: Site<'s>, parent: Option<&JSDoc>) {
        for signature in signatures {
            let (start, key, kind, method_params) = match signature {
                TSSignature::TSPropertySignature(p) => (p.span.start, self.key(&p.key), Kind::Property, None),
                TSSignature::TSMethodSignature(m) => {
                    (m.span.start, self.key(&m.key), Kind::Method, Some(params(&m.params)))
                }
                _ => continue,
            };
            self.member(site, start, (owner, &key), kind, parent).params = method_params;
        }
    }

    /// The properties of an object literal whose block tags it `@enum`, `@namespace` or
    /// `@lends X`, as members of `name` (or `X`), which a matching `@property` there documents too.
    fn object(&mut self, object: &ObjectExpression<'s>, name: &str, parent: Option<JSDoc<'s>>, site: Site<'s>) {
        let Some(parent) = parent.filter(|doc| ["enum", "namespace", "lends"].iter().any(|tag| doc::has_tag(doc, tag)))
        else {
            return;
        };
        let lends = doc::text(&parent, "lends").and_then(|text| text.split_whitespace().next().map(str::to_owned));
        let owner = lends.as_deref().unwrap_or(name);
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(p) = property else { continue };
            let (key, kind) = (self.key(&p.key), if p.value.is_function() { Kind::Method } else { Kind::Property });
            self.member(site, p.span.start, (owner, &key), kind, Some(&parent)).params = value_params(&p.value);
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

/// A JSDoc comment's block, past its `/**`, as oxc_jsdoc reads it.
fn jsdoc<'s>(comment: &Comment, source: &'s str) -> JSDoc<'s> {
    let span = Span::new(comment.content_span().start + 1, comment.content_span().end);
    JSDoc::new(span.source_text(source), span)
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

/// Each of a function's parameters' names, or `None` for a destructured one.
fn params<'s>(params: &FormalParameters<'s>) -> Vec<Option<&'s str>> {
    let name = |pattern: &oxc_ast::ast::BindingPattern<'s>| pattern.get_binding_identifier().map(|id| id.name.as_str());
    let rest = params.rest.as_ref().map(|rest| name(&rest.rest.argument));
    params.items.iter().map(|p| name(&p.pattern)).chain(rest).collect()
}

/// A function value's parameters, or `None` for a value that isn't a function.
fn value_params<'s>(value: &Expression<'s>) -> Option<Vec<Option<&'s str>>> {
    match value {
        Expression::FunctionExpression(f) => Some(params(&f.params)),
        Expression::ArrowFunctionExpression(f) => Some(params(&f.params)),
        _ => None,
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
