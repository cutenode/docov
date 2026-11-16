//! `docov lsp` driven in-process with any sequence of the messages an editor sends: documents
//! opened, changed, saved and closed in any order, code actions on any range, file changes, and
//! known methods with any params. Its URIs are files under a project with every doc check on, with
//! percent-encoding, Windows' drive letters and UNC shares, and ones that aren't files. The server
//! has to answer each request once and shut down when asked.

#![no_main]

use std::{sync::OnceLock, thread};

use libfuzzer_sys::{
    arbitrary::{self, Arbitrary},
    fuzz_target,
};
use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{Value, json};

#[derive(Arbitrary, Debug)]
enum Step {
    Open {
        doc: u8,
        text: String,
    },
    /// Full sync sends the whole text, but a client can send none, or several.
    Change {
        doc: u8,
        texts: Vec<String>,
    },
    Save {
        doc: u8,
    },
    Close {
        doc: u8,
    },
    Actions {
        doc: u8,
        start: (u32, u32),
        end: (u32, u32),
    },
    Watched {
        doc: u8,
        kind: u8,
    },
    Request {
        method: u8,
        params: String,
    },
    Notify {
        method: u8,
        params: String,
    },
}

/// The methods `Request` and `Notify` send, with any params: those the server handles, and some it
/// doesn't.
const METHODS: [&str; 8] = [
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didSave",
    "textDocument/didClose",
    "textDocument/codeAction",
    "workspace/didChangeWatchedFiles",
    "textDocument/hover",
    "$/cancelRequest",
];

/// The project folder, with a docov.json that counts everything and turns every doc check on, as a
/// `file:` URI.
fn root() -> &'static (tempfile::TempDir, String) {
    static ROOT: OnceLock<(tempfile::TempDir, String)> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let rules: serde_json::Map<String, Value> = docov::check::ids().map(|id| (id.into(), json!("warn"))).collect();
        let config = json!({
            "scope": "all",
            "members": "all",
            "commonjsMembers": true,
            "exports": { "globals": true },
            "fileDocs": "require",
            "virtual": true,
            "reportUnusedIgnores": true,
            "rules": rules,
        });
        std::fs::write(dir.path().join("docov.json"), config.to_string()).expect("the config is written");
        let uri = format!("file://{}", dir.path().display());
        (dir, uri)
    })
}

/// The URI of document `doc`.
fn uri(doc: u8) -> String {
    let root = &root().1;
    match doc % 10 {
        0 => format!("{root}/a.ts"),
        1 => format!("{root}/b%20c.js"),
        2 => format!("{root}/%C3%BC.tsx"),
        3 => format!("{root}/sub/d.mts"),
        4 => format!("{root}/notes.md"),
        5 => format!("{root}/docov.json"),
        6 => "file:///C:/a.ts".into(),
        7 => "file://server/share/a.ts".into(),
        8 => "untitled:Untitled-1".into(),
        _ => "http://example.com/a.ts".into(),
    }
}

fn notification(method: &str, params: Value) -> Message {
    Notification::new(method.into(), params).into()
}

fuzz_target!(|steps: Vec<Step>| {
    let (server, client) = Connection::memory();
    let serving = thread::spawn(move || docov::lsp::serve(&server));
    let mut ids = 0;
    let mut request = |method: &str, params: Value| {
        ids += 1;
        Message::from(Request::new(RequestId::from(ids), method.into(), params))
    };
    let document = |doc: u8| json!({ "uri": uri(doc) });
    let mut messages = vec![
        request("initialize", json!({ "capabilities": {}, "rootUri": root().1 })),
        notification("initialized", json!({})),
    ];
    for step in steps {
        messages.push(match step {
            Step::Open { doc, text } => notification(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri(doc), "languageId": "typescript", "version": 1, "text": text } }),
            ),
            Step::Change { doc, texts } => {
                let changes: Vec<Value> = texts.into_iter().map(|text| json!({ "text": text })).collect();
                let params = json!({ "textDocument": { "uri": uri(doc), "version": 2 }, "contentChanges": changes });
                notification("textDocument/didChange", params)
            }
            Step::Save { doc } => notification("textDocument/didSave", json!({ "textDocument": document(doc) })),
            Step::Close { doc } => notification("textDocument/didClose", json!({ "textDocument": document(doc) })),
            Step::Actions { doc, start, end } => {
                let range = json!({
                    "start": { "line": start.0, "character": start.1 },
                    "end": { "line": end.0, "character": end.1 },
                });
                let context = json!({ "diagnostics": [] });
                request(
                    "textDocument/codeAction",
                    json!({ "textDocument": document(doc), "range": range, "context": context }),
                )
            }
            Step::Watched { doc, kind } => notification(
                "workspace/didChangeWatchedFiles",
                json!({ "changes": [{ "uri": uri(doc), "type": kind % 3 + 1 }] }),
            ),
            Step::Request { method, params } => {
                let params = serde_json::from_str(&params).unwrap_or(Value::String(params));
                request(METHODS[usize::from(method) % METHODS.len()], params)
            }
            Step::Notify { method, params } => {
                let params = serde_json::from_str(&params).unwrap_or(Value::String(params));
                notification(METHODS[usize::from(method) % METHODS.len()], params)
            }
        });
    }
    messages.push(request("shutdown", Value::Null));
    messages.push(notification("exit", Value::Null));
    for message in messages {
        client.sender.send(message).expect("the server is listening");
    }
    // A panic on the server's thread aborts the process first, as libFuzzer wants.
    assert_eq!(serving.join().expect("the server ran"), Ok(true));

    let mut answered = vec![0; ids as usize];
    for message in client.receiver.try_iter() {
        if let Message::Response(response) = message {
            let id = response.id.to_string().parse::<usize>().expect("a request's id");
            answered[id - 1] += 1;
        }
    }
    assert!(answered.iter().all(|&n| n == 1), "{answered:?}");
});
