//! Runs the doc checks that are on over a file's counted items with a block.

use std::path::Path;

use oxc_ast::{
    CommentContent,
    ast::{BindingIdentifier, Declaration, ExportDefaultDeclarationKind, Program, Statement},
};
use oxc_ecmascript::BoundNames;
use rustc_hash::FxHashSet;

use super::{Collected, Rules, doc, jsdoc};
use crate::check::Subject;

impl Collected<'_> {
    /// Runs `rules`' checks, if it has any, over the items that count (`reached` holds the local
    /// names the public scope's entry points reach in the file at `path`, if they reach it).
    pub(super) fn diagnose(&self, rules: &Rules, reached: Option<&FxHashSet<String>>, path: &Path) {
        let Some(checks) = &rules.checks else { return };
        for found in self.found.iter().filter(|f| rules.counts(f, &self.exported, reached)) {
            let Some(doc) = &found.doc else { continue };
            let exported = rules.exported(found, &self.exported);
            let subject = Subject {
                name: &found.name,
                exported,
                doc,
                params: found.params.as_deref(),
                shape: &found.shape,
                source: self.lines.source,
                names: &self.names,
            };
            checks.run(path, &subject, |at| self.lines.position(at as usize));
        }
    }
}

/// The names `program`, whose text is `source`, declares or imports at its top level, and the
/// types its JSDoc blocks declare with `@typedef` or `@callback`, when `rules` has checks to use
/// them.
pub(super) fn names<'s>(program: &Program<'s>, source: &'s str, rules: &Rules) -> FxHashSet<&'s str> {
    let mut names = FxHashSet::default();
    if rules.checks.is_none() {
        return names;
    }
    let mut add = |id: &BindingIdentifier<'s>| {
        names.insert(id.name.as_str());
    };
    for statement in &program.body {
        let declaration = match statement {
            Statement::ImportDeclaration(i) => {
                i.bound_names(&mut add);
                continue;
            }
            Statement::ExportDeclaration(e) => &e.declaration,
            Statement::ExportDefaultDeclaration(e) => {
                match &e.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => f.bound_names(&mut add),
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => c.bound_names(&mut add),
                    _ => {}
                }
                continue;
            }
            statement => {
                let Some(declaration) = statement.as_declaration() else { continue };
                declaration
            }
        };
        match declaration {
            Declaration::TSInterfaceDeclaration(d) => add(&d.id),
            Declaration::TSTypeAliasDeclaration(d) => add(&d.id),
            Declaration::TSEnumDeclaration(d) => add(&d.id),
            Declaration::TSNamespaceDeclaration(d) => add(&d.id),
            Declaration::TSImportEqualsDeclaration(d) => add(&d.id),
            declaration => declaration.bound_names(&mut add),
        }
    }
    let blocks = program.comments.iter().filter(|c| c.content == CommentContent::Jsdoc);
    names.extend(blocks.filter_map(|c| doc::declared_name(&jsdoc(c, source))));
    names
}
