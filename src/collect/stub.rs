//! Doc stubs: the JSDoc block `--fix` and the language server's code action write above an
//! undocumented item, prefilled from its code, with `TODO` for a description so it still counts as
//! undocumented until someone writes one.

use serde::{Deserialize, Serialize};

use super::{Found, Kind, Lines};

/// A block to put above an undocumented item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stub {
    /// The 1-based line to put it above, as [`insert_above`](crate::rewrite::insert_above) does.
    pub line: usize,
    /// The block, unindented.
    pub text: String,
}

impl Stub {
    /// The stub for `found`, in the file whose text `lines` has, unless something comes before it
    /// on its line, which a block above would document instead. It has a `@template` for each type
    /// parameter, a `@param` for each parameter (a destructured one by its place, as `param0`), and
    /// `@returns` when its body returns a value, the way the doc checks want them; a file's has
    /// `@file`.
    pub(super) fn of(found: &Found, lines: &Lines) -> Option<Self> {
        let at = found.at as usize;
        let (line, _) = lines.position(at);
        if !lines.source[lines.starts[line - 1]..at].trim().is_empty() {
            return None;
        }
        let mut tags: Vec<String> = found.shape.type_params.iter().map(|name| format!("@template {name}")).collect();
        let params = found.params.iter().flatten().enumerate();
        tags.extend(
            params.map(|(i, name)| format!("@param {}", name.map_or_else(|| format!("param{i}"), str::to_owned))),
        );
        if found.shape.returns && !found.shape.void {
            tags.push("@returns".into());
        }
        if found.kind == Kind::Module {
            tags.push("@file".into());
        }
        let text = if tags.is_empty() {
            "/** TODO */".to_owned()
        } else {
            let tags: String = tags.iter().map(|tag| format!(" * {tag}\n")).collect();
            format!("/**\n * TODO\n{tags} */")
        };
        Some(Self { line, text })
    }
}
