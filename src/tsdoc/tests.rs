use std::fs;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::*;

/// An entry of tests/fixtures/tsdoc/corpus.json, which tools/tsdoc-oracle.sh fills in with what
/// `@microsoft/tsdoc` finds.
#[derive(Deserialize)]
struct Entry {
    comment: String,
    #[serde(default)]
    config: Option<Map<String, Value>>,
    #[serde(default)]
    messages: Vec<Expected>,
    #[serde(default)]
    throws: Option<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Expected {
    id: String,
    text: String,
    at: u32,
}

/// The configuration a tsdoc.json with `keys` sets up.
fn configuration(keys: &Map<String, Value>) -> Configuration {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(config::FILE_NAME);
    let mut file = keys.clone();
    file.insert("$schema".into(), json!(config::SCHEMA));
    fs::write(&path, Value::Object(file).to_string()).unwrap();
    config::load(&path).unwrap().0
}

#[test]
fn messages_match_microsoft_tsdoc() {
    let corpus = include_str!("../../tests/fixtures/tsdoc/corpus.json");
    let entries: Vec<Entry> = serde_json::from_str(corpus).unwrap();
    let mut wrong = Vec::new();
    for entry in entries.iter().filter(|entry| entry.throws.is_none()) {
        let configuration = entry.config.as_ref().map(configuration).unwrap_or_default();
        let parsed = parse(&entry.comment, &configuration);
        let found: Vec<Expected> = parsed
            .messages
            .into_iter()
            .filter(|m| m.id.starts_with("tsdoc-"))
            .map(|m| Expected { id: m.id.into(), text: m.text, at: m.at })
            .collect();
        if found != entry.messages {
            wrong.push(format!("{:?}\n  tsdoc: {:?}\n  docov: {found:?}", entry.comment, entry.messages));
        }
    }
    assert!(wrong.is_empty(), "{} of {} differ:\n{}", wrong.len(), entries.len(), wrong.join("\n"));
}
