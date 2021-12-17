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

#[derive(Default)]
struct Status {
    connections: Vec<Client>,
    current_pages: HashMap<String, String>,
}

lazy_static! {
    static ref STATUS: Mutex<Status> = Mutex::new(Default::default());
}

pub async fn on_upgrade(ws: warp::ws::WebSocket) {
    let (tx, mut rx) = ws.split();
    let id = ID_CTR.fetch_add(1, Ordering::SeqCst);
    STATUS
        .lock()
        .await
        .connections
        .push(Client { id, socket: tx });
    while let Some(msg) = rx.next().await {
        println!("{:?}", msg);
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

async fn broadcast_page(sender_id: i64, comic: &str, page: &str) {
    let clients = &mut STATUS.lock().await.connections;
    let msg = warp::ws::Message::text(json!({"comic": comic, "page": page}).to_string());
    let mut disconnected_clients = vec![];
    for client in clients.iter_mut() {
        if client.id == sender_id {
            continue;
        }
        match client.socket.send(msg.clone()).await {
            Err(_) => disconnected_clients.push(client.id),
            _ => {}
        }
    }

    clients.retain(|client| !disconnected_clients.contains(&client.id));
}

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
    source_client
        .socket
        .send(warp::ws::Message::text(response.to_string()))
        .await
        .unwrap();
}
