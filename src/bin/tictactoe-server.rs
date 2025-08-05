use std::{
    collections::HashSet,
    io::{self, Cursor, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic::{self, AtomicUsize},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, oneshot},
};

use sternhalma_server::tictactoe::{Game, GameResult, GameStatus, Player, Position};

const LOCAL_CHANNEL_CAPACITY: usize = 64;
const REMOTE_MESSAGE_LENGTH: usize = 1024;

fn assign_player() -> Result<Player> {
    static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);

    match ATOMIC_ID.fetch_add(1, atomic::Ordering::Relaxed) {
        0 => Ok(Player::Cross),
        1 => Ok(Player::Nought),
        _ => bail!("All players are already connected"),
    }
}

/// Command line arguments
#[derive(Debug, Parser)]
#[command(name = "fc-server", version, about)]
struct Args {
    /// Host IP address
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))]
    host: IpAddr,

    /// Port to bind the server to
    #[arg(short, long)]
    port: u16,
}

/// Remote Client -> Local Client Thread
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteInMessage {
    /// Movement made by player
    Movement { position: Position },
}

/// Local Client Thread -> Remote Client
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteOutMessage {
    /// Inform remote client about their assigned player
    Assign {
        player: Player,
    },
    /// Inform remote client that it is their turn
    Turn {
        available_moves: Vec<Position>,
    },
    /// Inform remote client about a player's movement
    Movement {
        player: Player,
        position: Position,
    },
    GameFinished {
        result: GameResult,
    },
}

/// Server Thread -> Local Client Thread
#[derive(Debug, Clone)]
enum ServerMessage {
    /// Players turn
    Turn {
        player: Player,
        available_moves: Vec<Position>,
    },
    /// Player made a move
    Movement { player: Player, position: Position },
    /// Result of the game
    GameFinished { result: GameResult },
}

/// Local Client Thread -> Server Thread
#[derive(Debug)]
enum ClientRequest {
    /// Connection request
    Ready,
    /// Disconnection request
    Disconnect,
    /// Player made a movement
    Movement { position: Position },
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
        // ciborium::from_reader(bytes).with_context(||"Failed to deserialize remote message")
        serde_json::from_slice(bytes).with_context(|| "Failed to deserialize remote message")
    }
}

impl RemoteOutMessage {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // ciborium::into_writer(self, writer).with_context(||"Failed to serialize remote message")
        serde_json::to_writer(writer, self).with_context(|| "Failed to serialize remote message")
    }
}

#[derive(Debug)]
struct Server {
    // Channel for broadcasting messages to all local client threads
    broadcast_tx: broadcast::Sender<ServerMessage>,
    // Channel for receiving messages from local client threads
    client_msg_rx: mpsc::Receiver<ClientMessage>,
    // Game state
    game: Game,
}

impl Server {
    /// Creates a new server instance
    fn new(
        client_msg_rx: mpsc::Receiver<ClientMessage>,
        broadcast_tx: broadcast::Sender<ServerMessage>,
    ) -> Result<Self> {
        Ok(Self {
            broadcast_tx,
            client_msg_rx,
            game: Game::new(),
        })
    }

    /// Broadcast a message to all clients
    async fn message_clients(&self, message: ServerMessage) -> Result<usize> {
        log::debug!("Broadcasting message to clients: {message:?}");

        self.broadcast_tx
            .send(message)
            .with_context(|| "Failed to broadcast message to clients")
    }

    async fn wait_for_clients(&mut self, n_clients: usize) -> Result<()> {
        log::info!("Waiting for {n_clients} clients to connect...");
        // NOTE: One receiver instance remains in the main thread. Therefore the number of clients
        // is equal to number of receivers minus one.
        while self.broadcast_tx.receiver_count() - 1 < n_clients {
            // Wait for message
            let message = self
                .client_msg_rx
                .recv()
                .await
                .ok_or(anyhow!("Local channel closed"))?;
            // Identify client
            let player = message.player;
            log::debug!("Message received from player {player}");
            // Parse request
            match message.request {
                // Connection request
                ClientRequest::Ready => {
                    log::info!("Player {player} is ready");
                }

                // Invalid request
                request => {
                    log::warn!("Invalid request: {request:?}");
                }
            }
            log::info!(
                "{n_connected}/{n_clients} connected",
                n_connected = self.broadcast_tx.receiver_count() - 1,
            );
        }
        log::info!("All {n_clients} clients connected",);
        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        log::trace!("Server thread started");

        // Wait for players to get ready
        let all_players = HashSet::from(Player::variants());
        let mut ready_players = HashSet::<Player>::new();
        log::debug!("Waiting for players to get ready");
        while ready_players != all_players {
            // Wait for message
            let message = self
                .client_msg_rx
                .recv()
                .await
                .ok_or(anyhow!("Local channel closed"))?;
            // Identify client
            let player = message.player;
            log::debug!("Message received from player {player}");
            // Parse request
            match message.request {
                // Connection request
                ClientRequest::Ready => {
                    log::info!("Player {player} is ready");
                    ready_players.insert(player);
                }

                // Invalid request
                request => {
                    log::warn!("Invalid request: {request:?}");
                }
            }
            log::info!(
                "{n_ready}/{n_total} connected",
                n_ready = ready_players.len(),
                n_total = all_players.len()
            );
        }

        // Main game loop
        while let GameStatus::Playing(current_player) = self.game.status() {
            log::debug!("Player {current_player} turn");
            self.message_clients(ServerMessage::Turn {
                player: current_player,
                available_moves: self.game.available_moves(),
            })
            .await
            .with_context(|| "Falied to inform client of their turn")?;

            // Loop to receive messages from clients
            loop {
                // Receive client message
                let message = self
                    .client_msg_rx
                    .recv()
                    .await
                    .ok_or(anyhow!("Channel from clients to server closed"))?;

                // Identify client
                let player = message.player;

                // Handle request
                let request = message.request;
                match request {
                    ClientRequest::Disconnect => {
                        bail!("Player {player} disconnected mid-game")
                    }

                    ClientRequest::Movement { position } => {
                        if player != current_player {
                            log::error!("Player {player} attempted to move out of turn");
                            continue;
                        }

                        match self.game.make_move(position) {
                            Err(e) => {
                                bail!("Invalid movement: {e:?}");
                            }
                            Ok(status) => {
                                log::debug!("Player {player} made move {position:?}");
                                self.message_clients(ServerMessage::Movement { player, position })
                                    .await
                                    .with_context(|| "Unable to broadcast player movement")?;
                                match status {
                                    GameStatus::Playing(_) => {
                                        break;
                                    }
                                    GameStatus::Finished(game_result) => {
                                        log::info!("Game finished with result {game_result:?}");
                                        self.message_clients(ServerMessage::GameFinished {
                                            result: game_result,
                                        })
                                        .await?;
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    _ => {
                        log::error!("Invalid request: {request:?}");
                        continue;
                    }
                }
            }
        }

        log::debug!("Waiting for players to disconnect");
        while !ready_players.is_empty() {
            // Wait for message
            let message = self
                .client_msg_rx
                .recv()
                .await
                .ok_or(anyhow!("Local channel closed"))?;
            // Identify client
            let player = message.player;
            log::debug!("Message received from player {player}");
            // Parse request
            match message.request {
                // Connection request
                ClientRequest::Disconnect => {
                    log::info!("Player {player} disconnected");
                    ready_players.remove(&player);
                }

                // Invalid request
                request => {
                    log::warn!("Invalid request: {request:?}");
                }
            }
            log::info!(
                "{n_ready}/{n_total} connected",
                n_ready = ready_players.len(),
                n_total = all_players.len()
            );
        }

        log::debug!("Shutting down server thread");
        Ok(())
    }
}

#[derive(Debug)]
struct Client {
    /// Unique identifier for the client
    player: Player,
    /// TCP stream for communication with the remote client
    stream: TcpStream,
    /// Receiver for messages broadcast by the server
    broadcast_rx: broadcast::Receiver<ServerMessage>,
    /// Sender for messages to the server thread
    server_tx: mpsc::Sender<ClientMessage>,
    /// Buffer for incoming remote messages
    buffer_in: [u8; REMOTE_MESSAGE_LENGTH],
    /// Buffer for outgoing remote messages
    buffer_out: [u8; REMOTE_MESSAGE_LENGTH],
}

impl Client {
    fn new(
        stream: TcpStream,
        client_msg_tx: mpsc::Sender<ClientMessage>,
        broadcast_rx: broadcast::Receiver<ServerMessage>,
    ) -> Result<Self> {
        let player = assign_player().with_context(|| "Falied to assign player to client")?;
        log::debug!("Creating client {player}");

        Ok(Self {
            player,
            stream,
            broadcast_rx,
            server_tx: client_msg_tx,
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

        // NOTE: Flushing the stream may be necessary at this point.
        // Pro: The data is promptly sent to the underlying OS buffer
        // Con: Additional syscall and latency

        log::debug!(
            "[Player {}] Successfully sent payload of {bytes_written} to remote client",
            self.player,
        );
        Ok(())
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
            .with_context(
                || "Failed to receive message of length {message_length} from remote client",
            )?;
        log::debug!(
            "[Player {}] Remote message receive successfully: {}",
            self.player,
            std::str::from_utf8(&self.buffer_in)?
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
            RemoteInMessage::Movement { position } => self
                .send_request(ClientRequest::Movement { position })
                .await
                .with_context(|| "Unable to forward message to server"),
        }
    }

    async fn handle_server_message(&mut self, message: ServerMessage) -> Result<()> {
        log::debug!("[Player {}] Received message: {message:?}", self.player);

        match message {
            ServerMessage::Turn {
                player,
                available_moves,
            } => {
                if player == self.player {
                    self.send_remote_message(RemoteOutMessage::Turn { available_moves })
                        .await?;
                }
            }
            ServerMessage::Movement { player, position } => {
                self.send_remote_message(RemoteOutMessage::Movement { player, position })
                    .await?;
            }

            ServerMessage::GameFinished { result } => {
                self.send_remote_message(RemoteOutMessage::GameFinished { result })
                    .await?;
            }
        };

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
        self.server_tx
            .send(message)
            .await
            .with_context(|| "Failed to send message to server")
    }

    async fn run(mut self) -> Result<()> {
        log::info!("[Player {}] Task spawned", self.player);

        // Send player assignment message to remote client
        self.send_remote_message(RemoteOutMessage::Assign {
            player: self.player,
        })
        .await
        .with_context(|| "Falied to send player assignment message to remote client")?;

        // Tell server the client is ready
        self.send_request(ClientRequest::Ready)
            .await
            .with_context(|| "Failed to send connection request to server")?;

        let mut message_length_buffer = [0; 4];
        loop {
            tokio::select! {

                // Incoming messages from the server
                result = self.broadcast_rx.recv() => {
                    match result {
                        Err(broadcast::error::RecvError::Closed) => {
                            bail!("Server channel closed");
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::error!("[Player {}] Server channel lagged by {n} messages", self.player);
                        }
                        Ok(message) => {
                            log::debug!("[Player {}] Received server message: {message:?}",self.player);
                            self.handle_server_message(message).await.with_context(|| "Unable to handle server message")?;
                        }
                    }
                }

                // Incoming messages from remote client
                result = self.stream.read_exact(&mut message_length_buffer) => {
                    log::debug!("[Player {}] New message from remote client",self.player);
                    match result {
                        Ok(0) => {
                            log::info!("[Player {}] Remote client disconnected",self.player);
                            self.send_request(ClientRequest::Disconnect).await?;
                            return Ok(());
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
                            self.send_request(ClientRequest::Disconnect).await?;
                                    return Ok(());
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
    }
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
    let (broadcast_tx, broadcast_rx) = broadcast::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);

    // Channel for the server thread to send shutdown signal to main thread
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Spawn the server task
    let server =
        Server::new(client_msg_rx, broadcast_tx).with_context(|| "Failed to create server")?;
    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            log::error!("Server encountered an error: {e:?}");
        }
        log::trace!("Sending shutdown signal");
        let _ = shutdown_tx.send(());
    });

    // Bind TCP listener to the socket address
    let addr = SocketAddr::new(args.host, args.port);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| "Failed to bind TCP listener")?;
    log::info!("Lisening at {addr}");

    tokio::select! {

        // Connections loop
        connection = async {
            loop{
                match listener.accept().await {
                    Err(e) => {
                        log::error!("Failed to accept connection: {e:?}");
                        continue;
                    }
                    Ok((stream, addr)) => {
                        log::info!("Accepted connection from {addr}");

                        // Spawn client thread
                        match Client::new(stream, client_msg_tx.clone(), broadcast_rx.resubscribe()) {
                            Err(e) => {
                                log::error!("Failed to create client: {e:?}");
                                continue;
                            }
                            Ok(client) => tokio::spawn(async move {
                                if let Err(e) = client.run().await {
                                    log::error!("Client encountered an error: {e:?}");
                                }
                            }),
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
