use std::{collections::HashMap, path::PathBuf};

use crossbeam::channel::Sender;
use git2::Repository;
use lsp_server::{Connection, Message, RequestId};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeActionParams, CodeActionProviderCapability, DiagnosticOptions,
    DiagnosticServerCapabilities, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFilter, GotoDefinitionParams, HoverParams,
    HoverProviderCapability, InlayHintParams, OneOf, Position, Range, SaveOptions,
    SemanticTokenType, SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensRegistrationOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    StaticRegistrationOptions, TextDocumentRegistrationOptions, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Uri,
    WorkDoneProgressOptions, WorkspaceEdit,
    notification::{DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification},
    request::{
        ApplyWorkspaceEdit, CodeActionRequest, GotoDefinition, HoverRequest, InlayHintRequest,
        Request, SemanticTokensFullRequest,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::to_value;

use crate::{
    lsp::{self, Conn, Kind, Lsp},
    util::{Container, name_to_url},
    workgroup::{Root, ToolBox, WorkGroup},
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

pub struct App {
    office: Container<Office>,
    work_group: HashMap<Uri, (WorkGroup, OfficeId)>,
    lsp_conn: Sender<Kind<Message, MaAction>>,
    //ma_conn: Sender<MaAction>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MaAction {
    Init { anchor_path: Uri },
    FillContent { url: Uri, content: String },
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
                                    //ma_conn: &self.ma_conn,
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
                        self.work_group
                            .insert(anchor_path.clone(), (WorkGroup::Root(root), office_id));
                        //panic!("here!");
                        self.lsp_conn.send(Kind::Other(MaAction::FillContent {
                            url: anchor_path,
                            content: format,
                        }));
                        //self.lsp_conn.send()
                    }
                    _ => (),
                });
            }
            MaAction::FillContent { url, content } => {
                let mut wf = HashMap::new();
                wf.insert(
                    url,
                    vec![TextEdit {
                        new_text: content,
                        range: Range {
                            start: Position::new(0, 0),
                            end: Position::new(1000, 0),
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
        }
    }

    fn run(mut app: Self) {
        //let (send) = lsp::stdio::<MaAction>();

        //let (ma_conn, ma_rec) = crossbeam::channel::unbounded::<MaAction>();
        //loop {
        //    crossbeam::select! {
        //        recv(lsp_conn.receiver) -> msg => {
        //            if let Ok(msg) = msg {
        //                match msg {
        //                    Message::Request(request) => app.handle_request(request),
        //                    Message::Response(response) => (),
        //                    Message::Notification(notification) => app.handle_notification(notification),
        //                }
        //            }
        //        }
        //        recv(ma_rec) -> msg => {
        //            if let Ok(msg) = msg {
        //                app.ma_action(msg);
        //            }
        //        }
        //    }
        //}

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
        //io_t.join();
    }
}
