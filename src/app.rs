use std::{collections::HashMap, fs, path::PathBuf};

use crossbeam::channel::Sender;
use git2::{Oid, PackBuilderStage, Repository};
use lsp_server::{Message, RequestId};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeActionParams, CodeActionProviderCapability, DiagnosticOptions,
    DiagnosticServerCapabilities, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFilter, ExecuteCommandOptions, ExecuteCommandParams, GotoDefinitionParams, HoverParams,
    HoverProviderCapability, InlayHintParams, OneOf, Range, SaveOptions, SemanticTokenType,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensRegistrationOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    ShowDocumentParams, StaticRegistrationOptions, TextDocumentRegistrationOptions,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, TextEdit, Uri, WorkDoneProgressOptions, WorkspaceEdit,
    notification::{DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification},
    request::{
        ApplyWorkspaceEdit, CodeActionRequest, ExecuteCommand, GotoDefinition, HoverRequest,
        InlayHintRequest, Request, SemanticTokensFullRequest, ShowDocument,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, from_value, to_value};

use crate::{
    lsp::Kind,
    util::{self, Container, name_to_url},
    workgroup::{Diff, DiffView, Root, ToolBox, WorkGroup},
};
#[repr(u32)]
pub enum SemanticTokenList {
    NAMESPACE,
    VARIABLE,
    FUNCTION,
    KEYWORD,
    STRUCT,
    TYPE,
}

impl SemanticTokenList {
    fn token_types() -> Vec<SemanticTokenType> {
        vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::STRUCT,
            SemanticTokenType::TYPE,
        ]
    }
}

impl From<SemanticTokenList> for u32 {
    fn from(value: SemanticTokenList) -> Self {
        value as u32
    }
}

pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(
            SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                SemanticTokensRegistrationOptions {
                    text_document_registration_options: TextDocumentRegistrationOptions {
                        document_selector: Some(vec![
                            DocumentFilter {
                                language: Some("markdown".into()),
                                scheme: Some("file".into()),
                                pattern: Some("*.{md}".into()),
                            },
                            DocumentFilter {
                                language: Some("diff".into()),
                                scheme: Some("file".into()),
                                pattern: Some("*.{diff}".into()),
                            },
                        ]),
                    },
                    semantic_tokens_options: SemanticTokensOptions {
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        legend: lsp_types::SemanticTokensLegend {
                            token_types: SemanticTokenList::token_types(),
                            token_modifiers: vec![],
                        },
                        range: None,
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                    },
                    static_registration_options: StaticRegistrationOptions { id: None },
                },
            ),
        ),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(true),
                })),
                ..Default::default()
            },
        )),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
            DiagnosticOptions::default(),
        )),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![MaAction::COMMAND.into()],
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        //folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

pub struct Office {
    pub repo: Repository,
}

impl Office {
    pub fn new(repo: Repository) -> Self {
        Self { repo }
    }
}

pub type OfficeId = usize;

pub struct App {
    office: Container<Office>,
    work_group: HashMap<Uri, (WorkGroup, OfficeId)>,
    lsp_conn: Sender<Kind<Message, MaAction>>,
    //ma_conn: Sender<MaAction>,
}

pub enum MaAction {
    Init {
        anchor_path: Uri,
    },
    FillContent {
        url: Uri,
        content: String,
        range: Range,
    },
    //Insert {url: Uri, content: String},
    OpenDiff {
        uri: Uri,
        oid: Oid,
    },
    //OpenTree {
    //    uri: Uri,
    //    //commit_id: Oid,
    //    tree_id: Oid
    //},
    Command {
        //uri: Uri,
        command: Command,
    },
    GitPushProgress {
        uri: Uri,
        remote_id: usize,
        progress: GitPush,
    },
}

impl MaAction {
    pub const COMMAND: &str = "ma.command";
}

pub enum Progress {
    GitPush { remote_id: usize, progress: GitPush },
}

pub enum GitPush {
    Packing {
        percentage: usize,
        stage: PackBuilderStage,
    },
    Transfer {
        percentage: usize,
    },
    Error {
        message: String,
    },
    Done,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    StageAll { uri: Uri },
    StageFile { uri: Uri, path: usize },
    UnstageFile { uri: Uri, path: usize },
    SwitchBranch { uri: Uri, branch: String },
    Reload { uri: Uri },
    AttemptCommit { uri: Uri },
    CancelCommit { uri: Uri },
    PullRemote { uri: Uri, name: usize },
    PushRemote { uri: Uri, name: usize },
}

impl Command {
    //pub const ACTION: &str = "ma.action";

    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    pub fn from_string(json: &str) -> Self {
        serde_json::from_str(json).unwrap()
    }

    pub fn from_value(value: serde_json::Value) -> Self {
        from_value(value).unwrap()
    }

    pub fn to_value(&self) -> Value {
        to_value(self).unwrap()
    }
}

impl From<Command> for Option<Vec<Value>> {
    fn from(value: Command) -> Self {
        Some(vec![value.to_value()])
    }
}

impl From<Command> for MaAction {
    fn from(value: Command) -> Self {
        MaAction::Command { command: value }
    }
}

impl App {
    pub fn new(lsp_conn: Sender<Kind<Message, MaAction>>) -> Self {
        Self {
            office: Container::new(),
            work_group: HashMap::new(),
            lsp_conn,
        }
    }

    pub fn handle_request(&mut self, request: lsp_server::Request) {
        //let toolbox = ToolBox { office: &mut self.office, lsp_conn: &self.lsp_conn, ma_conn: &self.ma_conn };
        match request.method.as_str() {
            HoverRequest::METHOD => {
                //container::Container
                serde_json::from_value(request.params).map(|params: HoverParams| {
                    if let Some((wg, office_id)) = self
                        .work_group
                        .get_mut(&params.text_document_position_params.text_document.uri)
                    {
                        wg.hover(
                            ToolBox {
                                office: &mut self.office[office_id],
                                lsp_conn: &self.lsp_conn,
                                //ma_conn: &self.ma_conn,
                            },
                            request.id,
                            params,
                        );
                    }
                });
            }
            GotoDefinition::METHOD => {
                serde_json::from_value(request.params).map(|params: GotoDefinitionParams| {
                    if let Some((wg, office_id)) = self
                        .work_group
                        .get_mut(&params.text_document_position_params.text_document.uri)
                    {
                        wg.goto_definition(
                            ToolBox {
                                office: &mut self.office[office_id],
                                lsp_conn: &self.lsp_conn,
                                //ma_conn: &self.ma_conn,
                            },
                            request.id,
                            params,
                        );
                    }
                    //self.goto_definition(request.id, params);
                });
            }
            CodeActionRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: CodeActionParams| {
                    if let Some((wg, office_id)) =
                        self.work_group.get_mut(&params.text_document.uri)
                    {
                        wg.code_action(
                            ToolBox {
                                office: &mut self.office[office_id],
                                lsp_conn: &self.lsp_conn,
                                //ma_conn: &self.ma_conn,
                            },
                            request.id,
                            params,
                        );
                    }
                    //self.code_action(request.id, params);
                });
            }
            InlayHintRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: InlayHintParams| {
                    if let Some((wg, office_id)) =
                        self.work_group.get_mut(&params.text_document.uri)
                    {
                        wg.inlay_hint(
                            ToolBox {
                                office: &mut self.office[office_id],
                                lsp_conn: &self.lsp_conn,
                                //ma_conn: &self.ma_conn,
                            },
                            request.id,
                            params,
                        );
                    }
                    //self.inlay_hint(request.id, params);
                });
            }
            SemanticTokensFullRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: SemanticTokensParams| {
                    if let Some((wg, office_id)) =
                        self.work_group.get_mut(&params.text_document.uri)
                    {
                        wg.semantic_token(
                            ToolBox {
                                office: &mut self.office[office_id],
                                lsp_conn: &self.lsp_conn,
                                //ma_conn: &self.ma_conn,
                            },
                            request.id,
                            params,
                        );
                    }
                    //self.semantic_token(request.id, params);
                });
            }
            ExecuteCommand::METHOD => {
                serde_json::from_value(request.params).map(|params: ExecuteCommandParams| {
                    for command in params.arguments {
                        let action = Command::from_value(command);
                        self.ma_command(action);
                    }
                    //self.ma_action(params.arguments);
                });
            }
            _ => {}
        }
    }

    pub fn handle_notification(&mut self, notification: lsp_server::Notification) {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidOpenTextDocumentParams| {
                        if let Some((wg, office_id)) =
                            self.work_group.get_mut(&params.text_document.uri)
                        {
                            wg.did_open(
                                ToolBox {
                                    office: &mut self.office[office_id],
                                    lsp_conn: &self.lsp_conn,
                                    //ma_conn: &self.ma_conn,
                                },
                                params,
                            );
                        } else {
                            //panic!("here did_open!?");
                            self.lsp_conn.send(Kind::Other(MaAction::Init {
                                anchor_path: params.text_document.uri,
                            }));
                        }
                    },
                );
            }
            DidSaveTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidSaveTextDocumentParams| {
                        if let Some((wg, office_id)) =
                            self.work_group.get_mut(&params.text_document.uri)
                        {
                            wg.did_save(
                                ToolBox {
                                    office: &mut self.office[office_id],
                                    lsp_conn: &self.lsp_conn,
                                    //ma_conn: &self.ma_conn,
                                },
                                params,
                            );
                        }
                        //self.did_save(params);
                    },
                );
            }
            _ => (),
        }
    }

    pub fn ma_action(&mut self, action: MaAction) {
        match action {
            MaAction::Init { anchor_path } => {
                let path = PathBuf::from(anchor_path.path().as_str());
                path.file_name().map(|file_name| match file_name.to_str() {
                    Some("ma.md") => {
                        let candidate = path.parent().unwrap().strip_prefix("/").unwrap();
                        let mut office = match Repository::open(candidate) {
                            Ok(repo) => Office::new(repo),
                            Err(_) => return,
                        };
                        let mut root = Root::new();
                        root.fill_root(&mut office);
                        root.build_view();
                        let office_id = self.office.add(office);

                        root.refresh(anchor_path.clone(), &self.lsp_conn);
                        self.work_group
                            .insert(anchor_path, (WorkGroup::Root(root), office_id));
                    }
                    _ => (),
                });
            }
            MaAction::FillContent {
                url,
                content,
                range,
            } => {
                let mut wf = HashMap::new();
                wf.insert(
                    url,
                    vec![TextEdit {
                        new_text: content,
                        range,
                    }],
                );
                let we = ApplyWorkspaceEditParams {
                    label: None,
                    edit: WorkspaceEdit {
                        changes: Some(wf),
                        ..Default::default()
                    },
                };
                //panic!("here!");
                self.lsp_conn.send(Kind::Main(
                    lsp_server::Request {
                        id: RequestId::from(1),
                        method: ApplyWorkspaceEdit::METHOD.into(),
                        params: to_value(we).unwrap(),
                    }
                    .into(),
                ));
            }
            MaAction::OpenDiff { uri, oid } => {
                let new_wg = match self.work_group.get_mut(&uri) {
                    None => None,
                    Some((wg, office_id)) => {
                        let office = &self.office[*office_id];
                        //let oid = wg.open_diff(oid);
                        let commit = match office.repo.find_commit(oid) {
                            Ok(commit) => commit,
                            Err(_) => return,
                        };
                        let mut diff_group = Diff::new(office, oid);
                        let format = diff_group.build_diff(office, &commit);

                        let location = diff_group.write_diff(&format);
                        let url = name_to_url(&location).unwrap();
                        Some((url, (WorkGroup::Diff(diff_group), office_id.clone())))
                    }
                };
                if let Some((uri, wg)) = new_wg {
                    self.work_group.insert(uri.clone(), wg);
                    self.lsp_conn.send(Kind::Main(
                        lsp_server::Request {
                            id: RequestId::from(1),
                            method: ShowDocument::METHOD.into(),
                            params: to_value(ShowDocumentParams {
                                uri,
                                external: None,
                                take_focus: Some(true),
                                selection: None,
                            })
                            .unwrap(),
                        }
                        .into(),
                    ));
                }
                //let diff_file = diff_path.join(".diff");
            }
            MaAction::Command { command } => {
                self.ma_command(command);
            }
            MaAction::GitPushProgress {
                uri,
                remote_id,
                progress,
            } => {
                if let Some((wg, _office_id)) = self.work_group.get_mut(&uri) {
                    wg.git_push_progress(remote_id, progress, &self.lsp_conn);
                }
            },
        }
    }

    fn ma_command(&mut self, command: Command) {
        match command {
            Command::StageAll { uri } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    wg.stage_all(
                        ToolBox {
                            office: &mut self.office[office_id],
                            lsp_conn: &self.lsp_conn,
                        },
                        uri,
                    );
                }
            }
            Command::StageFile { uri, path } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    wg.stage_file(
                        ToolBox {
                            office: &mut self.office[office_id],
                            lsp_conn: &self.lsp_conn,
                        },
                        uri,
                        path,
                    );
                }
            }
            Command::UnstageFile { uri, path } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    wg.unstage_file(
                        ToolBox {
                            office: &mut self.office[office_id],
                            lsp_conn: &self.lsp_conn,
                        },
                        uri,
                        path,
                    );
                }
            }
            Command::SwitchBranch { uri, branch } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    wg.switch_branch(
                        ToolBox {
                            office: &mut self.office[office_id],
                            lsp_conn: &self.lsp_conn,
                        },
                        uri,
                        branch,
                    );
                }
            }
            Command::Reload { uri } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    wg.reload(
                        ToolBox {
                            office: &mut self.office[office_id],
                            lsp_conn: &self.lsp_conn,
                        },
                        uri,
                    );
                }
            }
            Command::AttemptCommit { uri } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    wg.attempt_commit(
                        ToolBox {
                            office: &mut self.office[office_id],
                            lsp_conn: &self.lsp_conn,
                        },
                        uri,
                    );
                }
            }
            Command::CancelCommit { uri } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    wg.cancel_commit(
                        ToolBox {
                            office: &mut self.office[office_id],
                            lsp_conn: &self.lsp_conn,
                        },
                        uri,
                    );
                }
            }
            Command::PullRemote { uri, name } => (),
            Command::PushRemote { uri, name } => {
                if let Some((wg, office_id)) = self.work_group.get_mut(&uri) {
                    let office = &mut self.office[office_id];
                    wg.push_remote(ToolBox { office: office, lsp_conn: &self.lsp_conn }, uri, name);
                    //let remote = office.repo.find_remote(&name);
                }
            }
        }
    }
}
