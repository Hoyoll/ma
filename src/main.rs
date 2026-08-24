use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf, Prefix},
    process::Command,
    str::FromStr,
};

use git2::{BranchType, Commit, DiffFormat, Oid, Repository, Sort};
use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    ApplyWorkspaceEditParams, DefinitionOptions, DidOpenTextDocumentParams, FoldingRangeParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, Location, LogMessageParams,
    MarkupContent, MessageType, OneOf, Position, Range, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextEdit, Uri,
    WorkspaceEdit, lsp_request,
    notification::{DidOpenTextDocument, LogMessage, Notification, ShowMessage},
    request::{FoldingRangeRequest, GotoDefinition, HoverRequest, Initialize, Request},
};
use serde::{Deserialize, Serialize};

const ROOT_NAME: &str = "ma.md";
const DIFF_NAME: &str = "ma.diff";
const GIT_DIR: &str = ".git";
const CACHE_DIR: &str = "ma-cache";
type RepoId = usize;

struct Client {
    work_group: HashMap<Uri, (WorkGroup, OfficeId)>,
    //repo: Vec<Repository>,
    office: Vec<Office>,
    caps: ServerCapabilities,
}

impl Client {
    fn new(caps: ServerCapabilities) -> Self {
        Self {
            work_group: HashMap::default(),
            //repo: Vec::default(),
            office: Vec::default(),
            caps,
        }
    }
}

type Root = usize;
type OfficeId = usize;
enum WorkGroup {
    RootView(RootView),
    DiffView(Diff),
    FileView(FileView),
}
struct Office {
    repo: Repository,
    cache: PathBuf,
}
struct FileView {
    //branch: String,
    commit: Oid,
    name: String,
}

enum MergeView {
    Padding,
    NewLine,
    Commit(usize),
}

enum NormalView {
    Padding,
    ParentCommit,
    Hunk { from_hunk: usize, change_on: u32 },
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
        parent: Oid,
        //format: String,
    },
    /// if parent == 0
    Root {
        hunk: Vec<Hunk>,
        view: Vec<NormalView>,
        //hash: Oid,
        //format: String,
    },
}

impl DiffView {
    fn new(commit: &Commit) -> Self {
        match commit.parent_count() {
            0 => Self::Root {
                hunk: Vec::new(),
                view: Vec::new(),
            },
            1 => Self::Normal {
                hunk: Vec::new(),
                view: Vec::new(),
                parent: commit.parent(0).unwrap().id(),
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

    fn format_header(commit: &Commit, format: &mut String) {}
    fn fill(&mut self, commit: &Commit, repo: &Repository, format: &mut String) {
        format.clear();
        match self {
            DiffView::Merge { view, parents } => {
                view.clear();
                view.push(MergeView::Padding);
                format.push_str("# ");
                format.push_str(&commit.id().to_string());
                format.push('\n');
                view.push(MergeView::Padding);

                format.push_str("Author: ");
                let author = commit.author();
                format.push_str(&author.name().unwrap_or_default());
                format.push_str(" ");
                format.push_str(author.email().unwrap_or_default());
                format.push('\n');

                view.push(MergeView::Padding);

                format.push_str("Date: ");
                // TO-DO: Date!
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

                //for p in parents {
                for (i, p) in parents.iter().enumerate() {
                    format.push_str("- ");
                    format.push_str(&p.to_string());
                    format.push('\n');
                    //parents.push(*p);
                    view.push(MergeView::Commit(i));
                }
                //}
            }
            DiffView::Normal {
                hunk: hunk_col,
                view,
                parent,
            } => {
                view.clear();
                view.push(NormalView::Padding);

                format.push_str("# ");
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
                format.push('\n');
                view.push(NormalView::Padding);

                format.push('\n');
                view.push(NormalView::Padding);

                format.push_str(&commit.message().unwrap_or_default());
                format.push('\n');
                view.push(NormalView::Padding);

                format.push('\n');
                view.push(NormalView::Padding);

                let tree = commit.tree().ok();
                let parent = commit.parent(0).unwrap().tree().ok();
                let diff = repo.diff_tree_to_tree(parent.as_ref(), tree.as_ref(), None);

                if let Ok(diff) = diff {
                    let mut anchor_id = Oid::ZERO_SHA1;
                    let mut hunk_current_line = 0;
                    let mut line_count = view.len() - 1;
                    let mut current_hunk = 0;
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
                                view.push(NormalView::Hunk {
                                    from_hunk: current_hunk,
                                    change_on: hunk.new_start(),
                                });
                            }
                            Some(_) => {
                                format.push(line.origin());
                                format.push(' ');
                                format.push_str(&String::from_utf8_lossy(line.content()));

                                view.push(NormalView::Padding);
                            }
                            _ => (),
                        }

                        true
                    });
                }
            }
            DiffView::Root {
                hunk: hunk_col,
                view,
            } => {
                view.clear();
                view.push(NormalView::Padding);

                format.push_str("# ");
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
                format.push('\n');
                view.push(NormalView::Padding);

                format.push('\n');
                view.push(NormalView::Padding);

                format.push_str(&commit.message().unwrap_or_default());
                format.push('\n');
                view.push(NormalView::Padding);

                format.push('\n');
                view.push(NormalView::Padding);

                let tree = commit.tree().ok();
                //let parent = commit.parent(0).unwrap().tree().ok();
                let diff = repo.diff_tree_to_tree(None, tree.as_ref(), None);

                if let Ok(diff) = diff {
                    let mut anchor_id = Oid::ZERO_SHA1;
                    let mut hunk_current_line = 0;
                    let mut line_count = view.len() - 1;
                    let mut current_hunk = 0;
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
                                view.push(NormalView::Hunk {
                                    from_hunk: current_hunk,
                                    change_on: hunk.new_start(),
                                });
                            }
                            Some(_) => {
                                format.push(line.origin());
                                format.push(' ');
                                format.push_str(&String::from_utf8_lossy(line.content()));

                                view.push(NormalView::Padding);
                            }
                            _ => (),
                        }

                        true
                    });
                }
            }
        }
    }
}

struct Diff {
    hash: Oid,
    format: String,
    diff_view: DiffView,
    //repo_id: RepoId,
}

struct Branch {
    name: String,
    commits: Vec<(Oid, Option<Uri>)>,
}

enum GitView {
    Padding,
    NewLine,
    BranchHeader(usize),
    CommitMember {
        from_branch: usize,
        from_commit: usize,
    },
}

//#[derive(Default)]
struct RootView {
    branch: Vec<Branch>,
    //branch_member: Vec<Oid>,
    //commits: Vec<(Uri, Oid)>,
    format: String,
    view: Vec<GitView>,
    //repo_id: RepoId,
    //cache: PathBuf,
}

impl RootView {
    fn rebuild_view(&mut self) {
        self.view.clear();
        self.view.push(GitView::Padding);
        for (from_branch, branch) in self.branch.iter().enumerate() {
            self.view.push(GitView::BranchHeader(from_branch));
            for (from_commit, _) in branch.commits.iter().enumerate() {
                self.view.push(GitView::CommitMember {
                    from_branch,
                    from_commit,
                });
            }
            self.view.push(GitView::NewLine);
        }
    }

    fn rebuild_format(&mut self) {
        self.format.clear();
        for view in &self.view {
            match view {
                GitView::Padding => (),
                GitView::NewLine => self.format.push('\n'),
                GitView::BranchHeader(i) => {
                    self.format.push_str("# ");
                    self.format.push_str(&self.branch[*i].name);
                    self.format.push('\n');
                }
                GitView::CommitMember {
                    from_branch,
                    from_commit,
                } => {
                    let (commit, _) = &self.branch[*from_branch].commits[*from_commit];
                    self.format.push_str("- ");
                    self.format.push_str(&commit.to_string());
                    self.format.push('\n');
                }
            }
        }
    }
}

trait Lsp {
    fn initialize(&mut self, conn: &Connection, id: RequestId, params: InitializeParams);

    fn hover(&mut self, conn: &Connection, id: RequestId, params: HoverParams);
    fn did_open(&mut self, conn: &Connection, params: DidOpenTextDocumentParams);
    fn goto_definition(&mut self, conn: &Connection, id: RequestId, params: GotoDefinitionParams);
    fn folding(&mut self, conn: &Connection, id: RequestId, params: FoldingRangeParams);
    fn handle_request(&mut self, conn: &Connection, request: lsp_server::Request) {
        let l_params = LogMessageParams {
            typ: MessageType::INFO,
            message: "HELLO FROM MY INIT METHOD".into(),
        };

        conn.sender
            .send(lsp_server::Message::Notification(
                lsp_server::Notification {
                    method: LogMessage::METHOD.to_string(),
                    params: serde_json::to_value(l_params).unwrap(),
                },
            ))
            .unwrap();

        match request.method.as_str() {
            Initialize::METHOD => {
                let l_params = LogMessageParams {
                    typ: MessageType::INFO,
                    message: "HELLO FROM MY INIT METHOD".into(),
                };

                conn.sender
                    .send(lsp_server::Message::Notification(
                        lsp_server::Notification {
                            method: LogMessage::METHOD.to_string(),
                            params: serde_json::to_value(l_params).unwrap(),
                        },
                    ))
                    .unwrap();
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
        //eprintln!("NOTIFICATION: {}", notification.method);

        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidOpenTextDocumentParams| {
                        self.did_open(conn, params);
                    },
                );
            }
            _ => (),
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

impl Lsp for Client {
    fn initialize(&mut self, conn: &Connection, id: RequestId, params: InitializeParams) {
        //conn.sender.send(Message::Response(()))
    }

    fn hover(&mut self, conn: &Connection, id: RequestId, params: HoverParams) {
        let idx = params.text_document_position_params.position.line as usize;
        //params.text_document_position_params.text_document.uri
    }

    fn goto_definition(&mut self, conn: &Connection, id: RequestId, params: GotoDefinitionParams) {
        let idx = params.text_document_position_params.position.line as usize;
        match self
            .work_group
            .get(&params.text_document_position_params.text_document.uri)
        {
            None => (),
//                log("None actually", conn);
            Some((WorkGroup::RootView(root),office_id)) => {
                match &root.view[idx] {
                    GitView::CommitMember {
                        from_branch,
                        from_commit,
                    } => {
                        match &root.branch[*from_branch].commits[*from_commit] {
                            (_, Some(uri)) => {
                                send_ok(
                                    conn,
                                    id,
                                    &Some(GotoDefinitionResponse::Scalar(Location::new(
                                        uri.clone(),
                                        Range::default(),
                                    ))),
                                );
                            }
                            (oid, uri) if *uri == None => {
                                let office = &self.office[*office_id];
                                let path = office.cache.clone().join(oid.to_string());
                                let repo = &office.repo;
                                match repo.find_commit(*oid) {
                                    Ok(commit) => {
                                        let mut format = String::new();
                                        let mut diff_view = DiffView::new(&commit);
                                        diff_view.fill(&commit, repo, &mut format);
                                        //let path = path.strip_prefix("/").unwrap();
                                        if let Err(_) = fs::create_dir_all(&path) {
                                            log("create_dir_all failed", conn);
                                        }
                                        let path = path.join(DIFF_NAME);
                                        if let Err(e) = fs::write(&path, format.as_bytes()) {
                                            log(e.to_string(), conn);
                                            log(path.to_str().unwrap(), conn);
                                            return;
                                        }
                                        let uri = name_to_url(&path).unwrap();
                                        self.work_group.insert(
                                            uri.clone(),
                                            (WorkGroup::DiffView(Diff {
                                                hash: *oid,
                                                format,
                                                diff_view,
                                            }), *office_id),
                                        );
                                        send_ok(
                                            conn,
                                            id,
                                            &Some(GotoDefinitionResponse::Scalar(Location::new(
                                                uri,
                                                Range::default(),
                                            ))),
                                        );
                                    }
                                    Err(_) => (),
                                }
                            }
                            _ => (),
                        }
                    }
                    _ => (),
                }
            }
            Some((WorkGroup::DiffView(diff), office_id)) => {
                match &diff.diff_view {
                    DiffView::Merge { view, parents } => {
                        let idx = params.text_document_position_params.position.line as usize;
                        if let MergeView::Commit(c) = &view[idx] {
                            //if let Some(d) = self.open_diff(&parents[*c], *office_id) { 
                            //}
                        }
                    }
                    DiffView::Normal { hunk, view, parent } => todo!(),
                    DiffView::Root { hunk, view } => todo!(),
                }
            }
            _ => (),
        }
    }

    fn folding(&mut self, conn: &Connection, id: RequestId, params: FoldingRangeParams) {}

    fn did_open(&mut self, conn: &Connection, params: DidOpenTextDocumentParams) {
        match self.work_group.get_mut(&params.text_document.uri) {
            Some(_) => (),
            None => {
                if let Some(mut wg) = self.new_workgroup(&params.text_document.uri) {
                    wg.rebuild_view();
                    wg.rebuild_format();
                    let format = wg.format.clone();
                    self.work_group
                        .insert(params.text_document.uri.clone(), (WorkGroup::RootView(wg), self.office.len() - 1));
                    let mut wf = HashMap::new();
                    wf.insert(
                        params.text_document.uri.clone(),
                        vec![TextEdit {
                            new_text: format,
                            range: Range {
                                start: Position::new(1, 1),
                                end: Position::new(1, 1),
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
                        RequestId::from(19),
                        "workspace/applyEdit".into(),
                        &we,
                    )));
                    //send_ok(conn, RequestId::from(1), &Some(TextEdit::new(Range { start: Position::new(1, 1), end: Position::new(1, 1) }, format)));
                }
            }
        }
    }
}

impl Client {
    fn new_workgroup(&mut self, uri: &lsp_types::Uri) -> Option<RootView> {
        let path = uri.path();
        let p = std::path::PathBuf::from(path.as_str());
        //log("HERE!", conn);
        match p.file_name() {
            Some(s) => match s.to_str() {
                Some(ROOT_NAME) => {
                    let parent = PathBuf::from(p.parent().unwrap());
                    let p = parent.strip_prefix("/").unwrap();
                    match Repository::open(&p) {
                        Err(_) => {
                            //log("It's Err actually", conn);
                            //log(format!("with name like {:?}", s), conn);
                            //log(format!("with repo pointing to: {}",p.display()), conn);
                            None
                        }
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
                                                                c.push((id, None));
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
                            self.office.push(Office { repo, cache });
                            //self.repo.push(repo);
                            Some(RootView {
                                branch,
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

    fn open_diff(&mut self,oid: &Oid, office_id: OfficeId) -> Option<(Diff, Uri)> {
        let office = &mut self.office[office_id];
        match office.repo.find_commit(*oid) {
            Ok(commit) => {
                let mut format = String::new();
                let mut diff_view = DiffView::new(&commit);
                diff_view.fill(&commit, &office.repo, &mut format);
                //let path = path.strip_prefix("/").unwrap();
                let path = office.cache.join(oid.to_string());
                if let Err(_) = fs::create_dir_all(&path) {
                    //log("create_dir_all failed", conn);
                    return None
                }
                let path = path.join(DIFF_NAME);
                if let Err(_) = fs::write(&path, format.as_bytes()) {
                    //log(e.to_string(), conn);
                    //log(path.to_str().unwrap(), conn);
                    return None;
                }
                let uri = name_to_url(&path).unwrap();
                Some((Diff {
                        hash: *oid,
                        format,
                        diff_view
                    }, uri))
            }
            Err(_) => None,
        }
    }
}

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

fn main() {
    let (conn, io_t) = Connection::stdio();
    let caps = ServerCapabilities {
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                ..Default::default()
            },
        )),
        ..Default::default()
    };

    let init = serde_json::json!({
        "capabilities": caps
    });
    //println!("{init}");
    //return;
    let init_params = conn
        .initialize(serde_json::to_value(&caps).unwrap())
        .unwrap();
    //println!("{init_params}");
    //return;
    let mut client = Client::new(caps);
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
                // A real drive renders with a bare colon (`file:///C:/…`) — the
                // form LSP clients emit and the only one that round-trips back
                // through `Path::is_absolute` on Windows. Other prefix kinds
                // (UNC, device namespaces) have no such convention, so keep the
                // safe percent-encoded form.
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
