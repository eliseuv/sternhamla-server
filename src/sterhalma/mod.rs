use std::fmt::{Debug, Display};

use anyhow::Result;

use crate::sterhalma::board::{
    Board, InvalidBoardIndex, lut,
    movement::{Movement, MovementError, MovementFull},
    player::Player,
};

/// Hexagonal Sternhalma board
pub mod board;

#[derive(Debug, Clone, Copy)]
pub enum GameStatus {
    /// Which piece is currently playing and the last movement made
    Playing { player: Player, turns: usize },
    /// Game finished
    Finished { winner: Player, turns: usize },
}

impl GameStatus {
    /// Get number of turns
    pub fn turns(&self) -> usize {
        match self {
            GameStatus::Playing { turns, .. } => *turns,
            GameStatus::Finished { turns, .. } => *turns,
        }
    }
}

#[derive(Debug)]
pub struct Game {
    /// Board state
    board: Board<Player>,
    /// Game status
    status: GameStatus,
}

impl Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            GameStatus::Playing { player, turns } => {
                writeln!(f, "Turn: {turns}\nPlaying: {player}")?
            }
            GameStatus::Finished { winner, turns } => {
                writeln!(f, "Game finished!\nWinner: {winner}\nTotal turns: {turns}")?;
            }
        }
        writeln!(f, "{}", self.board)
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
        }
    }

    pub fn board(&self) -> &Board<Player> {
        &self.board
    }

    pub fn status(&self) -> GameStatus {
        self.status
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
            GameStatus::Finished { .. } => self.status,
            // Game is ongoing
            GameStatus::Playing { player, turns } => {
                // Check if game is finished
                if lut::PLAYER2_STARTING_POSITIONS.iter().all(|idx| {
                    self.board
                        .get(idx)
                        .expect("Invalid index in Player 2 starting positions")
                        == &Some(Player::Player1)
                }) {
                    // Player 1 has moved all pieces to the opponent's home row
                    GameStatus::Finished {
                        winner: Player::Player1,
                        turns: turns + 1,
                    }
                } else if lut::PLAYER1_STARTING_POSITIONS.iter().all(|idx| {
                    self.board
                        .get(idx)
                        .expect("Invalid index in Player 1 starting positions")
                        == &Some(Player::Player2)
                }) {
                    // Player 2 has moved all pieces to the opponent's home row
                    GameStatus::Finished {
                        winner: Player::Player2,
                        turns: turns + 1,
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

    pub fn iter_available_moves(&self) -> impl Iterator<Item = MovementFull> {
        match &self.status {
            GameStatus::Finished { .. } => todo!(),
            GameStatus::Playing { player, .. } => self.board.iter_player_movements(player),
        }
    }

    pub fn apply_movement(&mut self, movement: &MovementFull) -> Result<GameStatus, GameError> {
        match self.status {
            GameStatus::Finished { .. } => Err(GameError::GameFinished),
            GameStatus::Playing {
                player: current_player,
                ..
            } => {
                // Check if the movement is made by the current player
                let indices = movement.get_indices();
                if self
                    .board
                    .get(&indices.from)
                    .map_err(|InvalidBoardIndex(idx)| {
                        GameError::Movement(MovementError::InvalidIndex(idx))
                    })?
                    .as_ref()
                    != Some(&current_player)
                {
                    return Err(GameError::OutOfTurn);
                }

                // Check if the movement is valid
                self.board
                    .validate_movement(movement)
                    .map_err(GameError::Movement)?;

                // Apply the movement to the board
                self.board.unsafe_apply_movement(&indices);

                // Update game status
                self.status = self.next_status();

                Ok(self.status)
            }
        }
    }

    pub fn unsafe_apply_movement(&mut self, indices: &Movement) -> GameStatus {
        // Unsafe apply movement without checking for errors
        self.board.unsafe_apply_movement(indices);

        // Update game status
        self.status = self.next_status();

        self.status
    }
}
