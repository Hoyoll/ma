use std::{
    io::{self, stdin, stdout},
    thread,
};

use crossbeam::channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use lsp_server::{ErrorCode, Message, RequestId, Response};

pub enum Kind<Left, Right> {
    Main(Left),
    Other(Right),
}

pub struct Connection<Right> {
    pub sender: Sender<Kind<Message, Right>>,
    pub receiver: Receiver<Kind<Message, Right>>,
}

impl<Right: 'static + Send + Sync> Connection<Right> {
    pub fn stdio() -> (Self, IoThreads) {
        let (sender, receiver, io_threads) = stdio_transport::<Right>();
        (Connection { sender, receiver }, io_threads)
    }

    pub fn initialize(
        &self,
        server_capabilities: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        let (id, params) = self.initialize_start()?;

        let initialize_data = serde_json::json!({
            "capabilities": server_capabilities,
        });

        self.initialize_finish(id, initialize_data)?;

        Ok(params)
    }

    pub fn initialize_start(&self) -> Result<(RequestId, serde_json::Value), ProtocolError> {
        self.initialize_start_while(|| true)
    }

    pub fn initialize_start_while<C>(
        &self,
        running: C,
    ) -> Result<(RequestId, serde_json::Value), ProtocolError>
    where
        C: Fn() -> bool,
    {
        while running() {
            let msg = match self
                .receiver
                .recv_timeout(std::time::Duration::from_secs(1))
            {
                Ok(msg) => msg,
                Err(RecvTimeoutError::Timeout) => {
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return Err(ProtocolError::disconnected()),
            };

            match msg {
                Kind::Main(Message::Request(req)) if req.method == "initialize" => {
                    return Ok((req.id, req.params));
                }
                // Respond to non-initialize requests with ServerNotInitialized
                Kind::Main(Message::Request(req)) => {
                    let resp = Response::new_err(
                        req.id.clone(),
                        ErrorCode::ServerNotInitialized as i32,
                        format!("expected initialize request, got {req:?}"),
                    );
                    self.sender
                        .send(Kind::Main(Message::Response(resp.into())))
                        .unwrap();
                    continue;
                }
                Kind::Main(Message::Notification(n)) if n.method != "exit" => {
                    continue;
                }
                _ => {
                    return Err(ProtocolError::new(format!("expected initialize request!")));
                }
            };
        }

        Err(ProtocolError::new(String::from(
            "Initialization has been aborted during initialization",
        )))
    }

    pub fn initialize_finish(
        &self,
        initialize_id: RequestId,
        initialize_result: serde_json::Value,
    ) -> Result<(), ProtocolError> {
        let resp = Response::new_ok(initialize_id, initialize_result);
        self.sender.send(Kind::Main(resp.into())).unwrap();
        match &self.receiver.recv() {
            Ok(Kind::Main(Message::Notification(n))) if n.method == "initialized" => Ok(()),
            Ok(Kind::Main(msg)) => Err(ProtocolError::new(format!(
                r#"expected initialized notification, got: {msg:?}"#
            ))),
            Err(RecvError) => Err(ProtocolError::disconnected()),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolError(String, bool);

impl ProtocolError {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        ProtocolError(msg.into(), false)
    }

    pub(crate) fn disconnected() -> ProtocolError {
        ProtocolError("disconnected channel".into(), true)
    }

    /// Whether this error occurred due to a disconnected channel.
    pub fn channel_is_disconnected(&self) -> bool {
        self.1
    }
}

pub struct IoThreads {
    reader: thread::JoinHandle<io::Result<()>>,
    writer: thread::JoinHandle<io::Result<()>>,
    //dropper: thread::JoinHandle<()>,
}

impl IoThreads {
    pub fn join(self) -> io::Result<()> {
        match self.reader.join() {
            Ok(r) => r?,
            Err(err) => std::panic::panic_any(err),
        }
        match self.writer.join() {
            Ok(r) => r,
            Err(err) => {
                std::panic::panic_any(err);
            }
        }
    }
}

fn stdio_transport<Right: 'static + Send + Sync>() -> (
    Sender<Kind<Message, Right>>,
    Receiver<Kind<Message, Right>>,
    IoThreads,
) {
    //let (drop_sender, drop_receiver) = bounded::<Kind<Message,Right>>(0);
    let (writer_sender, writer_receiver) = unbounded::<Kind<Message, Right>>();
    let (reader_sender, reader_receiver) = unbounded::<Kind<Message, Right>>();
    let cloned_reader_sender = reader_sender.clone();
    let writer = thread::Builder::new()
        .name("LspServerWriter".to_owned())
        .spawn(move || {
            let stdout = stdout();
            let mut stdout = stdout.lock();
            writer_receiver.into_iter().try_for_each(|it| {
                let result = match &it {
                    Kind::Main(it) => it.write(&mut stdout),
                    Kind::Other(_) => {
                        cloned_reader_sender.send(it);
                        Ok(())
                    }
                };
                //let result = it.write(&mut stdout);
                result
            })
        })
        .unwrap();
    let reader = thread::Builder::new()
        .name("LspServerReader".to_owned())
        .spawn(move || {
            let stdin = stdin();
            let mut stdin = stdin.lock();
            while let Some(msg) = Message::read(&mut stdin)? {
                //let is_exit = matches!(&msg, Message::Notification(n) if n.is_exit());
                let is_exit = match &msg {
                    //Message::Request(request) => todo!(),
                    //Message::Response(response) => todo!(),
                    Message::Notification(notification) => notification.method == "exit",
                    _ => false,
                };
                //dbg!("sending message {msg:#?}");
                if let Err(e) = reader_sender.send(Kind::Main(msg)) {
                    return Err(io::Error::other(e));
                }

                if is_exit {
                    break;
                }
            }
            Ok(())
        })
        .unwrap();
    let threads = IoThreads {
        reader,
        writer,
        //dropper,
    };
    (writer_sender, reader_receiver, threads)
}
