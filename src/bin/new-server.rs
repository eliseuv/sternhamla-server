use std::{
    collections::HashMap,
    fmt::Display,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::mpsc::{Receiver, Sender, channel},
};

use sternhalma_server::tictactoe;

const LOCALHOST_ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
const CHANNEL_CAPACITY: usize = 32;
const NUM_PLAYERS: usize = 2;
const MSG_BUF_SIZE: usize = 1024;

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
enum ClientMessage {}

/// Server Thread -> Local Client Thread
#[derive(Debug)]
enum ServerMessage {}

/// Local Client Thread -> Remote Client
#[derive(Debug, Serialize)]
enum RemoteOutMessage {
    /// Inform client about the assigned player
    Init(tictactoe::Player),
}

/// Remote Client -> Local Client Thread
#[derive(Debug, Deserialize)]
enum RemoteInMessage {}

#[derive(Debug)]
struct Server {
    clients: HashMap<tictactoe::Player, Sender<ServerMessage>>,
    receiver: Receiver<ClientMessage>,
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
            let (stream, addr) = listener
                .accept()
                .await
                .context("Failed to accept client connection")?;
            log::info!("Client connected from {addr}");

            let client_id = clients.len();

            let player = match clients.keys().collect::<Vec<_>>()[..] {
                [] => tictactoe::Player::Nought,
                [first_player] => first_player.opposite(),
                _ => bail!("Too many clients connected"),
            };

            // Channel for the server thread to send messages to the client thread
            let (sender_to_client, client_reciever) = channel::<ServerMessage>(CHANNEL_CAPACITY);
            clients.insert(player, sender_to_client);

            let client = Client {
                id: client_id,
                player,
                sender: sender_to_server.clone(),
                receiver: client_reciever,
                stream,
                buffer: Vec::with_capacity(MSG_BUF_SIZE),
            };

            tokio::spawn(async move {
                if let Err(e) = client.run().await {
                    log::error!("Error running client {client_id}: {e}");
                }
            });
        }

        let n_clients = clients.len();
        log::info!("All clients connected. Number of clients connected: {n_clients}",);

        Ok(Self {
            clients,
            receiver: server_receiver,
        })
    }

    async fn run(self) -> Result<()> {
        log::debug!("Running server with {} clients", self.clients.len());

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
    /// Message buffer for incoming messages
    buffer: Vec<u8>,
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

        ciborium::into_writer(message, &mut self.buffer).context("Failed to serialize message")?;
        log::debug!("[{self}] Serialized message: {} bytes", self.buffer.len());

        self.stream
            .write_all(&self.buffer)
            .await
            .context("Failed to send message to remote client")?;

        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        log::trace!("Running client thread for {self}");

        self.send_remote_message(&RemoteOutMessage::Init(self.player))
            .await
            .context("Failed to send initialization message to remote client")?;

        Ok(())
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
        .context("Failed to initialize server")?;

    server.run().await.context("Failed to run server")?;

    Ok(())
}
