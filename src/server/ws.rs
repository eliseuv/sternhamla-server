use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt, future};

use super::{
    client::{ClientSink, ClientStream},
    handshake::{AppState, handle_handshake},
    protocol::{RemoteInMessage, RemoteOutMessage},
};

/// Axum handler for WebSocket upgrades.
/// Upgrades the HTTP connection to a WebSocket connection.
pub async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

/// Handles the WebSocket connection.
///
/// This function acts as an adapter, converting the WebSocket stream (of `Message::Binary`)
/// into the `RemoteInMessage` and `RemoteOutMessage` types used by the core server logic.
/// It effectively wraps the WebSocket in the same interface as the TCP connection (`ClientSink` / `ClientStream`)
/// and then delegates to `handle_handshake`.
async fn handle_ws(socket: WebSocket, state: AppState) {
    let (ws_write, ws_read) = socket.split();

    // Adapter for Stream -> RemoteInMessage
    let stream = Box::pin(ws_read.map(|msg_res| match msg_res {
        Ok(Message::Binary(bin)) => RemoteInMessage::from_bytes(&bin),
        Ok(Message::Close(_)) => Err(anyhow::anyhow!("Connection closed")),
        Ok(_) => Err(anyhow::anyhow!("Unexpected message type")),
        Err(e) => Err(anyhow::anyhow!("WS Error: {e}")),
    }));

    // Adapter for Sink -> RemoteOutMessage
    let sink = Box::pin(SinkExt::with(ws_write, |msg: RemoteOutMessage| {
        let mut buf = Vec::new();
        // Ciborium to vec
        let res = ciborium::into_writer(&msg, &mut buf)
            .map_err(|e| anyhow::anyhow!("Serialization error: {e}"))
            .map(|_| Message::Binary(Bytes::from(buf)));
        future::ready(res)
    }));

    // Reuse handle_handshake logic
    // We need to pass ClientSink/Stream. The adapters should match.
    // Wait, SinkExt::with returns a struct, I need to wrap in Box/Pin.
    // And Sink must satisfy the trait.
    // ws_write is SplitSink<WebSocket>.
    // SinkExt::with takes `self`.

    // Boxing helper
    let sink: ClientSink = Box::pin(sink);
    let stream: ClientStream = Box::pin(stream);

    handle_handshake(stream, sink, state).await;
}
