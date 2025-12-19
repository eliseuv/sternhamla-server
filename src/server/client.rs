use std::pin::Pin;

use anyhow::{Context, Result, bail};
use futures::{Sink, SinkExt, Stream, StreamExt};
use tokio::sync::{broadcast, mpsc};

use crate::{
    protocol::{RemoteInMessage, RemoteOutMessage},
    sternhalma::board::player::Player,
};

use super::messages::{ClientMessage, ClientRequest, ServerBroadcast, ServerMessage};

// Transport abstraction
pub type ClientSink = Pin<Box<dyn Sink<RemoteOutMessage, Error = anyhow::Error> + Send + Unpin>>;
pub type ClientStream =
    Pin<Box<dyn Stream<Item = Result<RemoteInMessage, anyhow::Error>> + Send + Unpin>>;

pub struct Client {
    /// Player assigned to client
    player: Player,
    /// Sink for messages to remote client
    sink: ClientSink,
    /// Stream of messages from remote client
    stream: ClientStream,
    /// Receiver for direct message from the server
    server_rx: mpsc::Receiver<ServerMessage>,
    /// Receiver for messages broadcast by the server
    broadcast_rx: broadcast::Receiver<ServerBroadcast>,
    /// Sender for messages to the server thread
    client_tx: mpsc::Sender<ClientMessage>,
}

impl Client {
    pub fn new(
        player: Player,
        sink: ClientSink,
        stream: ClientStream,
        server_rx: mpsc::Receiver<ServerMessage>,
        broadcast_rx: broadcast::Receiver<ServerBroadcast>,
        client_tx: mpsc::Sender<ClientMessage>,
    ) -> Result<Self> {
        log::debug!("Creating client {player}");

        Ok(Self {
            player,
            sink,
            stream,
            server_rx,
            broadcast_rx,
            client_tx,
        })
    }

    async fn send_remote_message(&mut self, message: RemoteOutMessage) -> Result<()> {
        log::debug!(
            "[Player {}] Sending remote message: {message:?}",
            self.player
        );

        self.sink
            .send(message)
            .await
            .with_context(|| "Failed to send remote message")
    }

    async fn send_request(&mut self, request: ClientRequest) -> Result<()> {
        log::debug!(
            "[Player {}] Sending request to server: {request:?}",
            self.player
        );

        // Package message
        let message = ClientMessage {
            player: self.player,
            request,
        };

        // Send it to server
        self.client_tx
            .send(message)
            .await
            .with_context(|| "Failed to send message to server")
    }

    async fn handle_remote_message(&mut self, message: RemoteInMessage) -> Result<()> {
        log::debug!(
            "[Player {}] Handling remote message: {message:?}",
            self.player
        );

        match message {
            RemoteInMessage::Choice { movement_index } => self
                .send_request(ClientRequest::Choice { movement_index })
                .await
                .with_context(|| "Unable to forward message to server"),
            _ => Ok(()), // Handshake handled separately
        }
    }

    async fn handle_server_broadcast(&mut self, message: ServerBroadcast) -> Result<()> {
        log::debug!("[Player {}] Received broadcast: {message:?}", self.player);

        match message {
            ServerBroadcast::Disconnect => {
                self.send_remote_message(RemoteOutMessage::Disconnect)
                    .await?;
            }
            ServerBroadcast::Movement {
                player,
                movement,
                scores,
            } => {
                self.send_remote_message(RemoteOutMessage::Movement {
                    player,
                    movement,
                    scores,
                })
                .await?;
            }
            ServerBroadcast::GameFinished { result } => {
                self.send_remote_message(RemoteOutMessage::GameFinished { result })
                    .await?;
            }
        };

        Ok(())
    }

    async fn handle_server_message(&mut self, message: ServerMessage) -> Result<()> {
        log::debug!("[Player {}] Received message: {message:?}", self.player);

        match message {
            ServerMessage::Turn { movements } => {
                self.send_remote_message(RemoteOutMessage::Turn { movements })
                    .await?;
            }
        }

        Ok(())
    }

    /// Client thread main loop
    pub async fn run(&mut self) -> Result<()> {
        log::trace!("[Player {}] Task spawned", self.player);

        // Send player assignment message to remote client
        self.send_remote_message(RemoteOutMessage::Assign {
            player: self.player,
        })
        .await
        .with_context(|| "Falied to send player assignment message to remote client")?;

        loop {
            tokio::select! {

                // Incoming message from server
                server_message = self.server_rx.recv() => {
                    match server_message {
                        None => bail!("Server message channel closed"),
                        Some(message) => {
                            log::debug!("[Player {}] Received server message: {message:?}",self.player);
                            self.handle_server_message(message).await.with_context(|| "Unable to handle server message")?;
                        }
                    }

                }

                // Incoming broadcast from the server
                broadcast = self.broadcast_rx.recv() => {
                    match broadcast {
                        Err(broadcast::error::RecvError::Closed) => {
                            bail!("Server broadcast channel closed");
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::error!("[Player {}] Server channel lagged by {n} messages", self.player);
                        }
                        Ok(message) => {
                            log::debug!("[Player {}] Received server broadcast: {message:?}",self.player);
                            self.handle_server_broadcast(message).await.with_context(|| "Unable to handle server broadcast")?;
                        }
                    }
                }

                // Incoming messages from remote client
                remote_message = self.stream.next() => {
                    log::debug!("[Player {}] New message from remote client",self.player);
                    match remote_message {
                        Some(Ok(message)) => {
                            if let Err(e) = self.handle_remote_message(message).await {
                                log::error!("[Player {}] Error handling remote message: {e:?}",self.player);
                                continue;
                            }
                        }
                        Some(Err(e)) => {
                            log::error!("[Player {}] Failed to receive remote message: {e:?}",self.player);
                            continue;
                        }
                        None => {
                            log::info!("[Player {}] Remote client disconnected",self.player);
                            break;
                        }
                    }
                }
            }
        }

        // Send disconnect request to server
        // We ignore the error here because if the server is down, we are shutting down anyway.
        // And we can't do much if it fails.
        let _ = self.send_request(ClientRequest::Disconnect).await;

        Ok(())
    }
}
