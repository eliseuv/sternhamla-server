use std::{
    collections::{HashMap, hash_map},
    fmt::Display,
    io::{self, Cursor, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{broadcast, mpsc, oneshot},
};

use sternhalma_server::sterhalma::{
    Game, GameStatus,
    board::{HexIdx, movement::Movement, player::Player},
};

/// Capacity communication channels between local threads
const LOCAL_CHANNEL_CAPACITY: usize = 32;
/// Maximum length of a remote message in bytes
const REMOTE_MESSAGE_LENGTH: usize = 4 * 1024;

/// Command line arguments
#[derive(Debug, Parser)]
#[command(name = "sternhalma-server", version, about)]
struct Args {
    /// Host IP address
    #[arg(long)]
    socket: PathBuf,
}

/// Compact representation of movement indices for serialization
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
struct MovementList(Vec<[HexIdx; 2]>);

impl MovementList {
    /// Create a new compact movement list from a vector of movements
    fn new(movements: Vec<Movement>) -> Self {
        MovementList(
            movements
                .into_iter()
                .map(|indices| [indices.from, indices.to])
                .collect(),
        )
    }
}

/// Local Client Thread -> Remote Client
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteOutMessage {
    /// Inform remote client about their assigned player
    Assign {
        player: Player,
    },
    /// Disconnection signal
    Disconnect,
    /// Inform remote client that it is their turn
    Turn {
        /// Provide list of available movements
        movements: MovementList,
    },
    /// Inform remote client about a player's movement
    Movement {
        player: Player,
        movement: [HexIdx; 2],
    },
    GameFinished {
        winner: Player,
        turns: usize,
    },
}

/// Remote Client -> Local Client Thread
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteInMessage {
    /// Movement made by player
    Choice { movement: [HexIdx; 2] },
}

#[derive(Debug)]
enum ServerMessage {
    /// Players turn
    Turn {
        /// List of available movements
        movements: MovementList,
    },
}

/// Server Thread -> All Local Client Threads
#[derive(Debug, Clone)]
enum ServerBroadcast {
    /// Disconnection signal,
    Disconnect,
    /// Player made a move
    Movement { player: Player, movement: Movement },
    /// Result of the game
    GameFinished { winner: Player, turns: usize },
}

/// Local Client Thread -> Server Thread
#[derive(Debug)]
enum ClientRequest {
    /// Disconnection request
    Disconnect,
    /// Player made a movement
    Choice([HexIdx; 2]),
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
    // Channel for broadcasting messages to all local client threads
    broadcast_tx: broadcast::Sender<ServerBroadcast>,
    // Channel for receiving messages from local client threads
    clients_rx: mpsc::Receiver<ClientMessage>,
    // Game state
    game: Game,
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
            broadcast_tx,
            clients_rx,
            game: Game::new(),
        })
    }

    /// Wait for all players to connect
    async fn wait_players_connect(&mut self) -> Result<()> {
        let n_total = Player::count();
        log::info!("Waiting for {n_total} players to connect...");
        while self.clients_tx.len() != Player::count() {
            // Wait for message
            match self
                .main_rx
                .recv()
                .await
                .ok_or(anyhow!("Channel from main thread to server close"))?
            {
                MainThreadMessage::ClientConnected(player, client_tx) => {
                    if let hash_map::Entry::Vacant(entry) = self.clients_tx.entry(player) {
                        entry.insert(client_tx);
                        log::info!("Player {player} connected");
                    } else {
                        log::error!("Player {player} is already connected");
                    }
                }
            }
            log::info!(
                "Players connected: {n_players}/{n_total}",
                n_players = self.clients_tx.len()
            );
        }
        log::info!(
            "All {n_players} player connected",
            n_players = self.clients_tx.len()
        );
        Ok(())
    }

    async fn disconnect_players(&mut self) -> Result<()> {
        log::info!("Disconnecting all players");
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

    /// Main server thread loop
    async fn run(&mut self) -> Result<()> {
        log::trace!("Server thread started");

        // Wait for players to connect
        self.wait_players_connect()
            .await
            .with_context(|| "Failed to wait for players to connect")?;

        // Main game loop
        while let GameStatus::Playing {
            player: current_player,
            ..
        } = self.game.status()
        {
            println!("{game}", game = self.game);
            log::debug!("Player {current_player} turn");
            // Calculate available moves
            let movements = MovementList(
                self.game
                    .iter_available_moves()
                    .map(|movement| {
                        let Movement { from, to } = movement.get_indices();
                        [from, to]
                    })
                    .unique()
                    .collect(),
            );
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

            loop {
                tokio::select! {

                    client_msg = self.clients_rx.recv() => {
                        match client_msg {
                            None => {
                                log::error!("Clients message channel closed");
                                break;
                            }
                            Some(message) => {
                                // Identify client
                                let player = message.player;

                                // Handle request
                                match message.request {
                                    ClientRequest::Disconnect => {
                                        log::error!("Player {player} disconnected mid-game");
                                        // Finish the game
                                        bail!("Player {player} disconnected mid-game");
                                    }

                                    ClientRequest::Choice(movement) => {
                                        // Check if player is the current player
                                        if player != current_player {
                                            log::error!("Player {player} attempted to move out of turn");
                                            continue;
                                        }

                                        // Select chosen movement from list
                                        if !movements.0.contains(&movement){
                                            log::error!("Player {player} chose invalid movement {movement:?}");
                                            continue;
                                        }

                                        let movement = Movement {
                                            from: movement[0],
                                            to: movement[1],
                                        };
                                        log::debug!("Player {player} chose movement {movement:?}");

                                        // Apply chosen movement
                                        // Since it was previously calculated, it should always be valid
                                        let game_status = self.game.unsafe_apply_movement(&movement);

                                        // Broadcast movement to all players
                                        self.broadcast_tx.send(ServerBroadcast::Movement {
                                            player,
                                            movement,
                                        }).with_context(|| "Failed to broadcast movement")?;

                                        // Check if the game is finished
                                        if let GameStatus::Finished{ winner, turns } = game_status {
                                            log::info!("Game finished, player {winner} won");
                                            // Broadcast game finished message
                                            self.broadcast_tx.send(ServerBroadcast::GameFinished {
                                                winner, turns
                                            }).with_context(|| "Failed to broadcast game finished message")?;
                                        }

                                        // Next turn
                                        break;

                                    }
                                }
                            },
                        }

                    }

                }
            }
        }

        Ok(())
    }

    /// Server thread run wrapper
    async fn try_run(mut self) -> Result<()> {
        // Attempt to run server
        let result = self.run().await;

        // Disconnect all players
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
    stream: UnixStream,
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
        stream: UnixStream,
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
            RemoteInMessage::Choice { movement } => self
                .send_request(ClientRequest::Choice(movement))
                .await
                .with_context(|| "Unable to forward message to server"),
        }
    }

    async fn handle_server_broadcast(&mut self, message: ServerBroadcast) -> Result<()> {
        log::debug!("[Player {}] Received broadcast: {message:?}", self.player);

        match message {
            ServerBroadcast::Disconnect => {
                self.send_remote_message(RemoteOutMessage::Disconnect)
                    .await?;
            }
            ServerBroadcast::Movement { player, movement } => {
                self.send_remote_message(RemoteOutMessage::Movement {
                    player,
                    movement: [movement.from, movement.to],
                })
                .await?;
            }
            ServerBroadcast::GameFinished { winner, turns } => {
                self.send_remote_message(RemoteOutMessage::GameFinished { winner, turns })
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
    ClientConnected(Player, mpsc::Sender<ServerMessage>),
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    // Parse command line arguments
    let args = Args::parse();
    log::debug!("Command line arguments: {args:?}");

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
        if let Err(e) = server.try_run().await {
            log::error!("Server encountered an error: {e:?}");
        }
        log::trace!("Sending shutdown signal");
        let _ = shutdown_tx.send(());
    });

    // Bind TCP listener to the socket address
    let listener =
        UnixListener::bind(&args.socket).with_context(|| "Failed to bind listener to socket")?;
    log::info!("Lisening at {socket:?}", socket = args.socket);

    tokio::select! {

        // Connections loop
        connection = async {
            for client_id in 0.. {
                match listener.accept().await {
                    Err(e) => {
                        log::error!("Failed to accept connection: {e:?}");
                        continue;
                    }
                    Ok((stream, addr)) => {
                        log::info!("Accepted connection from {addr:?}");

                        // Assign player to new client
                        let player = match assign_player(client_id) {
                            Err(e) => {
                                log::error!("Failed to assign player to new client: {e}");
                                continue;
                            },
                            Ok(player) => {
                                log::info!("New client assign: {player}");
                                player
                            },
                        };

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
                                if let Err(e) = main_tx.send(MainThreadMessage::ClientConnected(player, server_tx)).await {
                                    log::error!("Failed to send client communitcation channel to server: {e:?}");
                                    task.abort();
                                    continue;
                                }

                            }
                        };
                    }
                }
            }
        } => {
            connection
        },

        // Shutdown signal
        _ = shutdown_rx => {
            log::trace!("Shutdown singnal received");
        }

    }

    Ok(())
}
