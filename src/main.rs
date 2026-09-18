trait Lsp {
    fn hover(&mut self, id: RequestId, params: HoverParams);
    fn goto_definition(&mut self, id: RequestId, params: GotoDefinitionParams);
    //fn folding(&mut self, id: RequestId, params: FoldingRangeParams);
    fn code_action(&mut self, id: RequestId, params: CodeActionParams);
    fn inlay_hint(&mut self, id: RequestId, params: InlayHintParams);
    fn handle_request(&mut self, request: lsp_server::Request) {
        match request.method.as_str() {
            HoverRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: HoverParams| {
                    self.hover(request.id, params);
                });
            }
            GotoDefinition::METHOD => {
                serde_json::from_value(request.params).map(|params: GotoDefinitionParams| {
                    self.goto_definition(request.id, params);
                });
            }
            CodeActionRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: CodeActionParams| {
                    self.code_action(request.id, params);
                });
            }
            InlayHintRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: InlayHintParams| {
                    self.inlay_hint(request.id, params);
                });
            }
            SemanticTokensFullRequest::METHOD => {}
            _ => {}
        }
    }

    fn handle_notification(&mut self, notification: lsp_server::Notification) {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidOpenTextDocumentParams| {
                        self.did_open(params);
                    },
                );
            }
            DidChangeTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidChangeTextDocumentParams| {
                        self.did_change(params);
                    },
                );
            }
            DidSaveTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidSaveTextDocumentParams| {
                        self.did_save(params);
                    },
                );
            }
            _ => (),
        }
    }

    fn did_save(&mut self, params: DidSaveTextDocumentParams);
    fn did_open(&mut self, params: DidOpenTextDocumentParams);
    fn did_change(&mut self, params: DidChangeTextDocumentParams);
}

use std::{
    collections::HashMap,
    fs,
    hash::Hash,
    io::BufRead,
    path::{Component, Path, PathBuf, Prefix},
    process::{self, Command},
    str::FromStr,
    usize,
};

use chrono::{DateTime, FixedOffset, TimeZone};
use crossbeam::channel::Sender;
use git2::{
    BranchType, Commit, DiffFormat, Error, ObjectType, Oid, Repository, Sort, Status,
    StatusOptions, Time, build::CheckoutBuilder,
};
use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CodeLens, DefinitionOptions,
    DiagnosticOptions, DiagnosticServerCapabilities, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, FoldingRange, FoldingRangeParams,
    FoldingRangeProviderCapability, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams, Location, LogMessageParams,
    MarkedString, MarkupContent, MarkupKind, MessageType, OneOf, Position, Range, SaveOptions,
    ServerCapabilities, ShowDocumentParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Uri, WorkspaceEdit,
    lsp_request,
    notification::{
        DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, LogMessage, Notification,
        ShowMessage,
    },
    request::{
        ApplyWorkspaceEdit, CodeActionRequest, CodeLensRequest, DocumentHighlightRequest,
        FoldingRangeRequest, GotoDefinition, HoverRequest, Initialize, InlayHintRequest, Request,
        SemanticTokensFullRequest, ShowDocument,
    },
};
use serde::{Deserialize, Serialize};

const ROOT_NAME: &str = "ma.md";
const DIFF_NAME: &str = "ma.diff";
const CACHE_DIR: &str = "ma-cache";
const INPUT_MOUTH: &str = "ma-input.md";

struct Client {
    work_group: HashMap<Uri, Company>,
    //repo: Vec<Repository>,
    office: Vec<Office>,
    conn: Conn, //caps: ServerCapabilities,
}

struct Conn(Sender<Message>);

struct Uniq<'ma> {
    count: i32,
    conn: &'ma Conn,
}

impl<'ma> Uniq<'ma> {
    fn make(conn: &'ma Conn) -> Self {
        Self { count: 0, conn }
    }

    fn req(&mut self, method: impl Into<String>, result: &impl Serialize) {
        self.count += 1;
        self.conn.req(method, RequestId::from(self.count), result);
    }
}

impl Conn {
    const ALPHA_REQ: i32 = 1;
    const BETA_REQ: i32 = 2;

    fn alpha_req() -> RequestId {
        RequestId::from(Self::ALPHA_REQ)
    }

    fn beta_req() -> RequestId {
        RequestId::from(Self::BETA_REQ)
    }

    fn log(&self, message: impl Into<String>) {
        let l_params = LogMessageParams {
            typ: MessageType::INFO,
            message: message.into(),
        };

        self.0
            .send(lsp_server::Message::Notification(
                lsp_server::Notification {
                    method: LogMessage::METHOD.to_string(),
                    params: serde_json::to_value(l_params).unwrap(),
                },
            ))
            .unwrap();
    }

    fn ok(&self, id: RequestId, result: &impl Serialize) {
        let resp = Response {
            id,
            response_result: Ok(serde_json::to_value(result).unwrap()),
        };
        self.0.send(Message::Response(resp));
    }

    fn req(&self, method: impl Into<String>, id: RequestId, result: &impl Serialize) {
        //let req = Request {}
        self.0.send(Message::Request(lsp_server::Request::new(
            id,
            method.into(),
            result,
        )));
    }

    fn err(&self, id: RequestId, code: lsp_server::ErrorCode, msg: &str) {
        let resp = Response {
            id,
            response_result: Err(lsp_server::ResponseError {
                code: code as i32,
                message: msg.into(),
                data: None,
            }),
        };
        self.0.send(Message::Response(resp));
    }
}

impl Client {
    fn new(sender: crossbeam::channel::Sender<Message>) -> Self {
        Self {
            work_group: HashMap::default(),
            //repo: Vec::default(),
            office: Vec::default(),
            conn: Conn(sender), //manifest: HashMap::default(),
                                //caps,
        }
    }

    fn create_hint(line: u32, character: u32, label: impl Into<String>) -> InlayHint {
        InlayHint {
            position: Position::new(line, character),
            label: InlayHintLabel::String(label.into()),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        }
    }
}

type OfficeId = usize;
type Company = (WorkGroup, OfficeId);

enum WorkGroup {
    RootView(RootView),
    DiffView(Diff),
    InputBuffer(InputBuffer),
    FileView,
    //BranchConflict {
    //    merge_target: String,
    //    merge_subject: String,
    //},
}

enum InputBuffer {
    AcceptNothing,
    AcceptCommit,
    AcceptMerge(usize),
    ShowErrorMessage(String),
}

impl InputBuffer {
    fn inlay_hint(&self) -> Option<InlayHint> {
        match self {
            InputBuffer::ShowErrorMessage(m) => Some(InlayHint {
                position: Position {
                    line: 0,
                    character: 0,
                },
                label: InlayHintLabel::String(m.clone()),
                kind: None,
                text_edits: None,
                tooltip: None,
                padding_left: None,
                padding_right: None,
                data: None,
            }),
            _ => None,
        }
    }

    fn commit(&mut self, office: &mut Office, conn: &mut Uniq, message: impl AsRef<str>) {
        match self {
            InputBuffer::ShowErrorMessage(_) => (),
            InputBuffer::AcceptNothing => (),
            InputBuffer::AcceptMerge(b) => {
                return;
                let head = office.repo.head().unwrap();
                let our = head.peel_to_commit().unwrap();

                let branch = &office.branch[*b];
                let b_repo = office
                    .repo
                    .find_branch(&branch.name, branch.b_type)
                    .unwrap();
                let their = b_repo.get().peel_to_commit().unwrap();
                let mut index = office.repo.merge_commits(&our, &their, None).unwrap();
                if index.has_conflicts() {
                    *self = InputBuffer::ShowErrorMessage("Merge conflicts detected!".into());
                    return;
                }
                let author = office.repo.author_from_env().unwrap();
                let committer = office.repo.committer_from_env().unwrap();

                //index.write();
                let tree_id = index.write_tree_to(&office.repo).unwrap();
                //let tree_id = index.write_tree().unwrap();
                let tree = office.repo.find_tree(tree_id).unwrap();
                let res = office.repo.commit(
                    Some("HEAD"),
                    &author,
                    &committer,
                    message.as_ref(),
                    &tree,
                    &[&our, &their],
                );
                if let Err(_) = res {
                    *self = InputBuffer::ShowErrorMessage("Commit failed!".into());
                    return;
                }

                *self = Self::AcceptNothing;
                office.repo.checkout_tree(&tree.into_object(), None);
                RootView::open_buffer(office, conn);
                RootView::reload(office, conn);
            }
            InputBuffer::AcceptCommit => {
                let author = office.repo.author_from_env().unwrap();
                let committer = office.repo.committer_from_env().unwrap();
                let mut index = office.repo.index().unwrap();
                let tree_id = index.write_tree().unwrap();
                let tree = office.repo.find_tree(tree_id).unwrap();
                let res = match office.repo.head().ok() {
                    Some(head) => {
                        let parent = head.peel_to_commit().unwrap();
                        // Existing commit
                        office.repo.commit(
                            Some("HEAD"),
                            &author,
                            &committer,
                            message.as_ref(),
                            &tree,
                            &[&parent],
                        )
                    }
                    None => office.repo.commit(
                        Some("HEAD"),
                        &author,
                        &committer,
                        message.as_ref(),
                        &tree,
                        &[],
                    ),
                };
                if let Ok(_) = res {
                    RootView::open_buffer(office, conn);
                    RootView::reload(office, conn);
                }
                *self = Self::AcceptNothing;
            }
        }
    }
}

struct Office {
    repo: Repository,
    cache: PathBuf,
    status: Vec<(PathBuf, Status)>,
    manifest: HashMap<Oid, Uri>,
    file_cache: HashMap<PathBuf, Uri>,
    ma_root: Uri,
    ma_input: Uri,
    branch: Vec<Branch>,
    head_branch: usize,
    viewed_branch: usize,
    //repo_state: RepoState,
}

impl Office {
    pub fn new(ma_root: Uri, path: &Path) -> Option<Office> {
        match Repository::open(path) {
            Ok(repo) => {
                let mut cache = PathBuf::from(repo.path());
                cache.push(CACHE_DIR);
                let ma_input = name_to_url(&cache.join(INPUT_MOUTH)).unwrap();
                let mut office = Office {
                    repo,
                    cache,
                    status: Vec::new(),
                    manifest: HashMap::new(),
                    file_cache: HashMap::new(),
                    ma_root,
                    ma_input,
                    branch: Vec::new(),
                    head_branch: 0,
                    viewed_branch: 0,
                    //repo_state: RepoState::AcceptNothing,
                };
                office.re_fill_status();
                office.reload_branch();
                Some(office)
            }
            Err(_) => None,
        }
    }

    fn reload_branch(&mut self) {
        self.branch.clear();
        if let Ok(branches) = self.repo.branches(None) {
            for (i, branch) in branches.enumerate() {
                branch.map(|(branch, b_type)| {
                    if branch.is_head() {
                        self.head_branch = i;
                        self.viewed_branch = i;
                    }
                    let commits = {
                        let mut c = Vec::default();
                        branch.get().peel_to_commit().map(|commit| {
                            let mut revwalk = self.repo.revwalk().unwrap();

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
                    self.branch.push(Branch {
                        name: branch.name().unwrap().unwrap().to_string(),
                        b_type,
                        commits,
                    });
                });
            }
        }
    }

    fn re_fill_status(&mut self) {
        self.status.clear();
        let st = self.repo.statuses(Some(
            StatusOptions::new()
                .include_untracked(true)
                .recurse_untracked_dirs(true),
        ));
        st.map(|status| {
            for entry in status.iter() {
                self.status
                    .push((PathBuf::from(entry.path().unwrap()), entry.status()));
            }
        });
    }
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

/// TO-DO: create a nice model for DiffView. with diff hunk head as "goto_definition" anchor
/// TO-DO done i suppose...
enum DiffView {
    /// if parent commit > 1
    Merge {
        view: Vec<MergeView>,
        parents: Vec<Oid>,
    },
    /// if parent <= 1
    Normal {
        hunk: Vec<Hunk>,
        view: Vec<NormalView>,
        parent: Option<Oid>,
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

        format.push_str("Date:   ");

        format.push_str(&format_git_time(author.when()).unwrap_or_default());
        format.push('\n');
        format.push('\n');
        format.push_str(&commit.message().unwrap_or_default());
        format.push('\n');
        format.push('\n');
    }

    fn fill(&mut self, commit: &Commit, repo: &Repository, format: &mut String) {
        format.clear();
        match self {
            DiffView::Merge { view, parents } => {
                view.clear();
                //view.push(MergeView::Padding);
                //view.push(MergeView::Padding);

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
                format.push_str(&format_git_time(author.when()).unwrap_or_default());
                format.push('\n');
                view.push(MergeView::Padding);

                format.push('\n');
                view.push(MergeView::Padding);

                let message = &commit.message().unwrap_or_default();

                for ms in message.lines() {
                    format.push_str(ms);
                    format.push('\n');
                    view.push(MergeView::Padding);
                }
                format.push('\n');
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
                let diff = match parent {
                    Some(p) => {
                        let parent = repo.find_commit(*p).unwrap().tree().ok();
                        repo.diff_tree_to_tree(parent.as_ref(), tree.as_ref(), None)
                    }
                    None => repo.diff_tree_to_tree(None, tree.as_ref(), None),
                };
                //let parent = commit.parent(0).unwrap().tree().ok();
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
        let message = &commit.message().unwrap_or_default();
        //format.push_str(&commit.message().unwrap_or_default());
        //format.push('\n');
        //view.push(NormalView::Padding);
        for ms in message.lines() {
            format.push_str(ms);
            format.push('\n');
            view.push(NormalView::Padding);
        }
        //view.push(NormalView::Padding);
        //view.push(NormalView::Padding);

        format.push('\n');
        //view.push(NormalView::Padding);
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

impl Diff {
    fn inlay_hint(&self, i: u32) -> Option<InlayHint> {
        match &self.diff_view {
            DiffView::Merge { .. } => None,
            DiffView::Normal { hunk, view, parent } => match view.get(i as usize) {
                Some(NormalView::Hunk { from_hunk, .. }) => {
                    let h = &hunk[*from_hunk];
                    Some(Client::create_hint(
                        i,
                        0,
                        format!("{} -> ", h.path.to_string_lossy()),
                    ))
                }
                _ => None,
            },
        }
    }
}

struct Branch {
    name: String,
    b_type: BranchType,
    commits: Vec<Oid>,
}

#[derive(Clone, Copy, Debug)]
enum GitView {
    //Padding,
    NewLine,
    Command,
    StatusHeader,
    StatusMember {
        from_file: usize,
    },
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
    Reload,
    AttemptCommit,
    StatusReload,
    StageFile(usize),
    UnstageFile(usize),
    //ReplaceFile(usize),
    MergeBranch(usize),
    ViewBranch(usize),
    CheckoutBranch(usize),
    ViewMore,
    ViewLess,
}

impl RootAction {
    fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    fn from_str(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    fn pack(
        uri: &lsp_types::Uri,
        arg: &[(impl AsRef<str>, RootAction, Option<CodeActionKind>)],
    ) -> CodeActionResponse {
        let mut vec = CodeActionResponse::new();
        for (title, action, kind) in arg {
            vec.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: title.as_ref().to_string(),
                kind: kind.clone(),
                edit: {
                    let mut edit = HashMap::new();
                    edit.insert(
                        uri.clone(),
                        vec![TextEdit {
                            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                            new_text: action.to_string(),
                        }],
                    ); //edit 
                    Some(WorkspaceEdit::new(edit))
                },
                ..Default::default()
            }));
        }
        vec
    }
}

//#[derive(Default)]
struct RootView {
    //branch: Vec<Branch>,
    //viewed_branch: usize,
    //head_branch: usize,
    limit_view: usize,
    format: String,
    view: Vec<GitView>,
}

impl RootView {
    fn open_buffer(office: &Office, conn: &mut Uniq) {
        let show = ShowDocumentParams {
            uri: office.ma_root.clone(),
            external: Some(false),
            take_focus: Some(true),
            selection: None,
        };
        //*wg = Some((uri, WorkGroup::InputBuffer(InputBuffer::AcceptCommit)));
        conn.req(ShowDocument::METHOD, &show);
    }

    fn reload(office: &Office, conn: &mut Uniq) {
        let mut wf = HashMap::new();
        wf.insert(
            office.ma_root.clone(),
            vec![TextEdit {
                new_text: RootAction::Reload.to_string(),
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(0, 0),
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
        conn.req(ApplyWorkspaceEdit::METHOD, &refresh);
    }

    fn hover(&mut self, i: usize, office: &Office) -> Option<Hover> {
        match self.view.get(i) {
            //GitView::BranchHeader => todo!(),
            Some(GitView::BranchMember(b)) => {
                let branch = &office.branch[*b];
                //let target = &office.branch[i].name;
                match office.repo.find_branch(&branch.name, branch.b_type) {
                    Ok(b) => {
                        let head = office.repo.head().unwrap();
                        let current = head.peel_to_commit().unwrap();
                        let subject = b.get().peel_to_commit().unwrap();
                        let index = office.repo.merge_commits(&current, &subject, None).unwrap();
                        if !index.has_conflicts() {
                            return Some(Hover {
                                contents: HoverContents::Scalar(MarkedString::String(
                                    "Clean branch, no conflict".into(),
                                )),
                                range: None,
                            });
                        }
                        enum FileMerge {
                            Our,
                            Their,
                            None,
                        }
                        let mut value = String::new();

                        value.push_str("# Potential merge conflicts detected!\n");

                        for conf in index.conflicts().unwrap() {
                            conf.map(|conflict| {
                                value.push('\n');
                                match (conflict.ancestor, conflict.our, conflict.their) {
                                    (Some(ancestor), Some(our), Some(their)) => {
                                        let res = office
                                            .repo
                                            .merge_file_from_index(&ancestor, &our, &their, None)
                                            .unwrap();
                                        let mut their_line = 0;
                                        let mut our_line = 0;
                                        let mut fm = FileMerge::None;

                                        let full =
                                            PathBuf::from_str(&String::from_utf8_lossy(&our.path))
                                                .unwrap();
                                        let ex = markdown_language(&full);
                                        let our_path = String::from_utf8_lossy(&our.path);
                                        let their_path = String::from_utf8_lossy(&their.path);
                                        value.push_str(&format!(
                                            "Cause: Conflicting contents in `{}`\n",
                                            &their_path
                                        ));

                                        value.push_str(&format!(
                                            "`^` ancestor: `{}`\n",
                                            String::from_utf8_lossy(&ancestor.path)
                                        ));

                                        for line in res.content().split(|&s| s == b'\n') {
                                            match line {
                                                _ if line.starts_with(b"<<<<<<<") => {
                                                    fm = FileMerge::Our;
                                                    value.push_str(&format!(
                                                        "`<<<<<<<` {}: `{}`\n",
                                                        head.shorthand().unwrap(),
                                                        &our_path
                                                    ));
                                                    value.push_str(&format!("```{}\n", &ex));
                                                }
                                                _ if line.starts_with(b"=======") => {
                                                    fm = FileMerge::Their;
                                                    value.push_str("```\n");
                                                    value.push_str("`=======`\n");
                                                    value.push_str(&format!("```{}\n", &ex));
                                                }
                                                _ if line.starts_with(b">>>>>>>") => {
                                                    fm = FileMerge::None;
                                                    value.push_str("```\n");

                                                    value.push_str(&format!(
                                                        "`>>>>>>>` {}: `{}`\n",
                                                        &branch.name, &their_path
                                                    ));
                                                    //value.push_str(">>>>>>>\n");
                                                }
                                                l => match fm {
                                                    FileMerge::Our => {
                                                        our_line += 1;
                                                        value.push_str(&format!(
                                                            "{our_line} {}\n",
                                                            String::from_utf8_lossy(l)
                                                        ));
                                                    }
                                                    FileMerge::Their => {
                                                        their_line += 1;
                                                        value.push_str(&format!(
                                                            "{their_line} {}\n",
                                                            String::from_utf8_lossy(l)
                                                        ));
                                                    }
                                                    FileMerge::None => {
                                                        their_line += 1;
                                                        our_line += 1;
                                                    }
                                                },
                                            }
                                        }
                                    }
                                    (Some(ancestor), Some(our), None) => {
                                        value.push_str(&format!(
                                            "Cause: `{}` was deleted in `{}`, modified in `{}`\n",
                                            String::from_utf8_lossy(&our.path),
                                            &branch.name,
                                            head.shorthand().unwrap(),
                                        ));
                                        value.push_str(&format!(
                                            "`^` ancestor: `{}`\n",
                                            String::from_utf8_lossy(&ancestor.path)
                                        ));

                                        value.push_str(&format!(
                                            "`<` {}: `{}`\n",
                                            head.shorthand().unwrap(),
                                            String::from_utf8_lossy(&our.path)
                                        ));

                                        value.push_str(&format!(
                                            "`>` {}: Does not exist\n",
                                            &branch.name,
                                        ));
                                    }
                                    (Some(ancestor), None, Some(their)) => {
                                        value.push_str(&format!(
                                            "Cause: `{}` was deleted in `{}`, modified in `{}`\n",
                                            String::from_utf8_lossy(&their.path),
                                            head.shorthand().unwrap(),
                                            &branch.name
                                        ));
                                        value.push_str(&format!(
                                            "`^` ancestor: `{}`\n",
                                            String::from_utf8_lossy(&ancestor.path)
                                        ));
                                        value.push_str(&format!(
                                            "`<` {}: Does not exist\n",
                                            head.shorthand().unwrap(),
                                        ));
                                        value.push_str(&format!(
                                            "`>` {}: `{}`\n",
                                            &branch.name,
                                            String::from_utf8_lossy(&their.path)
                                        ));
                                    }
                                    (None, Some(our), Some(their)) => {
                                        value.push_str(
                                            "Cause: Conflicting `add` in both branches\n",
                                        );

                                        value.push_str("`^` ancestor: Does not exist!\n");
                                        value.push_str(&format!(
                                            "`<` {}: `{}`\n",
                                            head.shorthand().unwrap(),
                                            String::from_utf8_lossy(&our.path)
                                        ));
                                        value.push_str(&format!(
                                            "`>` {}: `{}`\n",
                                            &branch.name,
                                            String::from_utf8_lossy(&their.path)
                                        ));
                                    }
                                    _ => {}
                                }
                            });
                        }
                        Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value,
                            }),
                            range: None,
                        })
                        // The branch we're merging INTO current HEAD
                    }
                    Err(_) => None,
                }
            }
            //GitView::CommitHeader => todo!(),
            Some(GitView::CommitMember {
                from_branch,
                from_commit,
            }) => {
                let commit = office.branch[*from_branch].commits[*from_commit];
                if let Ok(commit) = office.repo.find_commit(commit) {
                    let mut format = String::new();
                    DiffView::format_header(&commit, &mut format);
                    return Some(Hover {
                        contents: HoverContents::Scalar(MarkedString::String(format)),
                        range: None,
                    });
                }
                None
            }
            _ => None,
        }
    }

    fn inlay_hint(&self, i: u32, office: &Office) -> Option<InlayHint> {
        let root_view = self;
        match root_view.view.get(i as usize) {
            //GitView::NewLine => todo!(),
            Some(GitView::Command) => Some(Client::create_hint(
                i,
                0,
                office.repo.workdir().unwrap().to_string_lossy(),
            )),
            //GitView::StatusHeader => todo!(),
            Some(GitView::StatusMember { from_file }) => {
                let (_, status) = &office.status[*from_file];
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
                Some(Client::create_hint(
                    i,
                    0,
                    format!("{} | {} -> ", staged, unstaged),
                ))
            }
            Some(GitView::BranchHeader) => match office.branch.is_empty() {
                true => None,
                false => Some(Client::create_hint(
                    i,
                    GitView::COMMIT_HEADER.len() as u32,
                    format!(" {}", &office.branch[office.viewed_branch].name),
                )),
            },
            Some(GitView::CommitMember { .. }) => Some(Client::create_hint(i, 0, "- ")),
            Some(GitView::BranchMember(on_branch)) => match office.branch.is_empty() {
                true => None,
                false => {
                    let branch = &office.branch[*on_branch];
                    let label = match (office.head_branch == *on_branch, branch.b_type) {
                        (true, BranchType::Local) => "L HEAD -> ",
                        (true, BranchType::Remote) => "R HEAD -> ",
                        (false, BranchType::Local) => "L      -> ",
                        (false, BranchType::Remote) => "R      -> ",
                    };
                    Some(Client::create_hint(i, 0, label))
                }
            },
            _ => None,
        }
    } 

    fn refresh(&mut self, uri: &lsp_types::Uri, office: &Office) -> ApplyWorkspaceEditParams {
        let old = self.view.len();
        //self.reload_branch(office);
        self.rebuild_view(office);
        self.rebuild_format(office);
        let mut wf = HashMap::new();
        wf.insert(
            uri.clone(),
            vec![TextEdit {
                new_text: self.format.clone(),
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(old.max(self.view.len()) as u32, 0),
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
        refresh
    }

    fn rebuild_view(&mut self, office: &Office) {
        self.view.clear();
        self.view.push(GitView::Command);

        self.view.push(GitView::StatusHeader);
        for (from_file, _) in office.status.iter().enumerate() {
            self.view.push(GitView::StatusMember { from_file });
        }

        self.view.push(GitView::NewLine);
        self.view.push(GitView::BranchHeader);
        for (from_branch, _) in office.branch.iter().enumerate() {
            self.view.push(GitView::BranchMember(from_branch));
            //self.view.push(GitView::NewLine);
        }
        self.view.push(GitView::NewLine);
        self.view.push(GitView::CommitHeader);
        if let Some(branch) = office.branch.get(office.viewed_branch) {
            for (from_commit, _) in branch.commits.iter().take(self.limit_view).enumerate() {
                self.view.push(GitView::CommitMember {
                    from_branch: office.viewed_branch,
                    from_commit,
                });
                //self.view.push(GitView::ViewMore);
            }
        }
        //self.view.push(GitView::Command);
    }

    fn rebuild_format(&mut self, office: &Office) {
        self.format.clear();
        for view in &self.view {
            match view {
                //GitView::Padding => (),
                GitView::NewLine | GitView::Command => self.format.push('\n'),
                GitView::StatusHeader => {
                    self.format.push_str("# Status:");
                    self.format.push('\n');
                }
                GitView::StatusMember { from_file } => {
                    let (file, _) = &office.status[*from_file];
                    self.format.push_str(&file.to_string_lossy());
                    self.format.push('\n');
                }
                GitView::BranchHeader => {
                    self.format.push_str(GitView::BRANCH_HEADER);
                    //self.format.push_str(&self.branch[*i].name);
                    self.format.push('\n');
                }
                GitView::BranchMember(i) => {
                    //self.format.push_str("- ");
                    self.format.push_str(&office.branch[*i].name);
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
                    let commit = &office.branch[*from_branch].commits[*from_commit];
                    //self.format.push_str("- ");
                    self.format.push_str(&commit.to_string()[..8]);
                    self.format.push('\n');
                }
            }
        }
    }

    fn root_action(
        &mut self,
        conn: &mut Uniq,
        root_action: RootAction,
        office: &mut Office,
        uri: &lsp_types::Uri,
        wg: &mut Option<(Uri, WorkGroup)>,
    ) {
        match root_action {
            RootAction::AttemptCommit => {
                //let path = PathBuf::from(office.cache.join(INPUT_MOUTH));

                //let uri = name_to_url(&path).unwrap();
                let show = ShowDocumentParams {
                    uri: office.ma_input.clone(),
                    external: Some(false),
                    take_focus: Some(true),
                    selection: None,
                };

                *wg = Some((
                    office.ma_input.clone(),
                    WorkGroup::InputBuffer(InputBuffer::AcceptCommit),
                ));
                conn.req(ShowDocument::METHOD, &show);
            }
            RootAction::Reload => {
                office.manifest.clear();
                office.re_fill_status();
                office.reload_branch();
                self.limit_view = GitView::LIMIT_VIEW;
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
            }
            RootAction::StageFile(file_id) => {
                let (file, status) = &office.status[file_id];
                office.repo.index().map(|mut index| {
                    if status.is_wt_deleted() {
                        index.remove_path(&file);
                    } else {
                        index.add_path(&file);
                    }
                    index.write();
                });
                office.re_fill_status();
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //Some(root_view.refresh(uri, office))
            }
            RootAction::UnstageFile(file_id) => {
                let (file, _) = &office.status[file_id];
                office.repo.index().map(|mut index| {
                    index.remove_path(&file);
                    index.write();
                });
                office.re_fill_status();
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //Some(root_view.refresh(uri, office))
            }
            RootAction::StatusReload => {
                office.re_fill_status();
        
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //Some(root_view.refresh(uri, office))
            }
            RootAction::ViewBranch(b) => {
                office.viewed_branch = b;
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //Some(root_view.refresh(uri, office))
            }
            RootAction::MergeBranch(b) => {
                let branch_ref = &office.branch[b];
                let branch = office
                    .repo
                    .find_branch(&branch_ref.name, branch_ref.b_type)
                    .unwrap();
                let reference = branch.get();
                let annotated = office
                    .repo
                    .reference_to_annotated_commit(reference)
                    .unwrap();
                let (anal, _) = office.repo.merge_analysis(&[&annotated]).unwrap();

                if anal.is_up_to_date() {
                    return;
                }
                let mut head = office.repo.head().unwrap();

                //if anal.is_fast_forward() {
                //    let target = office.repo.find_commit(annotated.id()).unwrap();
                //    head.set_target(target.id(), "Fast-forward");
                //    office.repo.checkout_tree(target.as_object(), Some(CheckoutBuilder::new().safe()));
                    //office
                    //    .repo
                    //    .checkout_head(None);
                //    RootView::reload(office, conn);
                    //conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //    return;
                //}
                let our = head.peel_to_commit().unwrap();
                office.repo.merge(&[&annotated], None, None);
                let mut index = office.repo.index().unwrap();
                let tree_id = index.write_tree().unwrap();
                let tree = office.repo.find_tree(tree_id).unwrap();
                let their = office.repo.find_commit(annotated.id()).unwrap();
                let signature = office.repo.signature().unwrap();
                office.repo.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    &format!("Merge branch '{}'", &branch_ref.name),
                    &tree,
                    &[&our, &their],
                );
                office.repo.checkout_head(None);
                office.repo.cleanup_state();
                RootView::reload(office, conn);
                //conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office)); 
            }
            //RootAction::MergeBranch(b) => {
            //    let show = ShowDocumentParams {
            //        uri: office.ma_input.clone(),
            //        external: Some(false),
            //        take_focus: Some(true),
            //        selection: None,
            //    };

            //    *wg = Some((
            //        office.ma_input.clone(),
            //        WorkGroup::InputBuffer(InputBuffer::AcceptMerge(b)),
            //    ));
            //    conn.req(ShowDocument::METHOD, &show);
            //}
            RootAction::CheckoutBranch(b) => {
                let branch = office
                    .repo
                    .find_branch(&office.branch[b].name, office.branch[b].b_type)
                    .unwrap();
                let tree = branch.get().peel_to_tree().unwrap();

                if let Err(e) = office
                    .repo
                    .checkout_tree(tree.as_object(), Some(CheckoutBuilder::new().safe()))
                {
                    conn.conn.log(e.to_string());
                    //return;
                }

                if let Err(e) = office.repo.set_head(branch.get().name().unwrap()) {
                    conn.conn.log(e.to_string());
                    //return;
                }
                office.viewed_branch = b;
                office.head_branch = b;
 
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //Some(root_view.refresh(uri, office))
            }
            RootAction::ViewMore => {
                self.limit_view += GitView::LIMIT_VIEW;
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //Some(root_view.refresh(uri, office))
            }
            RootAction::ViewLess => {
                if self.limit_view < GitView::LIMIT_VIEW {
                    return;
                }
                self.limit_view -= GitView::LIMIT_VIEW;
                conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                //Some(root_view.refresh(uri, office))
            }
        }
    }
}

impl Lsp for Client {
    fn did_save(&mut self, params: DidSaveTextDocumentParams) {
        let url = params.text_document.uri;
        match self.work_group.get_mut(&url) {
            Some((WorkGroup::InputBuffer(input_state), office_id)) => {
                input_state.commit(
                    &mut self.office[*office_id],
                    &mut Uniq::make(&self.conn),
                    params.text.unwrap_or_default(),
                );
                //self.conn.req(, id, result);
            }
            _ => (),
        }
    }
    /// Yes, this is cursed.
    /// LSP has no standardized server-side notification for which
    /// CodeAction the user selected. I therefore decided to
    /// write a serialized data into the top of ma.md and clean it
    /// immediately and deserialized it here. And that act the "canonical"
    /// action that the user selected. I hate it too btw
    /// If LSP ever grows a proper two way action-selection mechanism,
    /// DELETE THIS.
    fn did_change(&mut self, params: DidChangeTextDocumentParams) {
        let mut company = None;
        let mut office_clone = 0usize;
        if let Some((wg, office_id)) = self.work_group.get_mut(&params.text_document.uri) {
            let office = &mut self.office[*office_id];
            office_clone = *office_id;
            match wg {
                WorkGroup::RootView(root_view) => {
                    let text = &params.content_changes[0];
                    if let Some(range) = text.range {
                        let idx = range.start.line as usize;

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
                                let mut conn = Uniq::make(&self.conn);
                                conn.req(ApplyWorkspaceEdit::METHOD, &we);
                                //self.conn
                                //    .req(ApplyWorkspaceEdit::METHOD, Conn::beta_req(), &we);
                                RootAction::from_str(&text.text).map(|root_action| {
                                    root_view.root_action(
                                        &mut conn,
                                        root_action,
                                        office,
                                        &params.text_document.uri,
                                        &mut company,
                                    );
                                });
                            }
                            _ => {}
                        }
                    }
                }
                _ => (),
            }
        }

        if let Some((uri, wg)) = company {
            self.work_group.insert(uri, (wg, office_clone));
        }
    }

    fn inlay_hint(&mut self, id: RequestId, params: InlayHintParams) {
        let uri = params.text_document.uri;
        let start = params.range.start.line;
        let end = params.range.end.line;
        self.work_group.get(&uri).map(|(wg, office_id)| {
            let office = &mut self.office[*office_id];
            let mut ret = Vec::new();
            match wg {
                WorkGroup::RootView(root_view) => {
                    for i in start..end {
                        if let Some(hint) = root_view.inlay_hint(i, office) {
                            ret.push(hint);
                        }
                        //ret.push(root_view.inlay_hint(i, office));
                    }
                    self.conn.ok(id, &ret);
                }
                WorkGroup::DiffView(diff) => {
                    for i in start..end {
                        if let Some(hint) = diff.inlay_hint(i) {
                            ret.push(hint);
                        }
                        //ret.push(root_view.inlay_hint(i, office));
                    }
                    self.conn.ok(id, &ret);
                }
                WorkGroup::InputBuffer(ib) => {
                    if let Some(hint) = ib.inlay_hint() {
                        self.conn.ok(id, &hint);
                    }
                }
                _ => (),
            }
        });
    }

    fn hover(&mut self, id: RequestId, params: HoverParams) {
        let idx = params.text_document_position_params.position.line as usize;
        let uri = params.text_document_position_params.text_document.uri;
        if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
            let office = &mut self.office[*office_id];
            match wg {
                WorkGroup::RootView(root_view) => {
                    let res = root_view.hover(idx, office);
                    if let Some(hover) = res {
                        self.conn.ok(id, &hover);
                    }
                }
                _ => (),
            }
            //WorkGroup::DiffView(diff) => todo!(),
            //WorkGroup::FileView => todo!(),
        }
    }

    fn goto_definition(&mut self, id: RequestId, params: GotoDefinitionParams) {
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
        self.conn.ok(id, &result);
        if let Some((uri, wg)) = new_work_group {
            self.work_group.insert(uri, (wg, off_id));
        }
    }

    fn code_action(&mut self, id: RequestId, params: CodeActionParams) {
        //return;
        if let Some((wg, office_id)) = self.work_group.get_mut(&params.text_document.uri) {
            let office = &mut self.office[*office_id];
            let idx = params.range.start.line as usize;
            let uri = params.text_document.uri;
            if let Some(result) = Self::work_group_action(wg, idx, office, &uri) {
                self.conn.ok(id, &result);
            }
        }
    }

    fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        match self.work_group.get_mut(&uri) {
            Some((wg, office_id)) => match wg {
                WorkGroup::RootView(root_view) => {
                    self.conn.req(
                        ApplyWorkspaceEdit::METHOD,
                        Conn::alpha_req(),
                        &root_view.refresh(&uri, &mut self.office[*office_id]),
                    );
                }
                _ => (),
            },
            None => {
                let path = uri.path();
                let p = std::path::PathBuf::from(path.as_str());
                if let Some(s) = p.file_name() {
                    match s.to_str() {
                        Some(ROOT_NAME) => {
                            let parent = PathBuf::from(p.parent().unwrap());
                            let p = parent.strip_prefix("/").unwrap();
                            Office::new(uri.clone(), p).map(|mut office| {
                                let mut root_view = Self::new_root(&mut office);

                                // clean up old stale cache if exist
                                fs::remove_dir_all(&office.cache);

                                root_view.rebuild_view(&mut office);
                                root_view.rebuild_format(&office);

                                self.office.push(office);

                                let mut wf = HashMap::new();
                                wf.insert(
                                    uri.clone(),
                                    vec![TextEdit {
                                        new_text: root_view.format.clone(),
                                        range: Range {
                                            start: Position::new(0, 0),
                                            end: Position::new(root_view.view.len() as u32, 0),
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
                                self.work_group.insert(
                                    uri,
                                    (WorkGroup::RootView(root_view), self.office.len() - 1),
                                );
                                self.conn
                                    .req(ApplyWorkspaceEdit::METHOD, Conn::alpha_req(), &we);
                            });
                        }
                        Some(INPUT_MOUTH) => {
                            //self.work_group.insert(uri, (WorkGroup::InputBuffer))
                        }
                        _ => (),
                    }
                }
            }
        }
    }
}

impl Client {
    fn work_group_action(
        wg: &mut WorkGroup,
        idx: usize,
        office: &Office,
        uri: &lsp_types::Uri,
    ) -> Option<impl Serialize + use<>> {
        match wg {
            WorkGroup::RootView(root_view) => {
                match root_view.view[idx] {
                    GitView::Command => Some(RootAction::pack(
                        uri,
                        &[(
                            "Reload?",
                            RootAction::Reload,
                            Some(CodeActionKind::REFACTOR),
                        )],
                    )),
                    GitView::StatusHeader => {
                        //let index = office.repo.index().unwrap();
                        if !has_staged_changes(&office.repo) {
                            return Some(RootAction::pack(
                                uri,
                                &[(
                                    "Reload status?",
                                    RootAction::StatusReload,
                                    Some(CodeActionKind::REFACTOR),
                                )],
                            ));
                        } else {
                            return Some(RootAction::pack(
                                uri,
                                &[
                                    (
                                        "Commit?",
                                        RootAction::AttemptCommit,
                                        Some(CodeActionKind::SOURCE),
                                    ),
                                    (
                                        "Reload status?",
                                        RootAction::StatusReload,
                                        Some(CodeActionKind::REFACTOR),
                                    ),
                                ],
                            ));
                        }
                    }
                    GitView::StatusMember { from_file } => {
                        let (_, status) = &office.status[from_file];
                        let staged = status.intersects(
                            Status::INDEX_NEW
                                | Status::INDEX_MODIFIED
                                | Status::INDEX_DELETED
                                | Status::INDEX_RENAMED
                                | Status::INDEX_TYPECHANGE,
                        );
                        if staged {
                            Some(RootAction::pack(
                                uri,
                                &[
                                    (
                                        "Unstage?",
                                        RootAction::UnstageFile(from_file),
                                        Some(CodeActionKind::QUICKFIX),
                                    ),
                                    (
                                        "Update file?",
                                        RootAction::StageFile(from_file),
                                        Some(CodeActionKind::QUICKFIX),
                                    ),
                                ],
                            ))
                        } else {
                            Some(RootAction::pack(
                                uri,
                                &[(
                                    "Stage file?",
                                    RootAction::StageFile(from_file),
                                    Some(CodeActionKind::QUICKFIX),
                                )],
                            ))
                        }
                    }
                    GitView::BranchMember(i) => {
                        //let vec = ;
                        let branch = &office.branch[i];
                        match branch.b_type {
                            BranchType::Local => {
                                match (office.head_branch == i, office.viewed_branch == i) {
                                    (true, true) => None,
                                    (true, false) => Some(RootAction::pack(
                                        uri,
                                        &[(
                                            format!("View {}?", &branch.name),
                                            RootAction::ViewBranch(i),
                                            None,
                                        )],
                                    )),
                                    (false, true) => {
                                        match office
                                            .repo
                                            .find_branch(&branch.name, branch.b_type)
                                            .ok()
                                        {
                                            None => Some(RootAction::pack(
                                                uri,
                                                &[(
                                                    format!("Checkout {}?", &branch.name),
                                                    RootAction::CheckoutBranch(i),
                                                    None,
                                                )],
                                            )),
                                            Some(b) => {
                                                let head = office.repo.head().unwrap();
                                                let current = head.peel_to_commit().unwrap();
                                                let subject = b.get().peel_to_commit().unwrap();
                                                let index = office
                                                    .repo
                                                    .merge_commits(&current, &subject, None)
                                                    .unwrap();
                                                if index.has_conflicts() {
                                                    return Some(RootAction::pack(
                                                        uri,
                                                        &[(
                                                            format!("Checkout {}?", &branch.name),
                                                            RootAction::CheckoutBranch(i),
                                                            None,
                                                        )],
                                                    ));
                                                }
                                                Some(RootAction::pack(
                                                    uri,
                                                    &[
                                                        (
                                                            format!("Checkout {}?", &branch.name),
                                                            RootAction::CheckoutBranch(i),
                                                            None,
                                                        ),
                                                        (
                                                            "Merge to HEAD?".into(),
                                                            RootAction::MergeBranch(i),
                                                            None,
                                                        ),
                                                    ],
                                                ))
                                            }
                                        }
                                    }
                                    (false, false) => Some(RootAction::pack(
                                        uri,
                                        &[
                                            (
                                                format!("View {}?", &branch.name),
                                                RootAction::ViewBranch(i),
                                                None,
                                            ),
                                            (
                                                format!("Checkout {}?", &branch.name),
                                                RootAction::CheckoutBranch(i),
                                                None,
                                            ),
                                        ],
                                    )),
                                }
                            }
                            BranchType::Remote => Some(RootAction::pack(
                                uri,
                                &[(
                                    format!("View {}?", &office.branch[i].name),
                                    RootAction::ViewBranch(i),
                                    Some(CodeActionKind::SOURCE),
                                )],
                            )),
                        }
                        //Some(ApplyWorkspaceEdit)
                    }
                    GitView::CommitHeader => Some(RootAction::pack(
                        uri,
                        &[
                            (
                                "View more?",
                                RootAction::ViewMore,
                                Some(CodeActionKind::REFACTOR),
                            ),
                            (
                                "View Less?",
                                RootAction::ViewLess,
                                Some(CodeActionKind::REFACTOR),
                            ),
                        ],
                    )),
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
                    let oid = office.branch[*from_branch].commits[*from_commit];
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
            _ => (None, None),
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

    fn new_open_file(office: &mut Office, oid: Oid, path: &Path) -> Option<(Uri, String)> {
        if let Ok(commit) = office.repo.find_commit(oid) {
            let tree = commit.tree().unwrap();
            if let Ok(entry) = tree.get_path(path) {
                if entry.kind() != Some(ObjectType::Blob) {
                    return None;
                }
                if let Ok(blob) = office.repo.find_blob(entry.id()) {
                    let p = office.cache.join(oid.to_string()).join(path);
                    //if let Err(_) = fs::create_dir_all(p.parent().unwrap()) {
                    //    return None;
                    //}

                    //if let Err(_) = fs::write(&p, blob.content()) {
                    //    return None;
                    //}
                    let uri = name_to_url(&p).unwrap();
                    office.file_cache.insert(p, uri.clone());
                    return Some((uri, String::from_utf8_lossy(blob.content()).to_string()));
                    //return (So);
                }
            }
        }
        None
    }

    fn new_root(office: &mut Office) -> RootView {
        //let repo = &office.repo;
        //let branch = {
        let mut root = RootView {
            //branch: Vec::new(),
            //viewed_branch: 0,
            //head_branch: 0,
            limit_view: GitView::LIMIT_VIEW,
            format: String::new(),
            view: Vec::new(),
        };
        //office.reload_branch();

        root
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
    //let req = ShowDocumentParams {
    //    uri: todo!(),
    //    external: todo!(),
    //    take_focus: todo!(),
    //    selection: todo!(),
    //};
    let (conn, io_t) = Connection::stdio();
    let caps = ServerCapabilities {
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(true),
                })),
                ..Default::default()
            },
        )),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
            DiagnosticOptions::default(),
        )),
        //folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
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
    let mut client = Client::new(conn.sender.clone());
    for msg in &conn.receiver {
        match msg {
            Message::Request(request) => client.handle_request(request),
            Message::Response(response) => (),
            Message::Notification(notification) => client.handle_notification(notification),
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

fn markdown_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        // Systems
        Some("rs") => "rust",
        Some("c") => "c",
        Some("h") => "c",
        Some("cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx") => "cpp",
        Some("m") => "objective-c",
        Some("mm") => "objective-cpp",
        Some("zig") => "zig",
        Some("asm" | "s") => "asm",

        // JVM
        Some("java") => "java",
        Some("kt" | "kts") => "kotlin",
        Some("scala") => "scala",
        Some("groovy") => "groovy",

        // .NET
        Some("cs") => "csharp",
        Some("fs" | "fsx" | "fsi") => "fsharp",
        Some("vb") => "vbnet",

        // JavaScript / TypeScript
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("jsx") => "jsx",
        Some("ts") => "typescript",
        Some("tsx") => "tsx",

        // Python / scripting
        Some("py" | "pyw" | "pyi") => "python",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("pl" | "pm") => "perl",
        Some("lua") => "lua",
        Some("r") => "r",
        Some("dart") => "dart",
        Some("ex" | "exs") => "elixir",
        Some("erl" | "hrl") => "erlang",
        Some("clj" | "cljs" | "cljc") => "clojure",
        Some("groovy") => "groovy",

        // Go / functional
        Some("go") => "go",
        Some("hs" | "lhs") => "haskell",
        Some("ml" | "mli") => "ocaml",
        Some("fs" | "fsx") => "fsharp",
        Some("swift") => "swift",

        // Web
        Some("html" | "htm") => "html",
        Some("css") => "css",
        Some("scss") => "scss",
        Some("sass") => "sass",
        Some("less") => "less",
        Some("vue") => "vue",
        Some("svelte") => "svelte",
        Some("astro") => "astro",

        // Data / config
        Some("json" | "jsonc") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("xml") => "xml",
        Some("ini") => "ini",
        Some("csv") => "csv",

        // Shell
        Some("sh" | "bash") => "bash",
        Some("zsh") => "zsh",
        Some("fish") => "fish",
        Some("ps1" | "psm1") => "powershell",
        Some("bat" | "cmd") => "bat",

        // SQL
        Some("sql") => "sql",

        // Mobile
        Some("swift") => "swift",
        Some("m") => "objective-c",
        Some("dart") => "dart",

        // Functional / misc
        Some("clj" | "cljs" | "cljc") => "clojure",
        Some("lisp" | "lsp" | "cl") => "lisp",
        Some("sol") => "solidity",
        Some("asm" | "s") => "asm",
        Some("v") => "verilog",
        Some("vhd" | "vhdl") => "vhdl",

        // Documentation / markup
        Some("md" | "markdown" | "mdx") => "markdown",
        Some("rst") => "rst",
        Some("tex") => "latex",

        // Default
        _ => "text",
    }
}

fn merge_branch(repo: &Repository, branch_name: &str) -> Result<(), Error> {
    // Current branch / HEAD
    let head = repo.head()?;
    let current = head.peel_to_commit()?;

    // The branch we're merging INTO current HEAD
    let branch = repo.find_branch(branch_name, BranchType::Local)?;
    let other = branch.get().peel_to_commit()?;

    let annotated = repo.find_annotated_commit(other.id())?;

    // Ask Git what kind of merge this is.
    let (analysis, _) = repo.merge_analysis(&[&annotated])?;

    if analysis.is_up_to_date() {
        println!("Already up to date.");
        return Ok(());
    }

    if analysis.is_fast_forward() {
        // No merge commit needed.
        //
        // Move the current branch to the other commit.
        let branch_ref = head.name().unwrap();
        repo.reference(
            branch_ref,
            other.id(),
            true,
            &format!("Fast-forward merge '{branch_name}'"),
        )?;
        //let tree = branch.get().peel_to_tree().unwrap();

        repo.checkout_tree(
            other.tree()?.as_object(),
            Some(CheckoutBuilder::new().safe()),
        )?;

        //repo.checkout_head(None)?;

        return Ok(());
    }
    let i = repo.merge_commits(&current, &other, None).unwrap();
    i.has_conflicts();
    for conf in i.conflicts().unwrap() {
        conf.map(|conflict| {
            conflict.ancestor; // common ancestor duh
            conflict.our; // HEAD
            conflict.their; // commit yang mau merge
        });
    }
    // Real three-way merge.
    //repo.merge(&[&annotated], None, None)?;

    // Check whether Git produced conflicts.
    let mut index = repo.index()?;

    if index.has_conflicts() {
        println!("Merge has conflicts!");

        // Don't create the merge commit yet.
        //
        // The user needs to resolve the conflicts first.
        return Ok(());
    }

    // The merge result is now in the index.
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let signature = repo.signature()?;

    // Current HEAD + merged branch HEAD
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        &format!("Merge branch '{branch_name}'"),
        &tree,
        &[&current, &other],
    )?;

    // Update working tree to match the new commit.
    repo.checkout_tree(tree.as_object(), Some(CheckoutBuilder::new().safe()))?;

    //repo.checkout_head(None)?;

    Ok(())
}

fn has_staged_changes(repo: &git2::Repository) -> bool {
    let index = repo.index().unwrap();

    let Ok(head) = repo.head() else {
        return !index.is_empty();
    };

    let head_tree = head.peel_to_tree().unwrap();

    let diff = repo
        .diff_tree_to_index(Some(&head_tree), Some(&index), None)
        .unwrap();

    diff.deltas().next().is_some()
}

fn format_git_time(time: Time) -> Option<String> {
    if let Some(offset) = FixedOffset::east_opt(time.offset_minutes() * 60) {
        if let Some(dt) = offset.timestamp_opt(time.seconds(), 0).single() {
            return Some(dt.format("%a %b %d %H:%M:%S %Y %z").to_string());
        }
    }
    None
}
