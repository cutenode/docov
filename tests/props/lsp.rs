//! Properties of `docov lsp`: the server driven in-process over lsp-server's in-memory connection
//! with the JSON-RPC messages an editor sends, and the binary over stdio.

use std::{
    io::{BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use docov::{FileReport, Rules, Scope, check, collect, lsp, rewrite};
use hegel::{TestCase, generators as gs};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::Url;
use serde_json::{Value, json};

use super::{Module, write};

/// An editor's end of a session with the server, which runs on a thread of its own.
struct Client {
    connection: Connection,
    server: JoinHandle<Result<bool, String>>,
    next: i32,
}

impl Client {
    /// Starts a server, without initializing it.
    fn new() -> Self {
        let (server, connection) = Connection::memory();
        Self { connection, server: thread::spawn(move || lsp::serve(&server)), next: 0 }
    }

    /// Starts a server and initializes it with `params`.
    fn start(params: Value) -> Self {
        let mut client = Self::new();
        let response = client.request("initialize", params);
        assert!(response.response_result.unwrap()["capabilities"]["codeActionProvider"] == json!(true));
        client.notify("initialized", json!({}));
        client
    }

    fn send(&self, message: impl Into<Message>) {
        self.connection.sender.send(message.into()).unwrap();
    }

    fn notify(&self, method: &str, params: Value) {
        self.send(Notification::new(method.into(), params));
    }

    /// Sends a request and gives the response, which comes next.
    fn request(&mut self, method: &str, params: Value) -> Response {
        self.next += 1;
        let id = RequestId::from(self.next);
        self.send(Request::new(id.clone(), method.into(), params));
        match self.receive() {
            Message::Response(response) => {
                assert_eq!(response.id, id);
                response
            }
            message => panic!("{message:?}"),
        }
    }

    fn receive(&self) -> Message {
        self.connection.receiver.recv_timeout(Duration::from_secs(30)).unwrap()
    }

    /// The notification that comes next, which is a `method`, by its params.
    fn expect(&self, method: &str) -> Value {
        match self.receive() {
            Message::Notification(notification) if notification.method == method => notification.params,
            message => panic!("{message:?}"),
        }
    }

    /// The diagnostics published next, for `uri`.
    fn published(&self, uri: &Url) -> Value {
        let params = self.expect("textDocument/publishDiagnostics");
        assert_eq!(params["uri"], json!(uri));
        params["diagnostics"].clone()
    }

    /// The message logged next.
    fn logged(&self) -> String {
        self.expect("window/logMessage")["message"].as_str().unwrap().to_owned()
    }

    fn open(&self, uri: &Url, text: &str) {
        let document = json!({ "uri": uri, "languageId": "typescript", "version": 1, "text": text });
        self.notify("textDocument/didOpen", json!({ "textDocument": document }));
    }

    fn change(&self, uri: &Url, text: &str) {
        let changes = json!([{ "text": text }]);
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": changes }),
        );
    }

    fn actions(&mut self, uri: &Url, lines: (u32, u32)) -> Value {
        let range = json!({ "start": { "line": lines.0, "character": 0 }, "end": { "line": lines.1, "character": 0 } });
        let params = json!({ "textDocument": { "uri": uri }, "range": range, "context": { "diagnostics": [] } });
        self.request("textDocument/codeAction", params).response_result.unwrap()
    }

    /// Shuts the server down and has it exit, as an editor does, and gives what it returned.
    fn stop(mut self) -> Result<bool, String> {
        assert_eq!(self.request("shutdown", Value::Null).response_result.unwrap(), Value::Null);
        self.notify("exit", Value::Null);
        self.server.join().unwrap()
    }
}

fn uri(path: &Path) -> Url {
    Url::from_file_path(path).unwrap()
}

/// What the server should publish for `report`, a scan of `text`: an information diagnostic at
/// each undocumented item and each doc check finding at its level, from its line and column (in
/// UTF-16 code units, from 0) to the end of its line.
fn diagnostics(report: &FileReport, text: &str) -> Value {
    let range = |line: usize, column: usize| {
        let text = text.lines().nth(line - 1).unwrap_or_default().trim_end();
        let start: String = text.chars().take(column - 1).collect();
        let (line, start, end) = (line - 1, start.encode_utf16().count(), text.encode_utf16().count());
        json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
    };
    let mut all = Vec::new();
    for item in report.counted() {
        if !item.documented {
            let message = format!("{} {} is undocumented", item.kind.as_str(), item.name);
            let code = "docov/undocumented";
            all.push(json!({ "range": range(item.line, item.column), "severity": 3, "code": code, "source": "docov", "message": message }));
        }
        for d in &item.facts.diagnostics {
            let severity = if d.severity == check::Level::Error { 1 } else { 2 };
            let code = format!("docov/{}", d.rule);
            all.push(json!({ "range": range(d.line, d.column), "severity": severity, "code": code, "source": "docov", "message": d.message }));
        }
    }
    Value::Array(all)
}

/// `text` with `edit`, a code action's insert, applied.
fn apply(text: &str, edit: &Value) -> String {
    let start = &edit["range"]["start"];
    assert_eq!(*start, edit["range"]["end"]);
    let (line, mut units) = (start["line"].as_u64().unwrap() as usize, start["character"].as_u64().unwrap() as usize);
    let mut at: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
    for c in text[at..].chars() {
        if units == 0 {
            break;
        }
        (at, units) = (at + c.len_utf8(), units - c.len_utf16());
    }
    format!("{}{}{}", &text[..at], edit["newText"].as_str().unwrap(), &text[at..])
}

/// A generated module with a line that has characters past the Basic Multilingual Plane before
/// its item, whose column the server gives in UTF-16 code units, and a function whose block both
/// doc checks find something in.
fn document(tc: &TestCase) -> String {
    let checked = "/** No period */\nexport function checked(a) {}\n";
    format!("{}/* é😀 */ export const wide = 1;\n{checked}", Module::generate(tc).source)
}

/// For each open document, the server publishes what a scan of its unsaved text finds, by the
/// config docov finds from the workspace folder (or, with none, from the file's folder) with its
/// scope and doc checks: on open, change and save, and nothing once it closes. A document the
/// config's `paths`, its `exclude` or a nested config's leaves out gets none. Each undocumented
/// item with a stub has a code action that inserts it as `--fix` would, and the server shuts down
/// and exits when the editor asks.
#[hegel::test(test_cases = 30)]
fn the_server_publishes_what_a_scan_of_the_buffer_finds(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let scope = tc.draw(gs::sampled_from(vec![Scope::Exports, Scope::All]));
    // One of them, at least, is an error.
    let param = tc.draw(gs::sampled_from(vec!["off", "warn", "error"]));
    let levels = [("require-param", param), ("summary-sentence", if param == "error" { "warn" } else { "error" })];
    let paths = tc.draw(gs::booleans());
    let mut config = json!({ "scope": scope, "rules": levels.iter().map(|(id, level)| ((*id).to_owned(), json!(level))).collect::<serde_json::Map<_, _>>(), "nested": true, "exclude": ["skip.ts"] });
    if paths {
        config["paths"] = json!(["src"]);
    }
    write(&root, "docov.json", &config.to_string());
    write(&root, "pkg/docov.json", r#"{ "exclude": ["x.ts"] }"#);
    let settings: Vec<_> =
        levels.iter().map(|(id, level)| (*id, check::Setting::parse(id, &json!(level)).unwrap())).collect();
    let checks = check::Checks::new(settings.iter().map(|(id, setting)| (*id, setting))).map(Arc::new);
    let rules = Rules { checks, ..Rules::from(scope) };

    let folder = json!([{ "uri": uri(&root), "name": "project" }]);
    let params = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => json!({ "capabilities": {}, "workspaceFolders": folder }),
        1 => json!({ "capabilities": {}, "rootUri": uri(&root) }),
        // No folder: the config is the nearest one above the file.
        _ => {
            json!({ "capabilities": {}, "workspaceFolders": [{ "uri": uri(&root.join("elsewhere")), "name": "other" }] })
        }
    };
    let mut client = Client::start(params);

    let path = root.join("src/m.ts");
    let file = uri(&path);
    let text = document(&tc);
    let scan = |text: &str| collect::scan_source(&path, text, &rules).unwrap();
    client.open(&file, &text);
    assert_eq!(client.published(&file), diagnostics(&scan(&text), &text), "{text}");

    let changed = document(&tc);
    client.change(&file, &changed);
    let report = scan(&changed);
    assert_eq!(client.published(&file), diagnostics(&report, &changed), "{changed}");
    client.notify("textDocument/didSave", json!({ "textDocument": { "uri": file } }));
    assert_eq!(client.published(&file), diagnostics(&report, &changed));

    let lines = changed.lines().count() as u32;
    let actions = client.actions(&file, (0, lines));
    let stubbed: Vec<_> = report.counted().filter(|i| i.facts.stub.is_some()).collect();
    assert_eq!(actions.as_array().unwrap().len(), stubbed.len());
    for (action, item) in actions.as_array().unwrap().iter().zip(&stubbed) {
        assert_eq!(action["title"], json!(format!("Add a doc stub for {} {}", item.kind.as_str(), item.name)));
        assert_eq!(action["kind"], json!("quickfix"));
        assert_eq!(action["diagnostics"][0]["code"], json!("docov/undocumented"));
        let stub = item.facts.stub.as_ref().unwrap();
        let edits = &action["edit"]["changes"][file.as_str()];
        assert_eq!(apply(&changed, &edits[0]), rewrite::insert_above(&changed, &[(stub.line, &stub.text)]));
    }
    // An item past the lines asked about has none.
    if let Some(item) = stubbed.last() {
        let after = client.actions(&file, (item.line as u32, lines));
        assert!(
            after
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["title"] != json!(format!("Add a doc stub for {} {}", item.kind.as_str(), item.name)))
        );
    }

    client.notify("textDocument/didClose", json!({ "textDocument": { "uri": file } }));
    assert_eq!(client.published(&file), json!([]));
    assert_eq!(client.actions(&file, (0, lines)), json!([]));

    // Left out by the root config's exclude, a nested config's, or the root config's paths.
    let outside = root.join("other/o.ts");
    for left_out in [root.join("src/skip.ts"), root.join("pkg/x.ts"), outside.clone()] {
        let left_out = uri(&left_out);
        client.open(&left_out, "export const a = 1;\n");
        let published = client.published(&left_out);
        let undocumented = json!([{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 19 } }, "severity": 3, "code": "docov/undocumented", "source": "docov", "message": "variable a is undocumented" }]);
        let counted = left_out == uri(&outside) && !paths;
        assert_eq!(published, if counted { undocumented } else { json!([]) });
    }
    assert_eq!(client.stop(), Ok(true));
}

/// The server logs what keeps it from scanning a document (a file that isn't JavaScript, that
/// doesn't parse yet, that isn't on disk, a config that doesn't load), keeping what it published
/// last, and answers a request it doesn't handle, or whose params are wrong, with an error. It
/// passes over notifications it doesn't know and responses, and offers no code actions for a
/// document it can't scan.
#[hegel::test(test_cases = 2)]
fn the_server_says_what_it_can_not_do(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut client =
        Client::start(json!({ "capabilities": {}, "workspaceFolders": [{ "uri": uri(&root), "name": "p" }] }));
    let file = uri(&root.join("a.ts"));

    let text = tc.draw(gs::sampled_from(vec!["export const a = 1;\n", "/** Docs. */\nexport const a = 1;\n"]));
    client.open(&file, text);
    assert_eq!(client.published(&file).as_array().unwrap().len(), usize::from(!text.starts_with("/**")));
    // It doesn't parse yet: what it published stays.
    client.change(&file, "export function (");
    let logged = client.logged();
    assert!(logged.starts_with("docov: ") && logged.contains("a.ts:1:"), "{logged}");
    assert_eq!(client.actions(&file, (0, 1)), json!([]));

    client.open(&uri(&root.join("notes.md")), "# notes");
    assert!(client.logged().contains("not a JavaScript or TypeScript file"));
    let untitled = Url::parse("untitled:Untitled-1").unwrap();
    client.open(&untitled, "export const a = 1;\n");
    assert_eq!(client.logged(), "docov: untitled:Untitled-1: not a file");
    client.notify("textDocument/didOpen", json!({ "textDocument": 1 }));
    assert!(client.logged().starts_with("docov: invalid type"));

    // A config that doesn't load, a jsdoc config, baseline or package.json that isn't there.
    let configs = [
        (r#"{ "threshold": "x" }"#, "docov.json"),
        (r#"{ "jsdocConfig": "jsdoc.json" }"#, "jsdoc.json"),
        (r#"{ "baseline": "base.json" }"#, "base.json"),
        (r#"{ "scope": "public" }"#, "package.json"),
        (r#"{ "scope": "public", "paths": ["."] }"#, "package.json"),
    ];
    for (config, culprit) in configs {
        write(&root, "docov.json", config);
        client.notify("textDocument/didSave", json!({ "textDocument": { "uri": file } }));
        let logged = client.logged();
        assert!(logged.contains(culprit), "{logged}");
    }

    let response = client.request("textDocument/hover", json!({}));
    assert_eq!(response.response_result.unwrap_err().code, lsp_server::ErrorCode::MethodNotFound as i32);
    let response = client.request("textDocument/codeAction", json!({ "textDocument": 1 }));
    assert_eq!(response.response_result.unwrap_err().code, lsp_server::ErrorCode::InvalidParams as i32);
    client.notify("workspace/didChangeConfiguration", json!({ "settings": {} }));
    client.send(Response::new_ok(RequestId::from(99), ()));
    // Neither of those sent anything back: the next message is this request's response.
    assert_eq!(client.actions(&uri(&root.join("closed.ts")), (0, 1)), json!([]));
    assert_eq!(client.stop(), Ok(true));
}

/// A session that ends some other way than a shutdown and an exit says so: an exit without a
/// shutdown, the editor going away, a message after a shutdown that isn't an exit, or an initialize
/// that isn't one. A notification the server doesn't know changes none of that.
#[hegel::test(test_cases = 2)]
fn sessions_end_by_the_protocol(tc: TestCase) {
    let chatty = tc.draw(gs::booleans());
    let start = || {
        let client = Client::start(json!({ "capabilities": {} }));
        if chatty {
            client.notify("$/setTrace", json!({ "value": "off" }));
        }
        client
    };
    for ending in 0..5 {
        session(ending, start);
    }
}

/// Ends a session the way `ending` picks, and checks how the server took it.
fn session(ending: u8, start: impl Fn() -> Client) {
    match ending {
        0 => {
            let client = start();
            client.notify("exit", Value::Null);
            assert_eq!(client.server.join().unwrap(), Ok(false));
        }
        1 => {
            let Client { connection, server, .. } = start();
            drop(connection);
            assert_eq!(server.join().unwrap(), Ok(false));
        }
        2 => {
            let mut client = start();
            assert_eq!(client.request("shutdown", Value::Null).response_result.unwrap(), Value::Null);
            client.notify("textDocument/didSave", json!({}));
            let error = client.server.join().unwrap().unwrap_err();
            assert!(error.contains("unexpected message during shutdown"), "{error}");
        }
        3 => {
            let mut client = Client::new();
            let response = client.request("initialize", json!({ "capabilities": 1 }));
            assert!(response.response_result.is_ok());
            client.notify("initialized", json!({}));
            let error = client.server.join().unwrap().unwrap_err();
            assert!(error.starts_with("initialize: "), "{error}");
        }
        _ => {
            let Client { connection, server, .. } = Client::new();
            drop(connection);
            assert_eq!(server.join().unwrap(), Err("disconnected channel".into()));
        }
    }
}

/// `docov lsp` speaks the protocol on stdin and stdout: a session that shuts down and exits exits
/// 0, one that exits without a shutdown 1, and one whose input stops being the protocol, or ends
/// before it starts, 2.
#[hegel::test(test_cases = 1)]
fn the_binary_serves_on_stdio(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let name = tc.draw(gs::sampled_from(vec!["a.ts", "a b.js"]));
    let file = uri(&root.join(name));
    let frame = |message: Value| {
        let body = message.to_string();
        format!("Content-Length: {}\r\n\r\n{body}", body.len())
    };
    let start = [
        frame(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "capabilities": {}, "rootUri": uri(&root) } })),
        frame(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} })),
        frame(json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": file, "languageId": "typescript", "version": 1, "text": "export const a = 1;\n" } } })),
    ]
    .concat();
    let shutdown = frame(json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }));
    let exit = frame(json!({ "jsonrpc": "2.0", "method": "exit" }));
    for (end, code) in
        [(format!("{shutdown}{exit}"), 0), (exit.clone(), 1), ("Content-Length: x\r\n\r\n".to_owned(), 2)]
    {
        serve_stdio(&root, &format!("{start}{end}"), code);
    }
    serve_stdio(&root, "", 2);
}

/// Runs `docov lsp` in `root` on `input`, a session that opens a file with one undocumented item,
/// and checks that it exited with `code`, saying why on stderr when that's 2, and otherwise
/// published that item.
fn serve_stdio(root: &Path, input: &str, code: i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_docov"))
        .arg("lsp")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let mut stdout = BufReader::new(&out.stdout[..]);
    let mut messages = Vec::new();
    while let Some(message) = Message::read(&mut stdout).unwrap() {
        messages.push(message);
    }
    let published = messages.iter().find_map(|m| match m {
        Message::Notification(n) if n.method == "textDocument/publishDiagnostics" => Some(n.params.clone()),
        _ => None,
    });
    // When its input breaks, the server stops without waiting on what it was writing.
    if code != 2 {
        assert_eq!(published.unwrap()["diagnostics"][0]["message"], json!("variable a is undocumented"));
    }
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(code), "{stderr}");
    assert_eq!(stderr.is_empty(), code != 2, "{stderr}");
}
