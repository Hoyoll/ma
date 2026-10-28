use std::{collections::HashMap, path::PathBuf};

use lsp_types::notification::Notification;

use crate::{workgroup::WorkGroup, lsp, util::Container};

pub struct Office {
    pub repo: git2::Repository,
    pub cache: PathBuf,
    pub status: Vec<(PathBuf, git2::Status)>,
    pub manifest: HashMap<git2::Oid, lsp_types::Uri>,
    pub file_cache: HashMap<PathBuf, lsp_types::Uri>,
    pub ma_root: lsp_types::Uri,
    pub ma_input: lsp_types::Uri,
    pub branch: Vec<Branch>,
    pub head_branch: usize,
    pub viewed_branch: usize,
}

pub struct Branch {
    name: String,
    b_type: git2::BranchType,
    commits: Vec<git2::Oid>,
}

pub struct App {
    office: Container<Office>,
    work_group: HashMap<lsp_types::Uri, (WorkGroup, usize)>,
    conn: lsp::Conn,
}

impl lsp::Lsp for App {
    fn hover(&mut self, id: lsp_server::RequestId, params: lsp_types::HoverParams) {
    }

    fn goto_definition(&mut self, id: lsp_server::RequestId, params: lsp_types::GotoDefinitionParams) {
        
    }

    fn code_action(&mut self, id: lsp_server::RequestId, params: lsp_types::CodeActionParams) {
        
    }

    fn inlay_hint(&mut self, id: lsp_server::RequestId, params: lsp_types::InlayHintParams) {
        
    }

    fn did_save(&mut self, params: lsp_types::DidSaveTextDocumentParams) {
        
    }

    fn did_open(&mut self, params: lsp_types::DidOpenTextDocumentParams) {
        let url = params.text_document.uri;
        //self.work_group.get_mut(&url)
    }

    fn did_change(&mut self, params: lsp_types::DidChangeTextDocumentParams) {
        
    }
}
