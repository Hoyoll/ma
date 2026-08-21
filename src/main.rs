use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf, Prefix},
    process::Command,
    str::FromStr,
};

use git2::{BranchType, Commit, Oid, Repository, Sort};
use lsp_server::{Connection, Message, RequestId, Response};
use lsp_types::{
    DefinitionOptions, DidOpenTextDocumentParams, FoldingRangeParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, Location, MarkupContent, OneOf, Range, ServerCapabilities, Uri, lsp_request,
    notification::{DidOpenTextDocument, Notification},
    request::{FoldingRangeRequest, GotoDefinition, HoverRequest, Initialize, Request},
};
use serde::{Deserialize, Serialize};

const ROOT_NAME: &str = "ma.md";
const DIFF_NAME: &str = "ma.diff";
const GIT_DIR: &str = ".git";
const CACHE_DIR: &str = "ma-cache";
type RepoId = usize;

struct Client {
    work_group: HashMap<Uri, WorkGroup>,
    repo: Vec<Repository>,
}

impl Client {
    fn new() -> Self {
        Self {
            work_group: HashMap::default(),
            repo: Vec::default(),
        }
    }
}

type Root = usize;

enum WorkGroup {
    RootView(RootView),
    DiffView(Diff),
    FileView(FileView),
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
    Hunk { from_hunk: usize, change_on: usize },
}

struct Hunk {
    path: PathBuf,
    changes: Vec<LineCol>,
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
            0 => Self::Root { hunk: Vec::new(), view: Vec::new() },
            1 => Self::Normal { hunk: Vec::new(), view: Vec::new(), parent: commit.parent(0).unwrap().id() },
            _ => {
                let parents = {
                    let mut p = Vec::new();
                    for c in commit.parents() {
                        p.push(c.id());
                    }
                    p
                };
                Self::Merge { view: Vec::new(), parents }
            }
        }
    }
}

struct Diff {
    author: String,
    date: String,
    hash: Oid,
    message: String,
    format: String,
    diff_vew: DiffView,
    repo_id: RepoId,
}

impl Diff {

    fn fill(&mut self, repo: &mut Repository) {

    }
    fn rebuild_view_and_format(&mut self, repo: &mut Repository) {
        match &mut self.diff_vew {
            DiffView::Merge {
                view,
                parents,
                //hash,
                //format,
            } => {
                view.clear();
                view.push(MergeView::Padding);
                let format = &mut self.format;
                //let commit = repo.find_commit(*hash).unwrap();
                format.push_str("# ");
                format.push_str(&self.hash.to_string());
                format.push('\n');

                view.push(MergeView::Padding);

                format.push_str("Author: ");
                //let author = commit.author();
                format.push_str(&self.author);
                format.push('\n');

                view.push(MergeView::Padding);

                format.push_str("Date: ");
                // TO-DO: Date!
                format.push_str(&self.date);
                format.push('\n');
                view.push(MergeView::Padding);

                format.push('\n');
                view.push(MergeView::Padding);

                format.push_str(&self.message);
                format.push('\n');
                view.push(MergeView::Padding);

                format.push('\n');
                view.push(MergeView::Padding);

                format.push_str("# Parents:");
                format.push('\n');
                view.push(MergeView::Padding);

                for (i, p) in parents.iter().enumerate() {
                    format.push_str("- ");
                    format.push_str(&p.to_string());
                    format.push('\n');
                    view.push(MergeView::Commit(i));
                }
            }
            DiffView::Normal {
                hunk,
                view,
                //hash,
                parent,
                //format,
            } => {
                view.clear();
                view.push(NormalView::Padding);
                let format = &mut self.format;
                format.push_str("# Old: ");
                format.push_str(&parent.to_string());
                format.push('\n');

                format.push_str("# ");
                format.push_str(&self.hash.to_string());
                format.push('\n');

                view.push(NormalView::Padding);

                format.push_str("Author: ");
                //let author = commit.author();
                format.push_str(&self.author);
                format.push('\n');

                view.push(NormalView::Padding);

                format.push_str("Date: ");
                // TO-DO: Date!
                format.push('\n');
                view.push(NormalView::Padding);

                format.push('\n');
                view.push(NormalView::Padding);

                format.push_str(&self.message);
                format.push('\n');
                view.push(NormalView::Padding);

                format.push('\n');
                view.push(NormalView::Padding);
                let commit = repo.find_commit(self.hash).ok().unwrap();
                let tree = commit.tree().ok();
                let parent = commit.parent(0).unwrap().tree().ok();
                let diff = repo.diff_tree_to_tree(parent.as_ref(), tree.as_ref(), None);
 
                for (i, hunk) in hunk.iter().enumerate() {                    
                }
            }
            DiffView::Root {
                hunk,
                view,
                //hash,
                //format,
            } => {
                view.clear();
                view.push(NormalView::Padding);
                let commit = repo.find_commit(self.hash).ok().unwrap();
                let tree = commit.tree().ok();
                let diff = repo.diff_tree_to_tree(None, tree.as_ref(), None);
            }
        }
    }
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
    repo_id: RepoId,
    cache: PathBuf,
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
        if let Some(workspace) = params.workspace_folders {
            for w in workspace {}
        }
    }

    fn hover(&mut self, conn: &Connection, id: RequestId, params: HoverParams) {
        let idx = params.text_document_position_params.position.line as usize;
        //params.text_document_position_params.text_document.uri
    }

    fn goto_definition(&mut self, conn: &Connection, id: RequestId, params: GotoDefinitionParams) {
        let idx = params.text_document_position_params.position.line as usize;
        match self
            .work_group
            .get_mut(&params.text_document_position_params.text_document.uri)
        {
            None => (),
            Some(WorkGroup::RootView(root)) => {
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
                            (oid, None) => {
                                let mut path = root.cache.clone();
                                let repo = &mut self.repo[root.repo_id];
                                //self.repo[root.repo_id].find_commit(*oid);
                                // do the bullshit here
                                //path.push(oid.to_string());
                                //path.push("patch.diff"); 
                            }
                        }
                    }
                    _ => (),
                }
            }
            Some(WorkGroup::DiffView(diff)) => {}
            _ => (),
        }
    }

    fn folding(&mut self, conn: &Connection, id: RequestId, params: FoldingRangeParams) {}

    fn did_open(&mut self, conn: &Connection, params: DidOpenTextDocumentParams) {
        match self.work_group.get_mut(&params.text_document.uri) {
            Some(_) => (),
            None => self.new_workgroup(conn, params.text_document.uri),
        }
    }
}

impl Client {
    fn new_workgroup(&mut self, conn: &Connection, uri: lsp_types::Uri) {
        let path = uri.path();
        let mut p = std::path::PathBuf::from(path.as_str());
        p.file_name().map(|s| match s.to_str() {
            Some(ROOT_NAME) => match Repository::open(p.parent().unwrap()) {
                Err(_) => (),
                Ok(repo) => {
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
                                            name: branch.0.name().unwrap().unwrap().to_string(),
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
                    self.repo.push(repo);
                    let rootview = RootView {
                        branch,
                        format: String::default(),
                        view: Vec::default(),
                        repo_id: {self.repo.len() - 1},
                        cache,
                    };
                    self.work_group.insert(uri, WorkGroup::RootView(rootview));
                }
            },
            _ => (),
        });
    }
}

fn main() {
    let repo = Repository::open(".").unwrap();
    let br = repo.branches(None).unwrap();
    for b in br {
        let branch = b.unwrap();
        let name = branch.0.name().unwrap().unwrap_or_default();
        println!("{}", name);
        let branch = repo.find_branch(name, BranchType::Local).unwrap();

        let commit = branch.get().peel_to_commit().unwrap();
        let mut revwalk = repo.revwalk().unwrap();

        revwalk.push(commit.id());
        revwalk.set_sorting(Sort::TIME);

        for oid in revwalk {
            let oid = oid.unwrap();
            let commit = repo.find_commit(oid).unwrap();
            let tree = commit.tree().unwrap();

            for i in 0..commit.parent_count() {
                let parent_tree = commit.parent(i).unwrap().tree().ok();
                let diff = repo
                    .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
                    .unwrap();
                let mut patch = String::new();
                //diff.print(format, cb)
                diff.foreach(
                    &mut |delta, _progress| {
                        println!("FILE: {:?}", delta.new_file().path());
                        patch.push_str(delta.new_file().path().unwrap().to_str().unwrap());
                        true
                    },
                    None,
                    Some(&mut |delta, hunk| {
                        patch.push_str("here!");
                        println!(
                            "HUNK: -{},{} +{},{}",
                            hunk.old_start(),
                            hunk.old_lines(),
                            hunk.new_start(),
                            hunk.new_lines(),
                        );

                        true
                    }),
                    Some(&mut |delta, hunk, line| {
                        print!(
                            "{}{}",
                            line.origin(),
                            String::from_utf8_lossy(line.content())
                        );

                        true
                    }),
                );
            } //let mut patch = Vec::new();

            //diff.print(git2::DiffFormat::Patch, |delta, hunk, line| {
            //    println!("{}",delta.new_file().path().unwrap().to_str().unwrap());
            //    hunk.map(|hunk| {
            //        println!("{}", String::from_utf8_lossy(hunk.header()));
            //    });
            //    //println!("{}", String::from_utf8_lossy(hunk.unwrap().header()));
            //    println!("{}", String::from_utf8_lossy(line.content()));
            //    //patch.extend_from_slice(line.content());
            //    true
            //})
            //.unwrap();

            //println!("{} {}", commit.id(), commit.summary().unwrap().unwrap());
        }
    }

    //repo.find_branch(name, branch_type)

    return;
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
    let mut client = Client::new();
    for msg in &conn.receiver {
        match msg {
            Message::Request(request) => client.handle_request(&conn, request),
            Message::Response(response) => todo!(),
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
