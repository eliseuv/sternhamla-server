use std::{
    collections::{HashMap, HashSet, hash_map},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use itertools::Itertools;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::sternhalma::{
    Game, GameResult, GameStatus,
    board::{movement::MovementIndices, player::Player},
    timing::GameTimer,
};

pub mod client;
pub mod messages;
pub mod protocol;

use messages::{ClientMessage, ClientRequest, ServerBroadcast, ServerMessage};

/// Main thread message to server thread
///
/// Signals sent from the main application thread to the server thread
/// to handle new connections, reconnections, or player slot requests.
#[derive(Debug)]
pub enum MainThreadMessage {
    /// A new client has successfully completed the handshake
    ClientConnected(Player, Uuid, mpsc::Sender<ServerMessage>),
    /// A client is trying to reconnect with an existing session
    ClientReconnected(Player, mpsc::Sender<ServerMessage>),
    /// Request to check if a session ID is valid and get the associated player
    ClientReconnectedHandle(Uuid, oneshot::Sender<Option<Player>>),
    /// Request to get an available player slot
    RequestFreePlayer(oneshot::Sender<Option<Player>>),
}

/// The Main Server Logic
///
/// The `Server` struct runs in its own thread and orchestrates the game.
/// It manages client connections, tracks game state, and handles the flow of the game.
#[derive(Debug)]
pub struct Server {
    // Channel to receive messages from the main thread
    main_rx: mpsc::Receiver<MainThreadMessage>,
    // Clients list - Active connections to players
    clients_tx: HashMap<Player, mpsc::Sender<ServerMessage>>,
    // Session management - Maps Session IDs to Players
    sessions: HashMap<Uuid, Player>,
    // Disconnected players with active sessions - Players who dropped off but can reconnect
    disconnected: HashSet<Player>,
    // Channel for broadcasting messages to all local client threads
    broadcast_tx: broadcast::Sender<ServerBroadcast>,
    // Channel for receiving messages from local client threads
    clients_rx: mpsc::Receiver<ClientMessage>,
}

impl Server {
    /// Creates a new server instance
    pub fn new(
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
    ///
    /// This function blocks until the required number of players have connected.
    /// It handles incoming connections and assigns player slots (P1, P2, etc.).
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

    /// Disconnects all players
    ///
    /// Gracefully shuts down connections by sending a broadcast disconnect signal
    /// and waiting for clients to acknowledge/close.
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

    /// Coordinates a single turn in the game
    ///
    /// 1. Calculates available moves for the current player.
    /// 2. Sends the list of moves to the player.
    /// 3. Waits for the player's response (choice).
    /// 4. Validates the choice.
    /// 5. Applies the move to the game state.
    /// 6. Broadcasts the move to all players.
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

    /// Runs the main game loop
    ///
    /// Manages the state machine of the game:
    /// - Loops until the game finishes or max turns reached.
    /// - Checks for game end conditions.
    /// - Delegates turn handling to `handle_turn`.
    /// - Updates game timer and logs progress.
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
    ///
    /// 1. Waits for players to connect.
    /// 2. Runs the game loop.
    /// 3. Broadcasts the game result.
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
    ///
    /// Entry point for the server thread.
    /// Runs the server and ensures all players are disconnected when it finishes.
    pub async fn try_run(mut self, timeout: Duration, max_turns: usize) -> Result<()> {
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
