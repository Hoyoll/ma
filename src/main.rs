use std::fmt::Display;

use tokio::process::Command;
use tower_lsp_server::{Client, LspService, jsonrpc, ls_types::{self, GotoDefinitionResponse, }};

enum GitView {
    Padding,
    BranchHeader,
    BranchMember(usize), 
    CommitHeader,
    CommitMember(usize),
}

#[derive(PartialEq, Eq)]
enum Accordion {
    Expand,
    Revert,
}

// git branch --show-current
// git commands, for local branch = git branch -l --format='%(refname:short)'
// git commands, for remote branch = git branch -r --format='%(refname:short)'
// gt commands, for log = git log -n 10 --format='%h'
// git commands, for show = git show --no-patch <hash>

struct Lsp {
    client: Client,
    git_client: GitClient,
    git_view: Vec<GitView>
}

struct GitClient {
    branch: (usize, String, Accordion),
    branch_member: Vec<String>,
    commit: (usize, Accordion),
    commits: Vec<String>,
    format: String,
    view: Vec<GitView>
}

impl GitClient { 
    pub async fn get_branches(&mut self) { 
        let output = Command::new("git")
            .args(&["branch", "-;l", "--format=%(refname:short)"])
            .output().await.unwrap();
        self.branch_member = String::from_utf8(output.stdout).unwrap()
            .lines()
            .map(str::to_owned)
            .collect(); 
    }

    pub async fn view(&mut self) {
        self.view.clear();
        self.view.push(GitView::Padding);
        self.view.push(GitView::BranchHeader);
        if self.branch.2 == Accordion::Expand {
            let mut i: usize = 0;
            for _ in &self.branch_member {
                self.view.push(GitView::BranchMember(i));
                i += 1
            }
        }
    }

    pub async fn format(&mut self) {
        self.format.clear();
        // # Branch: <branch>
        self.format.push_str("# Branch: ");
        self.format.push_str(&self.branch.1);
        self.format.push_str("\n");
        if self.branch.2 == Accordion::Expand {
            for member in &self.branch_member {
                self.format.push_str("- ");
                self.format.push_str(member);
                self.format.push_str("\n");
            }
        }
        // Padding
        self.format.push_str("\n");
        
        // # Commit
        self.format.push_str("# Commit\n");
        if self.commit.1 == Accordion::Expand {
            for member in &self.commits {
                self.format.push_str("- ");
                self.format.push_str(member);
                self.format.push_str("\n");
            }
        }
    }

    pub async fn get_commits() {
        let output = Command::new("git")
            .args(&["branch", "--format=%(refname:short)"])
            .output().await.unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for brances in stdout {    
            println!("{}", brances);
        }
    }
}


impl tower_lsp_server::LanguageServer for Lsp {
    async fn initialize(&self,params: tower_lsp_server::ls_types::InitializeParams) -> tower_lsp_server::jsonrpc::Result<tower_lsp_server::ls_types::InitializeResult> {
        let mut ls = ls_types::InitializeResult::default(); 
        //println!("on!");
        //dbg!(params.workspace_folders);
        Ok(ls)
    }

    async fn shutdown(&self) -> tower_lsp_server::jsonrpc::Result<()> {
        //println!("shutdown!");
        jsonrpc::Result::Ok(())
    }

    async fn hover(&self,params: ls_types::HoverParams) -> jsonrpc::Result<Option<ls_types::Hover>> {
       self.client.show_document(params) 
    }

    async fn folding_range(&self,params: ls_types::FoldingRangeParams) -> impl ::std::future::Future<Output = jsonrpc::Result<Option<Vec<ls_types::FoldingRange>>> > +Send {
        
    }

    async fn goto_definition(&self,params: ls_types::GotoDefinitionParams) -> jsonrpc::Result<Option<ls_types::GotoDefinitionResponse>> {
        let index = params.text_document_position_params.position.line as usize;
        //GotoDefinitionResponse::Scalar(Location::new(Uri::from_file_path(path), range))
    }
    
}

#[tokio::main]
async fn main() {
    //println!("hello!");
    GitClient::get_branches().await;
    //println!("world!");
    return;
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Lsp {client});
    tower_lsp_server::Server::new(stdin, stdout, socket).serve(service).await;
}
