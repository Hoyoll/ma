use lsp_server::RequestId;
use lsp_types::{
    CodeActionParams, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, HoverParams, InlayHintParams,
    SemanticTokensParams,
    notification::{DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification},
    request::{
        CodeActionRequest, GotoDefinition, HoverRequest, InlayHintRequest, Request,
        SemanticTokensFullRequest,
    },
};

pub trait Lsp {
    fn hover(&mut self, id: RequestId, params: HoverParams) {}
    fn goto_definition(&mut self, id: RequestId, params: GotoDefinitionParams) {}
    fn code_action(&mut self, id: RequestId, params: CodeActionParams) {}
    fn inlay_hint(&mut self, id: RequestId, params: InlayHintParams) {}
    fn semantic_token(&mut self, id: RequestId, params: SemanticTokensParams) {}
    fn handle_request(&mut self, request: lsp_server::Request) {
        match request.method.as_str() {
            HoverRequest::METHOD => {
                //container::Container
                serde_json::from_value(request.params).map(|params: HoverParams| {
                    self.hover(request.id, params);
                });
            }
            GotoDefinition::METHOD => {
                serde_json::from_value(request.params).map(|params: GotoDefinitionParams| {
                    self.goto_definition(request.id, params);
                });
            }
            CodeActionRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: CodeActionParams| {
                    self.code_action(request.id, params);
                });
            }
            InlayHintRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: InlayHintParams| {
                    self.inlay_hint(request.id, params);
                });
            }
            SemanticTokensFullRequest::METHOD => {
                serde_json::from_value(request.params).map(|params: SemanticTokensParams| {
                    self.semantic_token(request.id, params);
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
                        self.did_open(params);
                    },
                );
            }
            DidChangeTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidChangeTextDocumentParams| {
                        self.did_change(params);
                    },
                );
            }
            DidSaveTextDocument::METHOD => {
                serde_json::from_value(notification.params).map(
                    |params: DidSaveTextDocumentParams| {
                        self.did_save(params);
                    },
                );
            }
            _ => (),
        }
    }

    fn did_save(&mut self, params: DidSaveTextDocumentParams) {}
    fn did_open(&mut self, params: DidOpenTextDocumentParams) {}
    fn did_change(&mut self, params: DidChangeTextDocumentParams) {}
}

pub struct Conn(crossbeam::channel::Sender<lsp_server::Message>);

impl Conn {
    pub fn uniq(&self) -> Uniq {
        Uniq {
            count: 0,
            conn: self,
        }
    }
}

impl Conn {
    pub fn log(&self, message: impl Into<String>) {
        let l_params = lsp_types::LogMessageParams {
            typ: lsp_types::MessageType::WARNING,
            message: message.into(),
        };

        self.0
            .send(lsp_server::Message::Notification(
                lsp_server::Notification {
                    method: lsp_types::notification::LogMessage::METHOD.to_string(),
                    params: serde_json::to_value(l_params).unwrap(),
                },
            ))
            .unwrap();
    }

    pub fn ok(&self, id: lsp_server::RequestId, result: &impl serde::Serialize) {
        let resp = lsp_server::Response {
            id,
            response_result: Ok(serde_json::to_value(result).unwrap()),
        };
        self.0.send(lsp_server::Message::Response(resp));
    }

    pub fn req(
        &self,
        method: impl Into<String>,
        id: lsp_server::RequestId,
        result: &impl serde::Serialize,
    ) {
        self.0
            .send(lsp_server::Message::Request(lsp_server::Request::new(
                id,
                method.into(),
                result,
            )));
    }

    pub fn err(&self, id: lsp_server::RequestId, code: lsp_server::ErrorCode, msg: &str) {
        let resp = lsp_server::Response {
            id,
            response_result: Err(lsp_server::ResponseError {
                code: code as i32,
                message: msg.into(),
                data: None,
            }),
        };
        self.0.send(lsp_server::Message::Response(resp));
    }
}

pub struct Uniq<'ma> {
    count: i32,
    conn: &'ma Conn,
}

impl<'ma> Uniq<'ma> {
    pub fn req(&mut self, method: impl Into<String>, result: &impl serde::Serialize) {
        self.count += 1;
        self.conn
            .req(method, lsp_server::RequestId::from(self.count), result);
    }
}
