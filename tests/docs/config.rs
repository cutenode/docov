//! docs/config.md, from docov.schema.json: a section for each key, with its type, its default, and a
//! table of its values or of the keys of its object.

use std::{fs, path::Path};

use serde_json::Value;

use crate::{current, generated};

const INTRO: &str = "
# docov.json

docov reads its settings from `docov.json`, `docov.jsonc` or `.docovrc`, in the folder it runs from or else the \
nearest folder above that has one, so a subfolder finds its project's config. In one folder, `docov.json` comes \
first and `.docovrc` last. `--config FILE` names one instead, and flags win over it. Comments and trailing commas \
are fine in all three. Its paths and globs are relative to its own folder, wherever docov runs from.

[`docov.schema.json`](../docov.schema.json) is its JSON Schema, which editors use for completion and checking. \
Point `$schema` at the copy for your release, \
`https://raw.githubusercontent.com/cutenode/docov/vX.Y.Z/docov.schema.json`, so it matches the docov you run, or \
write your own with `docov --print-schema > docov.schema.json`. Editors don't know `.docovrc` is JSON, so tell \
yours (in VS Code, `\"files.associations\": { \".docovrc\": \"jsonc\" }`).

`docov --print-config` prints the settings a scan would use, merged from the config files and the flags.
";

#[test]
fn the_config_reference_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docov.schema.json");
    let schema: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let defs = &schema["$defs"];
    let mut page = generated("docov.schema.json") + INTRO;
    for (key, property) in schema["properties"].as_object().unwrap() {
        page += &format!("\n## `{key}`\n\n{}\n\n{}\n", prose(property), facts(property, defs));
        if key == "rules" {
            page += "\nThe checks, and their options, are in [checks.md](checks.md).\n";
        } else {
            page += &table(property, defs);
        }
    }
    current("docs/config.md", &page);
}

/// A schema's description, its wrapped lines joined.
fn prose(node: &Value) -> String {
    let text = node["description"].as_str().unwrap_or_default();
    text.split("\n\n").map(|paragraph| paragraph.replace('\n', " ")).collect::<Vec<_>>().join("\n\n")
}

/// The schema `node` refers to, or `node`.
fn resolve<'a>(node: &'a Value, defs: &'a Value) -> &'a Value {
    match node["$ref"].as_str() {
        Some(path) => &defs[path.trim_start_matches("#/$defs/")],
        None => node,
    }
}

/// The alternatives of a union, past the `null` that leaves a key unset. One with a description of its
/// own stays whole.
fn alternatives<'a>(node: &'a Value, defs: &'a Value) -> Vec<&'a Value> {
    let node = resolve(node, defs);
    match node.get("anyOf").or(node.get("oneOf")) {
        Some(Value::Array(nodes)) => nodes
            .iter()
            .filter(|alt| alt["type"] != "null")
            .flat_map(|alt| if alt.get("description").is_some() { vec![alt] } else { alternatives(alt, defs) })
            .collect(),
        _ => vec![node],
    }
}

/// Its type, as JSON names it: `string`, `array of string`, `number or object`.
pub(crate) fn kind(node: &Value, defs: &Value) -> String {
    let mut kinds: Vec<String> = Vec::new();
    for alt in alternatives(node, defs) {
        let types = match &alt["type"] {
            _ if alt.get("anyOf").or(alt.get("oneOf")).is_some() => vec![kind(alt, defs)],
            Value::Array(types) => {
                types.iter().filter_map(Value::as_str).filter(|t| *t != "null").map(String::from).collect()
            }
            Value::String(t) => vec![t.clone()],
            _ => vec![],
        };
        for t in types {
            let t = match t.as_str() {
                "array" => format!("array of {}", kind(&alt["items"], defs)),
                // A whole number is a number too.
                "integer" => "number".to_owned(),
                _ => t,
            };
            if !kinds.contains(&t) {
                kinds.push(t);
            }
        }
    }
    kinds.join(" or ")
}

/// Its type and default, in a line.
fn facts(node: &Value, defs: &Value) -> String {
    let mut line = format!("Type: {}.", kind(node, defs));
    if let Some(default) = node.get("default").filter(|d| !d.is_null() && !d.is_object()) {
        line += &format!(" Default: `{default}`.");
    }
    line
}

/// A table of its values, for a choice of strings, or of its keys, for an object (or an array of
/// them), or nothing.
fn table(node: &Value, defs: &Value) -> String {
    let resolved = resolve(node, defs);
    let node = if resolved["type"].as_str() == Some("array") || resolved["type"][0] == "array" {
        &resolved["items"]
    } else {
        node
    };
    let alternatives = alternatives(node, defs);
    if alternatives.iter().all(|alt| alt.get("const").is_some()) {
        let rows: String = alternatives
            .iter()
            .map(|alt| format!("| `{}` | {} |\n", alt["const"].as_str().unwrap(), prose(alt)))
            .collect();
        return format!("\n| Value | Meaning |\n| --- | --- |\n{rows}");
    }
    let is_object = |alt: &&&Value| alt.get("properties").is_some();
    let mut out = String::new();
    if alternatives.len() > 1 {
        for alt in alternatives.iter().filter(|alt| !is_object(alt)) {
            out += &format!("- {}: {}\n", kind(alt, defs), prose(alt));
        }
    }
    let Some(object) = alternatives.iter().find(is_object) else {
        return if out.is_empty() { out } else { format!("\n{out}") };
    };
    let mut rows: String = object["properties"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            let default = value.get("default").filter(|d| !d.is_null()).map(|d| format!("`{d}`")).unwrap_or_default();
            format!("| `{key}` | {} | {default} | {} |\n", kind(value, defs), prose(value))
        })
        .collect();
    if let Some(other) = object.get("additionalProperties").filter(|other| other.is_object()) {
        rows += &format!("| any other key | {} |  | {} |\n", kind(other, defs), prose(other));
    }
    let out = if out.is_empty() { out } else { format!("\n{out}") };
    format!("{out}\n| Key | Type | Default | Meaning |\n| --- | --- | --- | --- |\n{rows}")
}
