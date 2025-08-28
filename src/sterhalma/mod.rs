use std::fmt::{Debug, Display};

use anyhow::Result;

use crate::sterhalma::board::{
    Board, HexIdx,
    movement::{Movement, MovementError, MovementIndices},
    player::Player,
};

/// Hexagonal Sternhalma board
pub mod board;

/// Statistics gathered over turns
pub mod timing;

#[derive(Debug, Clone, Copy)]
pub enum GameStatus {
    /// Which piece is currently playing and the last movement made
    Playing { player: Player, turns: usize },
    /// Game finished
    Finished { winner: Player, total_turns: usize },
}

impl GameStatus {
    /// Get number of turns
    pub fn turns(&self) -> usize {
        match self {
            GameStatus::Playing { turns, .. } => *turns,
            GameStatus::Finished { total_turns, .. } => *total_turns,
        }
    }
}

impl Display for GameStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameStatus::Playing { player, turns } => {
                write!(f, "Playing: {player} | Turn: {turns}")
            }
            GameStatus::Finished {
                winner,
                total_turns,
            } => {
                write!(f, "Winner: {winner} | Total turns: {total_turns}")
            }
        }
    }
}

#[derive(Debug)]
pub struct Game {
    /// Board state
    board: Board<Player>,
    /// Game status
    status: GameStatus,
    /// Game history
    history: Vec<MovementIndices>,
}

impl Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{board}{status}",
            board = self.board,
            status = self.status
        )
    }
}

impl Game {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            status: GameStatus::Playing {
                player: Player::Player1,
                turns: 0,
            },
            history: Vec::with_capacity(128),
        }
    }

    pub fn board(&self) -> &Board<Player> {
        &self.board
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    pub fn history(&self) -> &[[HexIdx; 2]] {
        &self.history
    }

    pub fn history_bytes(&self) -> usize {
        self.history.capacity() * std::mem::size_of::<[HexIdx; 2]>()
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

/// Error that can occur during game operations
#[derive(Debug, Clone, Copy)]
pub enum GameError {
    /// Movement error
    Movement(MovementError),
    /// Movement made out of turn
    OutOfTurn,
    /// Movement made after the game is finished
    GameFinished,
}

impl Game {
    /// Update the game status based on current state of the game
    fn next_status(&self) -> GameStatus {
        match self.status {
            // Game finished is absorbing state
            GameStatus::Finished { .. } => {
                log::warn!("Attempting to update state of finished game");
                self.status
            }
            // Game is ongoing
            GameStatus::Playing { player, turns } => {
                // Check winning conditions
                if let Some(player) = self.board.check_winner() {
                    GameStatus::Finished {
                        winner: player,
                        total_turns: turns + 1,
                    }
                } else {
                    // Game is still ongoing, switch to the opponent
                    GameStatus::Playing {
                        player: player.opponent(),
                        turns: turns + 1,
                    }
                }
            }
        }
    }

    /// Iterate over the available movements for the current turn's player
    pub fn iter_available_moves(&self) -> impl Iterator<Item = Movement> {
        match &self.status {
            GameStatus::Finished { .. } => todo!(),
            GameStatus::Playing { player, .. } => self.board.iter_player_movements(player),
        }
    }

    /// Apply movement to the current game
    pub fn apply_movement(&mut self, movement: &Movement) -> Result<GameStatus, GameError> {
        match self.status {
            GameStatus::Finished { .. } => Err(GameError::GameFinished),
            GameStatus::Playing {
                player: current_player,
                ..
            } => {
                // Validate movement
                let (movement, player) = self
                    .board
                    .validate_movement(movement)
                    .map_err(GameError::Movement)?;

                // Check if the movement is made by the current player
                if player != &current_player {
                    return Err(GameError::OutOfTurn);
                }

                // Apply the movement to the board
                unsafe {
                    self.apply_movement_unchecked(&movement.into());
                }

                Ok(self.status)
            }
        }
    }

    /// Apply movement in the game without validating it or the player
    ///
    /// # Safety
    ///
    /// It is advised have validated the movement on the current board and check the player's turn beforehand
    pub unsafe fn apply_movement_unchecked(&mut self, movement: &MovementIndices) -> GameStatus {
        // Apply movement on the board
        unsafe {
            self.board.apply_movement_unchecked(movement);
        }

        // Update game history
        self.history.push(*movement);

        // Update game status
        self.status = self.next_status();

        self.status
    }
}
