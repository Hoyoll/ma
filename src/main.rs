trait Lsp {
    fn log(message: impl Into<String>, conn: &Connection) {
        let l_params = LogMessageParams {
            typ: MessageType::INFO,
            message: message.into(),
        };

        conn.sender
            .send(lsp_server::Message::Notification(
                lsp_server::Notification {
                    method: LogMessage::METHOD.to_string(),
                    params: serde_json::to_value(l_params).unwrap(),
                },
            ))
            .unwrap();
    }

    fn ok(conn: &Connection, id: RequestId, result: &impl Serialize) {
        let resp = Response {
            id,
            response_result: Ok(serde_json::to_value(result).unwrap()),
        };
        conn.sender.send(Message::Response(resp));
    }

    fn req(conn: &Connection, method: impl Into<String>, id: RequestId, result: &impl Serialize) {
        //let req = Request {}
        conn.sender.send(Message::Request(lsp_server::Request::new(
            id,
            method.into(),
            result,
        )));
    }

    fn err(conn: &Connection, id: RequestId, code: lsp_server::ErrorCode, msg: &str) {
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

    fn initialize(&mut self, conn: &Connection, id: RequestId, params: InitializeParams);

    fn hover(&mut self, conn: &Connection, id: RequestId, params: HoverParams);
    fn goto_definition(&mut self, conn: &Connection, id: RequestId, params: GotoDefinitionParams);
    fn folding(&mut self, conn: &Connection, id: RequestId, params: FoldingRangeParams);
    fn code_action(&mut self, conn: &Connection, id: RequestId, params: CodeActionParams);
    fn inlay_hint(&mut self, conn: &Connection, id: RequestId, params: InlayHintParams);
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
                serde_json::from_value(request.params).map(|params: GotoDefinitionParams| {
                    self.goto_definition(conn, request.id, params);
                });
            }
            FoldingRangeRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: FoldingRangeParams| {
                    self.folding(conn, request.id, params);
                });
            }
            CodeActionRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: CodeActionParams| {
                    self.code_action(conn, request.id, params);
                });
            }
            InlayHintRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: InlayHintParams| {
                    self.inlay_hint(conn, request.id, params);
                });
            }
            _ => {
                Self::err(
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
                serde_json::from_value(notification.params).map(
                    |params: DidOpenTextDocumentParams| {
                        self.did_open(conn, params);
                    },
                );
            }
            DidChangeTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidChangeTextDocumentParams| {
                        self.did_change(conn, params);
                    },
                );
            }
            _ => (),
        }
    }
    fn did_open(&mut self, conn: &Connection, params: DidOpenTextDocumentParams);
    fn did_change(&mut self, conn: &Connection, params: DidChangeTextDocumentParams);
}

use std::{
    collections::HashMap,
    fs,
    hash::Hash,
    path::{Component, Path, PathBuf, Prefix},
    process::Command,
    str::FromStr,
};

use chrono::{DateTime, FixedOffset, TimeZone};
use git2::{BranchType, Commit, DiffFormat, ObjectType, Oid, Repository, Sort, Time};
use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CodeLens, DefinitionOptions,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, FoldingRange, FoldingRangeParams,
    FoldingRangeProviderCapability, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InlayHintParams, Location, LogMessageParams, MarkedString, MarkupContent, MessageType, OneOf,
    Position, Range, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Uri, WorkspaceEdit, lsp_request,
    notification::{
        DidChangeTextDocument, DidOpenTextDocument, LogMessage, Notification, ShowMessage,
    },
    request::{
        ApplyWorkspaceEdit, CodeActionRequest, CodeLensRequest, FoldingRangeRequest,
        GotoDefinition, HoverRequest, Initialize, InlayHintRequest, Request,
    },
};
use serde::{Deserialize, Serialize};

const ROOT_NAME: &str = "ma.md";
const DIFF_NAME: &str = "ma.diff";
const CACHE_DIR: &str = "ma-cache";
struct Client {
    work_group: HashMap<Uri, (WorkGroup, OfficeId)>,
    //repo: Vec<Repository>,
    office: Vec<Office>,
    //caps: ServerCapabilities,
}

impl Client {
    fn new() -> Self {
        Self {
            work_group: HashMap::default(),
            //repo: Vec::default(),
            office: Vec::default(),
            //manifest: HashMap::default(),
            //caps,
        }
    }
}

type OfficeId = usize;
enum WorkGroup {
    RootView(RootView),
    DiffView(Diff),
    FileView,
}
struct Office {
    repo: Repository,
    cache: PathBuf,
    manifest: HashMap<Oid, Uri>,
    file_cache: HashMap<PathBuf, Uri>,
}

enum MergeView {
    Padding,
    Commit(usize),
}

enum NormalView {
    Padding,
    ParentCommit,
    Hunk { from_hunk: usize, change_on: u32 },
    HunkLine { from_hunk: usize, change_on: u32 },
}

struct Hunk {
    path: PathBuf,
    changes: Vec<u32>,
}

struct LineCol {
    line: u32,
    col: u32,
}

/// TO-DO: create a nice model for DiffView. with diff hunk head as "goto_definition" anchor
/// TO-DO done i suppose...
enum DiffView {
    /// if parent commit > 1
    Merge {
        view: Vec<MergeView>,
        parents: Vec<Oid>,
        //hash: Oid,
        //format: String,
    },
    /// if parent == 1
    Normal {
        hunk: Vec<Hunk>,
        view: Vec<NormalView>,
        //hash: Oid,
        parent: Option<Oid>,
        //format: String,
    },
}

impl DiffView {
    fn new(commit: &Commit) -> Self {
        match commit.parent_count() {
            0 => Self::Normal {
                hunk: Vec::new(),
                view: Vec::new(),
                parent: None,
            },
            1 => Self::Normal {
                hunk: Vec::new(),
                view: Vec::new(),
                parent: Some(commit.parent(0).unwrap().id()),
            },
            _ => {
                let parents = {
                    let mut p = Vec::new();
                    for c in commit.parents() {
                        p.push(c.id());
                    }
                    p
                };
                Self::Merge {
                    view: Vec::new(),
                    parents,
                }
            }
        }
    }

    fn format_header(commit: &Commit, format: &mut String) {
        format.push_str("Author: ");
        let author = commit.author();
        format.push_str(&author.name().unwrap_or_default());
        format.push_str(" ");
        format.push_str(author.email().unwrap_or_default());
        format.push('\n');

        //view.push(NormalView::Padding);

        format.push_str("Date: ");
        // TO-DO: Date!
        //format.push_str(&self.date);
        format.push_str(&format_git_time(author.when()).unwrap_or_default());
        format.push('\n');
        //view.push(NormalView::Padding);

        format.push('\n');
        //view.push(NormalView::Padding);

        format.push_str(&commit.message().unwrap_or_default());
        format.push('\n');
        //view.push(NormalView::Padding);

        format.push('\n');
    }

    fn fill(&mut self, commit: &Commit, repo: &Repository, format: &mut String) {
        format.clear();
        match self {
            DiffView::Merge { view, parents } => {
                view.clear();
                //view.push(MergeView::Padding);
                view.push(MergeView::Padding);

                format.push_str("+ ");
                format.push_str(&commit.id().to_string());
                format.push('\n');
                view.push(MergeView::Padding);

                format.push_str("Author: ");
                let author = commit.author();
                format.push_str(&author.name().unwrap_or_default());
                format.push_str(" <");
                format.push_str(author.email().unwrap_or_default());
                format.push_str(">");
                format.push('\n');
                view.push(MergeView::Padding);

                format.push_str("Date:   ");
                // TO-DO: Date!
                format.push_str(&format_git_time(author.when()).unwrap_or_default());
                //format.push_str(&self.date);
                format.push('\n');
                view.push(MergeView::Padding);

                format.push('\n');
                view.push(MergeView::Padding);

                format.push_str(&commit.message().unwrap_or_default());
                format.push('\n');
                view.push(MergeView::Padding);

                format.push('\n');
                view.push(MergeView::Padding);

                format.push_str("# Parents:");
                format.push('\n');
                view.push(MergeView::Padding);
                view.push(MergeView::Padding);

                for (i, p) in parents.iter().enumerate() {
                    format.push_str("- ");
                    format.push_str(&p.to_string());
                    format.push('\n');
                    //parents.push(*p);
                    view.push(MergeView::Commit(i));
                }
            }
            DiffView::Normal {
                hunk: hunk_col,
                view,
                parent,
            } => {
                view.clear();
                //view.push(NormalView::Padding);
                format.push_str("- ");
                if let Some(parent) = parent {
                    format.push_str(&parent.to_string());
                } else {
                    format.push_str("Root");
                }
                format.push('\n');
                view.push(NormalView::ParentCommit);
                Self::fill_format(format, view, commit);

                let tree = commit.tree().ok();
                let parent = commit.parent(0).unwrap().tree().ok();
                let diff = repo.diff_tree_to_tree(parent.as_ref(), tree.as_ref(), None);

                if let Ok(diff) = diff {
                    Self::fill_view(diff, format, view, hunk_col);
                }
            }
        }
    }

    fn fill_format(format: &mut String, view: &mut Vec<NormalView>, commit: &Commit) {
        format.push_str("+ ");
        format.push_str(&commit.id().to_string());
        format.push('\n');
        view.push(NormalView::Padding);

        format.push_str("Author: ");
        let author = commit.author();
        format.push_str(&author.name().unwrap_or_default());
        format.push_str(" ");
        format.push_str(author.email().unwrap_or_default());
        format.push('\n');

        view.push(NormalView::Padding);

        format.push_str("Date: ");
        // TO-DO: Date!
        //format.push_str(&self.date);
        format.push_str(&format_git_time(author.when()).unwrap_or_default());
        format.push('\n');
        view.push(NormalView::Padding);

        format.push('\n');
        view.push(NormalView::Padding);

        format.push_str(&commit.message().unwrap_or_default());
        format.push('\n');
        view.push(NormalView::Padding);

        format.push('\n');
        view.push(NormalView::Padding);
    }

    fn fill_view(
        diff: git2::Diff,
        format: &mut String,
        view: &mut Vec<NormalView>,
        hunk_col: &mut Vec<Hunk>,
    ) {
        let mut anchor_id = Oid::ZERO_SHA1;
        let mut hunk_current_line = 0;
        let mut line_count = view.len() - 1;
        let mut current_hunk = 0;
        let mut change_on = 0;
        view.push(NormalView::Padding);
        diff.print(DiffFormat::Patch, |delta, hunk, line| {
            line_count += 1;
            let file = delta.new_file();
            if file.id() != anchor_id {
                hunk_col.push(Hunk {
                    path: PathBuf::from(file.path().unwrap()),
                    changes: Vec::new(),
                });
                current_hunk = hunk_col.len() - 1;
                anchor_id = file.id();
                hunk_current_line = 0;

                format.push_str("--- ");
                match delta.status() {
                    git2::Delta::Added => {
                        format.push_str("/dev/null");
                    }
                    git2::Delta::Modified => {
                        format.push_str(&delta.old_file().path().unwrap().to_string_lossy());
                    }
                    _ => (),
                }
                format.push('\n');
                view.push(NormalView::Padding);
                //format.push(line.origin());
                format.push_str("+++ ");
                format.push_str(&file.path().unwrap().to_string_lossy());
                format.push('\n');
                view.push(NormalView::Padding);
            }
            match hunk {
                Some(hunk) if hunk_current_line != hunk.new_start() => {
                    format.push_str("@@ -");
                    format.push_str(&hunk.old_start().to_string());
                    format.push(',');
                    format.push_str(&hunk.old_lines().to_string());

                    format.push_str(" +");
                    format.push_str(&hunk.new_start().to_string());
                    format.push(',');
                    format.push_str(&hunk.new_lines().to_string());

                    format.push_str(" @@");

                    format.push('\n');
                    hunk_col[current_hunk].changes.push(line_count as u32);

                    hunk_current_line = hunk.new_start();
                    change_on = hunk.new_start() - 1;
                    //view.push(NormalView::Padding);
                    view.push(NormalView::Hunk {
                        from_hunk: current_hunk,
                        change_on,
                    });
                }
                Some(_) => {
                    format.push(line.origin());
                    match line.origin() {
                        ' ' | '+' => {
                            view.push(NormalView::HunkLine {
                                from_hunk: current_hunk,
                                change_on,
                            });
                            change_on += 1;
                        }
                        _ => {
                            view.push(NormalView::Padding);
                        }
                    }
                    format.push(' ');
                    format.push_str(&String::from_utf8_lossy(line.content()));
                    //change_on += 1;
                    //view.push(NormalView::HunkLine { from_hunk: current_hunk, change_on });
                }
                _ => (),
            }

            true
        });
    }
}

struct Diff {
    oid: Oid,
    format: String,
    diff_view: DiffView,
    //repo_id: RepoId,
}

struct Branch {
    name: String,
    commits: Vec<Oid>,
}

#[derive(Clone, Copy, Debug)]
enum GitView {
    //Padding,
    NewLine,
    Command,
    BranchHeader,
    BranchMember(usize),
    CommitHeader,
    CommitMember {
        from_branch: usize,
        from_commit: usize,
    },
    //ViewMore,
}

impl GitView {
    const LIMIT_VIEW: usize = 10;
    const BRANCH_HEADER: &str = "# Branch:";
    const COMMIT_HEADER: &str = "# Commit:";
}
#[derive(Serialize, Deserialize)]
enum RootAction {
    MergeBranch(usize),
    SwitchBranch(usize),
    ViewMore,
}

impl RootAction {
    fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    fn from_str(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

//#[derive(Default)]
struct RootView {
    branch: Vec<Branch>,
    active_branch: usize,
    limit_view: usize,
    //branch_member: Vec<Oid>,
    //commits: Vec<(Uri, Oid)>,
    format: String,
    view: Vec<GitView>,
    //repo_id: RepoId,
    //cache: PathBuf,
}

impl RootView {
    const ALPHA_REQ: i32 = 1;
    const BETA_REQ: i32 = 2;

    fn rebuild_view(&mut self) {
        self.view.clear();
        self.view.push(GitView::Command);
        self.view.push(GitView::BranchHeader);
        for (from_branch, _) in self.branch.iter().enumerate() {
            self.view.push(GitView::BranchMember(from_branch));
            //self.view.push(GitView::NewLine);
        }
        self.view.push(GitView::NewLine);
        self.view.push(GitView::CommitHeader);
        if let Some(branch) = self.branch.get(self.active_branch) {
            for (from_commit, _) in branch.commits.iter().take(self.limit_view).enumerate() {
                self.view.push(GitView::CommitMember {
                    from_branch: self.active_branch,
                    from_commit,
                });
                //self.view.push(GitView::ViewMore);
            }
        }
        //self.view.push(GitView::Command);
    }

    fn rebuild_format(&mut self) {
        self.format.clear();
        for view in &self.view {
            match view {
                //GitView::Padding => (),
                GitView::NewLine | GitView::Command => self.format.push('\n'),
                GitView::BranchHeader => {
                    self.format.push_str(GitView::BRANCH_HEADER);
                    //self.format.push_str(&self.branch[*i].name);
                    self.format.push('\n');
                }
                GitView::BranchMember(i) => {
                    self.format.push_str("- ");
                    self.format.push_str(&self.branch[*i].name);
                    self.format.push('\n');
                }
                GitView::CommitHeader => {
                    self.format.push_str(GitView::COMMIT_HEADER);
                    self.format.push('\n');
                }
                GitView::CommitMember {
                    from_branch,
                    from_commit,
                } => {
                    let commit = &self.branch[*from_branch].commits[*from_commit];
                    self.format.push_str("- ");
                    self.format.push_str(&commit.to_string()[..8]);
                    self.format.push('\n');
                }
            }
        }
    }
}

impl Lsp for Client {
    fn initialize(&mut self, conn: &Connection, id: RequestId, params: InitializeParams) {
        //conn.sender.send(Message::Response(()))
    }

    fn did_change(&mut self, conn: &Connection, params: DidChangeTextDocumentParams) {
        if let Some((wg, office_id)) = self.work_group.get_mut(&params.text_document.uri) {
            //Self::log("up here!?", conn);

            match wg {
                WorkGroup::RootView(root_view) => {
                    let text = &params.content_changes[0];
                    //for text in params.content_changes {

                    if let Some(range) = text.range {
                        //Self::log("i mean it should be here right?", conn);
                        let idx = range.start.line as usize;
                        //Self::log(idx.to_string(), conn);
                        match root_view.view.get(idx) {
                            Some(GitView::Command) => {
                                if text.text == "" || text.text == "\n" {
                                    return;
                                }
                                let mut wf = HashMap::new();
                                wf.insert(
                                    params.text_document.uri.clone(),
                                    vec![TextEdit {
                                        new_text: "".into(),
                                        range: Range {
                                            start: Position::new(0, 0),
                                            end: Position::new(0, text.text.len() as u32),
                                        },
                                    }],
                                );
                                let we = ApplyWorkspaceEditParams {
                                    label: None,
                                    edit: WorkspaceEdit {
                                        changes: Some(wf.clone()),
                                        ..Default::default()
                                    },
                                };
                                //Self::ok(conn, id, result);
                                Self::req(
                                    conn,
                                    ApplyWorkspaceEdit::METHOD,
                                    RequestId::from(RootView::BETA_REQ),
                                    &we,
                                );
                                match RootAction::from_str(&text.text) {
                                    //RootAction::MergeBranch(_) => todo!(),
                                    Some(RootAction::SwitchBranch(b)) => {
                                        root_view.active_branch = b;
                                        root_view.rebuild_view();
                                        root_view.rebuild_format();
                                        wf.insert(
                                            params.text_document.uri.clone(),
                                            vec![TextEdit {
                                                new_text: root_view.format.clone(),
                                                range: Range {
                                                    start: Position::new(0, 0),
                                                    end: Position::new(
                                                        root_view.view.len() as u32,
                                                        0,
                                                    ),
                                                },
                                            }],
                                        );

                                        let refresh = ApplyWorkspaceEditParams {
                                            label: None,
                                            edit: WorkspaceEdit {
                                                changes: Some(wf),
                                                ..Default::default()
                                            },
                                        };

                                        Self::req(
                                            conn,
                                            ApplyWorkspaceEdit::METHOD,
                                            RequestId::from(RootView::ALPHA_REQ),
                                            &refresh,
                                        );
                                    }
                                    //RootAction::ViewMore => todo!(),
                                    _ => (),
                                }
                            }
                            _ => {
                                //Self::log("fallthrough", conn);
                            }
                        }
                    }
                    //}
                }
                //WorkGroup::DiffView(diff) => todo!(),
                //WorkGroup::FileView => todo!(),
                _ => (),
            }
        }
    }

    fn inlay_hint(&mut self, conn: &Connection, id: RequestId, params: InlayHintParams) {
        let uri = params.text_document.uri;
        let start = params.range.start.line;
        let end = params.range.end.line;

        for i in start..end {}
    }

    fn hover(&mut self, conn: &Connection, id: RequestId, params: HoverParams) {
        let idx = params.text_document_position_params.position.line as usize;
        let uri = params.text_document_position_params.text_document.uri;
        if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
            let office = &mut self.office[*office_id];
            match wg {
                WorkGroup::RootView(root_view) => {
                    match root_view.view[idx] {
                        //GitView::BranchHeader => todo!(),
                        GitView::BranchMember(_) => (),
                        //GitView::CommitHeader => todo!(),
                        GitView::CommitMember {
                            from_branch,
                            from_commit,
                        } => {
                            let commit = root_view.branch[from_branch].commits[from_commit];
                            if let Ok(commit) = office.repo.find_commit(commit) {
                                let mut format = String::new();
                                DiffView::format_header(&commit, &mut format);
                                Self::ok(
                                    conn,
                                    id,
                                    &Hover {
                                        contents: HoverContents::Scalar(MarkedString::String(
                                            format,
                                        )),
                                        range: None,
                                    },
                                );
                            }
                        }
                        _ => (),
                    }
                }
                //WorkGroup::DiffView(diff) => todo!(),
                //WorkGroup::FileView => todo!(),
                _ => (),
            }
        }
    }

    fn goto_definition(&mut self, conn: &Connection, id: RequestId, params: GotoDefinitionParams) {
        let idx = params.text_document_position_params.position.line as usize;
        let url = &params.text_document_position_params.text_document.uri;
        let mut off_id: usize = 0;
        let (result, new_work_group) = match self.work_group.get(url) {
            Some((wg, office_id)) => {
                off_id = *office_id;
                Self::work_group_meeting(wg, &mut self.office[*office_id], idx)
            }
            None => (None, None),
        };
        Self::ok(conn, id, &result);
        if let Some((uri, wg)) = new_work_group {
            self.work_group.insert(uri, (wg, off_id));
        }
    }

    fn code_action(&mut self, conn: &Connection, id: RequestId, params: CodeActionParams) {
        //return;
        if let Some((wg, office_id)) = self.work_group.get_mut(&params.text_document.uri) {
            let office = &mut self.office[*office_id];
            let idx = params.range.start.line as usize;
            let uri = params.text_document.uri;
            if let Some(result) = Self::work_group_action(wg, office, idx, &uri) {
                Self::ok(conn, id, &result);
            }
        }
    }

    fn folding(&mut self, conn: &Connection, id: RequestId, params: FoldingRangeParams) {
        let uri = params.text_document.uri;
        //Self::log("FOLDING INITIATED?", conn);
        // CURRENT
        if let Some((wg, _)) = self.work_group.get(&uri) {
            if let Some(fold) = Self::work_group_folding(wg, &uri) {
                //Self::log("HERE COMES THE FOLD", conn);
                Self::ok(conn, id, &fold);
                return;
            }
            //Self::log("NO FOLD AAAH", conn);
        }
    }

    fn did_open(&mut self, conn: &Connection, params: DidOpenTextDocumentParams) {
        match self.work_group.get_mut(&params.text_document.uri) {
            Some(_) => (),
            None => {
                if let Some(mut wg) = self.new_rootview(&params.text_document.uri) {
                    wg.rebuild_view();
                    wg.rebuild_format();
                    let end = wg.view.len();
                    let format = wg.format.clone();
                    self.work_group.insert(
                        params.text_document.uri.clone(),
                        (WorkGroup::RootView(wg), self.office.len() - 1),
                    );
                    let mut wf = HashMap::new();
                    wf.insert(
                        params.text_document.uri.clone(),
                        vec![TextEdit {
                            new_text: format,
                            range: Range {
                                start: Position::new(0, 0),
                                end: Position::new(end as u32, 0),
                            },
                        }],
                    );
                    let we = ApplyWorkspaceEditParams {
                        label: None,
                        edit: WorkspaceEdit {
                            changes: Some(wf),
                            ..Default::default()
                        },
                    };
                    conn.sender.send(Message::Request(lsp_server::Request::new(
                        RequestId::from(RootView::ALPHA_REQ),
                        ApplyWorkspaceEdit::METHOD.into(),
                        &we,
                    )));
                    //send_ok(conn, RequestId::from(1), &Some(TextEdit::new(Range { start: Position::new(1, 1), end: Position::new(1, 1) }, format)));
                }
            }
        }
    }
}

impl Client {
    fn work_group_folding(wg: &WorkGroup, uri: &lsp_types::Uri) -> Option<impl Serialize + use<>> {
        match wg {
            WorkGroup::RootView(root_view) => {
                let mut ret = Vec::new();
                let mut start = 0;
                let mut end = 0;
                for (idx, giv) in root_view.view.iter().enumerate() {
                    match giv {
                        GitView::Command => (),
                        GitView::NewLine => (),
                        GitView::BranchHeader => {
                            start = idx + 1;
                        }
                        GitView::BranchMember(_) => match root_view.view.get(idx) {
                            Some(GitView::BranchMember(_)) => (),
                            _ => {
                                ret.push(FoldingRange {
                                    start_line: start as u32,
                                    end_line: end as u32,
                                    ..Default::default()
                                });
                            }
                        },
                        GitView::CommitHeader => {
                            start = idx;
                        }
                        GitView::CommitMember { .. } => match root_view.view.get(idx) {
                            Some(GitView::CommitMember { .. }) => (),
                            _ => {
                                ret.push(FoldingRange {
                                    start_line: start as u32,
                                    end_line: end as u32,
                                    ..Default::default()
                                });
                            }
                        },
                    }
                }
                Some(ret)
            }
            WorkGroup::DiffView(diff) => None,
            _ => None, //WorkGroup::FileView => todo!(),
        }
    }

    fn work_group_action(
        wg: &mut WorkGroup,
        office: &mut Office,
        idx: usize,
        uri: &lsp_types::Uri,
    ) -> Option<impl Serialize + use<>> {
        match wg {
            WorkGroup::RootView(root_view) => {
                match root_view.view[idx] {
                    GitView::BranchMember(i) => {
                        //root_view.active_branch = i;
                        //root_view.rebuild_view();
                        //root_view.rebuild_format();
                        let mut vec = CodeActionResponse::new();
                        vec.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("Switch to {}", &root_view.branch[i].name),
                            kind: Some(CodeActionKind::REFACTOR),
                            edit: {
                                let mut edit = HashMap::new();
                                edit.insert(
                                    uri.clone(),
                                    vec![
                                        //let mut edit = Vec::new();
                                        TextEdit {
                                            range: Range::new(
                                                Position::new(0, 0),
                                                Position::new(0, 0),
                                            ),
                                            new_text: RootAction::SwitchBranch(i).to_string(),
                                        },
                                    ],
                                ); //edit 
                                Some(WorkspaceEdit::new(edit))
                            },
                            ..Default::default()
                        }));
                        Some(vec)
                        //Some(ApplyWorkspaceEdit)
                    }
                    GitView::CommitHeader => {
                        root_view.limit_view += GitView::LIMIT_VIEW;
                        root_view.rebuild_view();
                        root_view.rebuild_format();
                        let mut vec = CodeActionResponse::new();
                        vec.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "View more?".into(),
                            kind: Some(CodeActionKind::REFACTOR),
                            edit: {
                                let mut edit = HashMap::new();
                                edit.insert(
                                    uri.clone(),
                                    vec![
                                        //let mut edit = Vec::new();
                                        TextEdit {
                                            range: Range::new(
                                                Position::new(0, 1),
                                                Position::new(root_view.view.len() as u32 + 1, 1),
                                            ),
                                            new_text: root_view.format.clone(),
                                        },
                                    ],
                                ); //edit 
                                Some(WorkspaceEdit::new(edit))
                            },
                            ..Default::default()
                        }));
                        Some(vec)
                    }
                    //GitView::ViewMore => None,
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn work_group_meeting(
        wg: &WorkGroup,
        office: &mut Office,
        idx: usize,
    ) -> (Option<impl Serialize + use<>>, Option<(Uri, WorkGroup)>) {
        match wg {
            WorkGroup::RootView(root) => match &root.view.get(idx) {
                Some(GitView::CommitMember {
                    from_branch,
                    from_commit,
                }) => {
                    let oid = root.branch[*from_branch].commits[*from_commit];
                    match office.manifest.get(&oid) {
                        Some(uri) => (
                            Some(GotoDefinitionResponse::Scalar(Location::new(
                                uri.clone(),
                                Range::default(),
                            ))),
                            None,
                        ),
                        None => {
                            if let Some((diff, uri)) = Self::open_diff(oid, office) {
                                office.manifest.insert(oid, uri.clone());
                                return (
                                    Some(GotoDefinitionResponse::Scalar(Location::new(
                                        uri.clone(),
                                        Range::default(),
                                    ))),
                                    Some((uri, WorkGroup::DiffView(diff))),
                                );
                            }
                            return (None, None);
                        }
                    }
                }
                _ => (None, None),
            },
            WorkGroup::DiffView(diff) => match &diff.diff_view {
                DiffView::Merge { view, parents } => {
                    if let MergeView::Commit(c) = &view[idx] {
                        let oid = parents[*c];
                        if let Some(uri) = office.manifest.get(&oid) {
                            return (
                                Some(GotoDefinitionResponse::Scalar(Location::new(
                                    uri.clone(),
                                    Range::default(),
                                ))),
                                None,
                            );
                        }
                        if let Some((diff, uri)) = Self::open_diff(oid, office) {
                            office.manifest.insert(oid, uri.clone());
                            return (
                                Some(GotoDefinitionResponse::Scalar(Location::new(
                                    uri.clone(),
                                    Range::default(),
                                ))),
                                Some((uri, WorkGroup::DiffView(diff))),
                            );
                        }
                    }
                    return (None, None);
                }
                DiffView::Normal { hunk, view, parent } => match &view[idx] {
                    NormalView::Padding => (None, None),
                    NormalView::ParentCommit => {
                        if let Some(parent) = &parent {
                            if let Some(uri) = office.manifest.get(&parent) {
                                return (
                                    Some(GotoDefinitionResponse::Scalar(Location::new(
                                        uri.clone(),
                                        Range::default(),
                                    ))),
                                    None,
                                );
                            }
                            if let Some((diff, uri)) = Self::open_diff(*parent, office) {
                                return (
                                    Some(GotoDefinitionResponse::Scalar(Location::new(
                                        uri.clone(),
                                        Range::default(),
                                    ))),
                                    Some((uri, WorkGroup::DiffView(diff))),
                                );
                            }
                        }

                        (None, None)
                    }
                    NormalView::Hunk {
                        from_hunk,
                        change_on,
                    }
                    | NormalView::HunkLine {
                        from_hunk,
                        change_on,
                    } => {
                        let h = &hunk[*from_hunk];
                        if let Some(uri) = Self::open_file(office, diff.oid, &h.path) {
                            return (
                                Some(GotoDefinitionResponse::Scalar(Location::new(
                                    uri.clone(),
                                    Range {
                                        start: Position::new(*change_on, 1),
                                        ..Default::default()
                                    },
                                ))),
                                Some((uri, WorkGroup::FileView)),
                            );
                        }
                        return (None, None);
                    }
                },
            },
            WorkGroup::FileView => (None, None),
        }
    }

    fn open_file(office: &mut Office, oid: Oid, path: &Path) -> Option<Uri> {
        if let Ok(commit) = office.repo.find_commit(oid) {
            let tree = commit.tree().unwrap();
            if let Ok(entry) = tree.get_path(path) {
                if entry.kind() != Some(ObjectType::Blob) {
                    return None;
                }
                if let Ok(blob) = office.repo.find_blob(entry.id()) {
                    let p = office.cache.join(oid.to_string()).join(path);
                    if let Err(_) = fs::create_dir_all(p.parent().unwrap()) {
                        return None;
                    }

                    if let Err(_) = fs::write(&p, blob.content()) {
                        return None;
                    }
                    let uri = name_to_url(&p).unwrap();
                    office.file_cache.insert(p, uri.clone());
                    return Some(uri);
                    //return (So);
                }
            }
        }
        None
    }

    fn new_rootview(&mut self, uri: &lsp_types::Uri) -> Option<RootView> {
        let path = uri.path();
        let p = std::path::PathBuf::from(path.as_str());
        match p.file_name() {
            Some(s) => match s.to_str() {
                Some(ROOT_NAME) => {
                    let parent = PathBuf::from(p.parent().unwrap());
                    let p = parent.strip_prefix("/").unwrap();
                    match Repository::open(&p) {
                        Err(_) => None,
                        Ok(repo) => {
                            //log(s.to_str().unwrap(), conn);
                            let branch = {
                                let mut vec = Vec::default();
                                match repo.branches(None) {
                                    Err(_) => (),
                                    Ok(brances) => {
                                        for branch in brances {
                                            branch.map(|branch| {
                                                let commits = {
                                                    let mut c = Vec::default();
                                                    branch.0.get().peel_to_commit().map(|commit| {
                                                        let mut revwalk = repo.revwalk().unwrap();

                                                        revwalk.push(commit.id());
                                                        revwalk.set_sorting(Sort::TIME);
                                                        for res in revwalk {
                                                            res.map(|id| {
                                                                c.push(id);
                                                            });
                                                        }
                                                    });
                                                    c
                                                };
                                                vec.push(Branch {
                                                    name: branch
                                                        .0
                                                        .name()
                                                        .unwrap()
                                                        .unwrap()
                                                        .to_string(),
                                                    commits,
                                                });
                                            });
                                        }
                                    }
                                }
                                vec
                            };
                            let mut cache = PathBuf::from(repo.path());
                            cache.push(CACHE_DIR);
                            self.office.push(Office {
                                repo,
                                cache,
                                manifest: HashMap::new(),
                                file_cache: HashMap::new(),
                            });
                            //self.repo.push(repo);
                            Some(RootView {
                                branch,
                                active_branch: 0,
                                limit_view: GitView::LIMIT_VIEW,
                                format: String::default(),
                                view: Vec::default(),
                                //                         cache,
                            })
                            //self.work_group
                            //    .insert(uri.clone(), WorkGroup::RootView(rootview));
                        }
                    }
                }
                _ => None,
            },
            None => None,
        }
    }

    fn open_diff(oid: Oid, office: &mut Office) -> Option<(Diff, Uri)> {
        //let office = &mut self.office[office_id];
        match office.repo.find_commit(oid) {
            Ok(commit) => {
                let mut format = String::new();
                let mut diff_view = DiffView::new(&commit);
                diff_view.fill(&commit, &office.repo, &mut format);
                //let path = path.strip_prefix("/").unwrap();
                let path = office.cache.join(oid.to_string());
                if let Err(_) = fs::create_dir_all(&path) {
                    //log("create_dir_all failed", conn);
                    return None;
                }
                let path = path.join(DIFF_NAME);
                if let Err(_) = fs::write(&path, format.as_bytes()) {
                    return None;
                }
                let uri = name_to_url(&path).unwrap();
                Some((
                    Diff {
                        oid,
                        format,
                        diff_view,
                    },
                    uri,
                ))
            }
            Err(_) => None,
        }
    }
}

fn main() {
    let (conn, io_t) = Connection::stdio();
    let caps = ServerCapabilities {
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                ..Default::default()
            },
        )),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };
    //println!("{init}");
    //return;
    let init_params = conn
        .initialize(serde_json::to_value(&caps).unwrap())
        .unwrap();
    //println!("{init_params}");
    //return;
    let mut client = Client::new();
    for msg in &conn.receiver {
        match msg {
            Message::Request(request) => client.handle_request(&conn, request),
            Message::Response(response) => (),
            Message::Notification(notification) => client.handle_notification(&conn, notification),
        }
    }
    //client.main_loop(conn, init_params);
    io_t.join();
}

pub fn name_to_url(path: &Path) -> Option<Uri> {
    //let path = Path::new(name);
    if !path.is_absolute() {
        return None;
    }
    let mut raw = String::from("file://");
    for component in path.components() {
        match component {
            Component::Normal(seg) => {
                raw.push('/');
                raw.push_str(&component.as_os_str().to_string_lossy());
            }
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    raw.push('/');
                    raw.push(letter as char);
                    raw.push(':');
                }
                _ => {
                    raw.push('/');
                    raw.push_str(&prefix.as_os_str().to_string_lossy());
                }
            },
            Component::RootDir | Component::CurDir | Component::ParentDir => {}
        }
    }
    raw.parse().ok()
}

fn format_git_time(time: Time) -> Option<String> {
    if let Some(offset) = FixedOffset::east_opt(time.offset_minutes() * 60) {
        if let Some(dt) = offset.timestamp_opt(time.seconds(), 0).single() {
            return Some(dt.format("%a %b %d %H:%M:%S %Y %z").to_string());
        }
    }
    None
}
