use std::process::Command;

use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    DefinitionOptions, DidOpenTextDocumentParams, FoldingRangeParams, GotoDefinitionParams, Hover, HoverContents, HoverParams, HoverProviderCapability, InitializeParams, MarkupContent, OneOf, ServerCapabilities, lsp_request, notification::{DidOpenTextDocument, Notification}, request::{FoldingRangeRequest, GotoDefinition, HoverRequest, Initialize, Request}
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
enum GitView {
    Padding,
    BranchHeader,
    BranchMember(usize),
    CommitHeader,
    CommitMember(usize),
}

#[derive(PartialEq, Eq)]
enum Accordion {
    Expand,
    Revert,
}

// git branch --show-current
// git commands, for local branch = git branch -l --format='%(refname:short)'
// git commands, for remote branch = git branch -r --format='%(refname:short)'
// gt commands, for log = git log -n 10 --format='%h'
// git commands, for show = git show --no-patch <hash>
// git commands, for $current_git = git rev-parse --show-toplevel
// / git -C <dir_path> rev-parse --show-toplevel
#[derive(Default)]
struct GitClient {
    branch: (usize, String),
    branch_member: Vec<String>,
    commits: Vec<String>,
    format: String,
    view: Vec<GitView>,
}

trait Lsp {
    fn initialize(&mut self, conn: &Connection, id: RequestId, params: InitializeParams);

    fn hover(&mut self, conn: &Connection, id: RequestId, params: HoverParams);
    fn did_open(&mut self, conn: &Connection, params: DidOpenTextDocumentParams);
    fn goto_definition(&mut self, conn: &Connection, id: RequestId, params: GotoDefinitionParams);
    fn folding(&mut self, conn: &Connection, id: RequestId, params: FoldingRangeParams); 
    fn handle_request(&mut self, conn: &Connection, request: lsp_server::Request) { 
       match request.method.as_str() {
            Initialize::METHOD => {
                serde_json::from_value(request.params).map(|params: InitializeParams| {
                    self.initialize(conn, request.id, params);
                });
            }

            HoverRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: HoverParams| {
                    self.hover(conn, request.id, params);
                });
            }
            GotoDefinition::METHOD => {
                serde_json::from_value(request.params).map(|params: GotoDefinitionParams | {
                    self.goto_definition(conn, request.id, params);
                });
            }
            FoldingRangeRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: FoldingRangeParams| {
                    self.folding(conn, request.id, params);
                });   
            }
            _ => {
                send_err(
                    conn,
                    request.id,
                    lsp_server::ErrorCode::MethodNotFound,
                    "unhandled method",
                );
            }
        }
    }
    
    fn handle_notification(&mut self, conn: &Connection, notification: lsp_server::Notification) {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(|params: DidOpenTextDocumentParams| { 
                    self.did_open(conn, params);
                });
            }
            _ => () 
        }
    }
}

impl GitClient {
    pub async fn get_branches(&mut self) {
        let output = Command::new("git")
            .args(&["branch", "-l", "--format=%(refname:short)"])
            .output()
            .unwrap();
        self.branch_member = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
    }

    pub fn view(&mut self) {
        self.view.clear();
        self.view.push(GitView::Padding);
        self.view.push(GitView::BranchHeader);
        let mut i: usize = 0;
        for _ in &self.branch_member {
            self.view.push(GitView::BranchMember(i));
            i += 1
        }
    }

    pub fn format(&mut self) {
        self.format.clear();
        // # Branch: <branch>
        self.format.push_str("# Branch: ");
        self.format.push_str(&self.branch.1);
        self.format.push_str("\n");
        for member in &self.branch_member {
            self.format.push_str("- ");
            self.format.push_str(member);
            self.format.push_str("\n");
        }
        // Padding
        self.format.push_str("\n");

        // # Commit
        self.format.push_str("# Commit\n");
        for member in &self.commits {
            self.format.push_str("- ");
            self.format.push_str(member);
            self.format.push_str("\n");
        }
    }

    pub fn git_show(&self, hash: &str) -> String {
        let output = Command::new("git").args(&["show", hash]).output().unwrap();
        String::from_utf8(output.stdout).unwrap()
    }

    pub fn get_commits() {
        let output = Command::new("git")
            .args(&["branch", "--format=%(refname:short)"])
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for brances in stdout {
            println!("{}", brances);
        }
    }
}

fn send_ok<T: serde::Serialize>(conn: &Connection, id: RequestId, result: &T) {
    let resp = Response {
        id,
        response_result: Ok(serde_json::to_value(result).unwrap()),
    };
    conn.sender.send(Message::Response(resp));
}

fn send_err(conn: &Connection, id: RequestId, code: lsp_server::ErrorCode, msg: &str) {
    let resp = Response {
        id,
        response_result: Err(lsp_server::ResponseError {
            code: code as i32,
            message: msg.into(),
            data: None,
        }),
    };
    conn.sender.send(Message::Response(resp));
}

impl Lsp for GitClient {
    fn initialize(&mut self, conn: &Connection, id: RequestId, params: InitializeParams) {
        //conn.sender.send(Message::Response(()))
        if let Some(workspace) = params.workspace_folders {
            for w in workspace {
            
            }
        }
    }

    fn hover(&mut self, conn: &Connection, id: RequestId, params: HoverParams) {
        let idx = params.text_document_position_params.position.line as usize;
        //params.text_document_position_params.text_document.uri
        match self.view[idx] {
            GitView::BranchMember(member) => {
            }
            _ => ()
        }
    }

    fn goto_definition(&mut self, conn: &Connection, id: RequestId, params: GotoDefinitionParams) {
        let idx = params.text_document_position_params.position.line as usize;
        match self.view[idx] {
            GitView::BranchMember(member) => {

            }
            _ => ()
        }
    }

    fn folding(&mut self, conn: &Connection, id: RequestId, params: FoldingRangeParams) {
        
    }

    fn did_open(&mut self, conn: &Connection, params: DidOpenTextDocumentParams) {

    }
}
fn main() {
    let (conn, io_t) = Connection::stdio();
    let caps = ServerCapabilities {
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };

    let init = serde_json::json!({
        "capabilities": caps,
        "encoding": ["utf-8"]
    });

    let init_params = conn.initialize(init).unwrap();
    let mut client = GitClient::default();
    for msg in &conn.receiver {
            match msg {
                Message::Request(request) => client.handle_request(&conn, request),
                Message::Response(response) => todo!(),
                Message::Notification(notification) =>client.handle_notification(&conn, notification),
            }
        }
    //client.main_loop(conn, init_params);
    io_t.join();
}
