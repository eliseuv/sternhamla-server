//! # Handshake Module
//!
//! This module handles the initial connection phase for both TCP and WebSocket clients.
//! It implements the `handle_handshake` function, which:
//! 1. Negotiates a session (New or Reconnect).
//! 2. Contacts the main server thread to request a player slot.
//! 3. Spawns the `Client` task upon success.

use futures::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use super::{
    MainThreadMessage,
    client::{Client, ClientSink, ClientStream},
    messages::{ClientMessage, ServerBroadcast, ServerMessage},
    protocol::{RemoteInMessage, RemoteOutMessage},
};

const LOCAL_CHANNEL_CAPACITY: usize = 32;

/// Shared state for the application
/// This state is cloned and passed to new connections (both TCP and WebSocket)
#[derive(Clone)]
pub struct AppState {
    /// Channel to send messages to the main Game/Server loop
    pub main_tx: mpsc::Sender<MainThreadMessage>,
    /// Channel to send messages from Clients to the Server
    pub client_msg_tx: mpsc::Sender<ClientMessage>,
    /// Channel for the Server to broadcast messages to all Clients
    pub server_broadcast_tx: broadcast::Sender<ServerBroadcast>,
}

/// Handles the initial handshake with a client (both TCP and WebSocket).
///
/// This function:
/// 1. Waits for a `Hello` (new session) or `Reconnect` message.
/// 2. Contacts the main Server thread to request a player slot or validate a session.
/// 3. Sends a welcome message (or rejection) to the client.
/// 4. If successful, spawns a `Client` task to handle the connection for the duration of the game.
pub async fn handle_handshake(mut stream: ClientStream, mut sink: ClientSink, app_state: AppState) {
    let AppState {
        main_tx,
        client_msg_tx,
        server_broadcast_tx,
    } = app_state;

    // 1. Wait for Hello or Reconnect
    let handshake = match stream.next().await {
        Some(Ok(msg)) => msg,
        Some(Err(e)) => {
            log::error!("Failed to read handshake: {e}");
            return;
        }
        None => {
            log::error!("Connection closed during handshake");
            return;
        }
    };

    match handshake {
        RemoteInMessage::Hello => {
            // New Session - Ask Server for free player
            let (resp_tx, resp_rx) = oneshot::channel();
            if let Err(e) = main_tx
                .send(MainThreadMessage::RequestFreePlayer(resp_tx))
                .await
            {
                log::error!("Failed to contact server: {e}");
                return;
            }

            match resp_rx.await {
                Ok(Some(player)) => {
                    let session_id = Uuid::new_v4();
                    log::info!("New client assigned: {player} (Session: {session_id})");

                    // Send Welcome
                    if let Err(e) = sink.send(RemoteOutMessage::Welcome { session_id }).await {
                        log::error!("Failed to send Welcome: {e}");
                        return;
                    }

                    // Server thread -> Client thread
                    let (server_tx, server_rx) =
                        mpsc::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);

                    // Create client
                    match Client::new(
                        player,
                        sink,
                        stream,
                        server_rx,
                        server_broadcast_tx.subscribe(),
                        client_msg_tx,
                    ) {
                        Err(e) => log::error!("Failed to create client: {e:?}"),
                        Ok(mut client) => {
                            tokio::spawn(async move {
                                if let Err(e) = client.run().await {
                                    log::error!("Client task error: {e:?}");
                                }
                            });
                            if let Err(e) = main_tx
                                .send(MainThreadMessage::ClientConnected(
                                    player, session_id, server_tx,
                                ))
                                .await
                            {
                                log::error!("Failed to notify server of connection: {e:?}");
                            }
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("No free players");
                    let _ = sink
                        .send(RemoteOutMessage::Reject {
                            reason: "Server full".to_string(),
                        })
                        .await;
                }
                Err(e) => log::error!("Server channel error: {e}"),
            }
        }
        RemoteInMessage::Reconnect { session_id: uuid } => {
            log::info!("Reconnection attempt: {uuid}");
            let (resp_tx, resp_rx) = oneshot::channel();
            if let Err(e) = main_tx
                .send(MainThreadMessage::ClientReconnectedHandle(uuid, resp_tx))
                .await
            {
                log::error!("Failed to contact server: {e}");
                return;
            }

            match resp_rx.await {
                Ok(Some(player)) => {
                    // Ack
                    if let Err(e) = sink
                        .send(RemoteOutMessage::Welcome { session_id: uuid })
                        .await
                    {
                        log::error!("Failed to send Welcome: {e}");
                        return;
                    }

                    let (server_tx, server_rx) =
                        mpsc::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);
                    match Client::new(
                        player,
                        sink,
                        stream,
                        server_rx,
                        server_broadcast_tx.subscribe(),
                        client_msg_tx,
                    ) {
                        Err(e) => log::error!("Failed to create client: {e:?}"),
                        Ok(mut client) => {
                            tokio::spawn(async move {
                                if let Err(e) = client.run().await {
                                    log::error!("Client task error: {e:?}");
                                }
                            });
                            let _ = main_tx
                                .send(MainThreadMessage::ClientReconnected(player, server_tx))
                                .await;
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("Unknown session: {uuid}");
                    let _ = sink
                        .send(RemoteOutMessage::Reject {
                            reason: "Unknown Session".to_string(),
                        })
                        .await;
                }
                Err(e) => log::error!("Server channel error: {e}"),
            }
        }
        _ => {
            log::error!("Invalid handshake message");
        }
    };
}
