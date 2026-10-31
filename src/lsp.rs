//! `docov lsp`: a language server, for editors. For each open JavaScript or TypeScript document it
//! publishes an information diagnostic at each undocumented item and each doc check's finding at
//! its level, from the unsaved text, by the config its file is scanned with, and offers each
//! undocumented item's stub as a code action.
//!
//! It's on lsp-server, rust-analyzer's synchronous scaffold, rather than tower-lsp, which would
//! bring in tokio for a server that scans one document at a time.

mod convert;
mod project;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CodeActionProviderCapability, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidChangeWatchedFilesRegistrationOptions, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, FileSystemWatcher, GlobPattern, InitializeParams,
    LogMessageParams, MessageType, PublishDiagnosticsParams, Registration, RegistrationParams, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Url,
    notification::{
        DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
        LogMessage, Notification as _, PublishDiagnostics,
    },
    request::{CodeActionRequest, RegisterCapability, Request as _},
};
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;

use crate::{FileReport, collect};

/// The id of the server's request for the client to watch files, and of the watcher it registers.
const WATCH: &str = "docov/watch";

/// Serves the editor on stdin and stdout until it exits, and says whether it shut the server down
/// first, as it should, or why the session broke.
pub fn stdio() -> Result<bool, Vec<String>> {
    let (connection, threads) = Connection::stdio();
    let shut_down = serve(&connection).map_err(|e| vec![e])?;
    // The writer stops once nothing can send to it.
    drop(connection);
    threads.join().map_err(|e| vec![e.to_string()])?;
    Ok(shut_down)
}

/// Serves the client at the other end of `connection` until it exits or goes away, and says
/// whether it shut the server down first, or why the session broke.
pub fn serve(connection: &Connection) -> Result<bool, String> {
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::FULL),
            save: Some(TextDocumentSyncSaveOptions::Supported(true)),
            ..TextDocumentSyncOptions::default()
        })),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    };
    let capabilities = serde_json::to_value(capabilities).expect("capabilities serialize");
    let params = connection.initialize(capabilities).map_err(|e| e.to_string())?;
    let params: InitializeParams = serde_json::from_value(params).map_err(|e| format!("initialize: {e}"))?;
    let mut server = Server::new(connection, params);
    for message in &connection.receiver {
        match message {
            Message::Request(request) if connection.handle_shutdown(&request).map_err(|e| e.to_string())? => {
                return Ok(true);
            }
            Message::Request(request) => server.request(request),
            Message::Notification(notification) if notification.method == "exit" => return Ok(false),
            Message::Notification(notification) => server.notify(notification),
            Message::Response(response) => server.respond(&response),
        }
    }
    Ok(false)
}

/// The server's state: the open documents' text, and the rules they're scanned with.
struct Server<'c> {
    connection: &'c Connection,
    documents: FxHashMap<Url, String>,
    projects: project::Projects,
}

impl<'c> Server<'c> {
    /// The server for a client that initialized it with `params`, which asks the client to watch
    /// files for it when it can.
    fn new(connection: &'c Connection, params: InitializeParams) -> Self {
        // `rootUri` is from before workspace folders, which clients still send.
        #[expect(deprecated, reason = "older clients send only rootUri")]
        let root = params.root_uri;
        let folders = params.workspace_folders.unwrap_or_default().into_iter().map(|folder| folder.uri);
        let roots = folders.chain(root).filter_map(|uri| uri.to_file_path().ok()).collect();
        let server = Self { connection, documents: FxHashMap::default(), projects: project::Projects::new(roots) };
        let watched = params.capabilities.workspace.and_then(|w| w.did_change_watched_files);
        if watched.and_then(|w| w.dynamic_registration) == Some(true) {
            server.watch();
        }
        server
    }

    /// Asks the client to say when a file docov reads changes.
    fn watch(&self) {
        let glob = |glob: &str| FileSystemWatcher { glob_pattern: GlobPattern::String(glob.into()), kind: None };
        let options = DidChangeWatchedFilesRegistrationOptions { watchers: project::WATCHED.map(glob).into() };
        let registration = Registration {
            id: WATCH.into(),
            method: DidChangeWatchedFiles::METHOD.into(),
            register_options: Some(serde_json::to_value(options).expect("options serialize")),
        };
        let params = RegistrationParams { registrations: vec![registration] };
        self.send(Request::new(WATCH.to_owned().into(), RegisterCapability::METHOD.into(), params));
    }

    /// Trusts the client to say when a file changes, once it registered the watcher.
    fn respond(&mut self, response: &Response) {
        if response.id == RequestId::from(WATCH.to_owned()) && response.response_result.is_ok() {
            self.projects.watching = true;
        }
    }

    fn send(&self, message: impl Into<Message>) {
        // When the client has gone, the receiver ends the session.
        self.connection.sender.send(message.into()).ok();
    }

    /// Tells the client about a problem that isn't any one document's, in its log.
    fn log(&self, message: String) {
        let params = LogMessageParams { typ: MessageType::ERROR, message: format!("docov: {message}") };
        self.send(Notification::new(LogMessage::METHOD.into(), params));
    }

    fn request(&mut self, request: Request) {
        let response = match request.method.as_str() {
            CodeActionRequest::METHOD => match parse::<CodeActionParams>(request.params) {
                Ok(params) => Response::new_ok(request.id, self.actions(params)),
                Err(e) => Response::new_err(request.id, ErrorCode::InvalidParams as i32, e),
            },
            method => Response::new_err(
                request.id,
                ErrorCode::MethodNotFound as i32,
                format!("docov doesn't handle {method}"),
            ),
        };
        self.send(response);
    }

    /// Keeps each open document's text, and publishes its diagnostics when it opens, changes or is
    /// saved, and none once it closes. Forgets what was read from a file that changed.
    fn notify(&mut self, notification: Notification) {
        let uri = match notification.method.as_str() {
            DidChangeWatchedFiles::METHOD => {
                return match parse::<DidChangeWatchedFilesParams>(notification.params) {
                    Ok(params) => {
                        let paths = params.changes.into_iter().flat_map(|change| change.uri.to_file_path());
                        paths.for_each(|path| self.projects.changed(&path));
                    }
                    Err(e) => self.log(e),
                };
            }
            DidOpenTextDocument::METHOD => parse(notification.params).map(|p: DidOpenTextDocumentParams| {
                self.documents.insert(p.text_document.uri.clone(), p.text_document.text);
                p.text_document.uri
            }),
            // Full sync: the last change is the whole text.
            DidChangeTextDocument::METHOD => parse(notification.params).map(|mut p: DidChangeTextDocumentParams| {
                let uri = p.text_document.uri;
                self.documents.extend(p.content_changes.pop().map(|change| (uri.clone(), change.text)));
                uri
            }),
            // A client that watches files may say the saved one changed only after this.
            DidSaveTextDocument::METHOD => parse(notification.params).map(|p: DidSaveTextDocumentParams| {
                p.text_document.uri.to_file_path().into_iter().for_each(|path| self.projects.changed(&path));
                p.text_document.uri
            }),
            DidCloseTextDocument::METHOD => parse(notification.params).map(|p: DidCloseTextDocumentParams| {
                self.documents.remove(&p.text_document.uri);
                p.text_document.uri.to_file_path().into_iter().for_each(|path| self.projects.forget(&path));
                p.text_document.uri
            }),
            _ => return,
        };
        match uri {
            Ok(uri) => self.publish(uri),
            Err(e) => self.log(e),
        }
    }

    /// Publishes the diagnostics of the document at `uri`, none if it isn't open or its config
    /// leaves it out. When it can't be scanned (it doesn't parse yet, say), the ones published
    /// last stay, and the log says why.
    fn publish(&mut self, uri: Url) {
        let diagnostics = match self.documents.get(&uri).map(|text| (text, scan(&mut self.projects, &uri, text))) {
            None => Vec::new(),
            Some((text, Ok(report))) => report.map_or_else(Vec::new, |report| convert::diagnostics(&report, text)),
            Some((_, Err(e))) => return self.log(e),
        };
        self.send(Notification::new(
            PublishDiagnostics::METHOD.into(),
            PublishDiagnosticsParams { uri, diagnostics, version: None },
        ));
    }

    /// A stub for each undocumented item on the lines the client asks about.
    fn actions(&mut self, params: CodeActionParams) -> Vec<CodeActionOrCommand> {
        let uri = params.text_document.uri;
        let Some(text) = self.documents.get(&uri) else { return Vec::new() };
        let Ok(Some(report)) = scan(&mut self.projects, &uri, text) else { return Vec::new() };
        let lines = params.range.start.line as usize + 1..=params.range.end.line as usize + 1;
        let asked = report.counted().filter(|item| lines.contains(&item.line));
        asked.filter_map(|item| convert::action(&uri, item, text)).map(CodeActionOrCommand::CodeAction).collect()
    }
}

/// The document at `uri`, whose text is `text`, scanned by the rules `projects` has for it, or
/// nothing when its config leaves it out.
fn scan(projects: &mut project::Projects, uri: &Url, text: &str) -> Result<Option<FileReport>, String> {
    let path = uri.to_file_path().map_err(|()| format!("{uri}: not a file"))?;
    let Some(rules) = projects.rules(&path)? else { return Ok(None) };
    collect::scan_source(&path, text, &rules).map(Some).map_err(|e| e.to_string())
}

/// A message's params as `T`, or why they aren't.
fn parse<T: DeserializeOwned>(params: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(params).map_err(|e| e.to_string())
}
