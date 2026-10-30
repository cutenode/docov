//! `docov lsp`: a language server, for editors. For each open JavaScript or TypeScript document it
//! publishes an information diagnostic at each undocumented item and each doc check's finding at
//! its level, from the unsaved text, by the config its file is scanned with, and offers each
//! undocumented item's stub as a code action.
//!
//! It's on lsp-server, rust-analyzer's synchronous scaffold, rather than tower-lsp, which would
//! bring in tokio for a server that scans one document at a time.

mod convert;
mod project;

use std::path::PathBuf;

use clap::ArgMatches;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CodeActionProviderCapability, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams,
    LogMessageParams, MessageType, PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Url,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, LogMessage,
        Notification as _, PublishDiagnostics,
    },
    request::{CodeActionRequest, Request as _},
};
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;

use crate::{FileReport, collect, feature};

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
            // It sends no requests, so it waits on no responses.
            Message::Response(_) => {}
        }
    }
    Ok(false)
}

/// The server's state: the open documents' text.
struct Server<'c> {
    connection: &'c Connection,
    /// The workspace folders, which docov finds a document's config from as if it ran there.
    roots: Vec<PathBuf>,
    documents: FxHashMap<Url, String>,
    /// No flags, which the settings are made from with the config's keys.
    matches: ArgMatches,
}

impl<'c> Server<'c> {
    fn new(connection: &'c Connection, params: InitializeParams) -> Self {
        // `rootUri` is from before workspace folders, which clients still send.
        #[expect(deprecated, reason = "older clients send only rootUri")]
        let root = params.root_uri;
        let folders = params.workspace_folders.unwrap_or_default().into_iter().map(|folder| folder.uri);
        let roots = folders.chain(root).filter_map(|uri| uri.to_file_path().ok()).collect();
        let matches = feature::args(clap::Command::new("docov")).get_matches_from(["docov"]);
        Self { connection, roots, documents: FxHashMap::default(), matches }
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

    fn request(&self, request: Request) {
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
    /// saved, and none once it closes.
    fn notify(&mut self, notification: Notification) {
        let uri = match notification.method.as_str() {
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
            DidSaveTextDocument::METHOD => {
                parse(notification.params).map(|p: DidSaveTextDocumentParams| p.text_document.uri)
            }
            DidCloseTextDocument::METHOD => parse(notification.params).map(|p: DidCloseTextDocumentParams| {
                self.documents.remove(&p.text_document.uri);
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
    fn publish(&self, uri: Url) {
        let diagnostics = match self.documents.get(&uri).map(|text| (text, self.scan(&uri, text))) {
            None => Vec::new(),
            Some((text, Ok(report))) => report.map_or_else(Vec::new, |report| convert::diagnostics(&report, text)),
            Some((_, Err(e))) => return self.log(e),
        };
        self.send(Notification::new(
            PublishDiagnostics::METHOD.into(),
            PublishDiagnosticsParams { uri, diagnostics, version: None },
        ));
    }

    /// The document at `uri`, whose text is `text`, scanned by its config, or nothing when the
    /// config leaves it out.
    fn scan(&self, uri: &Url, text: &str) -> Result<Option<FileReport>, String> {
        let path = uri.to_file_path().map_err(|()| format!("{uri}: not a file"))?;
        let Some(rules) = project::rules(&path, &self.roots, &self.matches)? else { return Ok(None) };
        collect::scan_source(&path, text, &rules).map(Some).map_err(|e| e.to_string())
    }

    /// A stub for each undocumented item on the lines the client asks about.
    fn actions(&self, params: CodeActionParams) -> Vec<CodeActionOrCommand> {
        let uri = params.text_document.uri;
        let Some(text) = self.documents.get(&uri) else { return Vec::new() };
        let Ok(Some(report)) = self.scan(&uri, text) else { return Vec::new() };
        let lines = params.range.start.line as usize + 1..=params.range.end.line as usize + 1;
        let asked = report.counted().filter(|item| lines.contains(&item.line));
        asked.filter_map(|item| convert::action(&uri, item, text)).map(CodeActionOrCommand::CodeAction).collect()
    }
}

/// A message's params as `T`, or why they aren't.
fn parse<T: DeserializeOwned>(params: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(params).map_err(|e| e.to_string())
}
