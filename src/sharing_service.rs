use futures::{stream::SplitSink, SinkExt, StreamExt};
use lazy_static::lazy_static;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicI64, Ordering},
};
use tokio::sync::Mutex;
use warp::ws::{Message, WebSocket};

static ID_CTR: AtomicI64 = AtomicI64::new(0);

struct Client {
    id: i64,
    socket: SplitSink<WebSocket, Message>,
}

struct Status {
    connections: Vec<Client>,
    current_pages: HashMap<String, String>,
}

impl Status {
    fn init_from_environment() -> Self {
        let mut retval = Status {
            connections: vec![],
            current_pages: HashMap::new(),
        };
        if let Ok(file) = std::fs::read_to_string("current_pages.json") {
            if let Ok(pages) = serde_json::from_str(&file) {
                retval.current_pages = pages;
            }
        }
        retval
    }
}

lazy_static! {
    static ref STATUS: Mutex<Status> = Mutex::new(Status::init_from_environment());
}

// Initializes the connection to a client and runs the main loop that listens
// for websocket messages from them.
pub async fn connect_client(ws: warp::ws::WebSocket) {
    let (tx, mut rx) = ws.split();
    let id = ID_CTR.fetch_add(1, Ordering::SeqCst);
    STATUS
        .lock()
        .await
        .connections
        .push(Client { id, socket: tx });
    while let Some(msg) = rx.next().await {
        log::debug!(target: "scr::sharing_service", "Received message: {:?} from user {}", msg, id);
        let msg = msg.unwrap();
        if msg.is_close() {
            break;
        } else if let Ok(msg_text) = msg.to_str() {
            let payload: serde_json::Value = serde_json::from_str(msg_text).unwrap();
            let comic = payload.get("comic").unwrap().as_str().unwrap();
            if let Some(page) = payload.get("page").and_then(|val| val.as_str()) {
                // broadcast page to other clients and update current page
                STATUS
                    .lock()
                    .await
                    .current_pages
                    .insert(comic.to_string(), page.to_string());
                broadcast_page(id, comic, page).await;
            } else {
                // respond to same client with the current page, if there is one
                let current_page = STATUS.lock().await.current_pages.get(comic).cloned();
                reply_current_page(id, comic, current_page).await;
            }
        }
    }
}

// Run when the server exits, to notify clients and save data.
pub async fn shutdown() {
    std::fs::write(
        "current_pages.json",
        serde_json::to_value(&STATUS.lock().await.current_pages)
            .unwrap()
            .to_string(),
    )
    .unwrap();

    disconnect_clients().await;
}

// Sends a message to notify every client of the server shutdown.
async fn disconnect_clients() {
    let clients = &mut STATUS.lock().await.connections;
    let dc_msg = warp::ws::Message::text(json!({"disconnect": true}).to_string());
    for client in clients {
        log::debug!(target: "scr::sharing_service", "Sending message: {:?} to user {}", dc_msg, client.id);
        let _ = client.socket.send(dc_msg.clone()).await;
        let _ = client.socket.close().await;
    }
}

// When one client turns the page, broadcasts a message to every other client making
// them turn the page too.
async fn broadcast_page(sender_id: i64, comic: &str, page: &str) {
    let clients = &mut STATUS.lock().await.connections;
    let msg = warp::ws::Message::text(json!({"comic": comic, "page": page}).to_string());
    let mut disconnected_clients = vec![];
    for client in clients.iter_mut() {
        if client.id == sender_id {
            continue;
        }
        log::debug!(
            target: "scr::sharing_service",
            "Sending message: {:?} to user {}",
            msg,
            client.id
        );
        match client.socket.send(msg.clone()).await {
            Err(_) => disconnected_clients.push(client.id),
            _ => {}
        }
    }

    if !disconnected_clients.is_empty() {
        log::debug!(
            target: "scr::sharing_service",
            "Disconnected clients: {:?}",
            disconnected_clients
        );

        clients.retain(|client| !disconnected_clients.contains(&client.id));
    }
}

// When a new client loads in, tell it what the current page is.
async fn reply_current_page(sender_id: i64, comic: &str, page: Option<String>) {
    let mut response = json!({ "comic": comic });
    if let Some(page) = page {
        response["page"] = serde_json::Value::String(page);
    }
    let clients = &mut STATUS.lock().await.connections;
    let source_client = clients
        .iter_mut()
        .find(|client| client.id == sender_id)
        .unwrap();
    let response = warp::ws::Message::text(response.to_string());
    log::debug!(
        target: "scr::sharing_service",
        "Sending message: {:?} to user {}",
        response,
        source_client.id
    );
    source_client.socket.send(response).await.unwrap();
}
