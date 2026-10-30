//! The `public` scope: what a package's entry points reach. It starts at package.json's `exports`,
//! `main`, `module`, `types` and `typings`, finds each one's source (back through tsconfig's
//! `outDir` and `rootDir` for built files), and follows `export … from`, `export * from` and the
//! imports a module exports again through the module graph.

use std::{
    fs,
    path::{Path, PathBuf},
};

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind, ImportDeclarationSpecifier, Statement, StringLiteral};
use oxc_parser::Parser;
use oxc_resolver::{ResolveOptions, Resolver};
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;

/// The module-level names a package's entry points reach, by file.
#[derive(Debug, Default)]
pub struct Api {
    files: FxHashMap<PathBuf, FxHashSet<String>>,
    sources: Vec<PathBuf>,
}

impl Api {
    /// The local names the entry points reach in the file at `path`, if they reach it at all.
    pub fn reach(&self, path: &Path) -> Option<&FxHashSet<String>> {
        fs::canonicalize(path).ok().and_then(|path| self.files.get(&path))
    }

    /// What it was read from: package.json, the tsconfig.json beside it (there or not), each
    /// module, and each module's folder, where an import that didn't resolve would find a file
    /// added there.
    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    /// What the entry points of the package.json in `dir`, or else the nearest one above `paths`,
    /// reach. An entry point without a source file is an error, as is a package.json that can't be
    /// found or read.
    pub fn load(dir: &Path, paths: &[PathBuf]) -> Result<Self, Vec<String>> {
        let package = find(dir, paths).ok_or_else(|| vec!["the public scope needs a package.json".to_owned()])?;
        let fail = |e: &dyn std::fmt::Display| vec![format!("{}: {e}", package.display())];
        let text = fs::read_to_string(&package).map_err(|e| fail(&e))?;
        let manifest: Value = serde_json::from_str(&text).map_err(|e| fail(&e))?;
        let root = package.parent().expect("a file is in a folder");
        let resolver = Resolver::new(options());
        let layout = Layout::read(root);
        let (mut entries, mut errors) = (Vec::new(), Vec::new());
        for target in targets(&manifest) {
            match layout.source(&resolver, root, &target) {
                Some(entry) => entries.push(entry),
                None => errors.push(format!("{}: entry point {target} has no source file", package.display())),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut graph = Graph { resolver, modules: FxHashMap::default(), files: FxHashMap::default() };
        graph.walk(entries);
        let folders = graph.modules.keys().map(|module| module.parent().expect("a file is in a folder").to_owned());
        let mut sources: Vec<PathBuf> = [package.clone(), root.join("tsconfig.json")].into();
        sources.extend(graph.modules.keys().cloned().chain(folders));
        sources.sort();
        sources.dedup();
        Ok(Self { files: graph.files, sources })
    }
}

/// The package.json in `dir`, or else the nearest one above one of `paths` (the current folder
/// when there are none).
fn find(dir: &Path, paths: &[PathBuf]) -> Option<PathBuf> {
    let here = [PathBuf::from(".")];
    let paths = if paths.is_empty() { &here[..] } else { paths };
    let starts = std::iter::once(dir.to_path_buf()).chain(paths.iter().filter_map(|p| fs::canonicalize(p).ok()));
    starts.flat_map(|start| start.ancestors().map(|d| d.join("package.json")).collect::<Vec<_>>()).find(|p| p.is_file())
}

/// How the resolver finds a module: TypeScript's extensions first, with `./x.js` finding `x.ts` as
/// TypeScript's ESM imports have it, and every condition a package's exports may use.
fn options() -> ResolveOptions {
    let strings = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    ResolveOptions {
        extensions: strings(&[".ts", ".tsx", ".mts", ".cts", ".d.ts", ".js", ".jsx", ".mjs", ".cjs"]),
        extension_alias: vec![
            (".js".into(), strings(&[".ts", ".tsx", ".js", ".jsx"])),
            (".mjs".into(), strings(&[".mts", ".mjs"])),
            (".cjs".into(), strings(&[".cts", ".cjs"])),
        ],
        condition_names: strings(&["types", "import", "require", "node", "default"]),
        ..ResolveOptions::default()
    }
}

/// Every path package.json names as an entry point: its `main`, `module`, `types` and `typings`,
/// and each path in its `exports`, under every condition and subpath. Subpath patterns (`./*`) name
/// no one file, so they're left out.
fn targets(manifest: &Value) -> Vec<String> {
    fn leaves(value: &Value, into: &mut Vec<String>) {
        match value {
            Value::String(path) if !path.contains('*') => into.push(path.clone()),
            Value::Array(values) => values.iter().for_each(|v| leaves(v, into)),
            Value::Object(values) => values.values().for_each(|v| leaves(v, into)),
            _ => {}
        }
    }
    let mut targets = Vec::new();
    for field in ["main", "module", "types", "typings", "exports"] {
        leaves(&manifest[field], &mut targets);
    }
    let mut seen = FxHashSet::default();
    targets.retain(|target| seen.insert(target.clone()));
    targets
}

/// Where tsconfig.json says built files come from: its `outDir`, and its `rootDir` (or else `src`
/// or the package's own folder).
struct Layout {
    out: Option<PathBuf>,
    roots: Vec<PathBuf>,
}

impl Layout {
    /// The layout tsconfig.json in `root` gives. Without one, or with one that doesn't parse,
    /// nothing is built.
    fn read(root: &Path) -> Self {
        let mut text = fs::read_to_string(root.join("tsconfig.json")).unwrap_or_default();
        let options = json_strip_comments::strip(&mut text)
            .ok()
            .and_then(|()| serde_json::from_str::<Value>(&text).ok())
            .map(|tsconfig| tsconfig["compilerOptions"].clone())
            .unwrap_or_default();
        let dir = |key: &str| options[key].as_str().map(PathBuf::from);
        let roots = dir("rootDir").map_or_else(|| vec![PathBuf::from("src"), PathBuf::from(".")], |root| vec![root]);
        Self { out: dir("outDir"), roots }
    }

    /// The source file behind `target`, a path from package.json: what it was built from, when
    /// it's under `outDir`, or else itself, found the way a resolver finds `./target`. A
    /// declaration file (`.d.ts`) stands for the module it declares.
    fn source(&self, resolver: &Resolver, root: &Path, target: &str) -> Option<PathBuf> {
        let target = target.strip_prefix("./").unwrap_or(target);
        let target =
            Path::new([".d.ts", ".d.mts", ".d.cts"].iter().find_map(|ext| target.strip_suffix(ext)).unwrap_or(target));
        let built = self.out.as_ref().and_then(|out| target.strip_prefix(out.strip_prefix("./").unwrap_or(out)).ok());
        let sources = built.into_iter().flat_map(|rest| self.roots.iter().map(move |root| root.join(rest)));
        sources
            .chain([target.to_path_buf()])
            .find_map(|candidate| resolve(resolver, root, &format!("./{}", candidate.display())))
    }
}

/// What a module exports, and how.
#[derive(Default)]
struct Module {
    /// Each name it exports from its own declarations or its imports, and the local name behind it.
    locals: FxHashMap<String, String>,
    /// Each name it exports from another module: that module, and the name there (`None` for the
    /// whole module, `export * as name`).
    reexports: FxHashMap<String, (PathBuf, Option<String>)>,
    /// The modules `export * from` takes every name but `default` from.
    stars: Vec<PathBuf>,
    /// Each name it imports: the module, and the name there (`None` for `import * as name`).
    imports: FxHashMap<String, (PathBuf, Option<String>)>,
}

/// What's wanted of a module: some of the names it exports, or all of them (but `default`, through
/// `export * from`).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Wanted {
    Name(String),
    All,
    AllButDefault,
}

/// The modules read so far, and what's reached in each.
struct Graph {
    resolver: Resolver,
    /// Each module read, or `None` for one that can't be read or parsed.
    modules: FxHashMap<PathBuf, Option<Module>>,
    files: FxHashMap<PathBuf, FxHashSet<String>>,
}

impl Graph {
    /// Follows every name the `entries` export to where it's declared.
    fn walk(&mut self, entries: Vec<PathBuf>) {
        let mut work: Vec<(PathBuf, Wanted)> = entries.into_iter().map(|entry| (entry, Wanted::All)).collect();
        let mut seen = FxHashSet::default();
        while let Some((path, wanted)) = work.pop() {
            if !seen.insert((path.clone(), wanted.clone())) {
                continue;
            }
            if !self.modules.contains_key(&path) {
                let module = self.read(&path);
                self.modules.insert(path.clone(), module);
            }
            let Some(module) = &self.modules[&path] else { continue };
            let reached = self.files.entry(path.clone()).or_default();
            let names: Vec<&String> = match &wanted {
                Wanted::Name(name) => vec![name],
                every => {
                    work.extend(module.stars.iter().map(|star| (star.clone(), Wanted::AllButDefault)));
                    let names = module.locals.keys().chain(module.reexports.keys());
                    names.filter(|name| *every == Wanted::All || *name != "default").collect()
                }
            };
            for name in names {
                if let Some(local) = module.locals.get(name) {
                    match module.imports.get(local) {
                        Some((from, imported)) => work.push((from.clone(), wanted_of(imported))),
                        None => {
                            reached.insert(local.clone());
                        }
                    }
                } else if let Some((from, imported)) = module.reexports.get(name) {
                    work.push((from.clone(), wanted_of(imported)));
                } else if name != "default" {
                    work.extend(module.stars.iter().map(|star| (star.clone(), Wanted::Name(name.clone()))));
                }
            }
        }
    }

    /// What the module at `path` exports, if it can be read and parsed.
    fn read(&self, path: &Path) -> Option<Module> {
        let source_type = SourceType::from_path(path).ok()?;
        let source = fs::read_to_string(path).ok()?;
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, &source, source_type).parse();
        let dir = path.parent().expect("a file is in a folder");
        let resolve =
            |specifier: &StringLiteral| specifier.value.as_str().and_then(|s| resolve(&self.resolver, dir, s));
        let mut module = Module::default();
        for statement in &parsed.program.body {
            match statement {
                Statement::ImportDeclaration(import) => {
                    let Some(from) = resolve(&import.source) else { continue };
                    for specifier in import.specifiers.iter().flatten() {
                        let (local, imported) = match specifier {
                            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                                (&s.local, Some(s.imported.name().to_string()))
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                                (&s.local, Some("default".to_owned()))
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => (&s.local, None),
                        };
                        module.imports.insert(local.name.to_string(), (from.clone(), imported));
                    }
                }
                Statement::ExportDeclaration(export) => {
                    module.locals.extend(declared(&export.declaration).into_iter().map(|name| (name.clone(), name)));
                }
                Statement::ExportNamedDeclaration(export) => {
                    let names =
                        export.specifiers.iter().map(|s| (s.exported.name().to_string(), s.local.name().to_string()));
                    module.locals.extend(names);
                }
                Statement::ExportFromDeclaration(export) => {
                    let Some(from) = resolve(&export.source) else { continue };
                    for s in &export.specifiers {
                        let imported = Some(s.local.name().to_string());
                        module.reexports.insert(s.exported.name().to_string(), (from.clone(), imported));
                    }
                }
                Statement::ExportDefaultDeclaration(export) => {
                    let local = match &export.declaration {
                        ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                            f.id.as_ref().map(|id| id.name.as_str())
                        }
                        ExportDefaultDeclarationKind::ClassDeclaration(c) => c.id.as_ref().map(|id| id.name.as_str()),
                        ExportDefaultDeclarationKind::Identifier(id) => Some(id.name.as_str()),
                        _ => None,
                    };
                    module.locals.insert("default".to_owned(), local.unwrap_or("default").to_owned());
                }
                Statement::ExportAllDeclaration(export) => {
                    let Some(from) = resolve(&export.source) else { continue };
                    match &export.exported {
                        Some(name) => {
                            module.reexports.insert(name.name().to_string(), (from, None));
                        }
                        None => module.stars.push(from),
                    }
                }
                _ => {}
            }
        }
        Some(module)
    }
}

/// The file `specifier` names from `dir`, found the way a resolver finds it, by its real path.
fn resolve(resolver: &Resolver, dir: &Path, specifier: &str) -> Option<PathBuf> {
    resolver.resolve(dir, specifier).ok().and_then(|resolved| fs::canonicalize(resolved.path()).ok())
}

/// What's wanted of a module for a name imported or exported from it.
fn wanted_of(imported: &Option<String>) -> Wanted {
    imported.clone().map_or(Wanted::All, Wanted::Name)
}

/// The names a declaration binds.
fn declared(declaration: &Declaration) -> Vec<String> {
    match declaration {
        Declaration::VariableDeclaration(v) => {
            v.declarations.iter().flat_map(|d| d.id.get_binding_identifiers()).map(|id| id.name.to_string()).collect()
        }
        declaration => declaration.id().map(|id| id.name.to_string()).into_iter().collect(),
    }
}
