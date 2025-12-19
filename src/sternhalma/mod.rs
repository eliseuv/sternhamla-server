use std::fmt::{Debug, Display};

use anyhow::Result;

use crate::sternhalma::board::{
    Board, HexIdx, goal_indices,
    movement::{Movement, MovementError, MovementIndices},
    player::{PLAYER_COUNT, Player},
};

/// Hexagonal Sternhalma board
pub mod board;

/// Statistics gathered over turns
pub mod timing;

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy)]
pub enum GameStatus {
    /// Game is ongoing
    Playing {
        player: Player,
        turns: usize,
        scores: [usize; PLAYER_COUNT],
    },
    /// Game finished
    Finished {
        winner: Player,
        total_turns: usize,
        scores: [usize; PLAYER_COUNT],
    },
}

impl GameStatus {
    /// Get number of turns
    pub fn turns(&self) -> usize {
        match self {
            GameStatus::Playing { turns, .. } => *turns,
            GameStatus::Finished { total_turns, .. } => *total_turns,
        }
    }

    /// Get scores
    pub fn scores(&self) -> [usize; PLAYER_COUNT] {
        match self {
            GameStatus::Playing { scores, .. } => *scores,
            GameStatus::Finished { scores, .. } => *scores,
        }
    }
}

impl Display for GameStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameStatus::Playing {
                player,
                turns,
                scores,
            } => {
                write!(f, "Playing: {player} | Turn: {turns} | Scores: {scores:?}")
            }
            GameStatus::Finished {
                winner,
                total_turns,
                scores,
            } => {
                write!(
                    f,
                    "Winner: {winner} | Total turns: {total_turns} | Scores: {scores:?}"
                )
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
                scores: [0; PLAYER_COUNT],
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
    fn next_status(&self, movement: &MovementIndices) -> GameStatus {
        match self.status {
            // Game finished is absorbing state
            GameStatus::Finished { .. } => {
                log::warn!("Attempting to update state of finished game");
                self.status
            }
            // Game is ongoing
            GameStatus::Playing {
                player,
                turns,
                mut scores,
            } => {
                // Update game scores
                let goal = goal_indices(&player);
                if goal.contains(&movement[0]) {
                    scores[player as usize] -= 1;
                }
                if goal.contains(&movement[1]) {
                    scores[player as usize] += 1;
                }

                // Check winning conditions
                if let Some(player) = self.board.check_winner() {
                    GameStatus::Finished {
                        winner: player,
                        total_turns: turns + 1,
                        scores,
                    }
                } else {
                    // Game is still ongoing, switch to the opponent
                    GameStatus::Playing {
                        player: player.opponent(),
                        turns: turns + 1,
                        scores,
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
        self.status = self.next_status(movement);

        self.status
    }
}
