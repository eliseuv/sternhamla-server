use std::{
    collections::{HashMap, HashSet, hash_map},
    fmt::Display,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use itertools::Itertools;
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, oneshot},
};
use uuid::Uuid;

use axum::{
    Router,
    // ADDED
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures::{Sink, SinkExt, Stream, StreamExt};
use std::pin::Pin;
use sternhalma_server::{
    protocol::{GameResult, RemoteInMessage, RemoteOutMessage, Scores, ServerCodec},
    sternhalma::{
        Game, GameStatus,
        board::{movement::MovementIndices, player::Player},
        timing::GameTimer,
    },
};
use tokio_util::codec::Framed; // ADDED

/// Capacity communication channels between local threads
const LOCAL_CHANNEL_CAPACITY: usize = 32;

#[derive(Debug)]
enum ServerMessage {
    /// Players turn
    Turn {
        /// List of available movements
        movements: Vec<MovementIndices>,
    },
}

/// Server Thread -> All Local Client Threads
#[derive(Debug, Clone)]
enum ServerBroadcast {
    /// Disconnection signal,
    Disconnect,
    /// Player made a move
    Movement {
        player: Player,
        movement: MovementIndices,
        scores: Scores,
    },
    /// Game has finished
    GameFinished { result: GameResult },
}

// Ensure variants are clonable if needed, or broadcast handles it.
// ServerBroadcast is Clone.

/// Local Client Thread -> Server Thread
#[derive(Debug)]
enum ClientRequest {
    /// Disconnection request
    Disconnect,
    /// Player made a movement
    Choice { movement_index: usize },
}

/// Packaged client request with identification
/// Local Client Thread -> Server Thread
#[derive(Debug)]
struct ClientMessage {
    player: Player,
    request: ClientRequest,
}

#[derive(Debug)]
struct Server {
    // Channel to receive messages from the main thread
    main_rx: mpsc::Receiver<MainThreadMessage>,
    // Clients list
    clients_tx: HashMap<Player, mpsc::Sender<ServerMessage>>,
    // Session management
    sessions: HashMap<Uuid, Player>,
    // Disconnected players with active sessions
    disconnected: HashSet<Player>,
    // Channel for broadcasting messages to all local client threads
    broadcast_tx: broadcast::Sender<ServerBroadcast>,
    // Channel for receiving messages from local client threads
    clients_rx: mpsc::Receiver<ClientMessage>,
}

impl Server {
    /// Creates a new server instance
    fn new(
        main_rx: mpsc::Receiver<MainThreadMessage>,
        clients_rx: mpsc::Receiver<ClientMessage>,
        broadcast_tx: broadcast::Sender<ServerBroadcast>,
    ) -> Result<Self> {
        Ok(Self {
            main_rx,
            clients_tx: HashMap::new(),
            sessions: HashMap::new(),
            disconnected: HashSet::new(),
            broadcast_tx,
            clients_rx,
        })
    }

    /// Wait for all players to connect
    async fn wait_players_connect(&mut self, n_players: usize) -> Result<()> {
        while self.clients_tx.len() < n_players {
            // Wait for message from main thread
            match self
                .main_rx
                .recv()
                .await
                .ok_or(anyhow!("Channel from main thread to server close"))?
            {
                // A client has connected
                MainThreadMessage::ClientConnected(player, session_id, client_tx) => {
                    // Check if player is already assigned
                    if let hash_map::Entry::Vacant(entry) = self.clients_tx.entry(player) {
                        entry.insert(client_tx);
                        self.sessions.insert(session_id, player);
                        log::info!(
                            "Player {player} connected with session {session_id}. ({n_connected}/{n_players})",
                            n_connected = self.clients_tx.len()
                        );
                    } else {
                        log::error!("Player {player} is already connected");
                        // TODO: Inform main thread about wrong assignment of player
                        continue;
                    }
                }
                MainThreadMessage::ClientReconnected(..) => {
                    log::warn!("Reconnection attempt during connection phase ignored");
                }
                MainThreadMessage::ClientReconnectedHandle(uuid, resp_tx) => {
                    // Check if session exists
                    let player = self.sessions.get(&uuid).copied();
                    let _ = resp_tx.send(player);
                }
                MainThreadMessage::RequestFreePlayer(resp_tx) => {
                    // Find a player not in sessions/clients
                    // We check which players are connected.
                    // Connected = in self.clients_tx
                    // We assume max 2 players.
                    // This logic assumes we want to fill P1 then P2.
                    // Or any available.
                    let free_player = [Player::Player1, Player::Player2]
                        .into_iter()
                        .find(|p| !self.clients_tx.contains_key(p));
                    let _ = resp_tx.send(free_player);
                }
            }
        }
        log::info!(
            "All {n_connected} players connected",
            n_connected = self.clients_tx.len()
        );
        Ok(())
    }

    async fn disconnect_players(&mut self) -> Result<()> {
        let _ = self
            .broadcast_tx
            .send(ServerBroadcast::Disconnect)
            .with_context(|| "Failed to broadcast disconnect signal");
        while !self.clients_tx.is_empty() {
            log::info!(
                "Connected players: {n_players}",
                n_players = self.clients_tx.len()
            );
            // Wait for message
            let message = self
                .clients_rx
                .recv()
                .await
                .ok_or(anyhow!("Local channel closed"))?;
            // Identify client
            let player = message.player;
            log::debug!("Message received from player {player}");
            match message.request {
                // Disconnection request
                ClientRequest::Disconnect => {
                    log::info!("Player {player} disconnected");
                    if self.clients_tx.remove(&player).is_none() {
                        log::warn!("Player {player} was already disconnected");
                        continue;
                    }
                }

                // Invalid request
                request => {
                    log::warn!("Invalid request: {request:?}");
                    continue;
                }
            }
        }

        Ok(())
    }

    async fn handle_turn(&mut self, game: &mut Game, current_player: Player) -> Result<GameStatus> {
        log::debug!("Player {current_player} turn");

        // Calculate available moves
        let movements: Vec<MovementIndices> = game
            .iter_available_moves()
            .map(|movement| (&movement).into())
            .unique()
            .collect();

        // If player is disconnected, wait for reconnection logic to trigger in loop
        if !self.disconnected.contains(&current_player) {
            // Send turn message to current player
            self.clients_tx
                .get_mut(&current_player)
                .ok_or(anyhow!("Unable to find player {current_player}"))?
                .send(ServerMessage::Turn {
                    movements: movements.clone(),
                })
                .await
                .with_context(|| {
                    format!("Failed to send turn message to player {current_player}")
                })?;
        }

        // Message receiving loop
        loop {
            tokio::select! {

                // Message from main threat
                main_msg = self.main_rx.recv() => {
                   match main_msg {
                        None => bail!("Channel from main thread closed"),
                        Some(MainThreadMessage::ClientReconnected(player, tx)) => {
                            // Resume disconnected player
                            if self.disconnected.remove(&player) {
                                log::info!("Player {player} reconnected");
                                self.clients_tx.insert(player, tx);

                                // Resend turn if it is their turn
                                if player == current_player {
                                    self.clients_tx
                                        .get_mut(&current_player)
                                        .expect("Player just reconnected")
                                        .send(ServerMessage::Turn {
                                            movements: movements.clone(),
                                        })
                                        .await
                                        .with_context(|| format!("Failed to send turn message to player {current_player}"))?;
                                }
                            } else {
                                log::warn!("Player {player} reconnected but was not marked as disconnected");
                            }
                        }
                         Some(MainThreadMessage::ClientConnected(..)) => {
                             log::warn!("New client connected during game loop - ignored");
                         }
                         Some(MainThreadMessage::ClientReconnectedHandle(uuid, resp_tx)) => {
                             // Check if session exists
                             let player = self.sessions.get(&uuid).copied();
                             let _ = resp_tx.send(player);
                         }
                         Some(MainThreadMessage::RequestFreePlayer(resp_tx)) => {
                             let free_player = [Player::Player1, Player::Player2]
                                .into_iter()
                                .find(|p| !self.clients_tx.contains_key(p));
                            let _ = resp_tx.send(free_player);
                        }
                    }
                }

                // Message from client thread
                client_msg = self.clients_rx.recv() => {
                    match client_msg {
                        None => {
                            log::error!("Channel from clients closed");
                            bail!("Channel from client closed");
                        }
                        Some(message) => {
                            // Identify client
                            let player = message.player;

                            // Handle request
                            match message.request {
                                // Client will disconnect
                                ClientRequest::Disconnect => {
                                    log::info!("Player {player} disconnected mid-game");
                                    self.clients_tx.remove(&player);
                                    self.disconnected.insert(player);
                                    // We continue waiting for other players or reconnection
                                    // This pauses the turn if it was their turn, until they reconnect or timeout
                                }

                                // Client chose a movement
                                ClientRequest::Choice { movement_index } => {

                                    // Check if player is the current player
                                    if player != current_player {
                                        log::error!("Player {player} attempted to move out of turn");
                                        // TODO: Inform player of the out of turn movement
                                        continue;
                                    }

                                    // Validate movement index
                                    let movement = match movements.get(movement_index) {
                                        Some(m) => m,
                                        None => {
                                             log::warn!("Player {player} sent invalid movement index: {movement_index}");
                                             // TODO: Inform player
                                             continue;
                                        }
                                    };

                                    log::debug!("Player {player} chose movement {movement:?}");

                                    // Apply chosen movement
                                    // Validated by index selection
                                    let status = unsafe { game.apply_movement_unchecked(movement) };

                                    // Broadcast movement to all players
                                    self.broadcast_tx.send(ServerBroadcast::Movement {player,movement: *movement, scores: status.scores() }).with_context(|| "Failed to broadcast movement")?;

                                    return Ok(status);

                                }
                            }
                        },
                    }

                }

            }
        }
    }

    async fn game_loop(&mut self, max_turns: usize) -> Result<GameResult> {
        // Create game
        let mut game = Game::new();

        // Print initial board state
        println!("{game}");

        // Game timer
        let mut game_timer = GameTimer::<256>::new();

        // Game loop
        loop {
            match game.status() {
                // Game has finished
                GameStatus::Finished {
                    winner,
                    total_turns,
                    scores,
                } => {
                    // Calculate scores
                    return Ok(GameResult::Finished {
                        winner,
                        total_turns,
                        scores,
                    });
                }

                // Game is ongoing
                GameStatus::Playing {
                    player: current_player,
                    turns,
                    scores,
                } => {
                    // Check for maximum turns
                    if turns >= max_turns {
                        // Calculate scores
                        return Ok(GameResult::MaxTurns {
                            total_turns: turns,
                            scores,
                        });
                    }

                    // Handle turn
                    self.handle_turn(&mut game, current_player)
                        .await
                        .with_context(|| "Falied to handle game turn")?;

                    // // Update timing
                    // game_timer.on_trigger(&game, |timer| {
                    //     // Calculate size of game history in memory
                    //     let hist_size: ByteSize = game.history_bytes().into();
                    //     // Log information
                    //     log::info!(
                    //         "Turns: {turns} | Rate: {rate:.2} turn/s | History: {hist_size}",
                    //         turns = game.status().turns(),
                    //         rate = timer.turns_rate(),
                    //     )
                    // });

                    // Print board state
                    game_timer.update(&game);
                    println!(
                        "{game} | Rate: {rate:.2} turn/s",
                        rate = game_timer.turns_rate(),
                    );
                }
            }
        }
    }

    /// Main server thread loop
    async fn run(&mut self, timeout: Duration, max_turns: usize) -> Result<()> {
        log::trace!("Server thread started");

        // Wait for players to connect
        let n_players = Player::count();
        log::info!(
            "Waiting {timeout_secs} seconds for {n_players} players to connect...",
            timeout_secs = timeout.as_secs()
        );
        tokio::time::timeout(timeout, self.wait_players_connect(n_players))
            .await
            .with_context(|| "Timed out waiting for players to connect")?
            .with_context(|| "Failed to wait for players to connect")?;

        // Main game loop
        match self
            .game_loop(max_turns)
            .await
            .with_context(|| "Game loop encountered an error")?
        {
            GameResult::MaxTurns {
                total_turns,
                scores,
            } => {
                log::warn!("Game reached maximum number of turns: {total_turns}");
                self.broadcast_tx
                    .send(ServerBroadcast::GameFinished {
                        result: GameResult::MaxTurns {
                            total_turns,
                            scores,
                        },
                    })
                    .with_context(|| "Failed to broadcast maximum turns message")?;
            }
            GameResult::Finished {
                winner,
                total_turns,
                scores,
            } => {
                log::info!("Game finished, player {winner} won after {total_turns} turns");
                // Broadcast game finished message
                self.broadcast_tx
                    .send(ServerBroadcast::GameFinished {
                        result: GameResult::Finished {
                            winner,
                            total_turns,
                            scores,
                        },
                    })
                    .with_context(|| "Failed to broadcast game finished message")?;
            }
        }

        Ok(())
    }

    /// Server thread run wrapper
    async fn try_run(mut self, timeout: Duration, max_turns: usize) -> Result<()> {
        // Attempt to run server
        let result = self.run(timeout, max_turns).await;

        // Disconnect all players
        log::info!("Disconnecting all players");
        if let Err(e) = self.disconnect_players().await {
            log::error!("Failed to disconnect all players: {e:?}");
        }

        result
    }
}

// Transport abstraction
type ClientSink = Pin<Box<dyn Sink<RemoteOutMessage, Error = anyhow::Error> + Send + Unpin>>;
type ClientStream =
    Pin<Box<dyn Stream<Item = Result<RemoteInMessage, anyhow::Error>> + Send + Unpin>>;

struct Client {
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
    fn new(
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
    async fn run(&mut self) -> Result<()> {
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

/// Command line arguments
#[derive(Debug, Parser)]
#[command(name = "sternhalma-server", version, about)]
struct Args {
    /// Host IP address
    #[arg(short, long, value_name = "ADDRESS", default_value = "127.0.0.1:8080")]
    socket: String,
    /// WebSocket port
    #[arg(long, value_name = "PORT", default_value_t = 8081)]
    ws_port: u16,
    /// Maximum number of turns
    #[arg(short = 'n', long, value_name = "N")]
    max_turns: usize,
    #[arg(short, long, value_name = "SECONDS", default_value_t = 30)]
    timeout: u64,
}

// Deleted assign_player

/// Main thread message to server thread
#[derive(Debug)]
enum MainThreadMessage {
    ClientConnected(Player, Uuid, mpsc::Sender<ServerMessage>),
    ClientReconnected(Player, mpsc::Sender<ServerMessage>),
    ClientReconnectedHandle(Uuid, oneshot::Sender<Option<Player>>),
    RequestFreePlayer(oneshot::Sender<Option<Player>>),
}

#[derive(Clone)]
struct AppState {
    main_tx: mpsc::Sender<MainThreadMessage>,
    client_msg_tx: mpsc::Sender<ClientMessage>,
    server_broadcast_tx: broadcast::Sender<ServerBroadcast>,
}

async fn handle_handshake(mut stream: ClientStream, mut sink: ClientSink, app_state: AppState) {
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
                    if let Err(e) = sink
                        .send(RemoteOutMessage::Welcome { session_id, player })
                        .await
                    {
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
                        .send(RemoteOutMessage::Welcome {
                            session_id: uuid,
                            player,
                        })
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

async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

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

use futures::future;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    // Parse command line arguments
    let args = Args::parse();
    log::debug!("Command line arguments: {args:?}");
    let timeout = Duration::from_secs(args.timeout);

    // Client threads -> Server thread
    let (client_msg_tx, client_msg_rx) = mpsc::channel::<ClientMessage>(LOCAL_CHANNEL_CAPACITY);

    // Server thread -> Client threads
    let (server_broadcast_tx, _server_broadcast_rx) =
        broadcast::channel::<ServerBroadcast>(LOCAL_CHANNEL_CAPACITY);

    // Main thread -> Server thread
    let (main_tx, main_rx) = mpsc::channel::<MainThreadMessage>(LOCAL_CHANNEL_CAPACITY);

    // Channel for the server thread to send shutdown signal to main thread
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Spawn the server task
    let server = Server::new(main_rx, client_msg_rx, server_broadcast_tx.clone())
        .with_context(|| "Failed to create server")?;
    tokio::spawn(async move {
        if let Err(e) = server.try_run(timeout, args.max_turns).await {
            log::error!("Server encountered an error: {e:?}");
        }
        log::trace!("Sending shutdown signal");
        let _ = shutdown_tx.send(());
    });

    // App State
    let app_state = AppState {
        main_tx: main_tx.clone(),
        client_msg_tx,
        server_broadcast_tx,
    };

    // Bind TCP listener to the socket address
    let listener = TcpListener::bind(&args.socket)
        .await
        .with_context(|| "Failed to bind listener to socket")?;
    log::info!("Listening (TCP) at {socket:?}", socket = args.socket);

    // Axum server
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(app_state.clone());

    let ws_addr = format!("0.0.0.0:{}", args.ws_port);
    let ws_listener = tokio::net::TcpListener::bind(&ws_addr).await?;
    log::info!("Listening (WS) at {ws_addr}");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(ws_listener, app).await {
            log::error!("Axum server error: {e}");
        }
    });

    tokio::select! {
        // TCP Connections loop
        _ = async {
            loop {
                match listener.accept().await {
                    Err(e) => {
                        log::error!("Failed to accept connection: {e:?}");
                        continue;
                    }
                    Ok((stream, _addr)) => {
                        let framed = Framed::new(stream, ServerCodec::new());
                        let (write, read) = framed.split();
                        let sink: ClientSink = Box::pin(write.sink_map_err(|e| anyhow::anyhow!(e)));
                        let stream: ClientStream = Box::pin(read.map(|msg| msg.map_err(|e| anyhow::anyhow!(e))));

                        tokio::spawn(handle_handshake(stream, sink, app_state.clone()));
                    }
                }
            }
        } => {},

        // Shutdown signal
        _ = shutdown_rx => {
            log::trace!("Shutdown singnal received");
        }
    }

    Ok(())
}
