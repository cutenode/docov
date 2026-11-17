//! What the fuzz targets share: rules with every feature and doc check on, and what has to hold of
//! any report a scan gives.

use std::sync::Arc;

use docov::{
    FileReport, Rules, Scope,
    check::{self, Checks, Level, Setting},
    feature::{exports::Exports, file_docs::FileDocs, members::Members, syntax::Syntax},
};

/// The file names whose extensions pick each language oxc parses.
pub const FILES: [&str; 9] = ["a.js", "a.mjs", "a.cjs", "a.jsx", "a.ts", "a.mts", "a.cts", "a.tsx", "a.d.ts"];

/// Rules that count everything there is to count and run every doc check, so a scan goes down
/// every path. `bits` picks what can't all be on at once: the syntax, whether a block has to
/// describe its item, whether generated files are skipped, and the scope.
pub fn rules(bits: u8) -> Rules {
    let warn = Setting { level: Level::Warn, options: None };
    Rules {
        scope: if bits & 1 == 0 { Scope::All } else { Scope::Exports },
        syntax: if bits & 2 == 0 { Syntax::Jsdoc } else { Syntax::Tsdoc },
        require_description: bits & 4 != 0,
        skip_generated: bits & 8 != 0,
        exports: Exports { esm: true, cjs: true, globals: true },
        file_docs: FileDocs::Require,
        members: Members::All,
        virtual_types: true,
        commonjs_members: true,
        unused_ignores: true,
        checks: Checks::new(check::ids().map(|id| (id, &warn))).map(Arc::new),
        ..Rules::default()
    }
}

/// Fails unless everything `report` puts at a line and column, from a scan of `source`, is in it.
pub fn check(report: &FileReport, source: &str) {
    // A line past the last is where a stub for an empty file goes.
    let lines = source.split('\n').count() + 1;
    let at = |what: &str, line: usize, column: usize| {
        assert!((1..=lines).contains(&line) && column >= 1, "{what} at {line}:{column}, in {lines} lines");
    };
    for item in &report.items {
        at(&item.name, item.line, item.column);
        for diagnostic in &item.facts.diagnostics {
            at(diagnostic.rule, diagnostic.line, diagnostic.column);
        }
        if let Some(stub) = &item.facts.stub {
            at("stub", stub.line, 1);
        }
    }
    for ignore in &report.unused_ignores {
        at("ignore", ignore.line, ignore.column);
    }
    at("code", report.code_line, 1);
}
