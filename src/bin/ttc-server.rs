use core::net::SocketAddr;
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use sternhalma_server::tictactoe::{self, GameStatus, Player};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, WriteHalf},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc},
};

const IP_ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
const PORT: u16 = 6502;
const SOCKET_ADDR: SocketAddr = SocketAddr::new(IP_ADDR, PORT);
const NUM_PLAYERS: usize = 2;

/// Server messages
/// Server Thread -> Remote Client
#[derive(Debug, Serialize)]
enum ServerMessage {
    Init(tictactoe::Player),
    AvailableMoves(Vec<[usize; 2]>),
    WorngTurn(tictactoe::Player),
    GameError(tictactoe::TicTacToeError),
    GameFinished(tictactoe::GameResult),
}

/// Client messages
/// Local Client Thread -> Server Thread
#[derive(Debug, Deserialize)]
struct ClientMessage {
    player: tictactoe::Player,
    action: PlayerAction,
}

/// Action taken by a player
/// Remote Client -> Local Client Thread
#[derive(Debug, Deserialize)]
struct PlayerAction {
    row: usize,
    col: usize,
}

type PlayersList = Arc<Mutex<HashMap<Player, WriteHalf<TcpStream>>>>;

#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::init();

    // TCP listener
    let listener = TcpListener::bind(SOCKET_ADDR).await?;
    log::info!("[Server] Listening on {SOCKET_ADDR}");

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
            handle_client(stream, addr, client_id, players_list_clone, sender_clone).await
        });

        client_id += 1;
    }
    log::info!("[Server] All players connected.");

    game_loop(players_list, receiver).await;

    Ok(())
}

/// Handles a single client connection
/// TODO: Error handling
async fn handle_client(
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
        [] => Player::Nought,
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
    message_client(&players_list, &player, ServerMessage::Init(player)).await;

    // Loop read incoming actions from client
    let mut buffer = [0; 1024];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => {
                log::info!("[Client {client_id} | Player {player}] Disconnected.");
                return;
            }
            Ok(n) => match serde_json::from_slice::<PlayerAction>(&buffer[..n]) {
                Err(e) => {
                    log::error!(
                        "[Client {client_id} | Player {player}] Failed to decode message: {e}"
                    );
                }
                Ok(action) => {
                    let msg = ClientMessage { player, action };
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

    let mut game_state = tictactoe::TicTacToeGame::new();

    while let GameStatus::Playing(current_player) = game_state.status() {
        log::debug!("[Server] Game state:\n{game_state}");

        // Send available moves to current player
        log::info!("[Server] Sending available moves to player {current_player}...");
        let available_moves = game_state.available_moves();
        message_client(
            &players_list,
            current_player,
            ServerMessage::AvailableMoves(available_moves),
        )
        .await;

        // Wait for a player action
        log::info!("[Server] Waiting for player {current_player} turn...");
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
                let server_response = ServerMessage::WorngTurn(*current_player);
                message_client(&players_list, &msg.player, server_response).await;
                continue;
            }

            // Make the move in the game state
            match game_state.make_move(msg.action.row, msg.action.col) {
                Ok(None) => {
                    log::info!(
                        "[Server] Player {player} made a valid move.",
                        player = msg.player
                    );
                }
                Ok(Some(result)) => {
                    log::info!("[Server] Game finished with result: {result:?}");
                    // Broadcast final game state
                    broadcast_message(&players_list, ServerMessage::GameFinished(result)).await;
                    return; // Exit the game loop after a finished game
                }
                Err(e) => {
                    log::warn!(
                        "[Server] Player {player} made an invalid move: {e:?}",
                        player = msg.player
                    );
                    message_client(&players_list, &msg.player, ServerMessage::GameError(e)).await;
                    continue;
                }
            }
        } else {
            log::info!("[Server] No more messages from players. Exiting game loop.");
            return;
        }
    }
}
