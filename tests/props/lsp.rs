//! Properties of `docov lsp`: the server driven in-process over lsp-server's in-memory connection
//! with the JSON-RPC messages an editor sends, and the binary over stdio.

use std::{
    fs,
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

impl Client {
    /// Starts a server for the workspace folder `folder`, initialized by a client that watches
    /// files for servers, and accepts its watcher, or refuses it.
    fn watching(folder: &Path, accept: bool) -> Self {
        let capabilities = json!({ "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } } });
        let client = Self::start(
            json!({ "capabilities": capabilities, "workspaceFolders": [{ "uri": uri(folder), "name": "p" }] }),
        );
        let Message::Request(request) = client.receive() else { panic!("the server asks to watch files") };
        assert_eq!(request.method, "client/registerCapability");
        let registration = &request.params["registrations"][0];
        assert_eq!(registration["method"], json!("workspace/didChangeWatchedFiles"));
        let globs = json!(["**/*.{json,jsonc,js,jsx,mjs,cjs,ts,tsx,mts,cts}", "**/.docovrc"]);
        let watchers = registration["registerOptions"]["watchers"].as_array().unwrap();
        assert_eq!(Value::Array(watchers.iter().map(|w| w["globPattern"].clone()).collect()), globs);
        client.send(if accept {
            Response::new_ok(request.id, ())
        } else {
            Response::new_err(request.id, lsp_server::ErrorCode::RequestFailed as i32, "no".into())
        });
        client
    }

    /// What the server sent next: the diagnostics it published, or the message it logged.
    fn outcome(&self) -> Value {
        match self.receive() {
            Message::Notification(n) if n.method == "textDocument/publishDiagnostics" => {
                n.params["diagnostics"].clone()
            }
            Message::Notification(n) if n.method == "window/logMessage" => n.params["message"].clone(),
            message => panic!("{message:?}"),
        }
    }

    /// Tells the server that the files at `paths` changed, as a client that watches them does.
    fn watched(&self, paths: &[(&Path, u8)]) {
        let changes: Vec<_> = paths.iter().map(|(path, kind)| json!({ "uri": uri(path), "type": kind })).collect();
        self.notify("workspace/didChangeWatchedFiles", json!({ "changes": changes }));
    }
}

/// What a server that has kept nothing does first for the document at `path` with `text`, with
/// `folder` as its workspace folder.
fn fresh(folder: &Path, path: &Path, text: &str) -> Value {
    let client =
        Client::start(json!({ "capabilities": {}, "workspaceFolders": [{ "uri": uri(folder), "name": "p" }] }));
    client.open(&uri(path), text);
    let outcome = client.outcome();
    assert_eq!(client.stop(), Ok(true));
    outcome
}

/// Long enough after a file changes that its modified time can't be mistaken for one during a
/// read, which the server doesn't keep: it reads it again, which no property here should lean on.
fn settle() {
    thread::sleep(Duration::from_millis(30));
}

/// The document the cache properties edit around: `a` has a summary but no `@param`, `b` has no
/// docs, and `c` isn't exported.
const LIB: &str = "/** Adds. */\nexport function a(x) {}\nexport function b(y) {}\nfunction c() {}\n";

/// A file a cache property edits, from the workspace folder, and what it writes there, or `None`
/// to delete it.
type Edit = (&'static str, Option<&'static str>);

/// Edits that each change what the document's diagnostics are read from, in a project as
/// [`kept_rules_follow_the_files_they_were_read_from`] writes it, each ending as it started: a
/// config the root one extends from outside the workspace folder (also while it leaves the
/// document out), the root config, its jsdoc config, a nested config that turns up and goes (also
/// outside the public scope), one that extends another file, package.json, tsconfig.json, a module
/// the entry points reach, and one an import names that's added.
const SCENARIOS: [&[Edit]; 11] = [
    &[("../shared.json", Some(r#"{ "rules": { "require-param": "error" } }"#)), ("../shared.json", Some(SHARED))],
    &[("../shared.json", Some(r#"{ "exclude": ["ws/src/lib.ts"] }"#)), ("../shared.json", Some(SHARED))],
    &[
        ("docov.json", Some(r#"{ "extends": "../shared.json", "nested": true, "scope": "exports" }"#)),
        ("src/docov.json", Some(r#"{ "scope": "all" }"#)),
        ("src/docov.json", None),
        ("docov.json", Some(CONFIG)),
    ],
    &[
        ("docov.json", Some(r#"{ "extends": "../shared.json", "nested": true, "scope": "exports" }"#)),
        ("docov.json", Some(CONFIG)),
    ],
    &[("jsdoc.json", Some(r#"{ "source": { "excludePattern": "lib" } }"#)), ("jsdoc.json", Some("{}"))],
    &[("src/docov.json", Some(r#"{ "scope": "all" }"#)), ("src/docov.json", None)],
    &[
        ("src/base.json", Some("{}")),
        ("src/docov.json", Some(r#"{ "extends": "./base.json" }"#)),
        ("src/base.json", Some(r#"{ "scope": "all" }"#)),
        ("src/docov.json", None),
        ("src/base.json", None),
    ],
    &[("package.json", Some(r#"{ "exports": "./src/lib.ts" }"#)), ("package.json", Some(PACKAGE))],
    &[
        ("package.json", Some(r#"{ "exports": "./dist/index.js" }"#)),
        ("tsconfig.json", Some(r#"{ "compilerOptions": { "outDir": "dist", "rootDir": "src" } }"#)),
        ("package.json", Some(PACKAGE)),
        ("tsconfig.json", None),
    ],
    &[("src/index.ts", Some("export * from './lib.js';\n")), ("src/index.ts", Some(INDEX))],
    &[("src/extra.ts", Some("export { b } from './lib.js';\n")), ("src/extra.ts", None)],
];

const SHARED: &str = r#"{ "rules": { "require-param": "warn" } }"#;
const CONFIG: &str =
    r#"{ "extends": "../shared.json", "nested": true, "scope": "public", "jsdocConfig": "jsdoc.json" }"#;
const PACKAGE: &str = r#"{ "exports": "./src/index.ts" }"#;
/// The entry point: `a`, and whatever `./extra.js` exports once it's there.
const INDEX: &str = "export { a } from './lib.js';\nexport * from './extra.js';\n";

/// After a file the document's rules were read from changes, the next diagnostics are what a
/// server that kept nothing publishes (or the error it logs), for each of [`SCENARIOS`]' edits, in
/// any order. The server learns of a change from the client when the client watches files (except
/// for the file outside the folder, which no watcher covers), and otherwise from each file's
/// modified time and size.
#[hegel::test(test_cases = 2)]
fn kept_rules_follow_the_files_they_were_read_from(tc: TestCase) {
    let mut order: Vec<_> = (0..SCENARIOS.len()).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, tc.draw(gs::integers::<usize>().max_value(i)));
    }
    for watch in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let ws = root.join("ws");
        write(&root, "shared.json", SHARED);
        for (name, text) in [
            ("docov.json", CONFIG),
            ("jsdoc.json", "{}"),
            ("package.json", PACKAGE),
            ("src/index.ts", INDEX),
            ("src/lib.ts", LIB),
        ] {
            write(&ws, name, text);
        }
        let path = ws.join("src/lib.ts");
        let client = if watch {
            Client::watching(&ws, true)
        } else {
            Client::start(json!({ "capabilities": {}, "workspaceFolders": [{ "uri": uri(&ws), "name": "p" }] }))
        };
        settle();
        client.open(&uri(&path), LIB);
        let mut last = client.outcome();
        assert_eq!(last, fresh(&ws, &path, LIB));
        for &i in &order {
            // Each scenario changes the diagnostics somewhere, or it would show nothing.
            let mut changed = false;
            for &(name, text) in SCENARIOS[i] {
                let file = if name.starts_with("..") { root.join("shared.json") } else { ws.join(name) };
                let existed = file.is_file();
                match text {
                    Some(text) => write(&ws, name, text),
                    None => fs::remove_file(&file).unwrap(),
                }
                // Created, changed or deleted, as a watcher says of a file in the folder.
                if watch && file.starts_with(&ws) {
                    client.watched(&[(
                        &file,
                        if !existed {
                            1
                        } else if text.is_some() {
                            2
                        } else {
                            3
                        },
                    )]);
                }
                settle();
                client.change(&uri(&path), LIB);
                let outcome = client.outcome();
                assert_eq!(outcome, fresh(&ws, &path, LIB), "{name}: {text:?}");
                changed |= outcome != last;
                last = outcome;
            }
            assert!(changed, "{:?}", SCENARIOS[i]);
        }
        assert_eq!(client.stop(), Ok(true));
    }
}

/// Rewrites the config at `path` with `scope`, padded with spaces to `size` bytes, and gives it
/// the modified time `modified` when that's given.
fn rewrite(path: &Path, scope: &str, size: usize, modified: Option<std::time::SystemTime>) {
    let text = format!(r#"{{ "scope": "{scope}" }}"#);
    fs::write(path, format!("{text:<size$}")).unwrap();
    if let Some(modified) = modified {
        fs::File::options().write(true).open(path).unwrap().set_modified(modified).unwrap();
    }
}

/// While nothing says a file a document's rules were read from changed, the server keeps them: a
/// config rewritten to the same size and modified time leaves the diagnostics as they were, and so
/// does any rewrite the client's watcher says nothing about, once the client has registered the
/// watcher. When the client says the file changed or was saved, or it refused the watcher and the
/// file's size changed, the diagnostics follow it. A watcher change that isn't about a file, or
/// whose params are wrong, changes nothing (and the latter is logged).
#[hegel::test(test_cases = 2)]
fn kept_rules_stay_while_nothing_says_their_files_changed(tc: TestCase) {
    let (from, to) = if tc.draw(gs::booleans()) { ("exports", "all") } else { ("all", "exports") };
    // Watching: no, yes, and refused.
    for (watch, accept) in [(false, false), (true, true), (true, false)] {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let config = ws.join("docov.json");
        rewrite(&config, from, 40, None);
        let path = ws.join("src/lib.ts");
        write(&ws, "src/lib.ts", LIB);
        let mut client = if watch {
            Client::watching(&ws, accept)
        } else {
            Client::start(json!({ "capabilities": {}, "workspaceFolders": [{ "uri": uri(&ws), "name": "p" }] }))
        };
        settle();
        client.open(&uri(&path), LIB);
        let before = client.outcome();
        assert_eq!(before, fresh(&ws, &path, LIB));

        rewrite(&config, to, 40, Some(fs::metadata(&config).unwrap().modified().unwrap()));
        let after = fresh(&ws, &path, LIB);
        assert_ne!(before, after);
        client.change(&uri(&path), LIB);
        assert_eq!(client.outcome(), before);

        rewrite(&config, to, 50, None);
        settle();
        client.change(&uri(&path), LIB);
        let trusted = watch && accept;
        assert_eq!(&client.outcome(), if trusted { &before } else { &after });
        if trusted {
            client.watched(&[(&config, 2)]);
            client.change(&uri(&path), LIB);
            assert_eq!(client.outcome(), after);
            rewrite(&config, from, 40, None);
            settle();
            client.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri(&config) } }));
            assert_eq!(client.published(&uri(&config)), json!([]));
            client.change(&uri(&path), LIB);
            assert_eq!(client.outcome(), before);
        }

        client.notify("workspace/didChangeWatchedFiles", json!({ "changes": [{ "uri": "untitled:x", "type": 2 }] }));
        client.notify("workspace/didChangeWatchedFiles", json!({ "changes": 1 }));
        assert!(client.logged().starts_with("docov: invalid type"));
        // The first sent nothing back: the next message is this request's response.
        assert_eq!(client.actions(&uri(&ws.join("closed.ts")), (0, 1)), json!([]));
        assert_eq!(client.stop(), Ok(true));
    }
}

/// Rules read from a file whose modified time is so close to the read (or past it) that it may
/// have changed during it aren't kept: a rewrite to the same size and modified time is read again.
#[hegel::test(test_cases = 2)]
fn rules_read_while_a_file_may_change_are_read_again(tc: TestCase) {
    let (from, to) = if tc.draw(gs::booleans()) { ("exports", "all") } else { ("all", "exports") };
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().canonicalize().unwrap();
    let config = ws.join("docov.json");
    let future = std::time::SystemTime::now() + Duration::from_secs(3600);
    rewrite(&config, from, 40, Some(future));
    let path = ws.join("src/lib.ts");
    write(&ws, "src/lib.ts", LIB);
    let client = Client::start(json!({ "capabilities": {}, "workspaceFolders": [{ "uri": uri(&ws), "name": "p" }] }));
    settle();
    client.open(&uri(&path), LIB);
    assert_eq!(client.outcome(), fresh(&ws, &path, LIB));
    rewrite(&config, to, 40, Some(future));
    client.change(&uri(&path), LIB);
    assert_eq!(client.outcome(), fresh(&ws, &path, LIB));
    assert_eq!(client.stop(), Ok(true));
}
