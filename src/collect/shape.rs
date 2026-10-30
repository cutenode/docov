//! What a declaration's code says that its JSDoc should say too, for the doc checks: for a
//! function, whether its own body returns or yields.

use oxc_ast::ast::{
    ArrowFunctionExpression, Class, Expression, Function, FunctionBody, ReturnStatement, TSType, TSTypeAnnotation,
    TSTypeName, YieldExpression,
};
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

/// What a declaration's code says that its JSDoc should say too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    /// Whether its own body returns a value: `return x;` (not a bare `return;`, `undefined` or
    /// `void x`), or an arrow function's expression body.
    pub returns: bool,
    /// Whether its own body yields a value.
    pub yields: bool,
    /// Whether its return type is `void`, `undefined` or `Promise<void>`.
    pub void: bool,
}

impl Shape {
    /// A function's or method's, from its return type and body.
    fn function(return_type: Option<&TSTypeAnnotation>, body: Option<&FunctionBody>) -> Self {
        let mut found = Body::default();
        if let Some(body) = body {
            found.visit_function_body(body);
        }
        let void = return_type.is_some_and(|t| nothing(&t.type_annotation));
        Self { returns: found.returns, yields: found.yields, void }
    }

    /// A function value's, or nothing for a value that isn't a function.
    pub(super) fn value(value: &Expression) -> Self {
        match value {
            Expression::FunctionExpression(f) => Self::of(f),
            Expression::ArrowFunctionExpression(f) => Self::arrow(f),
            _ => Self::default(),
        }
    }

    /// A function's.
    pub(super) fn of(f: &Function) -> Self {
        Self::function(f.return_type.as_deref(), f.body.as_deref())
    }

    fn arrow(f: &ArrowFunctionExpression) -> Self {
        let mut found = Body::default();
        found.visit_arrow_function_body(&f.body);
        let shape = Self::function(f.return_type.as_deref(), None);
        Self { returns: found.returns || f.body.as_expression().is_some_and(is_value), ..shape }
    }
}

/// Whether a returned expression is a value: not `undefined` or `void x`.
fn is_value(expression: &Expression) -> bool {
    !expression.is_undefined() && !expression.is_void()
}

/// Whether a return type says the function returns nothing: `void`, `undefined` or
/// `Promise<void>`.
fn nothing(t: &TSType) -> bool {
    match t {
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => true,
        TSType::TSTypeReference(r) => {
            matches!(&r.type_name, TSTypeName::IdentifierReference(id) if id.name == "Promise")
                && r.type_arguments.as_ref().and_then(|a| a.params.first()).is_some_and(nothing)
        }
        _ => false,
    }
}

/// What a function's own body does, past the functions and classes inside it.
#[derive(Default)]
struct Body {
    returns: bool,
    yields: bool,
}

impl<'a> Visit<'a> for Body {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    fn visit_class(&mut self, _: &Class<'a>) {}

    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        self.returns |= it.argument.as_ref().is_some_and(is_value);
        walk::walk_return_statement(self, it);
    }

    fn visit_yield_expression(&mut self, it: &YieldExpression<'a>) {
        self.yields |= it.argument.is_some();
        walk::walk_yield_expression(self, it);
    }
}
