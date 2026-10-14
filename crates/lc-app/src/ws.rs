use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;

pub async fn ws_handler(ws: WebSocketUpgrade, State(app_state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // 1. Send initial state
    let initial_view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };

    if let Ok(json) = serde_json::to_string(&initial_view) {
        if sender.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    // 2. Subscribe to broadcasts
    let mut rx = app_state.tx.subscribe();

    let mut send_task = tokio::spawn(async move {
        while let Ok(view) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&view) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    // If either task completes, abort the other
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
}
