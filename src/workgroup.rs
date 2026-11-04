use std::path::PathBuf;

use crossbeam::channel::Sender;
use git2::{BranchType, Oid, Status, StatusOptions};
use lsp_server::{Message, RequestId, Response};
use lsp_types::{
    CodeActionParams, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, HoverParams, InlayHint, InlayHintParams,
    SemanticTokensParams,
};
use serde_json::to_value;

use crate::{
    app::{MaAction, Office},
    lsp::{Conn, Kind, Lsp},
    util::create_inlay_hint,
};

pub struct ToolBox<'ma> {
    pub office: &'ma mut Office,
    pub lsp_conn: &'ma Sender<Kind<Message, MaAction>>,
    //pub ma_conn: &'ma Sender<MaAction>,
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
    pub fn new() -> Self {
        Self {
            status: vec![],
            head_branch: "".into(),
            remote: vec![],
            tag: vec![],
            commits: vec![],
            view: vec![],
        }
    }

    pub fn format(&self) -> String {
        let mut format = String::new();
        self.view.iter().for_each(|root_view| match root_view {
            RootView::LineBreak | RootView::MessageBus => format.push_str("\n"),
            RootView::HeadLabel => {
                format.push_str("# Head\n");
            }
            RootView::RemoteLabel => {
                format.push_str("# Remote\n");
            }
            RootView::RemoteMember(i) => {
                let member = &self.remote[*i];
                format.push_str(member);
                format.push_str("\n");
            }
            RootView::TagLabel => {
                format.push_str("# Tag\n");
            }
            RootView::TagMember(i) => {
                let member = &self.tag[*i];
                format.push_str(member);
                format.push_str("\n");
            }
            RootView::StatusLabel => {
                format.push_str("# Current Status\n");
            }
            RootView::StatusMember(i) => {
                let (member, _) = &self.status[*i];
                format.push_str(&member.to_string_lossy());
                format.push_str("\n");
            }
            RootView::CommitLabel => {
                format.push_str("# Recent Commits\n");
            }
            RootView::CommitMember(i) => {
                let member = &self.commits[*i];
                format.push_str(&member.to_string()[0..8]);
                format.push_str("\n");
            }
        });
        format
    }

    pub fn build_view(&mut self) {
        self.view.push(RootView::MessageBus);

        self.view.push(RootView::HeadLabel);
        self.view.push(RootView::RemoteLabel);
        self.remote.iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::RemoteMember(i));
        });
        self.view.push(RootView::TagLabel);
        self.tag.iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::TagMember(i));
        });
        self.view.push(RootView::LineBreak);
        self.view.push(RootView::StatusLabel);
        self.status.iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::StatusMember(i));
        });
        self.view.push(RootView::LineBreak);
        self.view.push(RootView::CommitLabel);
        self.commits.iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::CommitMember(i));
        });
    }

    pub fn fill_root(&mut self, office: &mut Office) {
        let head = office.repo.head().unwrap();

        self.head_branch = head.shorthand().unwrap().to_string();

        office.repo.remotes().map(|remotes| {
            for name in remotes.iter().flatten() {
                if let Some(name) = name {
                    self.remote.push(name.into());
                }
            }
        });

        office.repo.tag_names(None).map(|tags| {
            for tag in tags.iter().flatten() {
                if let Some(tag) = tag {
                    self.tag.push(tag.into());
                }
            }
        });

        office
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

        office.repo.revwalk().map(|mut revwalk| {
            revwalk.push(head.target().unwrap());
            for oid in revwalk.take(10) {
                if let Ok(oid) = oid {
                    self.commits.push(oid);
                }
            }
        });
    }
}

impl Root {
    fn inlay_hint(
        &self,
        (start_line, end_line): (u32, u32),
        office: &Office,
        ret: &mut Vec<InlayHint>,
    ) {
        for i in start_line..end_line {
            self.view.get(i as usize).map(|view| {
                match view {
                    RootView::LineBreak => (),
                    RootView::MessageBus => {
                        ret.push(create_inlay_hint(
                            i,
                            0,
                            office.repo.workdir().unwrap().to_string_lossy(),
                        ));
                    }
                    RootView::HeadLabel => {
                        ret.push(create_inlay_hint(i, 7, format!(" {}", &self.head_branch)));
                    }
                    RootView::RemoteLabel => (),
                    RootView::RemoteMember(_) => {
                        //let remote = &self.remote[*member];
                        ret.push(create_inlay_hint(i, 0, "      -> "));
                    }
                    RootView::TagLabel => (),
                    RootView::TagMember(_) => {
                        ret.push(create_inlay_hint(i, 0, "      -> "));
                    }
                    RootView::StatusLabel => (),
                    RootView::StatusMember(member) => {
                        let (_, status) = &self.status[*member];
                        let staged = if status.contains(Status::INDEX_NEW) {
                            "A"
                        } else if status.contains(Status::INDEX_MODIFIED) {
                            "M"
                        } else if status.contains(Status::INDEX_DELETED) {
                            "D"
                        } else if status.contains(Status::INDEX_RENAMED) {
                            "R"
                        } else if status.contains(Status::INDEX_TYPECHANGE) {
                            "T"
                        } else {
                            " "
                        };

                        let unstaged = if status.contains(Status::WT_NEW) {
                            "?"
                        } else if status.contains(Status::WT_MODIFIED) {
                            "M"
                        } else if status.contains(Status::WT_DELETED) {
                            "D"
                        } else if status.contains(Status::WT_RENAMED) {
                            "R"
                        } else if status.contains(Status::WT_TYPECHANGE) {
                            "T"
                        } else {
                            " "
                        };
                        ret.push(create_inlay_hint(
                            i,
                            0,
                            format!("{} | {} -> ", staged, unstaged),
                        ));
                    }
                    RootView::CommitLabel => (),
                    RootView::CommitMember(_) => {
                        ret.push(create_inlay_hint(i, 0, "* "));
                    },
                }
            });
        }
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
    pub fn inlay_hint(&mut self, toolbox: ToolBox, id: RequestId, params: InlayHintParams) {
        let mut ret = Vec::<InlayHint>::new();
        match self {
            WorkGroup::Root(root) => {
                root.inlay_hint((params.range.start.line, params.range.end.line), toolbox.office, &mut ret);
            }
        }

        toolbox
            .lsp_conn
            .send(Kind::Main(Message::Response(Response {
                id,
                response_result: Ok(to_value(ret).unwrap()),
            })));
    }
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
