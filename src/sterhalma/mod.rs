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
    Playing(Player),
    /// Game finished
    Finished(Player),
}

#[derive(Debug)]
pub struct Game {
    /// Board state
    board: Board<Player>,
    /// Game status
    status: GameStatus,
    /// Count of turns played
    turns: usize,
}

impl Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.board)?;
        match self.status {
            GameStatus::Playing(player) => {
                write!(f, "Turn: {turn}\tPlaying: {player}", turn = self.turns)?
            }
            GameStatus::Finished(winner) => {
                write!(f, "Game finished!")?;
                write!(f, "Winner: {winner}")?;
            }
        }
        Ok(())
    }
}

impl Game {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            status: GameStatus::Playing(Player::Player1),
            turns: 0,
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
            GameStatus::Finished(_) => self.status,
            // Game is ongoing
            GameStatus::Playing(player) => {
                // Check if game is finished
                if lut::PLAYER2_STARTING_POSITIONS.iter().all(|idx| {
                    self.board
                        .get(idx)
                        .expect("Player 2 starting position indices are valid")
                        == &Some(Player::Player1)
                }) {
                    // Player 1 has moved all pieces to the opponent's home row
                    GameStatus::Finished(Player::Player1)
                } else if lut::PLAYER1_STARTING_POSITIONS.iter().all(|idx| {
                    self.board
                        .get(idx)
                        .expect("Player 1 starting position indices are valid")
                        == &Some(Player::Player2)
                }) {
                    // Player 2 has moved all pieces to the opponent's home row
                    GameStatus::Finished(Player::Player2)
                } else {
                    // Game is still ongoing, switch to the opponent
                    GameStatus::Playing(player.opponent())
                }
            }
        }
    }

    pub fn iter_available_moves(&self) -> impl Iterator<Item = MovementFull> {
        match &self.status {
            GameStatus::Finished(_) => todo!(),
            GameStatus::Playing(player) => self.board.iter_player_movements(player),
        }
    }

    pub fn apply_movement(&mut self, movement: &MovementFull) -> Result<GameStatus, GameError> {
        match self.status {
            GameStatus::Finished(_) => Err(GameError::GameFinished),
            GameStatus::Playing(current_player) => {
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

                // Increment the turn count
                self.turns += 1;

                Ok(self.status)
            }
        }
    }

    pub fn unsafe_apply_movement(&mut self, indices: &Movement) -> GameStatus {
        // Unsafe apply movement without checking for errors
        self.board.unsafe_apply_movement(indices);

        // Update game status
        self.status = self.next_status();

        // Increment the turn count
        self.turns += 1;

        self.status
    }
}
