use std::pin::Pin;

use anyhow::{Context, Result, bail};
use futures::{Sink, SinkExt, Stream, StreamExt};
use tokio::sync::{broadcast, mpsc};

use crate::{
    server::protocol::{RemoteInMessage, RemoteOutMessage},
    sternhalma::board::{BOARD_LENGTH, HexIdx, movement::MovementIndices, player::Player},
};

use super::messages::{ClientMessage, ClientRequest, ServerBroadcast, ServerMessage};

// Transport abstraction
/// Value trait object for sending messages to the remote client
pub type ClientSink = Pin<Box<dyn Sink<RemoteOutMessage, Error = anyhow::Error> + Send + Unpin>>;
/// Value trait object for receiving messages from the remote client
pub type ClientStream =
    Pin<Box<dyn Stream<Item = Result<RemoteInMessage, anyhow::Error>> + Send + Unpin>>;

/// Representation of a connected client in the server
///
/// This struct manages the state and communication for a single connected player.
/// It runs in its own thread (tokio task) and acts as an intermediary between
/// the remote client (via TCP) and the main server logic (via mpsc channels).
pub struct Client {
    /// Player assigned to client
    player: Player,
    /// Sink for messages to remote client (TCP Output)
    sink: ClientSink,
    /// Stream of messages from remote client (TCP Input)
    stream: ClientStream,
    /// Receiver for direct message from the server (Server -> Client)
    server_rx: mpsc::Receiver<ServerMessage>,
    /// Receiver for messages broadcast by the server (Server -> All Clients)
    broadcast_rx: broadcast::Receiver<ServerBroadcast>,
    /// Sender for messages to the server thread (Client -> Server)
    client_tx: mpsc::Sender<ClientMessage>,
}

impl Client {
    /// Creates a new Client instance
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

    /// Transforms an absolute player to a relative player for the client
    ///
    /// # Design Decision
    /// The protocol uses relative player identities so that every client views themselves
    /// as `Player1` (at the bottom) and the opponent as `Player2` (at the top).
    /// This simplifies client-side logic by providing a consistent perspective.
    fn relative_player(&self, player: Player) -> Player {
        match self.player {
            Player::Player1 => player,
            Player::Player2 => player.opponent(),
        }
    }

    /// Transforms an absolute index to a relative index for the client
    ///
    /// # Design Decision
    /// See `relative_player`. Coordinates are rotated 180 degrees for Player 2
    /// so they effectively play from the "bottom" perspective as well.
    fn relative_idx(&self, idx: HexIdx) -> HexIdx {
        match self.player {
            Player::Player1 => idx,
            Player::Player2 => idx.map(|c| BOARD_LENGTH - 1 - c),
        }
    }

    /// Transforms an absolute movement to a relative movement for the client
    fn relative_movement(&self, movement: MovementIndices) -> MovementIndices {
        movement.map(|idx| self.relative_idx(idx))
    }

    /// Sends a message to the remote client via the TCP connection
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

    /// Sends a request to the main server thread
    async fn send_request(&mut self, request: ClientRequest) -> Result<()> {
        log::debug!(
            "[Player {}] Sending request to server: {request:?}",
            self.player
        );

        // Package message with player identity
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

    /// Handles an incoming message from the remote client (TCP)
    async fn handle_remote_message(&mut self, message: RemoteInMessage) -> Result<()> {
        log::debug!(
            "[Player {}] Handling remote message: {message:?}",
            self.player
        );

        match message {
            // Forward player choice to the server
            RemoteInMessage::Choice { movement_index } => self
                .send_request(ClientRequest::Choice { movement_index })
                .await
                .with_context(|| "Unable to forward message to server"),
            _ => Ok(()), // Handshake handled separately during connection phase
        }
    }

    /// Handles a broadcast message from the server
    ///
    /// These messages are sent to all connected clients (e.g., game updates).
    async fn handle_server_broadcast(&mut self, message: ServerBroadcast) -> Result<()> {
        log::debug!("[Player {}] Received broadcast: {message:?}", self.player);

        match message {
            // Server is shutting down or resetting
            ServerBroadcast::Disconnect => {
                self.send_remote_message(RemoteOutMessage::Disconnect)
                    .await?;
            }
            // A player made a move, update remote client
            ServerBroadcast::Movement {
                player,
                movement,
                scores,
            } => {
                // Transform scores to match the relative player perspective
                let scores = match self.player {
                    Player::Player1 => scores,
                    Player::Player2 => [scores[1], scores[0]],
                };
                self.send_remote_message(RemoteOutMessage::Movement {
                    player: self.relative_player(player),
                    movement: self.relative_movement(movement),
                    scores,
                })
                .await?;
            }
            // Game has ended
            ServerBroadcast::GameFinished { result } => {
                let result = match result {
                    crate::sternhalma::GameResult::Finished {
                        winner,
                        total_turns,
                        scores,
                    } => {
                        let scores = match self.player {
                            Player::Player1 => scores,
                            Player::Player2 => [scores[1], scores[0]],
                        };
                        crate::sternhalma::GameResult::Finished {
                            winner: self.relative_player(winner),
                            total_turns,
                            scores,
                        }
                    }
                    crate::sternhalma::GameResult::MaxTurns {
                        total_turns,
                        scores,
                    } => {
                        let scores = match self.player {
                            Player::Player1 => scores,
                            Player::Player2 => [scores[1], scores[0]],
                        };
                        crate::sternhalma::GameResult::MaxTurns {
                            total_turns,
                            scores,
                        }
                    }
                };
                self.send_remote_message(RemoteOutMessage::GameFinished { result })
                    .await?;
            }
        };

        Ok(())
    }

    /// Handles a direct message from the server
    ///
    /// These messages are specific to this client (e.g., "It's your turn").
    async fn handle_server_message(&mut self, message: ServerMessage) -> Result<()> {
        log::debug!("[Player {}] Received message: {message:?}", self.player);

        match message {
            // It is this player's turn
            ServerMessage::Turn { movements } => {
                let movements = movements
                    .into_iter()
                    .map(|m| self.relative_movement(m))
                    .collect();
                self.send_remote_message(RemoteOutMessage::Turn { movements })
                    .await?;
            }
        }

        Ok(())
    }

    /// Client thread main loop
    ///
    /// This method runs indefinitely until the client disconnects or an error occurs.
    /// It multiplexes events from:
    /// 1. Messages from the main server thread (Direct)
    /// 2. Broadcasts from the main server thread (Broadcast)
    /// 3. Messages from the remote client (Network)
    pub async fn run(&mut self) -> Result<()> {
        log::trace!("[Player {}] Task spawned", self.player);

        log::trace!("[Player {}] Task spawned", self.player);

        loop {
            tokio::select! {

                // Incoming message from server (Direct)
                server_message = self.server_rx.recv() => {
                    match server_message {
                        None => bail!("Server message channel closed"),
                        Some(message) => {
                            log::debug!("[Player {}] Received server message: {message:?}",self.player);
                            self.handle_server_message(message).await.with_context(|| "Unable to handle server message")?;
                        }
                    }

                }

                // Incoming broadcast from the server (Broadcast)
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

                // Incoming messages from remote client (Network)
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
