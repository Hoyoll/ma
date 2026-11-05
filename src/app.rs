use std::{collections::HashMap, path::PathBuf};

use crossbeam::channel::Sender;
use git2::Repository;
use lsp_server::{Message, RequestId};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeActionParams, CodeActionProviderCapability, DiagnosticOptions,
    DiagnosticServerCapabilities, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFilter, ExecuteCommandOptions, ExecuteCommandParams,
    GotoDefinitionParams, HoverParams, HoverProviderCapability, InlayHintParams, OneOf, Position,
    Range, SaveOptions, SemanticTokenType, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRegistrationOptions, SemanticTokensServerCapabilities,
    ServerCapabilities, StaticRegistrationOptions, TextDocumentRegistrationOptions,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, TextEdit, Uri, WorkDoneProgressOptions, WorkspaceEdit,
    notification::{DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification},
    request::{
        ApplyWorkspaceEdit, CodeActionRequest, ExecuteCommand, GotoDefinition, HoverRequest,
        InlayHintRequest, Request, SemanticTokensFullRequest,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, from_value, to_value};

use crate::{
    lsp::Kind,
    util::Container,
    workgroup::{Root, ToolBox, WorkGroup},
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
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![MaAction::ACTION.into()],
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

#[derive(Debug, Serialize, Deserialize)]
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
    StageAll {
        uri: Uri,
    },
    StageFile {
        uri: Uri,
        path: usize,
    },
    UnstageFile {
        uri: Uri,
        path: usize,
    },
    SwitchBranch {
        branch: String,
    },
}

impl MaAction {
    pub const ACTION: &str = "ma.action";

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

impl From<MaAction> for Option<Vec<Value>> {
    fn from(value: MaAction) -> Self {
        Some(vec![value.to_value()])
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
                        let action = MaAction::from_value(command);
                        self.ma_action(action);
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
        //panic!("action work!?");
        match action {
            MaAction::Init { anchor_path } => {
                let path = PathBuf::from(anchor_path.path().as_str());
                path.file_name().map(|file_name| match file_name.to_str() {
                    Some("ma.md") => {
                        //panic!("init works?");
                        let candidate = path.parent().unwrap().strip_prefix("/").unwrap();
                        //panic!("{}", candidate.to_string_lossy());
                        let mut office = match Repository::open(candidate) {
                            Ok(repo) => Office::new(repo),
                            Err(_) => return,
                        };
                        let mut root = Root::new();
                        root.fill_root(&mut office);
                        root.build_view();
                        let format = root.format();
                        let office_id = self.office.add(office);
                        //let url = name_to_url(&path).unwrap();
                        //panic!("here!");
                        root.refresh(anchor_path.clone(), &self.lsp_conn);
                        self.work_group
                            .insert(anchor_path, (WorkGroup::Root(root), office_id));

                        //self.lsp_conn.send()
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
            MaAction::StageAll { uri } => {
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
            MaAction::StageFile { uri, path } => {
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
            MaAction::UnstageFile { uri, path } => {
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
            MaAction::SwitchBranch { branch } => {}
        }
    }
}
