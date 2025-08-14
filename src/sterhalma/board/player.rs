use std::fmt::{Debug, Display};

use serde::Serialize;

/// Sternhalma players
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Player {
    #[serde(rename = "1")]
    Player1,
    #[serde(rename = "2")]
    Player2,
}

impl Player {
    /// List all player variants
    pub const fn variants() -> [Player; 2] {
        [Player::Player1, Player::Player2]
    }

    /// Number of players
    pub const fn count() -> usize {
        Player::variants().len()
    }

    pub const fn opponent(&self) -> Self {
        match self {
            Player::Player1 => Player::Player2,
            Player::Player2 => Player::Player1,
        }
    }

    pub const fn piece(&self) -> char {
        match self {
            Player::Player1 => '🔵',
            Player::Player2 => '🔴',
        }
    }
}

impl Display for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Player1 => write!(f, "Player 1 ({piece})", piece = self.piece()),
            Self::Player2 => write!(f, "Player 2 ({piece})", piece = self.piece()),
        }
    }
}
