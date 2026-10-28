use std::{collections::HashMap, path::PathBuf};

use lsp_types::{ApplyWorkspaceEditParams, CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionResponse, Hover, Position, Range, TextEdit, WorkspaceEdit, request::{ApplyWorkspaceEdit, Request}};

use crate::{app, lsp};

pub enum WorkGroup {
    RootView(Root),
    DiffView(Diff),
    InputBuffer(InputBuffer),
    TreeView(Tree),
    FileView,
}

struct Root {
    limit_view: usize,
    //format: String,
    view: Vec<RootView>,
}

#[derive(Clone, Copy, Debug)]
enum RootView {
    RootHead,
    NewLine,
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
}

#[derive(serde::Serialize, serde::Deserialize)]
enum RootAction {
    Reload,
    AttemptCommit,
    StatusReload,
    StageFile(usize),
    UnstageFile(usize),
    MergeBranch(usize),
    ViewBranch(usize),
    CheckoutBranch(usize),
    ViewMore,
    ViewLess,
    ViewTree {
        from_branch: usize,
        from_commit: usize,
    },
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

impl Root {
    fn open_buffer(office: &app::Office, conn: &mut lsp::Uniq) {
        let show = lsp_types::ShowDocumentParams {
            uri: office.ma_root.clone(),
            external: Some(false),
            take_focus: Some(true),
            selection: None,
        };
        conn.req(lsp_types::request::ShowDocument::METHOD, &show);
    }

    fn reload(office: &app::Office, conn: &mut lsp::Uniq) {
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
            }
            RootAction::ViewTree {
                from_branch,
                from_commit,
            } => {
                let branch_data = &office.branch[from_branch];
                let commit_id = branch_data.commits[from_commit];
                let mut vt = Tree {
                    commit: commit_id,
                    path: Vec::new(),
                    view: Vec::new(),
                };
                let commit = office.repo.find_commit(commit_id).unwrap();

                let tree = commit.tree().unwrap();
                tree.walk(TreeWalkMode::PreOrder, |root, entry| {
                    if entry.kind() == Some(ObjectType::Blob) {
                        let path = format!("{}{}", root, entry.name().unwrap());
                        vt.path.push(PathBuf::from(path));
                    }
                    0
                });
                vt.fill();
                let p = office.cache.join(commit_id.to_string()).join(COMMIT_TREE);

                let url = match office.file_cache.get(&p) {
                    Some(url) => url.clone(),
                    None => {
                        if let Err(e) = fs::create_dir_all(&p.parent().unwrap()) {
                            conn.conn.log(e.to_string());
                        }
                        if let Err(e) = fs::write(&p, vt.format()) {
                            conn.conn.log(e.to_string());
                        }

                        name_to_url(&p).unwrap()
                    }
                };
                let show = ShowDocumentParams {
                    uri: url.clone(),
                    external: Some(false),
                    take_focus: Some(true),
                    selection: None,
                };
                *wg = Some((url, WorkGroup::TreeView(vt)));
                conn.req(ShowDocument::METHOD, &show);
            }
            RootAction::CheckoutBranch(b) => {
                let mut proc = process::Command::new("git");
                proc.args(&[
                    "-C",
                    &format!("{}", office.repo.workdir().unwrap().to_string_lossy()),
                    "switch",
                    &office.branch[b].name,
                ]);
                let mut git = proc.spawn().unwrap();
                match git.wait() {
                    Ok(e) if e.success() => {
                        office.viewed_branch = b;
                        office.head_branch = b;

                        conn.req(ApplyWorkspaceEdit::METHOD, &self.refresh(uri, office));
                    }
                    _ => (),
                }
                return;
                // tbh, currently i just block it out.
                panic!("How did we get here?");
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

struct Diff {
    oid: git2::Oid,
    format: String,
    diff_view: DiffView,
}

enum DiffView {
    /// if parent commit > 1
    Merge {
        view: Vec<NormalView>,
        parents: Vec<git2::Oid>,
    },
    /// if parent <= 1
    Normal {
        hunk: Vec<Hunk>,
        view: Vec<NormalView>,
        parent: Option<git2::Oid>,
    },
}

struct Hunk {
    path: PathBuf,
    changes: Vec<u32>,
}

enum NormalView {
    NewLine,
    ParentCommit,
    Hunk { from_hunk: usize, change_on: u32 },
    HunkLine { from_hunk: usize, change_on: u32 },
}

enum InputBuffer {
    AcceptNothing,
    AcceptCommit,
    ShowErrorMessage(String),
}

struct Tree {
    commit: git2::Oid,
    path: Vec<PathBuf>,
    view: Vec<TreeView>,
}

enum TreeView {
    NewLine,
    Padding,
    Path(usize),
}
