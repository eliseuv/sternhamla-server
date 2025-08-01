use std::{
    collections::HashMap,
    io::{self, Cursor, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sternhalma_server::tictactoe;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc},
    time::sleep,
};

const LOCALHOST_ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
const LOCAL_CHANNEL_CAPACITY: usize = 64;
const REMOTE_MESSAGE_LENGTH: usize = 1024;

static ATOMIC_CLIENT_ID: atomic::AtomicUsize = atomic::AtomicUsize::new(0);

/// Command line arguments
#[derive(Debug, Parser)]
#[command(name = "fc-server", version, about)]
struct Args {
    /// IP address to bind the server to
    #[arg(short, long, default_value_t = LOCALHOST_ADDR)]
    addr: IpAddr,

    /// Port to bind the server to
    #[arg(short, long)]
    port: u16,
}

/// Remote Client -> Local Client Thread
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteInMessage {
    Test {
        num: i32,
    },
    /// Receive a player movement
    Movement {
        position: [usize; 2],
    },
}

/// Local Client Thread -> Remote Client
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteOutMessage {
    Test {
        num: i32,
    },
    /// Inform client about the assigned player
    Assign {
        player: tictactoe::Player,
    },
    /// Inform client that it is their turn
    Turn {
        available_moves: Vec<[usize; 2]>,
    },
    /// Inform client about a player movement
    Movement {
        player: tictactoe::Player,
        position: [usize; 2],
    },
    /// Inform client that the game has finished
    GameOver {
        result: tictactoe::GameResult,
    },
    /// Inform client about a game error
    GameError {
        error: tictactoe::GameError,
    },
}

/// Server Thread -> Local Client Thread
#[derive(Debug, Clone)]
enum ClientMessage {
    /// Forward message to remote client
    Forward(RemoteOutMessage),
}

/// Local Client Thread -> Server Thread
#[derive(Debug)]
enum ServerMessage {
    /// Connection request
    Connection {
        id: usize,
        tx: mpsc::Sender<ClientMessage>,
    },
    /// Disconnection request
    Disconnection { id: usize },
    /// Forward message to server
    Forward {
        client_id: usize,
        message: RemoteInMessage,
    },
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
    // Channel for receiving messages from local client threads
    server_rx: mpsc::Receiver<ServerMessage>,
    // Channel for broadcasting messages to all local client threads
    broadcast_tx: broadcast::Sender<ClientMessage>,
    // Channel for communication with specific local client threads
    clients_tx: HashMap<tictactoe::Player, mpsc::Sender<ClientMessage>>,
    // Game state
    game: tictactoe::Game,
}

impl Server {
    /// Creates a new server instance
    async fn new(
        server_rx: mpsc::Receiver<ServerMessage>,
        broadcast_tx: broadcast::Sender<ClientMessage>,
    ) -> Result<Self> {
        Ok(Self {
            server_rx,
            broadcast_tx,
            clients_tx: HashMap::new(),
            game: tictactoe::Game::new(),
        })
    }

    async fn message_client(
        &self,
        player: tictactoe::Player,
        message: ClientMessage,
    ) -> Result<()> {
        log::debug!("Sending message to player {player}: {message:?}");

        // Find client
        self.clients_tx
            .get(&player)
            .ok_or_else(|| anyhow!("Player {player} not found"))?
            // Send local message
            .send(message)
            .await
            .with_context(|| "Failed to send message to client {client_it}")
    }

    fn handle_message(&mut self, message: ServerMessage) -> Result<()> {
        // Handle incoming client messages here
        log::debug!("Handling local message: {message:?}");

        match message {
            ServerMessage::Connection { id, tx } => {
                if self.clients_tx.get(, tx).is_some() {
                    log::warn!("Client {id} already connected, replacing existing connection");
                }
                log::info!("Client {id} connected");
            }

            ServerMessage::Disconnection { id } => {
                log::debug!("Received disconnection request from client {id}");
                match self.clients_tx.remove(&id) {
                    None => log::error!("Client {id} is already disconnected"),
                    Some(_) => log::info!("Client {id} successfully disconnected"),
                }
                if self.clients_tx.is_empty() {
                    log::warn!("Last client disconnected");
                }
            }

            ServerMessage::Forward { client_id, message } => {
                log::debug!("Remote message from client {client_id}: {message:?}");
                match message {
                    RemoteInMessage::Test { num } => {
                        log::debug!("Client {client_id} send test message with {num}");
                    }
                    RemoteInMessage::Movement { position } => todo!(),
                }
            }
        }

        Ok(())
    }

    async fn connect_client(&mut self, client_tx: mpsc::Sender<ClientMessage>) -> Result<()>{
                    // Assign player to client
                    let player = match self.clients_tx.keys().collect::<Vec<_>>().as_slice() {
                        [] => tictactoe::Player::Cross,
                        [other] => other.opposite(),
                        _ => bail!("Server has too many players"),
                    };
                    self.clients_tx.insert(player, client_tx);
                    self.message_client(
                        player,
                        ClientMessage::Forward(RemoteOutMessage::Assign { player }),
                    ).await.with_context(||"Falied to send player assignment message to client")
    }

    async fn wait_for_clients(&mut self) -> Result<()> {
        log::info!("Waiting for clients...");
        while self.clients_tx.len() < tictactoe::NUM_PLAYERS {
            let message = self
                .server_rx
                .recv()
                .await
                .ok_or(anyhow!("Local channel closed"))?;
            match message {
                ServerMessage::Connection { id, tx } => {
                    log::debug!("Received connection request from client {id}");
                    self.connect_client(tx).await.with_context(
                        ||"Failed to connect client"
                    )?;
                }
                message => {
                    log::warn!("Invalid message received: {message:?}");
                    continue;
                }
            }
            log::info!(
                "{n_clients}/{n_total} connected",
                n_clients = self.clients_tx.len(),
                n_total = tictactoe::NUM_PLAYERS
            );
            sleep(Duration::from_secs(1)).await;
        }
        log::info!(
            "All {n_total} clients connected",
            n_total = tictactoe::NUM_PLAYERS
        );
        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        log::trace!("Server thread started");

        // Wait for all players to connect
        self.wait_for_clients()
            .await
            .with_context(|| "Failed to wait for clients to connect")?;

        // Assign players
        for (id, player) in tictactoe::PLAYERS_LIST.into_iter().enumerate() {
            self.message_client(
                id,
                ClientMessage::Forward(RemoteOutMessage::Assign {
                    player: tictactoe::Player::Cross,
                }),
            )
            .await?
        }

        loop {
            tokio::select! {
                // Handle incoming connection requests from local client threads
                Some(message) = self.server_rx.recv() => {
                    if let Err(e) = self.handle_message(message) {
                        log::error!("Failed to handle server message: {e:?}");
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct Client {
    /// Unique identifier for the client
    id: usize,
    /// TCP stream for communication with the remote client
    stream: TcpStream,
    /// Receiver for messages broadcasted by the server
    broadcast_rx: broadcast::Receiver<ClientMessage>,
    /// Receiver for messages from the server thread
    client_rx: mpsc::Receiver<ClientMessage>,
    /// Sender for messages to the server thread
    server_tx: mpsc::Sender<ServerMessage>,
    /// Buffer for incoming remote messages
    buffer_in: [u8; REMOTE_MESSAGE_LENGTH],
    /// Buffer for outgoing remote messages
    buffer_out: [u8; REMOTE_MESSAGE_LENGTH],
}

impl Client {
    async fn new(
        stream: TcpStream,
        broadcast_rx: broadcast::Receiver<ClientMessage>,
        server_tx: mpsc::Sender<ServerMessage>,
    ) -> Result<Self> {
        let id = ATOMIC_CLIENT_ID.fetch_add(1, atomic::Ordering::SeqCst);
        log::debug!("Creating client {id}");

        // Channel: Server Thread -> Local Client Thread
        let (client_tx, client_rx) = mpsc::channel::<ClientMessage>(LOCAL_CHANNEL_CAPACITY);

        // Send connection request to the server
        server_tx
            .send(ServerMessage::Connection {
                id: id,
                tx: client_tx,
            })
            .await
            .with_context(|| "Failed to send connection request to server")?;

        Ok(Self {
            id,
            stream,
            broadcast_rx,
            client_rx,
            server_tx,
            buffer_in: [0; REMOTE_MESSAGE_LENGTH],
            buffer_out: [0; REMOTE_MESSAGE_LENGTH],
        })
    }

    async fn send_remote_message(&mut self, message: RemoteOutMessage) -> Result<()> {
        log::debug!("[Client {}] Sending remote message: {message:?}", self.id);

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
            "[Client {}] Successfully sent payload of {bytes_written} to remote client",
            self.id,
        );
        Ok(())
    }

    async fn receive_remote_message(&mut self, message_length: usize) -> Result<RemoteInMessage> {
        log::debug!(
            "[Client {}] Message length: {message_length} bytes",
            self.id
        );
        if message_length == 0 || message_length > REMOTE_MESSAGE_LENGTH {
            // TODO: Disconnect client after a number of errors
            bail!(
                "[Client {}] Invalid message length: {message_length} bytes",
                self.id
            );
        }

        // Receive actual message
        self.stream
            .read_exact(&mut self.buffer_in[..message_length])
            .await
            .with_context(
                || "Failed to receive message of length {message_length} from remote client",
            )?;
        log::debug!("[Client {}] Remote message receive successfully", self.id);

        // Decode message
        RemoteInMessage::from_bytes(&self.buffer_in[..message_length])
            .with_context(|| "Failed to decode remote message")
    }

    fn handle_remote_message(&self, message: RemoteInMessage) -> Result<()> {
        log::debug!("[Client {}] Handling remote message: {message:?}", self.id);

        match message {
            RemoteInMessage::Test { num } => {
                log::debug!(
                    "[Client {}] Remote client sent a test message with num: {num}",
                    self.id
                );
                Ok(())
            }
            RemoteInMessage::Movement { position } => todo!(),
        }
    }

    async fn handle_local_message(&mut self, message: ClientMessage) -> Result<()> {
        log::debug!("[Client {}] Received message: {message:?}", self.id);

        match message {
            ClientMessage::Forward(message) => self
                .send_remote_message(message)
                .await
                .with_context(|| "Failed to forward message to remote client")?,
        }

        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        let id = self.id;
        log::info!("[Client {id}] Task spawned");

        let mut message_length_buffer = [0; 4];
        loop {
            tokio::select! {
                // Handle incoming messages from the server thread
                Some(message) = self.client_rx.recv() => {
                    log::debug!("[Client {id}] Received local message: {message:?}");
                    self.handle_local_message(message).await.with_context(|| "Unable to handle message from server")?;
                }

                // Handle broadcast messages from the server
                result = self.broadcast_rx.recv() => {
                    match result {
                        Err(broadcast::error::RecvError::Closed) => {
                            bail!("Broadcast channel closed");
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::error!("[Client {id}] Broadcast channel lagged by {n} messages");
                        }
                        Ok(message) => {
                            log::debug!("[Client {id}] Received broadcast message: {message:?}");
                            self.handle_local_message(message).await.with_context(|| "Unable to handle message from server")?;
                        }
                    }
                }

                // Handle incoming messages from the remote client
                result = self.stream.read_exact(&mut message_length_buffer) => {
                    log::debug!("[Client {id}] New message from remote client");
                    match result {
                        Ok(0) => {
                            log::info!("[Client {id}] Remote client disconnected");
                            return Ok(());
                        }
                        Ok(4) => {
                            // Message length properly received
                            let message_length = u32::from_be_bytes(message_length_buffer) as usize;
                            // Receive actual message
                            match self.receive_remote_message(message_length).await {
                                Err(e) => {
                                    log::error!("[Client {id}] Failed to receive remote message: {e:?}");
                                    continue;
                                }
                                Ok(message) => {
                                    if let Err(e) = self.handle_remote_message(message) {
                                        log::error!("[Client {id}] Error handling remote message: {e:?}");
                                        continue;
                                    }
                                }
                            }

                        }
                        Ok(n) => {
                            log::error!("[Client {id}] Failed to receive message length: {n}/4 bytes received");
                            continue;
                        }
                        Err(e) => {
                            match e.kind() {
                                io::ErrorKind::UnexpectedEof => {
                                    log::info!("[Client {id}] Remote client disconnected");
                                    return Ok(());
                                }
                                _ => {
                                    log::error!("[Client {id}] Failed to receive message length: {e:?}");
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

    // Channel: Local Client Thread -> Server Thread
    let (server_tx, server_rx) = mpsc::channel::<ServerMessage>(LOCAL_CHANNEL_CAPACITY);

    // Broadcast: Server Thread -> Local Client Threads
    let (broadcast_tx, broadcast_rx) = broadcast::channel::<ClientMessage>(LOCAL_CHANNEL_CAPACITY);

    // Spawn the server task
    let server = Server::new(server_rx, broadcast_tx)
        .await
        .with_context(|| "Failed to create server")?;
    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            log::error!("Server encountered an error: {e:?}");
        }
    });
    log::info!("Server started successfully");

    // Bind TCP listener to the socket address
    let socket_addr = SocketAddr::new(args.addr, args.port);
    let listener = TcpListener::bind(socket_addr)
        .await
        .with_context(|| "Failed to bind TCP listener")?;
    log::info!("Lisening at {socket_addr}");

    // Connections loop
    loop {
        match listener.accept().await {
            Err(e) => {
                log::error!("Failed to accept connection: {e:?}");
                continue;
            }
            Ok((stream, addr)) => {
                log::info!("Accepted connection from {addr}");

                // Create a new client instance
                let client = match Client::new(
                    stream,
                    broadcast_rx.resubscribe(),
                    server_tx.clone(),
                )
                .await
                {
                    Err(e) => {
                        log::error!("Failed to create client: {e:?}");
                        continue;
                    }
                    Ok(client) => client,
                };

                // Spawn a task to handle the client
                tokio::spawn(async move {
                    if let Err(e) = client.run().await {
                        log::error!("Client encountered an error: {e:?}");
                    }
                });
            }
        }
    }
}
