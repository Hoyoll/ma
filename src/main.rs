use std::io::stdout;

use tokio::{io::{AsyncRead, AsyncWrite}, process::Command};
use tower_lsp_server::{Client, LspService, jsonrpc, ls_types};

struct CommitMember {
    hash: String,
    message: String,
}

enum GitView {
    Padding,
    Branch,
    BranchMember(String, BranchRef),
    //RemoteBranch,
    //RemoteMember(String),
    CommitHead,
    CommitMember(CommitMember),
}

enum BranchRef {
    Local,
    Remote
}

// git commands, for local branch = git branch --format='%(refname:short)'
// git commands, for remote branch = git branch -r --format='%(refname:short)'
// gt commands, for log = git log -n 10 --format='%h%x00%s'
// git commands, for show = git show --no-patch <hash>

struct Lsp {
    client: Client
}

struct GitClient {

}

impl GitClient {
    pub async fn get_branches() {
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
        println!("shutdown!");
        jsonrpc::Result::Ok(())
    }
}

#[tokio::main]
async fn main() {
    println!("hello!");
    GitClient::get_branches().await;
    println!("world!");
    return;
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Lsp {client});
    tower_lsp_server::Server::new(stdin, stdout, socket).serve(service).await;
}
