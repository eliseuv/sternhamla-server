//! # Internal Messages Module
//!
//! This module defines the message types used for communication between internal threads/tasks.
//!
//! ## Message Types
//! - [`ServerMessage`]: Direct messages from Server to a specific Client.
//! - [`ServerBroadcast`]: Messages broadcast from Server to all Clients.
//! - [`ClientMessage`]: Requests from a Client to the Server.

use crate::sternhalma::{
    GameResult, Scores,
    board::{movement::MovementIndices, player::Player},
};

/// Message from the Server Thread to a specific Local Client Thread
///
/// This enum encapsulates messages that are directed to a single client,
/// such as informing them that it is their turn to play.
#[derive(Debug, Clone)]
pub enum ServerMessage {
    /// Players turn
    ///
    /// Sent when it is the player's turn to make a move.
    Turn {
        /// List of available movements
        ///
        /// Contains all valid moves the player can make from the current board state.
        movements: Vec<MovementIndices>,
    },
}

/// Message from the Server Thread to ALL Local Client Threads
///
/// This enum represents events that affect all players, such as someone making a move,
/// the game finishing, or a server shutdown signal.
#[derive(Debug, Clone)]
pub enum ServerBroadcast {
    /// Disconnection signal
    ///
    /// Sent when the server is shutting down or wants to force a disconnect for all clients.
    Disconnect,
    /// Player made a move
    ///
    /// Broadcasted after a player has successfully performed a valid move.
    /// Used to update the local game state on all clients.
    Movement {
        /// The player who made the move
        player: Player,
        /// The movement that was performed
        movement: MovementIndices,
        /// The updated scores after the move
        scores: Scores,
    },
    /// Game has finished
    ///
    /// Broadcasted when the game reaches a terminal state (win or draw).
    GameFinished {
        /// The result of the game
        result: GameResult,
    },
}

/// Message from a Local Client Thread to the Server Thread
///
/// This enum represents actions or requests initiated by a specific client,
/// such as making a move or requesting to disconnect.
#[derive(Debug)]
pub enum ClientRequest {
    /// Disconnection request
    ///
    /// Sent when a client wants to gracefully leave the game.
    Disconnect,
    /// Player made a movement choice
    ///
    /// Sent when the player selects a move from the available options.
    /// The move is identified by its index in the list of available moves sent by the server.
    Choice {
        /// Index of the chosen movement
        movement_index: usize,
    },
}

/// Packaged client request with identification
///
/// This struct wraps a `ClientRequest` with the identity of the player making the request.
/// It is used to send messages from Local Client Threads to the Server Thread through the shared channel.
///
/// Local Client Thread -> Server Thread
#[derive(Debug)]
pub struct ClientMessage {
    /// The player who sent this message
    pub player: Player,
    /// The content of the request
    pub request: ClientRequest,
}
