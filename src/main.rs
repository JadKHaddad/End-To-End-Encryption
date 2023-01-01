use std::{collections::HashMap, sync::Arc};

use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use poem::{
    endpoint::StaticFilesEndpoint,
    get, handler,
    listener::TcpListener,
    web::{
        websocket::{Message, WebSocket},
        Data, Path,
    },
    EndpointExt, IntoResponse, Route, Server,
};

struct Client {
    sender: tokio::sync::mpsc::UnboundedSender<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
enum WebsocketMessage {
    Message(WSMessage),
    Setup(WSSetup),
    SetupResponse(WSSetupResponse),
}

#[derive(serde::Deserialize, serde::Serialize)]
struct WSMessage {
    text: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct WSSetup {
    n: i32,
    g: i32,
    gymodn: i32,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct WSSetupResponse {
    gxmodn: i32,
}

#[handler]
fn ws(
    Path((from, to, n, g, gymodn)): Path<(String, String, i32, i32, i32)>,
    ws: WebSocket,
    clients: Data<&Arc<RwLock<HashMap<String, Client>>>>,
) -> impl IntoResponse {
    //This is done the easy way. Don't do this in production.
    //Use some channels and options instead of locking the whole hashmap.

    println!("{} -> {}: {} {} {}", from, to, n, g, gymodn);

    let clients = clients.clone();
    let clients_recv = clients.clone();
    let from_recv = from.clone();

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();

    {
        let mut clients_g = clients.write();
        clients_g.insert(from.clone(), Client { sender });
    }

    //if the buddy is online, send him the setup message
    if let Some(buddy_sender) = clients.read().get(&to) {
        let msg = WebsocketMessage::Setup(WSSetup { n, g, gymodn });
        let _ = buddy_sender
            .sender
            .send(serde_json::to_string(&msg).unwrap());
    }

    ws.on_upgrade(move |socket| async move {
        let (mut sink, mut stream) = socket.split();

        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                if let Message::Text(text) = msg {
                    println!("{} -> {}: {}", from, to, text);
                    if let Some(buddy_sender) = clients.read().get(&to) {
                        if buddy_sender.sender.send(text).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        tokio::spawn(async move {
            while let Some(msg) = receiver.recv().await {
                if sink.send(Message::Text(msg)).await.is_err() {
                    break;
                }
            }
            //remove client
            clients_recv.write().remove(&from_recv);
        });
    })
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "poem=debug");
    }
    tracing_subscriber::fmt::init();

    let clients: Arc<RwLock<HashMap<String, Client>>> = Arc::new(RwLock::new(HashMap::new()));
    let app = Route::new()
        .at("/ws/:from/:to/:n/:g/:gxmodn", get(ws.data(clients)))
        .nest(
            "/",
            StaticFilesEndpoint::new(std::path::Path::new(".").join("html"))
                .index_file("index.html"),
        );

    Server::new(TcpListener::bind("127.0.0.1:3000"))
        .run(app)
        .await
}
