use std::{collections::HashMap, path::PathBuf};

use crate::container::Container;

struct Office {
    repo: git2::Repository,
    cache: PathBuf,
    status: Vec<(PathBuf, git2::Status)>,
    manifest: HashMap<git2::Oid, lsp_types::Uri>,
    file_cache: HashMap<PathBuf, lsp_types::Uri>,
    ma_root: lsp_types::Uri,
    ma_input: lsp_types::Uri,
    branch: Vec<Branch>,
    head_branch: usize,
    viewed_branch: usize,
}

struct Branch {
    name: String,
    b_type: git2::BranchType,
    commits: Vec<git2::Oid>,
}

pub struct App {
    office: Container<Office>
}
