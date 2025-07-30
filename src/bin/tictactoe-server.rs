use std::{
    collections::HashMap,
    fmt::Display,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic::{self, AtomicUsize},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select,
    sync::mpsc::{Receiver, Sender, channel},
    time::timeout,
};

use sternhalma_server::tictactoe;

const LOCALHOST_ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
const CHANNEL_CAPACITY: usize = 32;
const NUM_PLAYERS: usize = 2;
const MSG_BUF_SIZE: usize = 1024;
const TIMEOUT_DURATION: Duration = Duration::from_secs(10);

/// Unique client ID generator
static NEXT_CLIENT_ID: AtomicUsize = AtomicUsize::new(0);

/// Command line arguments
#[derive(Debug, Parser)]
#[command(name = "tictactoe-server", version, about)]
struct Args {
    /// IP address to bind the server to
    #[arg(short, long, default_value_t = LOCALHOST_ADDR)]
    addr: IpAddr,

    /// Port to bind the server to
    #[arg(short, long)]
    port: u16,
}

/// Local Client Thread -> Server Thread
#[derive(Debug)]
enum ClientMessage {
    /// Inform server about a player movement
    PlayerMovement(tictactoe::Player, [usize; 2]),
    /// Inform server that a player has disconnected
    Disconnect(tictactoe::Player),
}

/// Server Thread -> Local Client Thread
#[derive(Debug, Clone)]
enum ServerMessage {
    /// Inform client about the available moves for the current player
    PlayerTurn(Vec<[usize; 2]>),
    /// Inform client about a player movement
    PlayerMovement(tictactoe::Player, [usize; 2]),
    /// Inform client that the game has finished
    GameOver(tictactoe::GameResult),
}

/// Local Client Thread -> Remote Client
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteOutMessage {
    /// Inform client about the assigned player
    Initialization { player: tictactoe::Player },
    /// Inform client that it is their turn
    YourTurn { available_moves: Vec<[usize; 2]> },
    /// Inform client about a player movement
    Movement {
        player: tictactoe::Player,
        position: [usize; 2],
    },
    /// Inform client that the game has finished
    GameOver { result: tictactoe::GameResult },
    /// Inform client about a game error
    GameError { error: tictactoe::GameError },
}

/// Remote Client -> Local Client Thread
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum RemoteInMessage {
    /// Receive a player movement
    Movement { position: [usize; 2] },
}

#[derive(Debug)]
struct Server {
    clients: HashMap<tictactoe::Player, Sender<ServerMessage>>,
    receiver: Receiver<ClientMessage>,
    game: tictactoe::Game,
}

impl Server {
    async fn new(addr: SocketAddr) -> Result<Self> {
        log::info!("Creating server at {addr}");

        // Bind TCP listener to socket
        let listener = TcpListener::bind(addr)
            .await
            .context("Failed to bind listener to socket")?;

        // Channel for the client threads to send messages to the server thread
        let (sender_to_server, server_receiver) = channel::<ClientMessage>(CHANNEL_CAPACITY);

        // Senders to client threads
        let mut clients: HashMap<tictactoe::Player, Sender<ServerMessage>> =
            HashMap::with_capacity(NUM_PLAYERS);

        log::debug!("Waiting for clients to connect...");

        while clients.len() < NUM_PLAYERS {
            log::debug!(
                "Waiting for clients... {n_conn}/{NUM_PLAYERS}",
                n_conn = clients.len()
            );
            let (stream, addr) = timeout(TIMEOUT_DURATION, listener.accept())
                .await
                .context("Timeout on client connection")?
                .context("Failed to accept client connection")?;
            log::info!("Client connected from {addr}");

            let client_id = NEXT_CLIENT_ID.fetch_add(1, atomic::Ordering::Relaxed);

            let player = match clients.keys().collect::<Vec<_>>()[..] {
                [] => tictactoe::Player::Cross,
                [first_player] => first_player.opposite(),
                _ => bail!("Too many clients connected"),
            };

            // Channel for the server thread to send messages to the client thread
            let (sender_to_client, client_receiver) = channel::<ServerMessage>(CHANNEL_CAPACITY);

            let client = Client {
                id: client_id,
                player,
                sender: sender_to_server.clone(),
                receiver: client_receiver,
                stream,
                buffer_out: Vec::with_capacity(MSG_BUF_SIZE),
            };

            tokio::spawn(async move {
                if let Err(e) = client.run().await {
                    log::error!("Error running client {client_id}: {e}");
                }
            });

            clients.insert(player, sender_to_client);
        }

        let n_clients = clients.len();
        log::info!("All clients connected. Number of clients connected: {n_clients}",);

        Ok(Self {
            clients,
            receiver: server_receiver,
            game: tictactoe::Game::new(),
        })
    }

    /// Broadcast a message to all clients
    async fn broadcast_message(&self, message: &ServerMessage) -> Result<()> {
        log::debug!("Broadcasting message to all clients: {message:?}");
        for (player, client) in self.clients.iter() {
            if let Err(e) = client.send(message.clone()).await {
                log::error!("Failed to send message to client {player}: {e}");
            }
        }
        Ok(())
    }

    /// Send a message to a specific client
    async fn message_client(
        &self,
        player: &tictactoe::Player,
        message: ServerMessage,
    ) -> Result<()> {
        log::debug!("Sending message to client {player}: {message:?}");
        if let Some(client) = self.clients.get(player) {
            client
                .send(message)
                .await
                .context("Failed to send message to client")?;
        } else {
            bail!("Client for player {player} not found");
        }
        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        log::debug!("Running server with {} clients", self.clients.len());

        // Main game loop
        while let tictactoe::GameStatus::Playing(current_player) = self.game.status() {
            log::debug!("Current player: {current_player}");

            // Inform the current player that it's their turn
            let available_moves = self.game.available_moves();
            self.message_client(current_player, ServerMessage::PlayerTurn(available_moves))
                .await?;

            // Receive messages from clients
            match self.receiver.recv().await {
                None => bail!("All clients dropped, exiting game loop"),
                Some(message) => match message {
                    ClientMessage::PlayerMovement(player, pos) => {
                        log::debug!("Received player movement from {player} at position {pos:?}");
                        // Check if the player is the current player
                        if player != *current_player {
                            log::warn!(
                                "Received move from {player} but expected {current_player} to play"
                            );
                            // TODO: Send wrong move message to client
                            continue;
                        }
                        log::debug!("Player {player} move at {pos:?}");
                        // Make the move in the game
                        match self.game.make_move(pos) {
                            // Valid move
                            Ok(status) => {
                                // Notify all clients about the move
                                self.broadcast_message(&ServerMessage::PlayerMovement(player, pos))
                                    .await
                                    .context("Failed to broadcast player movement")?;

                                match status {
                                    // Game still in progress
                                    tictactoe::GameStatus::Playing(_) => continue,

                                    // Game finished
                                    tictactoe::GameStatus::Finished(result) => {
                                        log::info!("Game finished: {result:?}");
                                        self.broadcast_message(&ServerMessage::GameOver(result))
                                            .await
                                            .context("Failed to broadcast game result")?;

                                        return Ok(());
                                    }
                                }
                            }
                            // Invalid move
                            Err(e) => {
                                // TODO: Notify client about the error
                                log::error!("Failed to make move: {e:?}");
                                continue;
                            }
                        }
                    }

                    ClientMessage::Disconnect(player) => {
                        log::info!("Client {player} disconnected");
                        // Remove the client from the game
                        self.clients.remove(&player);
                        // If there are no more clients, exit the game loop
                        if self.clients.is_empty() {
                            log::info!("No more clients connected, exiting game loop");
                            return Ok(());
                        }
                        // TODO: Handle disconnection properly
                        return Ok(());
                    }
                },
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
struct Client {
    /// Client ID
    id: usize,
    /// Assigned player
    player: tictactoe::Player,
    /// Sender for messages to the server thread
    sender: Sender<ClientMessage>,
    /// Receiver for messages from the server thread
    receiver: Receiver<ServerMessage>,
    /// Stream to remote client
    stream: TcpStream,
    /// Buffer for serialization of outgoing messages
    buffer_out: Vec<u8>,
}

impl Display for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Client {} ({})", self.id, self.player)
    }
}

impl Client {
    /// Send a message to the remote client
    async fn send_remote_message(&mut self, message: &RemoteOutMessage) -> Result<()> {
        log::debug!("[{self}] Sending message to remote client:\n{message:?}");

        self.buffer_out.clear();
        ciborium::into_writer(message, &mut self.buffer_out)
            .context("Failed to serialize message")?;

        self.stream
            .write_all(&self.buffer_out)
            .await
            .context("Failed to send message to remote client")?;
        self.stream
            .flush()
            .await
            .context("Failed to flush message to remote client")?;

        log::debug!("[{self}] Message successfully sent to remote client");
        Ok(())
    }

    /// Handle message received from the server thread
    async fn handle_local_message(&mut self, message: ServerMessage) -> Result<bool> {
        log::debug!("[{self}] Local message received:\n{message:?}");
        match message {
            ServerMessage::PlayerMovement(player, position) => {
                self.send_remote_message(&RemoteOutMessage::Movement { player, position })
                    .await
                    .context("Failed to send player movement to remote client")?;
            }

            ServerMessage::PlayerTurn(available_moves) => {
                self.send_remote_message(&RemoteOutMessage::YourTurn { available_moves })
                    .await
                    .context("Failed to inform remote client that its their turn")?;
            }

            ServerMessage::GameOver(result) => {
                self.send_remote_message(&RemoteOutMessage::GameOver { result })
                    .await
                    .context("Failed to inform remote client that the game has finished")?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Handle a message received from the remote client
    async fn handle_remote_message(&mut self, message: &RemoteInMessage) -> Result<()> {
        log::debug!("[{self}] Remote message received:\n{message:?}");
        match message {
            RemoteInMessage::Movement { position: pos } => self
                .sender
                .send(ClientMessage::PlayerMovement(self.player, *pos))
                .await
                .context("Unable forward player movement to server thread"),
        }
    }

    /// Client thread
    async fn run(mut self) -> Result<()> {
        log::debug!("[{self}] Starting client thread");

        // Send initialization message to remote client
        self.send_remote_message(&RemoteOutMessage::Initialization {
            player: self.player,
        })
        .await
        .context("Failed to send initialization message to remote client")?;

        // Buffer for incoming remote messages
        let mut buffer_in = Vec::with_capacity(MSG_BUF_SIZE);

        loop {
            select! {
                // Check for local message from server thread
                message = self.receiver.recv() => {
                    match message {
                        Some(message) => {
                            match self.handle_local_message(message).await {
                                Ok(should_exit) => {
                                    if should_exit {
                                        log::info!("[{self}] Shutting down client thread");
                                        self.sender.send(ClientMessage::Disconnect(self.player)).await?;
                                        return Ok(());
                                    }
                                }
                                Err(e) => {
                                    log::error!("[{self}] Failed to handle local message: {e}");
                                }
                            }
                        }
                        None => {
                            log::info!("[{self}] Server thread channel closed, exiting client thread.");
                            return Ok(());
                        }
                    }
                },

                // Check for remote message from client
                result = self.stream.read(&mut buffer_in) => {
                    log::debug!("[{self}] Remote message received");
                    match result {
                        Ok(0) => {
                            log::info!("[{self}] Remote client disconnected");
                            // Inform server thread about the disconnection
                            return self.sender
                                .send(ClientMessage::Disconnect(self.player))
                                .await
                                .context("Failed to inform server thread about disconnection")
                        }
                        Ok(n) => {
                            match ciborium::from_reader::<RemoteInMessage, _>(&buffer_in[..n]) {
                                Ok(message) => {
                                    if let Err(e) = self.handle_remote_message(&message).await {
                                        log::error!("[{self}] Failed to handle remote message: {e}");
                                    }
                                }
                                Err(e) => log::error!("[{self}] Failed to decode message: {e}"),
                            }
                        }
                        Err(e) => log::error!("[{self}] Failed to receive remote message: {e}"),
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

    let socket_addr = SocketAddr::new(args.addr, args.port);
    let server = Server::new(socket_addr)
        .await
        .context("Failed to create server")?;

    server.run().await.context("Failed to run server")?;

    Ok(())
}
