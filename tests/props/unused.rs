//! Properties of unused `docov-ignore` comments: `--report-unused-ignores` and `--explain`.

use std::{fs, path::Path};

use docov::feature::{Feature, Run, file_docs::FileDocs, report_unused_ignores::ReportUnusedIgnores};
use docov::{FileReport, Kind, Rules, Scope, collect, root::Root};
use hegel::{TestCase, generators as gs};

use super::settings;

/// A `docov-ignore` comment written into a generated module: where it is, 0-based, in bytes.
struct Written {
    line: usize,
    start: usize,
    end: usize,
}

/// Comment lines, each with whether it's a `docov-ignore` comment.
const COMMENTS: [(&str, bool); 6] = [
    ("// docov-ignore-next-line", true),
    ("/* docov-ignore-next-line: why */", true),
    ("// docov-ignore-file", true),
    ("/* docov-ignore-file: generated */", true),
    ("// docov-ignore-next-lines", false),
    ("// note", false),
];

/// A module of lines drawn from items (documented or not, counted under some scopes only, two on a
/// line), statements that declare nothing, JSDoc lines, blank lines, function bodies, and
/// `docov-ignore` comments placed anywhere among them, on lines of their own, after a statement
/// and inside one. Gives its lines and where each comment is.
fn module(tc: &TestCase) -> (Vec<String>, Vec<Written>) {
    let mut lines: Vec<String> = Vec::new();
    let mut written = Vec::new();
    let header = tc.draw(gs::sampled_from(vec!["", "// @generated", "/** @file Doc. */", "\"use strict\";"]));
    if !header.is_empty() {
        lines.push(header.into());
    }
    for n in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        match tc.draw(gs::integers::<u8>().max_value(10)) {
            0 => lines.push(String::new()),
            1 => lines.push("go();".into()),
            2 => lines.push(format!("export function f{n}() {{}}")),
            3 => lines.push(format!("/** Doc. */ export const v{n} = 1;")),
            4 => lines.push("/** Doc. */".into()),
            5 => lines.push(format!("const h{n} = 1;")),
            6 => lines.push(format!("export class C{n} {{ m{n}() {{}} }}")),
            7 => {
                written.push(Written { line: lines.len(), start: 6, end: 31 });
                lines.push("go(); // docov-ignore-next-line".into());
            }
            8 => {
                written.push(Written { line: lines.len(), start: 9, end: 32 });
                lines.push("if (x) { /* docov-ignore-file */ }".into());
            }
            9 => {
                lines.push(format!("export function g{n}() {{"));
                for i in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
                    match tc.draw(gs::integers::<u8>().max_value(2)) {
                        0 => lines.push(format!("  const i{n}_{i} = 1;")),
                        1 => lines.push("  go();".into()),
                        _ => comment(tc, &mut lines, &mut written, "  "),
                    }
                }
                lines.push("}".into());
            }
            _ => comment(tc, &mut lines, &mut written, ""),
        }
    }
    (lines, written)
}

/// Writes a comment line, indented by `indent`, noting where it is if it's a `docov-ignore` one.
fn comment(tc: &TestCase, lines: &mut Vec<String>, written: &mut Vec<Written>, indent: &str) {
    let (text, directive) = tc.draw(gs::sampled_from(COMMENTS.to_vec()));
    if directive {
        written.push(Written { line: lines.len(), start: indent.len(), end: indent.len() + text.len() });
    }
    lines.push(format!("{indent}{text}"));
}

/// The module with the comments `gone` picks out blanked, which leaves every other position as
/// it was.
fn without(lines: &[String], written: &[Written], gone: impl Fn(usize) -> bool) -> String {
    let mut lines = lines.to_vec();
    for (_, comment) in written.iter().enumerate().filter(|&(i, _)| gone(i)) {
        lines[comment.line].replace_range(comment.start..comment.end, &" ".repeat(comment.end - comment.start));
    }
    lines.join("\n") + "\n"
}

/// Each item, where it is, and whether it's documented and ignored.
fn states(report: &FileReport) -> Vec<(String, Kind, usize, usize, bool, bool)> {
    report.items.iter().map(|i| (i.name.clone(), i.kind, i.line, i.column, i.documented, i.ignored())).collect()
}

/// With `--report-unused-ignores`, the scan finds the `docov-ignore` comments that change
/// nothing: removing any one of them, or all of them, leaves every item ignored or not as it was,
/// and removing any comment left then changes some item's. So a comment is reported exactly when
/// removing it changes nothing, but of two that cover the same items, one is kept. Those in a file
/// skipped as `@generated` aren't unused, the gate fails the scan at each one, `--explain` lists
/// them, and without the flag the scan doesn't look.
#[hegel::test(test_cases = 200)]
fn unused_ignores_are_those_that_change_nothing(tc: TestCase) {
    let (lines, written) = module(&tc);
    let rules = Rules {
        scope: tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::All])),
        file_docs: tc.draw(gs::sampled_from(vec![FileDocs::Off, FileDocs::Require])),
        skip_generated: tc.draw(gs::booleans()),
        unused_ignores: true,
        ..Rules::default()
    };
    let path = Path::new("m.ts");
    let scan = |source: &str| collect::scan_source(path, source, &rules).unwrap();
    let source = without(&lines, &written, |_| false);
    let report = scan(&source);
    let at = |c: &Written| (c.line + 1, c.start + 1);
    let unused: Vec<(usize, usize)> = report.unused_ignores.iter().map(|i| (i.line, i.column)).collect();
    let is_unused = |i: usize| unused.contains(&at(&written[i]));
    assert_eq!(unused.iter().filter(|&&u| written.iter().any(|c| at(c) == u)).count(), unused.len(), "{source}");

    let skipped = rules.skip_generated && lines.first().is_some_and(|l| l == "// @generated");
    if skipped {
        assert_eq!(unused, [], "{source}");
    } else {
        let trimmed = scan(&without(&lines, &written, is_unused));
        assert_eq!(states(&trimmed), states(&report), "{source}");
        assert_eq!(trimmed.unused_ignores, [], "{source}");
        for kept in (0..written.len()).filter(|&i| !is_unused(i)) {
            let fewer = scan(&without(&lines, &written, |i| i == kept || is_unused(i)));
            assert_ne!(states(&fewer), states(&report), "{source}\nwithout {:?}", at(&written[kept]));
        }
    }
    for gone in (0..written.len()).filter(|&i| is_unused(i)) {
        let alone = scan(&without(&lines, &written, |i| i == gone));
        assert_eq!(states(&alone), states(&report), "{source}\nwithout {:?}", at(&written[gone]));
    }

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("m.ts");
    fs::write(&file, &source).unwrap();
    let explained: Vec<String> = collect::explain(&file, &rules)
        .unwrap()
        .into_iter()
        .filter_map(|line| line.contains("  unused docov-ignore").then(|| line.split("  ").next().unwrap().to_owned()))
        .collect();
    let positions: Vec<String> =
        unused.iter().map(|(line, column)| format!("{}:{line}:{column}", file.display())).collect();
    assert_eq!(explained, positions, "{source}");

    let reports = [collect::scan_source(&file, &source, &rules).unwrap()];
    let (settings, root) = (settings(dir.path(), r#"{ "reportUnusedIgnores": true }"#, &[]), Root::new(Path::new("")));
    assert!(settings.rules().unused_ignores);
    let failures = ReportUnusedIgnores(true).gate(&Run { reports: &reports, root: &root, settings: &settings });
    let wanted: Vec<String> = positions.iter().map(|p| format!("{p}: unused docov-ignore")).collect();
    assert_eq!(failures, wanted);
    let off = Rules { unused_ignores: false, ..rules };
    assert_eq!(collect::scan_source(path, &source, &off).unwrap().unused_ignores, []);
}
