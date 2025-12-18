use std::{
    collections::{HashMap, HashSet, hash_map},
    fmt::Display,
    io::{self, Cursor, Write},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, oneshot},
};
use uuid::Uuid;

use sternhalma_server::sternhalma::{
    Game, GameStatus,
    board::{
        movement::MovementIndices,
        player::{PLAYER_COUNT, Player},
    },
    timing::GameTimer,
};

/// Capacity communication channels between local threads
const LOCAL_CHANNEL_CAPACITY: usize = 32;
/// Maximum length of a remote message in bytes
const REMOTE_MESSAGE_LENGTH: usize = 4 * 1024;

type Scores = [usize; PLAYER_COUNT];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum GameResult {
    Finished {
        winner: Player,
        total_turns: usize,
        scores: Scores,
    },
    MaxTurns {
        total_turns: usize,
        scores: Scores,
    },
}

/// Local Client Thread -> Remote Client
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteOutMessage {
    /// Welcome message with session ID
    Welcome { session_id: Uuid, player: Player },
    /// Reconnect reject
    Reject(String),
    /// Inform remote client about their assigned player
    Assign { player: Player },
    /// Disconnection signal
    Disconnect,
    /// Inform remote client that it is their turn
    Turn {
        /// List of available movements
        /// Each movement is represented by a pair of indices
        movements: Vec<MovementIndices>,
    },
    /// Inform remote client about a player's movement
    Movement {
        player: Player,
        movement: MovementIndices,
        scores: Scores,
    },
    /// Inform remote client that the game has finished with a result
    GameFinished { result: GameResult },
}

/// Remote Client -> Local Client Thread
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteInMessage {
    /// Hello - Request new session
    Hello,
    /// Reconnect - Request resume session
    Reconnect(Uuid),
    /// Movement made by player (index based)
    Choice { movement_index: usize },
}

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

impl RemoteInMessage {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ciborium::from_reader(bytes).with_context(|| "Failed to deserialize remote message")
    }
}

impl RemoteOutMessage {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        ciborium::into_writer(self, writer).with_context(|| "Failed to serialize remote message")
    }
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

#[derive(Debug)]
struct Client {
    /// Player assigned to client
    player: Player,
    /// TCP stream for communication with the remote client
    stream: TcpStream,
    /// Receiver for direct message from the server
    server_rx: mpsc::Receiver<ServerMessage>,
    /// Receiver for messages broadcast by the server
    broadcast_rx: broadcast::Receiver<ServerBroadcast>,
    /// Sender for messages to the server thread
    client_tx: mpsc::Sender<ClientMessage>,
    /// Buffer for incoming remote messages
    buffer_in: [u8; REMOTE_MESSAGE_LENGTH],
    /// Buffer for outgoing remote messages
    buffer_out: [u8; REMOTE_MESSAGE_LENGTH],
}

impl Client {
    fn new(
        player: Player,
        stream: TcpStream,
        server_rx: mpsc::Receiver<ServerMessage>,
        broadcast_rx: broadcast::Receiver<ServerBroadcast>,
        client_tx: mpsc::Sender<ClientMessage>,
    ) -> Result<Self> {
        log::debug!("Creating client {player}");

        Ok(Self {
            player,
            stream,
            server_rx,
            broadcast_rx,
            client_tx,
            buffer_in: [0; REMOTE_MESSAGE_LENGTH],
            buffer_out: [0; REMOTE_MESSAGE_LENGTH],
        })
    }

    async fn send_remote_message(&mut self, message: RemoteOutMessage) -> Result<()> {
        log::debug!(
            "[Player {}] Sending remote message: {message:?}",
            self.player
        );

        // Serialize message to buffer
        let mut writer = Cursor::new(&mut self.buffer_out[..]);
        message
            .write(&mut writer)
            .with_context(|| "Failed to write remote message")?;

        // Send the message length
        let bytes_written = writer.position() as usize;
        self.stream
            .write_u32(bytes_written as u32)
            .await
            .with_context(|| "Failed to send message length to remote client")?;

        // Send the actual message
        self.stream
            .write_all(&writer.get_ref()[..bytes_written])
            .await
            .with_context(|| "Failed to send message to remote client")?;

        log::debug!(
            "[Player {}] Successfully sent payload of {bytes_written} to remote client",
            self.player,
        );
        Ok(())
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

    async fn receive_remote_message(&mut self, message_length: usize) -> Result<RemoteInMessage> {
        log::debug!(
            "[Player {}] Message length: {message_length} bytes",
            self.player
        );
        if message_length == 0 || message_length > REMOTE_MESSAGE_LENGTH {
            // TODO: Disconnect client after a number of errors
            bail!(
                "[Player {}] Invalid message length: {message_length} bytes",
                self.player
            );
        }

        // Receive actual message
        self.stream
            .read_exact(&mut self.buffer_in[..message_length])
            .await
            .with_context(|| {
                format!("Failed to receive message of length {message_length} from remote client")
            })?;
        log::debug!(
            "[Player {}] Remote message receive successfully",
            self.player,
        );

        // Decode message
        RemoteInMessage::from_bytes(&self.buffer_in[..message_length])
            .with_context(|| "Failed to decode remote message")
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

        let mut message_length_buffer = [0; 4];
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
                remote_message = self.stream.read_exact(&mut message_length_buffer) => {
                    log::debug!("[Player {}] New message from remote client",self.player);
                    match remote_message {
                        Ok(0) => {
                            log::info!("[Player {}] Remote client disconnected",self.player);
                            break;
                        }
                        Ok(4) => {
                            // Message length properly received
                            let message_length =  u32::from_be_bytes(message_length_buffer) as usize;
                            // Receive actual message
                            match self.receive_remote_message(message_length).await {
                                Err(e) => {
                                    log::error!("[Player {}] Failed to receive remote message: {e:?}",self.player);
                                    continue;
                                }
                                Ok(message) => {
                                    if let Err(e) = self.handle_remote_message(message).await {
                                        log::error!("[Player {}] Error handling remote message: {e:?}",self.player);
                                        continue;
                                    }
                                }
                            }

                        }
                        Ok(n) => {
                            log::error!("[Player {}] Failed to receive message length: {n}/4 bytes received", self.player);
                            continue;
                        }
                        Err(e) => {
                            match e.kind() {
                                io::ErrorKind::UnexpectedEof => {
                                    log::info!("[Player {}] Remote client disconnected", self.player);
                                    break;
                                }
                                _ => {
                                    log::error!("[Player {}] Failed to receive message length: {e:?}", self.player);
                                    continue;
                                }

                            }
                        }
                    }
                }

            }
        }

        Ok(())
    }

    /// Client thread run wrapper
    async fn try_run(mut self) -> Result<()> {
        // Attempt to run client
        let result = self.run().await;

        // Send disconnect request to server
        if let Err(e) = self.send_request(ClientRequest::Disconnect).await {
            log::error!(
                "[Player {}] Failed to send disconnect request to server: {e:?}",
                self.player
            );
        }

        result
    }
}

/// Command line arguments
#[derive(Debug, Parser)]
#[command(name = "sternhalma-server", version, about)]
struct Args {
    /// Host IP address
    #[arg(short, long, value_name = "ADDRESS", default_value = "127.0.0.1:8080")]
    socket: String,
    /// Maximum number of turns
    #[arg(short = 'n', long, value_name = "N")]
    max_turns: usize,
    #[arg(short, long, value_name = "SECONDS", default_value_t = 30)]
    timeout: u64,
}

/// Error type for too many players
/// Can later be expanded to an enum of all different errors related to player connection
#[derive(Debug)]
struct TooManyPlayers;

impl Display for TooManyPlayers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Too many players connected")
    }
}

/// Assign player to a new client
const fn assign_player(client_id: usize) -> Result<Player, TooManyPlayers> {
    match client_id {
        0 => Ok(Player::Player1),
        1 => Ok(Player::Player2),
        _ => Err(TooManyPlayers),
    }
}

/// Main thread message to server thread
#[derive(Debug)]
enum MainThreadMessage {
    ClientConnected(Player, Uuid, mpsc::Sender<ServerMessage>),
    ClientReconnected(Player, mpsc::Sender<ServerMessage>),
    ClientReconnectedHandle(Uuid, oneshot::Sender<Option<Player>>),
}

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
    let (server_broadcast_tx, server_broadcast_rx) =
        broadcast::channel::<ServerBroadcast>(LOCAL_CHANNEL_CAPACITY);

    // Main thread -> Server thread
    let (main_tx, main_rx) = mpsc::channel::<MainThreadMessage>(LOCAL_CHANNEL_CAPACITY);

    // Channel for the server thread to send shutdown signal to main thread
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Spawn the server task
    let server = Server::new(main_rx, client_msg_rx, server_broadcast_tx)
        .with_context(|| "Failed to create server")?;
    tokio::spawn(async move {
        if let Err(e) = server.try_run(timeout, args.max_turns).await {
            log::error!("Server encountered an error: {e:?}");
        }
        log::trace!("Sending shutdown signal");
        let _ = shutdown_tx.send(());
    });

    // Bind TCP listener to the socket address
    let listener = TcpListener::bind(&args.socket)
        .await
        .with_context(|| "Failed to bind listener to socket")?;
    log::info!("Lisening at {socket:?}", socket = args.socket);

    tokio::select! {
        // Connections loop
        _ = async {
            let mut client_id_counter = 0;
            loop {
                match listener.accept().await {
                    Err(e) => {
                        log::error!("Failed to accept connection: {e:?}");
                        continue;
                    }
                    Ok((stream, _addr)) => {
                        let client_id = client_id_counter;
                        client_id_counter += 1;

                         // Handshake
                        let mut stream = stream;
                        // 1. Wait for Hello or Reconnect
                        let mut buffer = [0u8; REMOTE_MESSAGE_LENGTH];
                        let mut length_buffer = [0u8; 4];

                        if let Err(e) = stream.read_exact(&mut length_buffer).await {
                             log::error!("Failed to read handshake length: {e}");
                             continue;
                        }
                        let length = u32::from_be_bytes(length_buffer) as usize;
                        if length > REMOTE_MESSAGE_LENGTH {
                            log::error!("Handshake too large");
                            continue;
                        }
                        if let Err(e) = stream.read_exact(&mut buffer[..length]).await {
                             log::error!("Failed to read handshake: {e}");
                             continue;
                        }

                        let message = match RemoteInMessage::from_bytes(&buffer[..length]) {
                            Ok(m) => m,
                            Err(e) => {
                                log::error!("Invalid handshake message: {e}");
                                continue;
                            }
                        };

                        match message {
                            RemoteInMessage::Hello => {
                                // New connection
                                // Assign player to new client
                                let player = match assign_player(client_id) {
                                    Err(e) => {
                                        log::error!("Failed to assign player to new client: {e}");
                                        // Send reject
                                        let reject = RemoteOutMessage::Reject(e.to_string());
                                         let mut buf = Vec::new();
                                        if let Ok(_) = ciborium::into_writer(&reject, &mut buf) {
                                            let len = buf.len() as u32;
                                            let _ = stream.write_all(&len.to_be_bytes()).await;
                                            let _ = stream.write_all(&buf).await;
                                        }
                                        continue;
                                    },
                                    Ok(player) => {
                                        player
                                    },
                                };

                                let session_id = Uuid::new_v4();
                                log::info!("New client assign: {player} (Session: {session_id})");

                                // Send Welcome
                                let welcome = RemoteOutMessage::Welcome { session_id, player };
                                let mut buf = Vec::new();
                                if let Ok(_) = ciborium::into_writer(&welcome, &mut buf) {
                                    let len = buf.len() as u32;
                                    if let Err(e) = stream.write_all(&len.to_be_bytes()).await {
                                         log::error!("Failed to write welcome length: {e}");
                                         continue;
                                    }
                                    if let Err(e) = stream.write_all(&buf).await {
                                         log::error!("Failed to write welcome message: {e}");
                                         continue;
                                    }
                                } else {
                                     log::error!("Failed to serialize welcome");
                                     continue;
                                }

                                // Server thread -> Client thread
                                let (server_tx, server_rx) = mpsc::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);

                                // Spawn client thread
                                match Client::new(player, stream, server_rx, server_broadcast_rx.resubscribe(), client_msg_tx.clone()) {
                                    Err(e) => {
                                        log::error!("Failed to create client: {e:?}");
                                        continue;
                                    }
                                    Ok(client) =>{

                                        // Spawn client thread
                                        let task = tokio::spawn(async move {
                                            if let Err(e) = client.try_run().await {
                                                log::error!("Client for player {player} encountered an error: {e:?}");
                                            }
                                        });

                                        // Send client communication channel to server
                                        if let Err(e) = main_tx.send(MainThreadMessage::ClientConnected(player, session_id, server_tx)).await {
                                            log::error!("Failed to send client communitcation channel to server: {e:?}");
                                            task.abort();
                                            continue;
                                        }

                                    }
                                };
                            }
                            RemoteInMessage::Reconnect(uuid) => {
                                 // TODO: Check if session is valid? Wait, only server knows.
                                 // We don't have access to server state here easily to validate UUID *before* connecting.
                                 // Logic: We assume it's valid, try to connect, if server accepts, good.
                                 // But we need the Player associated with UUID to spawn Client.
                                 // The main thread does NOT know the mapping. Only Server thread does.
                                 // DESIGN FLAW FIX: We need to ask Server to adopt this stream.
                                 // BUT Client struct owns the stream.

                                 // WORKAROUND for this iteration:
                                 // We cannot easily validate reconnect here without shared state.
                                 // But we can spawn a "Provisional Client" or ask Server.
                                 // Simpler: Just allow reconnect if we can map it? No, we don't have the map.

                                 // We need to change MainThreadMessage to pass the stream to the server?
                                 // Or have the Server manage connections?
                                 // Current arc: Main -> ClientThread. Server -> ClientThread.

                                 // FIX: The server must handle the reconnection logic deeper or we need shared state.
                                 // Let's go with shared state for simplicity in this refactor.
                                 // Main thread maintains a minimal Session Map? No, that duplicates state.

                                 // Correct approach for this architecture:
                                 // Connection comes in. We handshake.
                                 // If Reconnect:
                                 // We can't know which Player it is.
                                 // We can't spawn Client(Player).

                                 // Let's modify the architecture slightly:
                                 // Main thread keeps the Session Map?
                                 // OR
                                 // We ask the server "Who is session X?" via a channel?
                                 // That requires a return channel (oneshot).

                                 log::info!("Reconnection attempt with session {uuid}");

                                 // Ask server for player info
                                 let (resp_tx, resp_rx) = oneshot::channel();
                                 if let Err(_) = main_tx.send(MainThreadMessage::ClientReconnectedHandle(uuid, resp_tx)).await {
                                     log::error!("Server closed main channel");
                                     continue;
                                 }

                                 let player = match resp_rx.await {
                                     Ok(Some(p)) => p,
                                     Ok(None) => {
                                         log::warn!("Unknown session {uuid}");
                                         // Send reject
                                        let reject = RemoteOutMessage::Reject("Unknown Session".to_string());
                                         let mut buf = Vec::new();
                                        if let Ok(_) = ciborium::into_writer(&reject, &mut buf) {
                                            let len = buf.len() as u32;
                                            let _ = stream.write_all(&len.to_be_bytes()).await;
                                            let _ = stream.write_all(&buf).await;
                                        }
                                         continue;
                                     },
                                     Err(_) => {
                                         log::error!("Server did not reply to session query");
                                         continue;
                                     }
                                 };

                                 // Send Welcome (Ack)
                                let welcome = RemoteOutMessage::Welcome { session_id: uuid, player };
                                let mut buf = Vec::new();
                                if let Ok(_) = ciborium::into_writer(&welcome, &mut buf) {
                                    let len = buf.len() as u32;
                                    let _ = stream.write_all(&len.to_be_bytes()).await;
                                    let _ = stream.write_all(&buf).await;
                                }

                                // Spawn client
                                let (server_tx, server_rx) = mpsc::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);
                                 match Client::new(player, stream, server_rx, server_broadcast_rx.resubscribe(), client_msg_tx.clone()) {
                                    Err(e) => log::error!("Failed to create client: {e:?}"),
                                    Ok(client) => {
                                        tokio::spawn(async move {
                                            if let Err(e) = client.try_run().await {
                                                 log::error!("Client error: {e:?}");
                                            }
                                        });
                                        let _ = main_tx.send(MainThreadMessage::ClientReconnected(player, server_tx)).await;
                                    }
                                 }
                            }
                            _ => {
                                log::error!("Unexpected message during handshake");
                            }
                        }

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
