use std::fmt::{Debug, Display};

use serde::Serialize;

use crate::sterhalma::board::{BOARD_LENGTH, Board, lut};

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

    const fn piece(&self) -> char {
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

impl Board<Player> {
    /// Creates a new Sternhalma board with pieces placed in their starting positions
    pub fn new() -> Self {
        Self::empty()
            .with_pieces(&lut::PLAYER1_STARTING_POSITIONS, Player::Player1)
            .expect("Player 1 positions are valid")
            .with_pieces(&lut::PLAYER2_STARTING_POSITIONS, Player::Player2)
            .expect("Player 2 positions are valid")
    }
}

/// Board display
impl Display for Board<Player> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for i in 0..BOARD_LENGTH {
            write!(f, "{}", " ".repeat(i))?;
            for j in 0..BOARD_LENGTH {
                match &self[[i, j]] {
                    None => write!(f, "󠀠󠀠󠀠󠀠   ")?,
                    Some(None) => write!(f, "⚫ ")?,
                    Some(Some(player)) => write!(f, "{piece} ", piece = player.piece())?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}
