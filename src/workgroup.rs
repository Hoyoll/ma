use std::path::PathBuf;

use crossbeam::channel::Sender;
use git2::{BranchType, Oid, Status, StatusOptions};
use lsp_server::RequestId;
use lsp_types::{
    CodeActionParams, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, HoverParams, InlayHintParams,
    SemanticTokensParams,
};

use crate::{
    app::{MaAction, Office},
    lsp::{Conn, Lsp},
};

pub struct ToolBox<'ma> {
    pub office: &'ma mut Office,
    pub lsp_conn: &'ma Conn,
    pub ma_conn: &'ma Sender<MaAction>,
}

pub enum WorkGroup {
    Root(Root),
}

pub struct Root {
    status: Vec<(PathBuf, Status)>,
    head_branch: String,
    remote: Vec<String>,
    tag: Vec<String>,
    commits: Vec<Oid>,
    view: Vec<RootView>,
}

enum RootView {
    LineBreak,
    MessageBus,
    HeadLabel,
    RemoteLabel,
    RemoteMember(usize),
    TagLabel,
    TagMember(usize),
    StatusLabel,
    StatusMember(usize),
    CommitLabel,
    CommitMember(usize),
}

impl Root {
    fn format(&self) -> String {
        let mut format = String::new();
        self.view.iter().for_each(|root_view| {
            match root_view {
                RootView::LineBreak | RootView::MessageBus => format.push_str("\n"),
                RootView::HeadLabel => {
                    format.push_str(&format!("# Head {}\n", self.head_branch));
                },
                RootView::RemoteLabel => {
                    format.push_str("# Remote\n");
                },
                RootView::RemoteMember(i) => {
                    let member = &self.remote[*i];
                    format.push_str(member);
                    format.push_str("\n");
                },
                RootView::TagLabel => {
                    format.push_str("# Tag\n");
                },
                RootView::TagMember(i) => { 
                    let member = &self.tag[*i];
                    format.push_str(member);
                    format.push_str("\n");
                },
                RootView::StatusLabel => {
                    format.push_str("# Current Status\n");
                },
                RootView::StatusMember(i) => {
                    let (member,_) = &self.status[*i];
                    format.push_str(&member.to_string_lossy());
                    format.push_str("\n");
                },
                RootView::CommitLabel => {
                    format.push_str("# Recent Commits");
                },
                RootView::CommitMember(i) => {
                    let member = &self.commits[*i];
                    format.push_str(&member.to_string()[0..8]);
                    format.push_str("\n");
                },
            }
        });
        format
    }

    fn build_view(&mut self) {
        self.view.push(RootView::MessageBus);
        
        self.view.push(RootView::HeadLabel);
        self.view.push(RootView::RemoteLabel);
        self.remote.iter().enumerate().for_each(|(i,_)| {
            self.view.push(RootView::RemoteMember(i));
        });
        self.view.push(RootView::TagLabel);
        self.tag.iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::TagMember(i));
        });
        self.view.push(RootView::LineBreak);
        self.view.push(RootView::StatusLabel);
        self.status.iter().enumerate().for_each(|(i,_)| {
            self.view.push(RootView::StatusMember(i));
        });
        self.view.push(RootView::LineBreak);
        self.view.push(RootView::CommitLabel);
        self.commits.iter().enumerate().for_each(|(i,_)| {
            self.view.push(RootView::CommitMember(i));
        });
    }

    fn fill_root(&mut self, tk: &mut ToolBox) {
        let head = tk.office.repo.head().unwrap();

        self.head_branch = head.shorthand().unwrap().to_string();

        tk.office.repo.remotes().map(|remotes| {
            for name in remotes.iter().flatten() {
                if let Some(name) = name {
                    self.remote.push(name.into());
                }
            }
        });

        tk.office.repo.tag_names(None).map(|tags| {
            for tag in tags.iter().flatten() {
                if let Some(tag) = tag {
                    self.tag.push(tag.into());
                }
            }
        });

        tk.office
            .repo
            .statuses(Some(
                StatusOptions::new()
                    .include_untracked(true)
                    .recurse_untracked_dirs(true),
            ))
            .map(|status| {
                for entry in status.iter() {
                    self.status
                        .push((PathBuf::from(entry.path().unwrap()), entry.status()));
                }
            });

        tk.office.repo.revwalk().map(|mut revwalk| {
            revwalk.push(head.target().unwrap());
            for oid in revwalk {
                if let Ok(oid) = oid {
                    self.commits.push(oid); 
                } 
            }
        });
    }
}

impl WorkGroup {
    pub fn hover(&mut self, toolbox: ToolBox, id: RequestId, params: HoverParams) {}
    pub fn goto_definition(
        &mut self,
        toolbox: ToolBox,
        id: RequestId,
        params: GotoDefinitionParams,
    ) {
    }
    pub fn code_action(&mut self, toolbox: ToolBox, id: RequestId, params: CodeActionParams) {}
    pub fn inlay_hint(&mut self, toolbox: ToolBox, id: RequestId, params: InlayHintParams) {}
    pub fn semantic_token(
        &mut self,
        toolbox: ToolBox,
        id: RequestId,
        params: SemanticTokensParams,
    ) {
    }
}

impl WorkGroup {
    pub fn did_save(&mut self, toolbox: ToolBox, params: DidSaveTextDocumentParams) {}
    pub fn did_open(&mut self, toolbox: ToolBox, params: DidOpenTextDocumentParams) {}
    pub fn did_change(&mut self, toolbox: ToolBox, params: DidChangeTextDocumentParams) {}
}
