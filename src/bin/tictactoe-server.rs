use std::{
    collections::HashMap,
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, WriteHalf},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc},
    time::{self, Duration},
};

use sternhalma_server::tictactoe;

const LOCALHOST_ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
const NUM_PLAYERS: usize = 2;
const MSG_BUF_SIZE: usize = 1024;

/// Delay to wait for clients to connect
const WAIT_CLIENTS_DELAY: Duration = Duration::from_millis(500);

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

/// Server messages
/// Server Thread -> Remote Client
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum ServerMessage {
    /// Initialization message informing the client about their player
    Connect { player: tictactoe::Player },
    /// Inform the client that it is being disconnected
    Disconnect,
    /// Inform the client that it is their turn
    YourTurn {
        opponent_move: [usize; 2],
        available_moves: Vec<[usize; 2]>,
    },
    /// Update client's game state
    GameState { board: tictactoe::Board },
    /// Inform the clients that the game is over and it's result
    GameFinished { result: tictactoe::GameResult },
    /// Inform the client about an error in the game
    GameError { error: tictactoe::TicTacToeError },
}

/// Client messages
/// Local Client Thread -> Server Thread
#[derive(Debug, Deserialize)]
struct ClientMessage {
    player: tictactoe::Player,
    action: [usize; 2],
}

type PlayersList = Arc<Mutex<HashMap<tictactoe::Player, WriteHalf<TcpStream>>>>;

#[tokio::main]
async fn main() -> io::Result<()> {
    // Initialize logger
    env_logger::init();

    // Parse command line arguments
    let args = Args::parse();
    log::debug!("[Server] Command line arguments: {args:?}");

    let socket_addr = SocketAddr::new(args.addr, args.port);
    log::info!("[Server] Starting server on {socket_addr}");

    // TCP listener
    let listener = TcpListener::bind(socket_addr).await?;

    // Store client connections
    let players_list: PlayersList = Arc::new(Mutex::new(HashMap::new()));

    // Channel for the client threads to communicate with the server thread
    let (sender, receiver) = mpsc::channel::<ClientMessage>(32);

    // Accept client connections
    let mut client_id = 0;
    while client_id < NUM_PLAYERS {
        log::info!("[Server] Waiting for players... {client_id}/{NUM_PLAYERS} connected");
        // Accept a new connection
        let (stream, addr) = listener.accept().await?;
        log::info!("[Server] Incoming connection from {addr}");

        let players_list_clone = players_list.clone();
        let sender_clone = sender.clone();
        tokio::spawn(async move {
            client(stream, addr, client_id, players_list_clone, sender_clone).await
        });

        client_id += 1;
    }

    // Wait for all players to connect
    log::info!("[Server] Waiting for all players to connect...");
    while players_list.lock().await.len() < NUM_PLAYERS {
        time::sleep(WAIT_CLIENTS_DELAY).await;
    }
    log::info!("[Server] All players connected.");

    game_loop(players_list, receiver).await;
    log::trace!("[Server] Exited game loop.");

    Ok(())
}

/// Client thread
/// TODO: Error handling
async fn client(
    stream: TcpStream,
    addr: SocketAddr,
    client_id: usize,
    players_list: PlayersList,
    sender: mpsc::Sender<ClientMessage>,
) {
    log::info!("[Client {client_id}] Handling connection from {addr}");

    // Split the stream into reader and writer
    let (mut reader, writer) = tokio::io::split(stream);

    // Assign player to connection
    let player = match &players_list.lock().await.keys().collect::<Vec<_>>()[..] {
        // First player
        [] => tictactoe::Player::Nought,
        // Second player
        [p] => p.opposite(),
        // Too many players
        _ => {
            log::error!("[Client {client_id}] Too many players connected.");
            log::info!("[Client {client_id}] Disconnecting.");
            return;
        }
    };
    log::info!("[Client {client_id}] Assigned to player {player}.");

    // Insert player into players list
    players_list.lock().await.insert(player, writer);

    // Send initial message
    message_client(&players_list, &player, ServerMessage::Connect { player }).await;

    // Loop read incoming actions from client
    let mut buffer = [0; MSG_BUF_SIZE];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => {
                log::info!("[Client {client_id} | Player {player}] Disconnected.");
                return;
            }
            Ok(n) => match serde_json::from_slice::<[usize; 2]>(&buffer[..n]) {
                Err(e) => {
                    log::error!(
                        "[Client {client_id} | Player {player}] Failed to decode message: {e}"
                    );
                }
                Ok(action) => {
                    let msg = ClientMessage { player, action };
                    log::debug!(
                        "[Client {client_id} | Player {player}] Received action: {:?}",
                        msg.action
                    );
                    if let Err(e) = sender.send(msg).await {
                        log::error!(
                            "[Client {client_id} | Player {player}] Failed to send message to game loop: {e}"
                        );
                    }
                }
            },
            Err(e) => {
                log::error!(
                    "[Client {client_id} | Player {player}] Failed to read from socket: {e}"
                );
                log::info!("[Client {client_id} | Player {player}] Disconnecting.");
                return;
            }
        }
    }
}

async fn message_client(
    players_list: &PlayersList,
    player: &tictactoe::Player,
    msg: ServerMessage,
) {
    log::debug!("[Server] Sending message to player {player}:\n{msg:?}");
    let msg_encoded = serde_json::to_string(&msg).unwrap();
    if let Some(writer) = players_list.lock().await.get_mut(player) {
        if let Err(e) = writer.write_all(msg_encoded.as_bytes()).await {
            log::error!("[Server] Failed to send message to player {player}: {e}");
        }
    } else {
        log::error!("[Server] Player {player} not found in players list.");
    }
}

async fn broadcast_message(players_list: &PlayersList, msg: ServerMessage) {
    log::info!("[Server] Broadcasting message:\n{msg:?}");
    let msg_encoded = serde_json::to_string(&msg).unwrap();
    for (player, writer) in players_list.lock().await.iter_mut() {
        log::debug!("[Server] Sending message to player {player}");
        if let Err(e) = writer.write_all(msg_encoded.as_bytes()).await {
            log::error!("[Server] Failed to send message to player {player}: {e}");
        }
    }
}

async fn game_loop(players_list: PlayersList, mut receiver: mpsc::Receiver<ClientMessage>) {
    log::info!("[Server] Starting game...");
    let mut state = tictactoe::Game::new();

    // Wait for opening move
    log::debug!("[Server] Waiting for opening move...");
    let mut last_action = loop {
        match receiver.recv().await {
            Some(msg) => {
                log::debug!(
                    "[Server] Received opening move from player {}: {:?}",
                    msg.player,
                    msg.action
                );

                // Check if the player is the first player
                if msg.player != tictactoe::Player::Nought {
                    log::warn!(
                        "[Server] Player {player} tried to make an opening move.",
                        player = msg.player
                    );
                    continue;
                }

                // Make the opening move in the game state
                match state.make_move(msg.action) {
                    Ok(None) => {
                        log::debug!("[Server] Opening move accepted.");
                        break msg.action; // Exit the loop after a valid opening move
                    }
                    Err(e) => {
                        log::warn!("[Server] Invalid opening move: {e:?}");
                        message_client(
                            &players_list,
                            &msg.player,
                            ServerMessage::GameError { error: e },
                        )
                        .await;
                        continue;
                    }
                    Ok(Some(_)) => {
                        panic!("Game should not finish on opening move");
                    }
                }
            }
            None => {
                log::info!("[Server] No more messages from players. Exiting game loop.");
                return;
            }
        }
    };

    // Game loop
    while let tictactoe::GameStatus::Playing(current_player) = state.status() {
        log::debug!("[Server] Game state:\n{state}");

        // Send available moves to current player
        log::info!("[Server] Sending available moves to player {current_player}...");
        let available_moves = state.available_moves();
        message_client(
            &players_list,
            current_player,
            ServerMessage::YourTurn {
                opponent_move: last_action,
                available_moves,
            },
        )
        .await;

        // Wait for a player action
        log::debug!("[Server] Waiting for player {current_player} turn...");
        if let Some(msg) = receiver.recv().await {
            log::debug!(
                "[Server] Received action from player {}: {:?}",
                msg.player,
                msg.action
            );

            // Check if the player is the current player
            if msg.player != *current_player {
                log::warn!(
                    "[Server] Player {player} tried to play out of turn.",
                    player = msg.player
                );
                continue;
            }

            // Store the last action for potential future use
            last_action = msg.action;

            // Make the move in the game state
            match state.make_move(msg.action) {
                Ok(None) => {
                    log::debug!(
                        "[Server] Player {player} made move ({row}, {col}).",
                        player = msg.player,
                        row = msg.action[0],
                        col = msg.action[1]
                    );
                }
                Ok(Some(result)) => {
                    log::info!("[Server] Game finished with result: {result:?}");
                    log::debug!("[Server] Game state:\n{state}");
                    // Broadcast result
                    broadcast_message(&players_list, ServerMessage::GameFinished { result }).await;
                    // Broadcast final game state
                    broadcast_message(
                        &players_list,
                        ServerMessage::GameState {
                            board: state.board().clone(),
                        },
                    )
                    .await;
                    break;
                }
                Err(e) => {
                    log::warn!(
                        "[Server] Player {player} made an invalid move: {e:?}",
                        player = msg.player
                    );
                    message_client(
                        &players_list,
                        &msg.player,
                        ServerMessage::GameError { error: e },
                    )
                    .await;
                    continue;
                }
            }
        } else {
            log::info!("[Server] No more messages from players. Exiting game loop.");
            return;
        }
    }

    // Disconnect all clients
    broadcast_message(&players_list, ServerMessage::Disconnect).await;
}
