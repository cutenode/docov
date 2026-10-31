//! Items and doc check findings as the language server protocol has them: lines from 0 and columns
//! in UTF-16 code units.

use lsp_types::{
    CodeAction, CodeActionKind, Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, TextEdit, Url,
    WorkspaceEdit,
};

use crate::{FileReport, Item, check::Level, rewrite};

/// A diagnostic at each of `report`'s undocumented items, and one at each doc check finding, by
/// its level. `text` is what was scanned.
pub(super) fn diagnostics(report: &FileReport, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for item in report.counted() {
        diagnostics.extend((!item.documented).then(|| undocumented(item, text)));
        diagnostics.extend(item.facts.diagnostics.iter().map(|finding| {
            let severity =
                if finding.severity == Level::Error { DiagnosticSeverity::ERROR } else { DiagnosticSeverity::WARNING };
            diagnostic(text, (finding.line, finding.column), severity, finding.rule, finding.message.clone())
        }));
    }
    diagnostics
}

/// The information diagnostic at an undocumented `item`.
fn undocumented(item: &Item, text: &str) -> Diagnostic {
    let message = format!("{} {} is undocumented", item.kind.as_str(), item.name);
    diagnostic(text, (item.line, item.column), DiagnosticSeverity::INFORMATION, "undocumented", message)
}

/// A diagnostic from a 1-based line and column, in characters, to the end of that line, under the
/// code `docov/` and `rule`.
fn diagnostic(
    text: &str,
    (line, column): (usize, usize),
    severity: DiagnosticSeverity,
    rule: &str,
    message: String,
) -> Diagnostic {
    let line_text = text.lines().nth(line - 1).unwrap_or_default().trim_end();
    let utf16 = |chars: &str| chars.encode_utf16().count() as u32;
    let start =
        Position::new(line as u32 - 1, line_text.chars().take(column - 1).map(char::len_utf16).sum::<usize>() as u32);
    let end = Position::new(start.line, utf16(line_text).max(start.character));
    Diagnostic {
        range: Range::new(start, end),
        severity: Some(severity),
        code: Some(NumberOrString::String(format!("docov/{rule}"))),
        source: Some("docov".into()),
        message,
        ..Diagnostic::default()
    }
}

/// The code action that writes `item`'s stub into the document at `uri`, whose text is `text`, if
/// it has one.
pub(super) fn action(uri: &Url, item: &Item, text: &str) -> Option<CodeAction> {
    let stub = item.facts.stub.as_ref()?;
    // The stub as `--fix` writes it, as an insert where the text first differs.
    let edited = rewrite::insert_above(text, &[(stub.line, &stub.text)]);
    let at: usize = text.chars().zip(edited.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
    let inserted = edited[at..at + edited.len() - text.len()].to_owned();
    let before = &text[..at];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let position =
        Position::new(before.matches('\n').count() as u32, before[line_start..].encode_utf16().count() as u32);
    let edit = TextEdit { range: Range::new(position, position), new_text: inserted };
    Some(CodeAction {
        title: format!("Add a doc stub for {} {}", item.kind.as_str(), item.name),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![undocumented(item, text)]),
        edit: Some(WorkspaceEdit {
            changes: Some([(uri.clone(), vec![edit])].into_iter().collect()),
            ..WorkspaceEdit::default()
        }),
        ..CodeAction::default()
    })
}
