use std::{path::PathBuf, process, thread::spawn};

use crossbeam::channel::Sender;
use git2::{BranchType, IndexAddOption, Oid, Status, StatusOptions};
use lsp_server::{Message, RequestId, Response};
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CodeActionResponse, Command, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, HoverParams, InlayHint, InlayHintParams,
    Position, Range, SemanticToken, SemanticTokens, SemanticTokensParams, SemanticTokensResult,
    Uri,
};
use serde_json::to_value;

use crate::{
    app::{MaAction, Office, SemanticTokenList},
    lsp::Kind,
    util::{Container, create_inlay_hint, create_semantic_token},
};

pub struct ToolBox<'ma> {
    pub office: &'ma mut Office,
    pub lsp_conn: &'ma Sender<Kind<Message, MaAction>>,
    //pub ma_conn: &'ma Sender<MaAction>,
}

const HASH_LENGTH: usize = 7;

pub enum WorkGroup {
    Root(Root),
    //Diff(Diff),
}

pub struct Root {
    status: Container<(PathBuf, Status)>,
    head_branch: String,
    remote: Container<String>,
    tag: Container<String>,
    commits: Container<Oid>,
    view: Container<RootView>,
    mode: RootMode,
}

#[derive(PartialEq, Debug)]
pub enum RootMode {
    Dashboard,
    Input { message: String },
}

enum RootView {
    LineBreak,
    HeadFile,
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
            status: Container::new(),
            head_branch: "".into(),
            remote: Container::new(),
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

    pub fn fill_root(&mut self, office: &mut Office) {
        let head = office.repo.head().unwrap();

        self.head_branch = head.shorthand().unwrap().to_string();
        self.remote.inner_mut().clear();
        office.repo.remotes().map(|remotes| {
            for name in remotes.iter().flatten() {
                if let Some(name) = name {
                    self.remote.push(name.into());
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
    fn inlay_hint(
        &self,
        (start_line, end_line): (u32, u32),
        office: &Office,
        ret: &mut Vec<InlayHint>,
    ) {
        match &self.mode {
            RootMode::Input { .. } => {
                ret.push(create_inlay_hint(0, 0, "Waiting for commit message"));
            },
            RootMode::Dashboard => {
                for i in start_line..end_line {
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
                        RootView::RemoteLabel => (),
                        RootView::RemoteMember(member) => {
                            let remote = &self.remote[member];
                            let res = office.repo.find_remote(remote).unwrap();
                            ret.push(create_inlay_hint(i, 0, "  "));
                            ret.push(create_inlay_hint(
                                i,
                                remote.len() as u32,
                                format!(" * {}", res.url().unwrap()),
                            ));
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
                RootView::RemoteMember(member) => {
                    let remote = &self.remote[member];
                    current_line_token = line as u32;
                    let delta = current_line_token - prev_line_token;
                    ret.push(create_semantic_token(
                        delta,
                        0u32,
                        remote.len() as u32,
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
                RootView::HeadFile => (),
                RootView::HeadLabel => {
                    let branch = match office.repo.branches(Some(BranchType::Local)) {
                        Ok(branch) => branch,
                        Err(_) => return,
                    };
                    for b in branch {
                        match b {
                            Ok((branch, _)) => {
                                if let Ok(Some(name)) = branch.name() {
                                    if name == self.head_branch {
                                        continue;
                                    }
                                    ret.push(CodeActionOrCommand::Command(Command {
                                        title: format!("Switch branch to {}", name),
                                        command: MaAction::ACTION.into(),
                                        arguments: MaAction::SwitchBranch {
                                            uri: params.text_document.uri.clone(),
                                            branch: name.into(),
                                        }
                                        .into(),
                                    }));
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
                RootView::RemoteLabel => (),
                RootView::RemoteMember(_) => (),
                RootView::TagLabel => (),
                RootView::TagMember(_) => (),
                RootView::StatusLabel => {
                    ret.push(CodeActionOrCommand::Command(Command {
                        title: "Stage all".into(),
                        command: MaAction::ACTION.into(),
                        arguments: MaAction::StageAll {
                            uri: params.text_document.uri.clone(),
                        }
                        .into(),
                    }));
                    if !self.status.inner().is_empty() {
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: "Commit changes".into(),
                            command: MaAction::ACTION.into(),
                            arguments: MaAction::AttemptCommit {
                                uri: params.text_document.uri,
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
                            command: MaAction::ACTION.into(),
                            arguments: MaAction::StageFile {
                                uri: params.text_document.uri.clone(),
                                path: *path,
                            }
                            .into(),
                        }));
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: format!("Unstage {}", file.to_string_lossy()),
                            command: MaAction::ACTION.into(),
                            arguments: MaAction::UnstageFile {
                                uri: params.text_document.uri,
                                path: *path,
                            }
                            .into(),
                        }));
                    } else {
                        ret.push(CodeActionOrCommand::Command(Command {
                            title: format!("Stage {}", file.to_string_lossy()),
                            command: MaAction::ACTION.into(),
                            arguments: MaAction::StageFile {
                                uri: params.text_document.uri,
                                path: *path,
                            }
                            .into(),
                        }));
                    }
                }
                RootView::CommitLabel => (),
                RootView::CommitMember(_) => (),
            },
            RootMode::Input { .. } => {
                //ret.push(CodeActionOrCommand::Command(Command {
                //    title: "Commit".into(),
                //    command: MaAction::ACTION.into(),
                //    arguments: MaAction::AttemptCommit {
                //        uri: params.text_document.uri.clone(),
                //    }
                //    .into(),
                //}));

                ret.push(CodeActionOrCommand::Command(Command {
                    title: "Cancel commit".into(),
                    command: MaAction::ACTION.into(),
                    arguments: MaAction::CancelCommit {
                        uri: params.text_document.uri,
                    }
                    .into(),
                }));
            }
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

    pub fn code_action(&mut self, toolbox: ToolBox, id: RequestId, params: CodeActionParams) {
        let mut ret = CodeActionResponse::new();
        match self {
            WorkGroup::Root(root) => {
                root.code_action(toolbox.office, params, &mut ret);
            }
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
                root.inlay_hint(
                    (params.range.start.line, params.range.end.line),
                    toolbox.office,
                    &mut ret,
                );
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
                            conn.send(Kind::Other(MaAction::Reload { uri }));
                        }
                        _ => (),
                    }
                });
                // Nasty shit going on here, you need to send this action so the reload happen
                root.refresh(url, toolbox.lsp_conn);
            }
        }
    }

    pub fn reload(&mut self, toolbox: ToolBox, uri: Uri) {
        match self {
            WorkGroup::Root(root) => {
                root.fill_root(toolbox.office);
                root.build_view();
                root.refresh(uri, toolbox.lsp_conn);
            }
        }
    }

    pub fn attempt_commit(&mut self, toolbox: ToolBox, uri: Uri) {
        match self {
            WorkGroup::Root(root) => match &root.mode {
                RootMode::Dashboard => {
                    root.mode = RootMode::Input {
                        message: String::new(),
                        
                    };
                    toolbox.lsp_conn.send(Kind::Other(MaAction::FillContent {
                        url: uri,
                        content: "\n\n".into(),
                        range: Range::new(Position::new(0, 0), Position::new(1000, 0)),
                    }));
                }
                RootMode::Input { message, .. } => {
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
                                message.trim_start(),
                                &tree,
                                &[&parent],
                            )
                        }
                        None => toolbox.office.repo.commit(
                            Some("HEAD"),
                            &author,
                            &committer,
                            message.as_ref(),
                            &tree,
                            &[],
                        ),
                    };
                    if let Ok(_) = res {
                        root.mode = RootMode::Dashboard;
                        toolbox.lsp_conn.send(Kind::Other(MaAction::Reload { uri }));
                    }
                }
            },
        }
    }

    pub fn cancel_commit(&mut self, toolbox: ToolBox, uri: Uri) {
        match self {
            WorkGroup::Root(root) => {
                root.mode = RootMode::Dashboard;
                toolbox.lsp_conn.send(Kind::Other(MaAction::Reload { uri }));
            },
        }
    }

    pub fn open_diff(&mut self, oid_id: usize) -> Oid {
        match self {
            WorkGroup::Root(root) => root.commits[oid_id],
        }
    }
}

impl WorkGroup {
    pub fn did_save(&mut self, toolbox: ToolBox, params: DidSaveTextDocumentParams) {
        match self {
            WorkGroup::Root(root) => {
                match &mut root.mode {
                    RootMode::Dashboard => (),
                    RootMode::Input { message } => {
                        if let Some(text) = params.text {
                            *message = text;
                            toolbox.lsp_conn.send(Kind::Other(MaAction::AttemptCommit { uri: params.text_document.uri }));
                        }
                    },
                }
            },
        }
    }

    pub fn did_open(&mut self, toolbox: ToolBox, params: DidOpenTextDocumentParams) {}
}

pub struct Diff {
    pub view: Container<DiffView>,
    //pub diff_file: Container<PathBuf>
}

impl Diff {
    pub fn new() -> Self {
        Self { view: Container::new() }
    }
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
    MessageLine(String),
    DiffFile { old: PathBuf, new: PathBuf }, 
    HunkHeader {
        from_file: usize,
        old: Position,
        new: Position,
    },
    Parent {
        oid: Oid
    },
    Current {
        oid: Oid
    },
    HunkLine {
        from_diff_file: usize,
        position: u32,
        //content: Length,
    },
    OldLine {
        content: Length
    }
}
