//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! WebSocket signal streaming (§5.D).
//!
//! Single endpoint: `GET /ws/signals`
//!
//! On connection the client must supply a valid API key via the
//! `Authorization: Bearer <key>` header (checked in the upgrade handler).
//! Once authenticated, the server streams `ServerEvent` JSON frames until
//! the client disconnects or the connection is closed.

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/ws/signals", get(ws_handler))
}

/// WebSocket upgrade handler — checks the API key before upgrading.
async fn ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    // Validate API key before upgrading.
    let provided = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    match provided {
        Some(key) if key == state.api_key() => {
            ws.on_upgrade(move |socket| handle_socket(socket, state))
        }
        _ => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
    }
}

/// Drive a single WebSocket connection: subscribe to the event bus and forward
/// events as JSON text frames until the client disconnects.
async fn handle_socket(socket: WebSocket, state: AppState) {
    let mut rx = state.event_bus().subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Task: forward events from the bus to the WebSocket.
    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(e) => {
                            tracing::warn!("failed to serialize event: {e}");
                            continue;
                        }
                    };
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        // Client disconnected.
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("WS client lagged, dropped {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    // Drain incoming frames (ping/pong and close) so the TCP stack stays healthy.
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    // Either side closing should abort the other.
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
