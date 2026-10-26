use std::path::PathBuf;

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
    Padding,
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
    Padding,
    Path(usize),
}
