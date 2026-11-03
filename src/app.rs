use std::{collections::HashMap, path::PathBuf};

use crossbeam::channel::Sender;
use git2::Repository;
use lsp_server::{Connection, Message};
use lsp_types::{
    CodeActionParams, CodeActionProviderCapability, DiagnosticOptions,
    DiagnosticServerCapabilities, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFilter, GotoDefinitionParams, HoverParams,
    HoverProviderCapability, InlayHintParams, OneOf, SaveOptions, SemanticTokenType,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensRegistrationOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    StaticRegistrationOptions, TextDocumentRegistrationOptions, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Uri,
    WorkDoneProgressOptions,
    notification::{DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification},
    request::{
        CodeActionRequest, GotoDefinition, HoverRequest, InlayHintRequest, Request,
        SemanticTokensFullRequest,
    },
};

use crate::{
    lsp::{Conn, Lsp},
    util::Container,
    workgroup::{ToolBox, WorkGroup},
};

pub struct Office {
    pub repo: Repository,
}

impl Office {
    pub fn new(repo: Repository) -> Self {
        Self { repo }
    }

    pub fn get_branch(&self) {}
}

pub type OfficeId = usize;

struct App {
    office: Container<Office>,
    work_group: HashMap<Uri, (WorkGroup, OfficeId)>,
    lsp_conn: Conn,
    ma_conn: Sender<MaAction>,
}

pub enum MaAction {
    Init { anchor_path: Uri },
}

impl App {
    fn handle_request(&mut self, request: lsp_server::Request) {
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
                                ma_conn: &self.ma_conn,
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
                                ma_conn: &self.ma_conn,
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
                                ma_conn: &self.ma_conn,
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
                                ma_conn: &self.ma_conn,
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
                                ma_conn: &self.ma_conn,
                            },
                            request.id,
                            params,
                        );
                    }
                    //self.semantic_token(request.id, params);
                });
            }
            _ => {}
        }
    }

    fn handle_notification(&mut self, notification: lsp_server::Notification) {
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
                                    ma_conn: &self.ma_conn,
                                },
                                params,
                            );
                        } else {
                            self.ma_conn.send(MaAction::Init {
                                anchor_path: params.text_document.uri,
                            });
                        }
                    },
                );
            }
            DidChangeTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidChangeTextDocumentParams| {
                        if let Some((wg, office_id)) =
                            self.work_group.get_mut(&params.text_document.uri)
                        {
                            wg.did_change(
                                ToolBox {
                                    office: &mut self.office[office_id],
                                    lsp_conn: &self.lsp_conn,
                                    ma_conn: &self.ma_conn,
                                },
                                params,
                            );
                        }
                        //self.did_change(params);
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
                                    ma_conn: &self.ma_conn,
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

    fn ma_action(&mut self, action: MaAction) {
        match action {
            MaAction::Init { anchor_path } => {
                let path = PathBuf::from(anchor_path.path().as_str());
                path.file_name().map(|file_name| match file_name.to_str() {
                    Some("ma.md") => {
                        let candidate = path.parent().unwrap();
                        Repository::open(candidate).map(|repo| {
                            let office = Office::new(repo);
                        });
                    }
                    _ => (),
                });
            }
        }
    }
    fn run(mut app: Self) {
        let (lsp_conn, io_t) = Connection::stdio();
        let caps = ServerCapabilities {
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
                                token_types: vec![
                                    SemanticTokenType::NAMESPACE,
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::TYPE,
                                ],
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
            //folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
            code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
            inlay_hint_provider: Some(OneOf::Left(true)),
            ..Default::default()
        };
        //println!("{init}");
        //return;
        let init_params = lsp_conn
            .initialize(serde_json::to_value(&caps).unwrap())
            .unwrap();
        let (ma_conn, ma_rec) = crossbeam::channel::unbounded::<MaAction>();
        loop {
            crossbeam::select! {
                recv(lsp_conn.receiver) -> msg => {
                    if let Ok(msg) = msg {
                        match msg {
                            Message::Request(request) => app.handle_request(request),
                            Message::Response(response) => (),
                            Message::Notification(notification) => app.handle_notification(notification),
                        }
                    }
                }
                recv(ma_rec) -> msg => {
                    if let Ok(msg) = msg {
                        app.ma_action(msg);
                    }
                }
            }
        }

        //println!("{init_params}");
        //return;
        //let mut client = Client::new(conn.sender.clone());
        //for msg in &conn.receiver {
        //    match msg {
        //        Message::Request(request) => client.handle_request(request),
        //        Message::Response(response) => (),
        //        Message::Notification(notification) => client.handle_notification(notification),
        //    }
        //}
        //client.main_loop(conn, init_params);
        io_t.join();
    }
}
