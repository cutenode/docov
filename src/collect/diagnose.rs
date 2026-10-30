//! Runs the doc checks that are on over a file's counted items with a block.

use oxc_ast::{
    CommentContent,
    ast::{BindingIdentifier, Declaration, ExportDefaultDeclarationKind, Program, Statement},
};
use oxc_ecmascript::BoundNames;
use rustc_hash::FxHashSet;

use super::{Found, Lines, Rules, doc, jsdoc};
use crate::check::{Checks, Diagnostic, Subject};

/// What `checks` find in `found`'s block, if it has one: an item that's `exported` or not, in the
/// file whose text `lines` has, which declares or imports `names` at its top level.
pub(super) fn diagnose<'s>(
    checks: &Checks,
    found: &Found<'s>,
    exported: bool,
    lines: &Lines<'s>,
    names: &FxHashSet<&'s str>,
) -> Vec<Diagnostic> {
    let Some(doc) = &found.doc else { return Vec::new() };
    let subject = Subject {
        name: &found.name,
        exported,
        doc,
        params: found.params.as_deref(),
        shape: &found.shape,
        source: lines.source,
        names,
    };
    checks.run(&subject, |at| lines.position(at as usize))
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
