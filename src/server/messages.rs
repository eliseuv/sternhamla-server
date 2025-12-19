use crate::sternhalma::{
    GameResult, Scores,
    board::{movement::MovementIndices, player::Player},
};

#[derive(Debug, Clone)]
pub enum ServerMessage {
    /// Players turn
    Turn {
        /// List of available movements
        movements: Vec<MovementIndices>,
    },
}

/// Server Thread -> All Local Client Threads
#[derive(Debug, Clone)]
pub enum ServerBroadcast {
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
pub enum ClientRequest {
    /// Disconnection request
    Disconnect,
    /// Player made a movement
    Choice { movement_index: usize },
}

/// Packaged client request with identification
/// Local Client Thread -> Server Thread
#[derive(Debug)]
pub struct ClientMessage {
    pub player: Player,
    pub request: ClientRequest,
}
