use lsp_server::Message;

use crate::app::{App, MaAction};

mod app;
mod lsp;
mod util;
mod workgroup;

fn main() {
    run();
}

fn run() {
    //panic!("here! it works!");
    let (conn, io_t) = lsp::Connection::<MaAction>::stdio();
    //let (lsp_conn, io_t) = Connection::stdio();
    let caps = app::capabilities(); //println!("{init}");
    //return;
    let init_params = conn
        .initialize(serde_json::to_value(&caps).unwrap())
        .unwrap();
    let mut app = App::new(conn.sender.clone());
    for kind in conn.receiver {
        match kind {
            lsp::Kind::Main(msg) => match msg {
                Message::Request(request) => app.handle_request(request),
                Message::Response(response) => (),
                Message::Notification(notification) => app.handle_notification(notification),
            },
            lsp::Kind::Other(ma) => {
                app.ma_action(ma);
            }
        }
    }
    io_t.join();
}
