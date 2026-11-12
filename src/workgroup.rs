use std::{
    fs,
    path::{Path, PathBuf},
    process,
    str::FromStr,
    thread::spawn,
};

use crossbeam::channel::Sender;
use git2::{
    BranchType, Commit, Delta, Index, IndexAddOption, MergeAnalysis, Oid, PackBuilderStage,
    PushOptions, RemoteCallbacks, Repository, Status, StatusOptions,
};
use lsp_server::{Message, RequestId, Response};
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CodeActionResponse, Command, Diagnostic,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, InlayHint, InlayHintParams, Location, Position, Range,
    SemanticToken, SemanticTokens, SemanticTokensParams, SemanticTokensResult, ShowDocumentParams,
    TextDocumentPositionParams, Uri,
    notification::PublishDiagnostics,
    request::{DocumentDiagnosticRequest, InlayHintRefreshRequest, Request, ShowDocument},
};
use serde::{Deserialize, Serialize};
use serde_json::to_value;

use crate::{
    app::{self, App, GitPush, MaAction, Office, Progress, SemanticTokenList},
    lsp::Kind,
    util::{self, Container, create_inlay_hint, create_semantic_token, name_to_url},
};

impl WorkGroup {
    pub fn did_save(&mut self, toolbox: ToolBox, params: DidSaveTextDocumentParams) {
        match self {
            WorkGroup::Root(root) => match &mut root.mode {
                RootMode::Dashboard => (),
                RootMode::CommitInput => {
                    let author = toolbox.office.repo.author_from_env().unwrap();
                    let committer = toolbox.office.repo.committer_from_env().unwrap();
                    let mut index = toolbox.office.repo.index().unwrap();
                    let tree_id = index.write_tree().unwrap();
                    let tree = toolbox.office.repo.find_tree(tree_id).unwrap();
                    let res = match toolbox.office.repo.head().ok() {
                        Some(head) => {
                            let parent = head.peel_to_commit().unwrap();
                            // Existing commit
                            toolbox.office.repo.commit(
                                Some("HEAD"),
                                &author,
                                &committer,
                                params.text.unwrap_or_default().trim_start(),
                                &tree,
                                &[&parent],
                            )
                        }
                        None => toolbox.office.repo.commit(
                            Some("HEAD"),
                            &author,
                            &committer,
                            params.text.unwrap_or_default().as_ref(),
                            &tree,
                            &[],
                        ),
                    };
                    if let Ok(_) = res {
                        //root.mode = RootMode::Dashboard;
                        toolbox.lsp_conn.send(Kind::Other(
                            app::Command::ChangeRootMode {
                                uri: params.text_document.uri,
                                mode: RootMode::Dashboard,
                            }
                            .into(),
                        ));
                    }
                }
                RootMode::TagInput { oid } => {
                    let input = match params.text {
                        Some(text) => text,
                        None => return,
                    };
                    let mut line = input.lines();
                    let name = match line.next() {
                        Some(name) => name,
                        None => return,
                    };
                    //Commit::amend(&self, update_ref, author, committer, message_encoding, message, tree)
                    let message = line.collect::<Vec<_>>().join("\n");
                    let oid = root.commits[oid];
                    let commit = toolbox.office.repo.find_commit(oid).unwrap();
                    let signature = toolbox.office.repo.signature().unwrap();
                    if let Ok(_) = toolbox.office.repo.tag(
                        name,
                        commit.as_object(),
                        &signature,
                        &message,
                        false,
                    ) {
                        toolbox.lsp_conn.send(Kind::Other(MaAction::Command {
                            command: app::Command::ChangeRootMode {
                                uri: params.text_document.uri,
                                mode: RootMode::Dashboard,
                            },
                        }));
                    }
                }
            },
            WorkGroup::Diff(diff) => (),

            _ => (),
        }
    }

    pub fn did_open(&mut self, toolbox: ToolBox, params: DidOpenTextDocumentParams) {}
}

impl WorkGroup {
    pub fn hover(&mut self, toolbox: ToolBox, id: RequestId, params: HoverParams) {
        match self {
            WorkGroup::Root(root) => (),
            WorkGroup::Diff(diff) => (),

            _ => (),
        }
    }

    pub fn goto_definition(
        &mut self,
        toolbox: ToolBox,
        id: RequestId,
        params: GotoDefinitionParams,
    ) {
        match self {
            WorkGroup::Root(root) => {
                root.goto_definition(&toolbox, params.text_document_position_params);
            }
            WorkGroup::Diff(diff) => {
                diff.goto_definition(&toolbox, (id, params.text_document_position_params));
            }
            WorkGroup::DiffIndex(diff_index) => {
                diff_index.goto_definition(&toolbox, (id, params.text_document_position_params));
            }
            _ => (),
        }
    }

    pub fn code_action(&mut self, toolbox: ToolBox, id: RequestId, params: CodeActionParams) {
        let mut ret = CodeActionResponse::new();
        match self {
            WorkGroup::Root(root) => {
                root.code_action(toolbox.office, params, &mut ret);
            }
            WorkGroup::Diff(diff) => (),

            _ => (),
        }
        toolbox.lsp_conn.send(Kind::Main(
            Response {
                id,
                response_result: Ok(to_value(Some(ret)).unwrap()),
            }
            .into(),
        ));
    }

    pub fn inlay_hint(&mut self, toolbox: ToolBox, id: RequestId, params: InlayHintParams) {
        let mut ret = Vec::<InlayHint>::new();
        //eprintln!("here!");
        match self {
            WorkGroup::Root(root) => {
                //eprintln!("{:?}", params.range);
                root.inlay_hint(
                    (params.range.start.line, params.range.end.line),
                    toolbox.office,
                    &mut ret,
                );
            }
            WorkGroup::Diff(diff) => {
                diff.inlay_hint(
                    (params.range.start.line, params.range.end.line),
                    toolbox.office,
                    &mut ret,
                );
            }
            WorkGroup::DiffIndex(diff_index) => {
                diff_index.inlay_hint((params.range.start.line, params.range.end.line), toolbox.office, &mut ret);
            }
            _ => (),
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
        _params: SemanticTokensParams,
    ) {
        let mut data: Vec<SemanticToken> = vec![];

        match self {
            WorkGroup::Root(root) => {
                root.semantic_token(&mut data);
            }
            _ => (),
        }
        let st = SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        });
        toolbox
            .lsp_conn
            .send(Kind::Main(Message::Response(Response {
                id,
                response_result: Ok(to_value(st).unwrap()),
            })));
    }
}

pub struct ToolBox<'ma> {
    pub office: &'ma mut Office,
    pub lsp_conn: &'ma Sender<Kind<Message, MaAction>>,
    //pub ma_conn: &'ma Sender<MaAction>,
}

const HASH_LENGTH: usize = 7;

pub enum WorkGroup {
    Root(Root),
    Diff(Diff),
    MergeConflict(MergeConflict),
    DiffIndex(DiffIndex),
    //BranchInfo(BranchInfo)
    //TagInfo(TagInfo)
}

pub struct Root {
    status: Container<(PathBuf, Status)>,
    head_branch: String,
    branch: Container<Branch>,
    remote: Container<Remote>,
    //remote_activity: Container<RemoteActivity>,
    tag: Container<String>,
    commits: Container<Oid>,
    view: Container<RootView>,
    mode: RootMode,
}

pub struct Branch {
    pub name: String,
    pub anal: MergeAnalysis,
    pub ahead: usize,
    pub behind: usize,
}

impl Branch {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            anal: MergeAnalysis::empty(),
            ahead: 0,
            behind: 0,
        }
    }
}

pub struct Remote {
    name: String,
    activity: RemoteActivity, //error: Option<String>,
}

pub enum RemoteActivity {
    None,
    Push(GitPush),
    Pull,
}

impl Remote {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            activity: RemoteActivity::None,
        }
    }
}

#[derive(PartialEq, Debug, Serialize, Deserialize)]
pub enum RootMode {
    Dashboard,
    CommitInput,
    TagInput { oid: usize },
}

enum RootView {
    LineBreak,
    HeadFile,
    HeadLabel,
    BranchMember(usize),
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
            status: Container::new(),
            head_branch: "".into(),
            branch: Container::new(),
            remote: Container::new(),
            //remote_activity: Container::new(),
            tag: Container::new(),
            commits: Container::new(),
            view: Container::new(),
            mode: RootMode::Dashboard,
        }
    }

    pub fn format(&self) -> String {
        let mut format = String::new();
        self.view
            .inner()
            .iter()
            .for_each(|root_view| match root_view {
                RootView::LineBreak | RootView::HeadFile => format.push_str("\n"),
                RootView::HeadLabel => {
                    format.push_str("# Head\n");
                }
                RootView::BranchMember(member) => {
                    format.push_str(&format!("{}\n", &self.branch[member].name));
                }
                RootView::RemoteLabel => {
                    format.push_str("# Remote\n");
                }
                RootView::RemoteMember(i) => {
                    let member = &self.remote[*i];
                    format.push_str(&member.name);
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
                    format.push_str(&member.to_string()[0..HASH_LENGTH]);
                    format.push_str("\n");
                }
            });
        format
    }

    pub fn build_view(&mut self) {
        self.view.inner_mut().clear();
        self.view.push(RootView::HeadFile);

        self.view.push(RootView::HeadLabel);
        self.branch.inner().iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::BranchMember(i));
        });

        self.view.push(RootView::RemoteLabel);
        self.remote.inner().iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::RemoteMember(i));
        });
        self.view.push(RootView::TagLabel);
        self.tag.inner().iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::TagMember(i));
        });
        self.view.push(RootView::LineBreak);
        self.view.push(RootView::StatusLabel);
        self.status.inner().iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::StatusMember(i));
        });
        self.view.push(RootView::LineBreak);
        self.view.push(RootView::CommitLabel);
        self.commits.inner().iter().enumerate().for_each(|(i, _)| {
            self.view.push(RootView::CommitMember(i));
        });
    }

    pub fn fill_status(&mut self, office: &Office) {
        self.status.inner_mut().clear();
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
    }

    pub fn fill_branch(&mut self, office: &Office) {
        self.branch.inner_mut().clear();
        let head_commit = office.repo.head().unwrap().peel_to_commit().unwrap();
        office
            .repo
            .branches(Some(BranchType::Local))
            .map(|branches| {
                branches.for_each(|branch| {
                    branch.map(|(branch, _)| {
                        let name = branch.name().unwrap_or_default().unwrap_or_default();
                        if name == self.head_branch {
                            return;
                        }
                        let branch_ref = branch.get();
                        let mut b = Branch::new(name);
                        let annotated = office
                            .repo
                            .reference_to_annotated_commit(branch_ref)
                            .unwrap();
                        let (anal, _) = office.repo.merge_analysis(&[&annotated]).unwrap();
                        b.anal = anal;
                        office
                            .repo
                            .graph_ahead_behind(
                                branch_ref.peel_to_commit().unwrap().id(),
                                head_commit.id(),
                            )
                            .map(|(ahead, behind)| {
                                b.ahead = ahead;
                                b.behind = behind;
                            });
                        self.branch.push(b);
                    });
                });
            });
    }

    pub fn fill_root(&mut self, office: &mut Office) {
        let head = office.repo.head().unwrap();

        self.head_branch = head.shorthand().unwrap().to_string();
        self.remote.inner_mut().clear();

        self.fill_branch(office);

        office.repo.remotes().map(|remotes| {
            for name in remotes.iter().flatten() {
                if let Some(name) = name {
                    self.remote.push(Remote::new(name));
                }
            }
        });

        self.fill_status(office);

        self.tag.inner_mut().clear();
        office.repo.tag_names(None).map(|tags| {
            for tag in tags.iter().flatten() {
                if let Some(tag) = tag {
                    self.tag.push(tag.into());
                }
            }
        });

        self.commits.inner_mut().clear();
        office.repo.revwalk().map(|mut revwalk| {
            revwalk.push(head.target().unwrap());
            for oid in revwalk.take(10) {
                if let Ok(oid) = oid {
                    self.commits.push(oid);
                }
            }
        });
    }

    pub fn refresh(&self, url: Uri, lsp_conn: &Sender<Kind<Message, MaAction>>) {
        lsp_conn.send(Kind::Other(MaAction::FillContent {
            url,
            content: self.format(),
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(1000, 0),
            },
        }));
    }
}

impl Root {
    fn goto_definition(&self, toolbox: &ToolBox, params: TextDocumentPositionParams) {
        match &self.view[params.position.line as usize] {
            RootView::LineBreak => (),
            RootView::HeadFile => (),
            RootView::HeadLabel => (),
            RootView::RemoteLabel => (),
            RootView::RemoteMember(_) => (),
            RootView::TagLabel => (),
            RootView::BranchMember(_) => (),
            RootView::TagMember(member) => {
                let tag = &self.tag[member];
                let reference = match toolbox
                    .office
                    .repo
                    .find_reference(&format!("refs/tags/{tag}"))
                {
                    Ok(reference) => reference,
                    Err(_) => return,
                };
                let object = reference.peel(git2::ObjectType::Any).unwrap();
                match object.kind().unwrap() {
                    git2::ObjectType::Commit => {
                        let oid = object.id();
                        toolbox.lsp_conn.send(Kind::Other(MaAction::OpenDiff {
                            uri: params.text_document.uri,
                            oid,
                        }));
                    }
                    git2::ObjectType::Tree => {
                        //toolbox.lsp_conn.send(Kind::Other(MaAction::OpenTree { uri: params.text_document.uri, tree_id: object.id() }));
                    }
                    _ => (),
                }
            }
            RootView::StatusLabel => {
                let index = match toolbox.office.repo.index() {
                    Ok(index) => {
                        if index.is_empty() {
                            return;
                        }
                        index
                    },
                    Err(_) => return,
                };
                let commit_head = match toolbox.office.repo.head() {
                    Ok(head) => match head.peel_to_commit() {
                        Ok(commit) => commit,
                        Err(_) => return,
                    },
                    Err(_) => return,
                };
                let tree = commit_head.tree().ok();
                let diff = match toolbox
                    .office
                    .repo
                    .diff_tree_to_index(tree.as_ref(), Some(&index), None)
                {
                    Ok(diff) => diff,
                    Err(_) => return,
                };
                if diff.deltas().len() == 0 {
                    return;
                }
                let mut diff_index = DiffIndex::new();
                let mut format = String::new();
                let path = toolbox
                    .office
                    .repo
                    .path()
                    .join(App::NAME)
                    .join(&self.head_branch)
                    .join("index.diff");
                Diff::fill_view(
                    &diff,
                    &mut diff_index.diff_file,
                    &mut diff_index.view,
                    &mut format,
                );
                if let Err(e) = fs::create_dir_all(path.parent().unwrap()) {
                    eprintln!("{}", e.to_string());
                    return;
                }
                if let Err(e) = fs::write(&path, &format) {
                    eprintln!("{}", e.to_string());
                    return;
                }
                let workgroup = WorkGroup::DiffIndex(diff_index);
                let uri = name_to_url(&path).unwrap();
                toolbox
                    .lsp_conn
                    .send(Kind::Other(MaAction::RegisterWorkgroup {
                        key: uri.clone(),
                        workgroup,
                        office_id: toolbox.office.o_id,
                    }));
                toolbox.lsp_conn.send(Kind::Main(
                    lsp_server::Request {
                        id: RequestId::from(1),
                        method: ShowDocument::METHOD.into(),
                        params: to_value(ShowDocumentParams {
                            uri,
                            external: None,
                            take_focus: Some(true),
                            selection: None,
                        })
                        .unwrap(),
                    }
                    .into(),
                ));
            }
            RootView::StatusMember(_) => (),
            RootView::CommitLabel => (),
            RootView::CommitMember(member) => {
                toolbox.lsp_conn.send(Kind::Other(MaAction::OpenDiff {
                    uri: params.text_document.uri,
                    oid: self.commits[member],
                }));
            }
        }
    }

    fn inlay_hint(
        &self,
        (start_line, end_line): (u32, u32),
        office: &Office,
        ret: &mut Vec<InlayHint>,
    ) {
        //eprintln!("hint!");
        match &self.mode {
            RootMode::CommitInput => {
                //eprintln!("it's impossible to be here!");
                ret.push(create_inlay_hint(0, 0, "Waiting for commit message"));
            }
            RootMode::Dashboard => {
                for i in start_line..end_line {
                    //eprintln!("{i}");
                    self.view.get(i as usize).map(|view| match view {
                        RootView::LineBreak => (),
                        RootView::HeadFile => {
                            ret.push(create_inlay_hint(
                                i,
                                0,
                                office.repo.workdir().unwrap().to_string_lossy(),
                            ));
                        }
                        RootView::HeadLabel => {
                            ret.push(create_inlay_hint(i, 7, format!(" {}", &self.head_branch)));
                        }
                        RootView::BranchMember(member) => {
                            let branch = &self.branch[member];
                            ret.push(create_inlay_hint(i, 0, "  "));
                            ret.push(create_inlay_hint(
                                i,
                                branch.name.len() as u32,
                                format!(" * {}, {}", branch.ahead, branch.behind),
                            ));
                            //let head = office.repo.head().unwrap().peel_to_commit().unwrap();
                        }
                        RootView::RemoteLabel => (),
                        RootView::RemoteMember(member) => {
                            let remote = &self.remote[member];
                            let res = office.repo.find_remote(&remote.name).unwrap();
                            ret.push(create_inlay_hint(i, 0, "  "));
                            let content = format!(" * {}", res.url().unwrap());

                            ret.push(create_inlay_hint(i, remote.name.len() as u32, &content));
                            match &remote.activity {
                                RemoteActivity::None => (),
                                RemoteActivity::Push(git_push) => match git_push {
                                    GitPush::Packing { percentage, stage } => {
                                        let stage = match stage {
                                            PackBuilderStage::AddingObjects => {
                                                format!("Adding objects: {percentage}%")
                                            }
                                            PackBuilderStage::Deltafication => {
                                                format!("Delta compression: {percentage}%")
                                            }
                                        };
                                        ret.push(create_inlay_hint(
                                            i,
                                            content.len() as u32,
                                            format!("* {stage}"),
                                        ));
                                    }
                                    GitPush::Transfer { percentage } => {
                                        ret.push(create_inlay_hint(
                                            i,
                                            content.len() as u32,
                                            format!("* Transferring data: {percentage}%"),
                                        ));
                                    }
                                    GitPush::Error { message } => {
                                        ret.push(create_inlay_hint(
                                            i,
                                            content.len() as u32,
                                            format!("* Error: {message}"),
                                        ));
                                    }
                                    GitPush::Done => {
                                        ret.push(create_inlay_hint(
                                            i,
                                            content.len() as u32,
                                            " * Done!",
                                        ));
                                    }
                                },
                                RemoteActivity::Pull => (),
                            }
                        }
                        RootView::TagLabel => (),
                        RootView::TagMember(member) => {
                            let tag = &self.tag[member];
                            let find = office
                                .repo
                                .find_tag(
                                    office
                                        .repo
                                        .refname_to_id(&format!("refs/tags/{tag}"))
                                        .unwrap(),
                                )
                                .unwrap();
                            ret.push(create_inlay_hint(i, 0, "  "));
                            if let Ok(Some(message)) = find.message() {
                                let content = format!(" * {}", message.trim_end());
                                ret.push(create_inlay_hint(i, tag.len() as u32, content));
                            }
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
                                format!("  {} | {} -> ", staged, unstaged),
                            ));
                        }
                        RootView::CommitLabel => (),
                        RootView::CommitMember(member) => {
                            let commit = &self.commits[member];
                            let com = office.repo.find_commit(*commit).unwrap();
                            ret.push(create_inlay_hint(i, 0, "  "));
                            let mut line = com.message().unwrap().lines();
                            if let Some(message) = line.next() {
                                ret.push(create_inlay_hint(
                                    i,
                                    HASH_LENGTH as u32,
                                    format!(" * {}", message),
                                ));
                            }
                        }
                    });
                }
            }
            RootMode::TagInput { .. } => {
                ret.push(create_inlay_hint(0, 0, "Tag name: "));
                ret.push(create_inlay_hint(1, 0, "Tag message: "));
            }
        }
    }

    fn semantic_token(&self, ret: &mut Vec<SemanticToken>) {
        let mut current_line_token = 0u32;
        let mut prev_line_token = 0u32;
        self.view
            .inner()
            .iter()
            .enumerate()
            .for_each(|(line, view)| match view {
                RootView::CommitLabel
                | RootView::StatusLabel
                | RootView::LineBreak
                | RootView::HeadFile
                | RootView::HeadLabel
                | RootView::RemoteLabel
                | RootView::TagLabel => (),
                RootView::BranchMember(member) => {
                    let branch = &self.branch[member];
                    current_line_token = line as u32;
                    let delta = current_line_token - prev_line_token;
                    ret.push(create_semantic_token(
                        delta,
                        0u32,
                        branch.name.len() as u32,
                        SemanticTokenList::NAMESPACE,
                    ));
                    prev_line_token = current_line_token;
                }
                RootView::RemoteMember(member) => {
                    let remote = &self.remote[member];
                    current_line_token = line as u32;
                    let delta = current_line_token - prev_line_token;
                    ret.push(create_semantic_token(
                        delta,
                        0u32,
                        remote.name.len() as u32,
                        SemanticTokenList::TYPE,
                    ));
                    prev_line_token = current_line_token;
                }
                RootView::TagMember(member) => {
                    let remote = &self.tag[member];
                    current_line_token = line as u32;
                    let delta = current_line_token - prev_line_token;
                    ret.push(create_semantic_token(
                        delta,
                        0u32,
                        remote.len() as u32,
                        SemanticTokenList::KEYWORD,
                    ));
                    prev_line_token = current_line_token;
                }
                RootView::StatusMember(member) => {
                    let (remote, _) = &self.status[member];
                    current_line_token = line as u32;
                    let delta = current_line_token - prev_line_token;
                    ret.push(create_semantic_token(
                        delta,
                        0u32,
                        remote.to_string_lossy().len() as u32,
                        SemanticTokenList::FUNCTION,
                    ));
                    prev_line_token = current_line_token;
                }
                RootView::CommitMember(member) => {
                    let remote = &self.commits[member];
                    current_line_token = line as u32;
                    let delta = current_line_token - prev_line_token;
                    ret.push(create_semantic_token(
                        delta,
                        0u32,
                        remote.to_string().len() as u32,
                        SemanticTokenList::STRUCT,
                    ));
                    prev_line_token = current_line_token;
                }
            });
    }

    fn code_action(
        &mut self,
        office: &Office,
        params: CodeActionParams,
        ret: &mut CodeActionResponse,
    ) {
        match &self.mode {
            RootMode::Dashboard => match &self.view[params.range.start.line as usize] {
                RootView::LineBreak => (),
                RootView::HeadFile => {
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: "Reload".into(),
                        command: MaAction::COMMAND.into(),
                        arguments: app::Command::Reload {
                            uri: params.text_document.uri,
                        }
                        .into(),
                    }));
                }
                RootView::HeadLabel => (),
                RootView::BranchMember(member) => {
                    let branch = &self.branch[member];
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: format!("Switch branch to {}", &branch.name),
                        command: MaAction::COMMAND.into(),
                        arguments: app::Command::SwitchBranch {
                            uri: params.text_document.uri.clone(),
                            branch: branch.name.clone(),
                        }
                        .into(),
                    }));
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: format!("Merge {} with HEAD", &branch.name),
                        command: MaAction::COMMAND.into(),
                        arguments: app::Command::MergeBranch {
                            uri: params.text_document.uri,
                            branch_id: *member,
                        }
                        .into(),
                    }));
                }
                RootView::RemoteLabel => {
                    let mut vec = vec![];
                    for (i, _) in self.remote.inner().iter().enumerate() {
                        vec.push(
                            app::Command::PushRemote {
                                uri: params.text_document.uri.clone(),
                                remote_id: i,
                                refspec: format!(
                                    "refs/heads/{0}:refs/heads/{0}",
                                    &self.head_branch
                                ),
                            }
                            .to_value(),
                        );
                    }
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: format!("Push to every remote"),
                        command: MaAction::COMMAND.into(),
                        arguments: Some(vec),
                    }));
                }
                RootView::RemoteMember(member) => {
                    let remote = &self.remote[member];
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: format!("Push {} to {}", self.head_branch, &remote.name),
                        command: MaAction::COMMAND.into(),
                        arguments: app::Command::PushRemote {
                            uri: params.text_document.uri.clone(),
                            remote_id: *member,
                            refspec: format!("refs/heads/{0}:refs/heads/{0}", &self.head_branch),
                        }
                        .into(),
                    }));
                    //return;
                }
                RootView::TagLabel => (),
                RootView::TagMember(member) => {
                    let tag = &self.tag[member];
                    for (remote_id, remote) in self.remote.inner().iter().enumerate() {
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: format!("Push tag {} to {}", tag, &remote.name),
                            command: MaAction::COMMAND.into(),
                            arguments: app::Command::PushRemote {
                                uri: params.text_document.uri.clone(),
                                remote_id,
                                refspec: format!("refs/tags/{0}:refs/tags/{0}", tag),
                            }
                            .into(),
                        }));
                    }
                }
                RootView::StatusLabel => {
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: "Stage all".into(),
                        command: MaAction::COMMAND.into(),
                        arguments: app::Command::StageAll {
                            uri: params.text_document.uri.clone(),
                        }
                        .into(),
                    }));
                    if !self.status.inner().is_empty() {
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: "Commit changes".into(),
                            command: MaAction::COMMAND.into(),
                            arguments: app::Command::ChangeRootMode {
                                uri: params.text_document.uri,
                                mode: RootMode::CommitInput,
                            }
                            .into(),
                        }));
                    }
                }
                RootView::StatusMember(path) => {
                    let (file, status) = &self.status[path];
                    let staged = status.intersects(
                        Status::INDEX_NEW
                            | Status::INDEX_MODIFIED
                            | Status::INDEX_DELETED
                            | Status::INDEX_RENAMED
                            | Status::INDEX_TYPECHANGE,
                    );
                    if staged {
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: format!("Restage {}", file.to_string_lossy()),
                            command: MaAction::COMMAND.into(),
                            arguments: app::Command::StageFile {
                                uri: params.text_document.uri.clone(),
                                path: *path,
                            }
                            .into(),
                        }));
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: format!("Unstage {}", file.to_string_lossy()),
                            command: MaAction::COMMAND.into(),
                            arguments: app::Command::UnstageFile {
                                uri: params.text_document.uri,
                                path: *path,
                            }
                            .into(),
                        }));
                    } else {
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: format!("Stage {}", file.to_string_lossy()),
                            command: MaAction::COMMAND.into(),
                            arguments: app::Command::StageFile {
                                uri: params.text_document.uri,
                                path: *path,
                            }
                            .into(),
                        }));
                    }
                }
                RootView::CommitLabel => (),
                RootView::CommitMember(member) => {
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: "Add tag".into(),
                        command: MaAction::COMMAND.into(),
                        arguments: app::Command::ChangeRootMode {
                            uri: params.text_document.uri,
                            mode: RootMode::TagInput { oid: *member },
                        }
                        .into(),
                    }));
                }
            },
            RootMode::CommitInput => {
                ret.push(CodeActionOrCommand::Command(Command {
                    title: "Cancel commit".into(),
                    command: MaAction::COMMAND.into(),
                    arguments: app::Command::ChangeRootMode {
                        uri: params.text_document.uri,
                        mode: RootMode::Dashboard,
                    }
                    .into(),
                }));
            }
            RootMode::TagInput { oid } => (),
        }
    }
}

impl WorkGroup {
    pub fn stage_file(&mut self, toolbox: ToolBox, uri: Uri, path: usize) {
        match self {
            WorkGroup::Root(root) => {
                let (path_buf, status) = &root.status[path];
                let mut index = match toolbox.office.repo.index() {
                    Ok(index) => index,
                    Err(_) => return,
                };
                if status.is_wt_deleted() {
                    index.remove_path(path_buf);
                } else {
                    index.add_path(path_buf);
                }
                index.write();
                root.fill_status(toolbox.office);
                root.build_view();

                root.refresh(uri, toolbox.lsp_conn);
            }
            WorkGroup::Diff(diff) => (),
            _ => (),
        }
    }

    pub fn unstage_file(&mut self, toolbox: ToolBox, uri: Uri, path: usize) {
        match self {
            WorkGroup::Root(root) => {
                let (path_buf, _) = &root.status[path];
                let mut index = match toolbox.office.repo.index() {
                    Ok(index) => index,
                    Err(_) => return,
                };
                index.remove_path(&path_buf);
                index.write();
                root.fill_status(toolbox.office);
                root.build_view();

                root.refresh(uri, toolbox.lsp_conn);
            }
            WorkGroup::Diff(diff) => (),
            _ => (),
        }
    }

    pub fn stage_all(&mut self, toolbox: ToolBox, uri: Uri) {
        match self {
            WorkGroup::Root(root) => {
                //let (path_buf, status) = &root.status[path];
                let mut index = match toolbox.office.repo.index() {
                    Ok(index) => index,
                    Err(_) => return,
                };
                index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None);
                index.write();
                root.fill_status(toolbox.office);
                root.build_view();

                root.refresh(uri, toolbox.lsp_conn);
            }
            WorkGroup::Diff(diff) => todo!(),

            _ => (),
        }
    }

    pub fn switch_branch(&mut self, toolbox: ToolBox, uri: Uri, branch: String) {
        match self {
            WorkGroup::Root(root) => {
                let mut cmd = process::Command::new("git");
                cmd.args(&[
                    "-C",
                    &toolbox.office.repo.workdir().unwrap().to_string_lossy(),
                    "switch",
                    &branch,
                ]);

                let conn = toolbox.lsp_conn.clone();
                let url = uri.clone();
                spawn(move || {
                    let mut git = match cmd.spawn() {
                        Ok(git) => git,
                        Err(_) => return,
                    };
                    match git.wait() {
                        Ok(e) if e.success() => {
                            conn.send(Kind::Other(app::Command::Reload { uri }.into()));
                        }
                        _ => (),
                    }
                });
                // Nasty shit going on here, you need to send this action so the reload happen
                root.refresh(url, toolbox.lsp_conn);
            }
            WorkGroup::Diff(diff) => todo!(),

            _ => (),
        }
    }

    pub fn push_remote(&mut self, toolbox: ToolBox, uri: Uri, remote_id: usize, refspec: String) {
        match self {
            WorkGroup::Root(root) => {
                //let config = toolbox.office.repo.config().unwrap();
                let remote_name = root.remote[remote_id].name.clone();
                let repo_path = toolbox.office.repo.workdir().unwrap().to_path_buf();
                //let head_branch = root.head_branch.clone();
                let sender = toolbox.lsp_conn.clone();

                spawn(move || {
                    let pack_sender = sender;
                    let transfer_sender = pack_sender.clone();
                    let ending_sender = pack_sender.clone();
                    let error_sender = pack_sender.clone();

                    let pack_uri = uri;
                    let transfer_uri = pack_uri.clone();
                    let ending_uri = pack_uri.clone();
                    let error_uri = pack_uri.clone();
                    //let uri = uri.clone();
                    let repo = Repository::open(&repo_path).unwrap();
                    let config = repo.config().unwrap();
                    let mut remote = repo.find_remote(&remote_name).unwrap();
                    let mut callback = RemoteCallbacks::new();
                    callback.credentials(|url, username, allowed| {
                        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
                            return git2::Cred::credential_helper(&config, url, username);
                        }

                        if allowed.contains(git2::CredentialType::SSH_KEY) {
                            return git2::Cred::ssh_key_from_agent(username.unwrap_or("git"));
                        }

                        git2::Cred::default()
                    });
                    callback.pack_progress(move |stage, current, total| {
                        let percentage = if total > 0 { current * 100 / total } else { 0 };
                        //let uri = uri.clone();
                        pack_sender.send(Kind::Other(MaAction::GitPushProgress {
                            uri: pack_uri.clone(),
                            remote_id,
                            progress: GitPush::Packing { percentage, stage },
                        }));
                        //sender.send(msg)
                    });
                    callback.push_transfer_progress(move |current, total, _bytes| {
                        let percentage = if total > 0 { current * 100 / total } else { 0 };
                        transfer_sender.send(Kind::Other(MaAction::GitPushProgress {
                            uri: transfer_uri.clone(),
                            remote_id,
                            progress: GitPush::Transfer { percentage },
                        }));
                    });
                    callback.push_update_reference(move |_name, message| {
                        if let Some(message) = message {
                            error_sender.send(Kind::Other(MaAction::GitPushProgress {
                                uri: error_uri.clone(),
                                remote_id,
                                progress: GitPush::Error {
                                    message: message.into(),
                                },
                            }));
                        }
                        Ok(())
                    });
                    let mut options = PushOptions::new();
                    options.remote_callbacks(callback);
                    //let refspec = format!("refs/heads/{0}:refs/heads/{0}", &head_branch);

                    if let Err(push) = remote.push(&[refspec], Some(&mut options)) {
                        eprintln!("{}", push.to_string());
                    }
                    ending_sender.send(Kind::Other(MaAction::GitPushProgress {
                        uri: ending_uri,
                        remote_id,
                        progress: GitPush::Done,
                    }));
                });
            }
            _ => (),
        }
    }

    pub fn merge_branch(&mut self, toolbox: ToolBox, uri: Uri, branch_id: usize) {
        match self {
            WorkGroup::Root(root) => {
                let mut ours_branch = toolbox
                    .office
                    .repo
                    .find_branch(&root.branch[branch_id].name, BranchType::Local)
                    .unwrap();
                let ours = ours_branch.get().peel_to_commit().unwrap();

                let theirs_branch = toolbox
                    .office
                    .repo
                    .find_branch(&root.head_branch, BranchType::Local)
                    .unwrap();
                let theirs_ref = theirs_branch.get();
                let theirs = theirs_ref.peel_to_commit().unwrap();

                let ancestor_oid = toolbox
                    .office
                    .repo
                    .merge_base(ours.id(), theirs.id())
                    .unwrap();
                let ancestor = toolbox.office.repo.find_commit(ancestor_oid).unwrap();

                let ancestor_tree = ancestor.tree().unwrap();
                let ours_tree = ours.tree().unwrap();
                let theirs_tree = theirs.tree().unwrap();
                //create the index
                let mut index = toolbox
                    .office
                    .repo
                    .merge_trees(&ancestor_tree, &ours_tree, &theirs_tree, None)
                    .unwrap();
                if index.has_conflicts() {
                    //index.conflicts();
                    let url = MergeConflict::build(index, (&ours, &theirs, &ancestor), &toolbox);
                    toolbox.lsp_conn.send(Kind::Main(
                        lsp_server::Request {
                            id: RequestId::from(1),
                            method: ShowDocument::METHOD.into(),
                            params: to_value(ShowDocumentParams {
                                uri: url,
                                external: None,
                                take_focus: Some(true),
                                selection: None,
                            })
                            .unwrap(),
                        }
                        .into(),
                    ));
                    //toolbox.lsp_conn.send(Kind::Main())
                    return;
                }

                //
                let tree_oid = index.write_tree_to(&toolbox.office.repo).unwrap();
                let merged_tree = toolbox.office.repo.find_tree(tree_oid).unwrap();
                let signature = toolbox.office.repo.signature().unwrap();
                let commit_oid = toolbox
                    .office
                    .repo
                    .commit(
                        None,
                        &signature,
                        &signature,
                        &format!(
                            "Merge branch '{}' into '{}'",
                            &root.head_branch, &root.branch[branch_id].name
                        ),
                        &merged_tree,
                        &[&ours, &theirs],
                    )
                    .unwrap();

                //move the ours to new commit
                ours_branch.get_mut().set_target(commit_oid, "Merge branch");
                toolbox.lsp_conn.send(Kind::Other(MaAction::Command {
                    command: app::Command::Reload { uri },
                }));
                //toolbox.office.repo.commit(update_ref, author, committer, message, tree, parents)
            }
            WorkGroup::Diff(diff) => (),

            _ => (),
        }
    }

    pub fn reload(&mut self, toolbox: ToolBox, uri: Uri) {
        match self {
            WorkGroup::Root(root) => {
                root.fill_root(toolbox.office);
                root.build_view();
                root.refresh(uri, toolbox.lsp_conn);
            }
            WorkGroup::Diff(diff) => (),

            _ => (),
        }
    }

    pub fn change_root_mode(&mut self, toolbox: ToolBox, uri: Uri, root_mode: RootMode) {
        match self {
            WorkGroup::Root(root) => {
                root.mode = root_mode;
                match &root.mode {
                    RootMode::Dashboard => {
                        //root.mode = ro
                        toolbox.lsp_conn.send(Kind::Other(MaAction::Command {
                            command: app::Command::Reload { uri },
                        }));
                        return;
                    }
                    RootMode::CommitInput => {
                        toolbox.lsp_conn.send(Kind::Other(MaAction::FillContent {
                            url: uri,
                            content: "\n\n".into(),
                            range: Range::new(Position::new(0, 0), Position::new(1000, 0)),
                        }));
                    }
                    RootMode::TagInput { .. } => {
                        toolbox.lsp_conn.send(Kind::Other(MaAction::FillContent {
                            url: uri,
                            content: "\n\n".into(),
                            range: Range::new(Position::new(0, 0), Position::new(1000, 0)),
                        }));
                    }
                }
            }
            WorkGroup::Diff(diff) => (),

            _ => (),
        }
    }

    pub fn cancel_commit(&mut self, toolbox: ToolBox, uri: Uri) {
        match self {
            WorkGroup::Root(root) => {
                root.mode = RootMode::Dashboard;
                toolbox
                    .lsp_conn
                    .send(Kind::Other(app::Command::Reload { uri }.into()));
            }
            _ => (),
        }
    }

    pub fn git_push_progress(
        &mut self,
        remote_id: usize,
        progress: GitPush,
        lsp_conn: &Sender<Kind<Message, MaAction>>,
    ) {
        match self {
            WorkGroup::Root(root) => {
                let remote = &mut root.remote[remote_id];
                remote.activity = RemoteActivity::Push(progress);
                lsp_conn.send(Kind::Main(
                    lsp_server::Request {
                        id: RequestId::from(1),
                        method: InlayHintRefreshRequest::METHOD.into(),
                        params: to_value(()).unwrap(),
                    }
                    .into(),
                ));
            }
            WorkGroup::Diff(diff) => (),

            _ => (),
        }
    }
}

pub struct Diff {
    pub view: Container<DiffView>,
    pub diff_file: Container<PathBuf>,
    pub diff_dir: PathBuf,
    pub oid: Oid,
    pub parent_oid: Option<Oid>,
}

pub type Length = usize;

pub enum DiffView {
    LineBreak,
    //Padding,
    Author {
        username: Length,
        email: Length,
    },
    Date {
        date: Length,
    },
    MessageLine(Length),
    OldFile(PathBuf),
    //    old: Option<PathBuf>,
    //},
    NewFile {
        from_file: usize,
    },
    HunkHeader {
        from_file: usize,
        length: Length,
    },
    Parent {
        oid: Oid,
    },
    Current {
        oid: Oid,
    },
    HunkLine {
        from_diff_file: usize,
        line: u32,
        //content: Length,
    },
    OldLine {
        content: Length,
    },
}

impl Diff {
    const DATE_LABEL: &str = "Date  : ";
    const AUTHOR_LABEL: &str = "Author: ";
    const MESSAGE_PADDING: &str = "        ";

    pub fn new(office: &Office, oid: Oid) -> Self {
        Self {
            view: Container::new(),
            diff_file: Container::new(),
            diff_dir: Self::build_commit_dir(office, oid),
            oid,
            parent_oid: None,
        }
    }

    fn build_commit_dir(office: &Office, oid: Oid) -> PathBuf {
        office.repo.path().join(App::NAME).join(oid.to_string())
    }

    fn build_old_file(office: &Office, path: &Path, oid: Oid) -> Option<String> {
        let commit = match office.repo.find_commit(oid) {
            Ok(commmit) => commmit,
            Err(_) => return None,
        };
        let tree = commit.tree().unwrap();
        let entry = match tree.get_path(path) {
            Ok(tree_entry) => tree_entry,
            Err(_) => return None,
        };
        if entry.kind() != Some(git2::ObjectType::Blob) {
            return None;
        }
        let blob = match office.repo.find_blob(entry.id()) {
            Ok(blob) => blob,
            Err(_) => return None,
        };
        Some(String::from_utf8_lossy(blob.content()).into())
    }

    fn write_old_file(&self, content: &str, path: &Path) -> PathBuf {
        let location = self.diff_dir.join(path);
        fs::create_dir_all(location.parent().unwrap());
        fs::write(&location, content);
        location
    }

    pub fn build_diff(&mut self, office: &Office, commit: &Commit) -> String {
        let diff_group = self;
        let mut format = String::new();
        //let commit = office.repo.find_commit(oid).unwrap();
        let author_signature = commit.author();
        let author_name = author_signature.name().unwrap_or_default();
        let author_email = author_signature.email().unwrap_or_default();
        let author_date = util::format_git_time(author_signature.when()).unwrap_or_default();

        let author_content = format!("{}{} {}\n", Diff::AUTHOR_LABEL, author_name, author_email);
        format.push_str(&author_content);
        diff_group.view.push(DiffView::Author {
            username: author_name.len(),
            email: author_email.len(),
        });

        let date_content = format!("{}{}\n", Diff::DATE_LABEL, &author_date);
        format.push_str(&date_content);
        diff_group.view.push(DiffView::Date {
            date: author_date.len(),
        });

        format.push_str("\n");
        diff_group.view.push(DiffView::LineBreak);

        commit.message().map(|message| {
            message.lines().for_each(|line| {
                diff_group.view.push(DiffView::MessageLine(line.len()));
                format.push_str(&format!("{}{}\n", Diff::MESSAGE_PADDING, line));
            });
        });
        let current_tree = commit.tree().ok();
        //office.repo.diff_tree_to_index(old_tree, index, opts)
        format.push_str("\n");
        diff_group.view.push(DiffView::LineBreak);

        let diff = match commit.parent_count() {
            0 => {
                office
                    .repo
                    .diff_tree_to_tree(None, current_tree.as_ref(), None)
                    .ok()
                //None
            }
            1 => {
                let parent_oid = commit.parent(0).unwrap().id();
                let parent_tree = office.repo.find_commit(parent_oid).unwrap().tree().ok();
                format.push_str(&format!("- {}\n", parent_oid.to_string()));
                diff_group.view.push(DiffView::Parent { oid: parent_oid });
                diff_group.parent_oid = Some(parent_oid);
                format.push_str(&format!("+ {}\n", commit.id().to_string()));
                diff_group.view.push(DiffView::Current { oid: commit.id() });

                office
                    .repo
                    .diff_tree_to_tree(parent_tree.as_ref(), current_tree.as_ref(), None)
                    .ok()
                //diff_group.view.push(DiffView::Parent { oid: commit. });
            }
            _ => {
                for parent in commit.parents() {
                    format.push_str(&format!("- {}\n", parent.id().to_string()));
                    diff_group.view.push(DiffView::Parent { oid: parent.id() });
                }

                None
            }
        };

        diff.map(|diff| {
            format.push_str("\n");
            diff_group.view.push(DiffView::LineBreak);

            Self::fill_view(
                &diff,
                &mut diff_group.diff_file,
                &mut diff_group.view,
                &mut format,
            );
        });
        format
    }

    pub fn fill_view(
        diff: &git2::Diff,
        diff_file: &mut Container<PathBuf>,
        view: &mut Container<DiffView>,
        format: &mut String,
    ) {
        //diff.map(|diff| {
        let mut file_id = Oid::ZERO_SHA1;
        let mut from_file = 0usize;
        let mut hunk_id = 0u32;
        let mut hunk_line = 0u32;
        diff.print(git2::DiffFormat::Patch, |delta, hunk, line| {
            //eprintln!("{}", String::from_utf8_lossy(line.content()));
            let file = delta.new_file();
            let new_file = file.id() != file_id;
            if new_file {
                file_id = file.id();
                hunk_id = 0;
                let path = file.path().unwrap();
                from_file = diff_file.add(path.into());
                let old_file = match delta.status() {
                    Delta::Added => {
                        view.push(DiffView::LineBreak);
                        "/dev/null"
                    },
                    _ => match delta.old_file().path() {
                        Some(path) => {
                            view.push(DiffView::OldFile(path.to_path_buf()));
                            &path.to_string_lossy()
                        }
                        None => {
                            view.push(DiffView::LineBreak);
                            "/dev/null"
                        }
                    },
                };

                let content = format!("--- {}\n+++ {}\n", old_file, path.to_string_lossy());
                format.push_str(&content);

                view.push(DiffView::NewFile { from_file });
            }

            match hunk {
                Some(hunk) if hunk_id != hunk.new_start() => {
                    hunk_id = hunk.new_start();
                    hunk_line = hunk.new_start() - 1;
                    let header = format!(
                        "@@ -{},{} +{},{} @@\n",
                        hunk.old_start(),
                        hunk.old_lines(),
                        hunk.new_start(),
                        hunk.new_lines()
                    );
                    view.push(DiffView::HunkHeader {
                        from_file,
                        length: header.len(),
                    });
                    format.push_str(&header);
                }
                Some(_) => match line.origin() {
                    ' ' | '+' => {
                        view.push(DiffView::HunkLine {
                            from_diff_file: from_file,
                            line: hunk_line,
                        });
                        format.push_str(&format!(
                            "{} {}",
                            line.origin(),
                            String::from_utf8_lossy(line.content())
                        ));
                        hunk_line += 1;
                    }
                    '-' => {
                        view.push(DiffView::OldLine {
                            content: line.content().len(),
                        });
                        format.push_str(&format!(
                            "{} {}",
                            line.origin(),
                            String::from_utf8_lossy(line.content())
                        ));
                    }
                    _ => {}
                },
                None => (),
            }
            true
        });
        //});
    }

    pub fn write_diff(&self, format: &str) -> PathBuf {
        fs::create_dir_all(&self.diff_dir);
        let location = self.diff_dir.join("commit.diff");
        if let Err(e) = fs::write(&location, &format) {
            eprintln!("{}", e.to_string());
            //return;
        }
        location
    }
}

impl Diff {
    fn inlay_hint(
        &self,
        (start_line, end_line): (u32, u32),
        office: &Office,
        ret: &mut Vec<InlayHint>,
    ) {
        for i in start_line..end_line {
            match &self.view[i as usize] {
                DiffView::LineBreak => (),
                DiffView::Author { username, email } => (),
                DiffView::Date { date } => (),
                DiffView::MessageLine(_) => (),
                DiffView::OldFile(path_buf) => (),
                DiffView::NewFile { from_file } => (),
                DiffView::HunkHeader { from_file, length } => {
                    ret.push(create_inlay_hint(
                        i,
                        *length as u32,
                        format!(" * {}", self.diff_file[from_file].to_string_lossy()),
                    ));
                }
                DiffView::Parent { oid } => (),
                DiffView::Current { oid } => (),
                DiffView::HunkLine {
                    from_diff_file,
                    line,
                } => (),
                DiffView::OldLine { content } => (),
            }
        }
    }

    fn goto_definition(
        &self,
        toolbox: &ToolBox,
        (id, params): (RequestId, TextDocumentPositionParams),
    ) {
        match &self.view[params.position.line as usize] {
            DiffView::LineBreak => (),
            DiffView::Author { username, email } => (),
            DiffView::Date { date } => (),
            DiffView::MessageLine(_) => (),
            DiffView::OldFile(path_buf) => {
                let old_file = match Self::build_old_file(
                    toolbox.office,
                    path_buf,
                    self.parent_oid.unwrap(),
                ) {
                    Some(old_file) => old_file,
                    None => return,
                };
                let location =
                    Self::build_commit_dir(toolbox.office, self.parent_oid.unwrap()).join(path_buf);
                fs::create_dir_all(location.parent().unwrap());
                fs::write(&location, old_file);
                let response = GotoDefinitionResponse::Scalar(Location::new(
                    name_to_url(&location).unwrap(),
                    Range {
                        start: Position::new(0, 0),
                        ..Default::default()
                    },
                ));
                toolbox.lsp_conn.send(Kind::Main(
                    Response {
                        id,
                        response_result: Ok(to_value(response).unwrap()),
                    }
                    .into(),
                ));
            }
            DiffView::NewFile { from_file } => {
                let path = &self.diff_file[from_file];
                let content = match Self::build_old_file(toolbox.office, path, self.oid) {
                    Some(old_file) => old_file,
                    None => return,
                };
                let location = Self::build_commit_dir(toolbox.office, self.oid).join(path);
                fs::create_dir_all(location.parent().unwrap());
                fs::write(&location, content);
                let response = GotoDefinitionResponse::Scalar(Location::new(
                    name_to_url(&location).unwrap(),
                    Range {
                        start: Position::new(0, 0),
                        ..Default::default()
                    },
                ));
                toolbox.lsp_conn.send(Kind::Main(
                    Response {
                        id,
                        response_result: Ok(to_value(response).unwrap()),
                    }
                    .into(),
                ));
            }
            DiffView::HunkHeader { from_file, length } => (),
            DiffView::Parent { oid } => {
                toolbox.lsp_conn.send(Kind::Other(MaAction::OpenDiff {
                    uri: params.text_document.uri,
                    oid: *oid,
                }));
            }
            DiffView::Current { oid } => (),
            DiffView::HunkLine {
                from_diff_file,
                line,
            } => {
                let path = &self.diff_file[from_diff_file];
                let content = match Self::build_old_file(toolbox.office, path, self.oid) {
                    Some(content) => content,
                    None => return,
                };
                let location = self.write_old_file(&content, path);
                let response = GotoDefinitionResponse::Scalar(Location::new(
                    name_to_url(&location).unwrap(),
                    Range {
                        start: Position::new(*line, params.position.character),
                        ..Default::default()
                    },
                ));
                toolbox.lsp_conn.send(Kind::Main(
                    Response {
                        id,
                        response_result: Ok(to_value(response).unwrap()),
                    }
                    .into(),
                ));
            }
            DiffView::OldLine { content } => (),
        }
    }
}

pub struct MergeConflict {
    view: Container<ConflictView>,
    format: String,
}

impl MergeConflict {
    fn new() -> Self {
        Self {
            view: Container::new(),
            format: String::new(),
        }
    }

    fn build(
        index: Index,
        (ours, theirs, ancestor_commit): (&Commit, &Commit, &Commit),
        toolbox: &ToolBox,
    ) -> Uri {
        let mut mc = Self::new();
        mc.view.push(ConflictView::CommitOid {
            oid: ancestor_commit.id(),
        });
        mc.format
            .push_str(&format!("^ {}\n", ancestor_commit.id().to_string()));

        mc.view.push(ConflictView::CommitOid { oid: ours.id() });
        mc.format
            .push_str(&format!("< {}\n", ours.id().to_string()));

        mc.view.push(ConflictView::CommitOid { oid: theirs.id() });
        mc.format
            .push_str(&format!("> {}\n", theirs.id().to_string()));
        //index.path()
        for conf in index.conflicts().unwrap() {
            let conflict = match conf {
                Ok(conf) => conf,
                Err(_) => continue,
            };

            mc.view.push(ConflictView::LineBreak);
            mc.format.push_str("\n");

            match (conflict.ancestor, conflict.our, conflict.their) {
                (Some(ancestor), Some(our), Some(their)) => {
                    let anc_path =
                        PathBuf::from(String::from_utf8_lossy(&ancestor.path).to_string());
                    mc.format
                        .push_str(&format!("^ {}\n", anc_path.to_string_lossy()));
                    mc.view.push(ConflictView::ConflictHeader {
                        oid: ancestor_commit.id(),
                        path: anc_path,
                    });
                    let merge_result = toolbox
                        .office
                        .repo
                        .merge_file_from_index(&ancestor, &our, &their, None)
                        .unwrap();
                    for line in merge_result.content().split(|&b| b == b'\n') {
                        match line {
                            _ if line.starts_with(b"<<<<<<<") => {
                                let path =
                                    PathBuf::from(String::from_utf8_lossy(&our.path).to_string());
                                mc.format
                                    .push_str(&format!("<<<<<<< {}\n", path.to_string_lossy()));
                                mc.view.push(ConflictView::ConflictHeader {
                                    oid: ours.id(),
                                    path,
                                });
                            }
                            _ if line.starts_with(b">>>>>>>") => {
                                let path =
                                    PathBuf::from(String::from_utf8_lossy(&our.path).to_string());
                                mc.format
                                    .push_str(&format!(">>>>>>> {}\n", path.to_string_lossy()));
                                mc.view.push(ConflictView::ConflictHeader {
                                    oid: ours.id(),
                                    path,
                                });
                            }
                            _ => {
                                mc.view.push(ConflictView::ConflictLine);
                                mc.format
                                    .push_str(&format!("{}\n", String::from_utf8_lossy(line)));
                            }
                        }
                    }
                }
                (Some(ancestor), Some(our), None) => {
                    let anc_path =
                        PathBuf::from(String::from_utf8_lossy(&ancestor.path).to_string());
                    mc.format
                        .push_str(&format!("^ {}\n", anc_path.to_string_lossy()));
                    mc.view.push(ConflictView::ConflictHeader {
                        oid: ancestor_commit.id(),
                        path: anc_path,
                    });

                    let our_path = PathBuf::from(String::from_utf8_lossy(&our.path).to_string());
                    mc.format
                        .push_str(&format!("< {}\n", our_path.to_string_lossy()));
                    mc.view.push(ConflictView::ConflictHeader {
                        oid: ours.id(),
                        path: our_path,
                    });

                    mc.view.push(ConflictView::MissingFile);
                    mc.format.push_str("> ?\n");
                }
                (Some(ancestor), None, Some(their)) => {
                    let anc_path =
                        PathBuf::from(String::from_utf8_lossy(&ancestor.path).to_string());
                    mc.format
                        .push_str(&format!("^ {}\n", anc_path.to_string_lossy()));
                    mc.view.push(ConflictView::ConflictHeader {
                        oid: ancestor_commit.id(),
                        path: anc_path,
                    });

                    mc.view.push(ConflictView::MissingFile);
                    mc.format.push_str("< ?\n");

                    let their_path =
                        PathBuf::from(String::from_utf8_lossy(&their.path).to_string());
                    mc.format
                        .push_str(&format!("> {}\n", their_path.to_string_lossy()));
                    mc.view.push(ConflictView::ConflictHeader {
                        oid: theirs.id(),
                        path: their_path,
                    });
                }
                (None, Some(our), Some(their)) => {
                    mc.view.push(ConflictView::MissingFile);
                    mc.format.push_str("^ ?\n");

                    let our_path = PathBuf::from(String::from_utf8_lossy(&our.path).to_string());
                    mc.format
                        .push_str(&format!("< {}\n", our_path.to_string_lossy()));
                    mc.view.push(ConflictView::ConflictHeader {
                        oid: ours.id(),
                        path: our_path,
                    });

                    let their_path =
                        PathBuf::from(String::from_utf8_lossy(&their.path).to_string());
                    mc.format
                        .push_str(&format!("> {}\n", their_path.to_string_lossy()));
                    mc.view.push(ConflictView::ConflictHeader {
                        oid: theirs.id(),
                        path: their_path,
                    });
                }
                _ => (),
            }
        }

        let path = toolbox
            .office
            .repo
            .path()
            .to_path_buf()
            .join(App::NAME)
            .join(ours.id().to_string())
            .join("conflict.diff");
        fs::create_dir_all(&path.parent().unwrap());
        //let path =
        fs::write(&path, &mc.format);
        let url = name_to_url(&path).unwrap();
        toolbox
            .lsp_conn
            .send(Kind::Other(MaAction::RegisterWorkgroup {
                key: url.clone(),
                workgroup: WorkGroup::MergeConflict(mc),
                office_id: toolbox.office.o_id,
            }));
        url
    }
}

pub enum ConflictView {
    LineBreak,
    ConflictLine,
    ConflictHeader { oid: Oid, path: PathBuf },
    CommitOid { oid: Oid },
    MissingFile,
}

pub struct DiffIndex {
    pub view: Container<DiffView>,
    pub diff_file: Container<PathBuf>,
    pub parent_oid: Option<Oid>
}

impl DiffIndex {
    pub fn new() -> Self {
        Self {
            view: Container::new(),
            diff_file: Container::new(),
            parent_oid: None,
        }
    }
}

impl DiffIndex {
    fn inlay_hint(
        &self,
        (start_line, end_line): (u32, u32),
        office: &Office,
        ret: &mut Vec<InlayHint>,
    ) {
        for i in start_line..end_line {
            match &self.view[i as usize] {
                DiffView::LineBreak => (),
                DiffView::Author { username, email } => (),
                DiffView::Date { date } => (),
                DiffView::MessageLine(_) => (),
                DiffView::OldFile(path_buf) => (),
                DiffView::NewFile { from_file } => (),
                DiffView::HunkHeader { from_file, length } => {
                    ret.push(create_inlay_hint(
                        i,
                        *length as u32,
                        format!(" * {}", self.diff_file[from_file].to_string_lossy()),
                    ));
                }
                DiffView::Parent { oid } => (),
                DiffView::Current { oid } => (),
                DiffView::HunkLine {
                    from_diff_file,
                    line,
                } => (),
                DiffView::OldLine { content } => (),
            }
        }
    }

    fn goto_definition(
        &self,
        toolbox: &ToolBox,
        (id, params): (RequestId, TextDocumentPositionParams),
    ) {
        match &self.view[params.position.line as usize] {
            DiffView::LineBreak => (),
            DiffView::Author { username, email } => (),
            DiffView::Date { date } => (),
            DiffView::MessageLine(_) => (),
            DiffView::OldFile(path_buf) => {
                let old_file = match Diff::build_old_file(
                    toolbox.office,
                    path_buf,
                    self.parent_oid.unwrap(),
                ) {
                    Some(old_file) => old_file,
                    None => return,
                };
                let location =
                    Diff::build_commit_dir(toolbox.office, self.parent_oid.unwrap()).join(path_buf);
                fs::create_dir_all(location.parent().unwrap());
                fs::write(&location, old_file);
                let response = GotoDefinitionResponse::Scalar(Location::new(
                    name_to_url(&location).unwrap(),
                    Range {
                        start: Position::new(0, 0),
                        ..Default::default()
                    },
                ));
                toolbox.lsp_conn.send(Kind::Main(
                    Response {
                        id,
                        response_result: Ok(to_value(response).unwrap()),
                    }
                    .into(),
                ));
            }
            DiffView::NewFile { from_file } => {
                let response = GotoDefinitionResponse::Scalar(Location::new(
                    name_to_url(&self.diff_file[from_file]).unwrap(),
                    Range {
                        start: Position::new(0, 0),
                        ..Default::default()
                    },
                ));
                toolbox.lsp_conn.send(Kind::Main(
                    Response {
                        id,
                        response_result: Ok(to_value(response).unwrap()),
                    }
                    .into(),
                ));
            }
            DiffView::HunkHeader { from_file, length } => (),
            DiffView::Parent { oid } => {
                toolbox.lsp_conn.send(Kind::Other(MaAction::OpenDiff {
                    uri: params.text_document.uri,
                    oid: *oid,
                }));
            }
            DiffView::Current { oid } => (),
            DiffView::HunkLine {
                from_diff_file,
                line,
            } => {
                let response = GotoDefinitionResponse::Scalar(Location::new(
                    name_to_url(&self.diff_file[from_diff_file]).unwrap(),
                    Range {
                        start: Position::new(*line, 0),
                        ..Default::default()
                    },
                ));
                toolbox.lsp_conn.send(Kind::Main(
                    Response {
                        id,
                        response_result: Ok(to_value(response).unwrap()),
                    }
                    .into(),
                ));
            }
            DiffView::OldLine { content } => (),
        }
    }
}
