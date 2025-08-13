use std::fmt::{Debug, Display};

use anyhow::Result;

use crate::sterhalma::board::{
    Board, HexIdx, InvalidBoardIndex, lut,
    movement::{Movement, MovementError, MovementIndices},
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

impl Display for GameStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameStatus::Playing { player, turns } => {
                write!(f, "Playing: {player}\tTurn: {turns}")
            }
            GameStatus::Finished { winner, turns } => {
                write!(f, "Winner: {winner}\tTotal turns: {turns}")
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
    history: Vec<[HexIdx; 2]>,
}

impl Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
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

    pub fn iter_available_moves(&self) -> impl Iterator<Item = Movement> {
        match &self.status {
            GameStatus::Finished { .. } => todo!(),
            GameStatus::Playing { player, .. } => self.board.iter_player_movements(player),
        }
    }

    pub fn apply_movement(&mut self, movement: &Movement) -> Result<GameStatus, GameError> {
        match self.status {
            GameStatus::Finished { .. } => Err(GameError::GameFinished),
            GameStatus::Playing {
                player: current_player,
                ..
            } => {
                // Check if the movement is made by the current player
                let [from, to] = movement.into();
                if self
                    .board
                    .get(&from)
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
                self.unsafe_apply_movement(&[from, to]);

                Ok(self.status)
            }
        }
    }

    pub fn unsafe_apply_movement(&mut self, indices: &MovementIndices) -> GameStatus {
        // Unsafe apply movement without checking for errors
        self.board.unsafe_apply_movement(indices);

        // Update game history
        self.history.push(*indices);

        // Update game status
        self.status = self.next_status();

        self.status
    }
}
