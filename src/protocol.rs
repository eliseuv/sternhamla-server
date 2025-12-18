use crate::sternhalma::board::{
    movement::MovementIndices,
    player::{PLAYER_COUNT, Player},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

/// Maximum length of a remote message in bytes
pub const REMOTE_MESSAGE_LENGTH: usize = 4 * 1024;

pub type Scores = [usize; PLAYER_COUNT];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum GameResult {
    Finished {
        winner: Player,
        total_turns: usize,
        scores: Scores,
    },
    MaxTurns {
        total_turns: usize,
        scores: Scores,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteOutMessage {
    /// Welcome message with session ID
    Welcome { session_id: Uuid, player: Player },
    /// Reconnect reject
    Reject { reason: String },
    /// Inform remote client about their assigned player
    Assign { player: Player },
    /// Disconnection signal
    Disconnect,
    /// Inform remote client that it is their turn
    Turn {
        /// List of available movements
        /// Each movement is represented by a pair of indices
        movements: Vec<MovementIndices>,
    },
    /// Inform remote client about a player's movement
    Movement {
        player: Player,
        movement: MovementIndices,
        scores: Scores,
    },
    /// Inform remote client that the game has finished with a result
    GameFinished { result: GameResult },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RemoteInMessage {
    /// Hello - Request new session
    Hello,
    /// Reconnect - Request resume session
    Reconnect { session_id: Uuid },
    /// Movement made by player (index based)
    Choice { movement_index: usize },
}

impl RemoteInMessage {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ciborium::from_reader(bytes).with_context(|| "Failed to deserialize remote message")
    }
}

impl RemoteOutMessage {
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        ciborium::into_writer(self, writer).with_context(|| "Failed to serialize remote message")
    }
}
