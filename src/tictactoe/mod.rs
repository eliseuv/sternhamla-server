//! Tic-Tac-Toe
//! Basic implementation of a very simple game in order to facilitate experimentation on architecture

use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Player {
    Nought,
    Cross,
}

impl Player {
    pub fn opposite(&self) -> Self {
        match self {
            Player::Nought => Player::Cross,
            Player::Cross => Player::Nought,
        }
    }
}

impl Display for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Player::Nought => write!(f, "⭕"),
            Player::Cross => write!(f, "❌"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board([[Option<Player>; 3]; 3]);

impl Display for Board {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for row in &self.0 {
            for cell in row {
                match cell {
                    Some(player) => write!(f, "{player} ")?,
                    None => write!(f, "⬜ ")?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

pub fn all_equal<T: Copy + PartialEq>(arr: &[T]) -> Option<T> {
    let mut it = arr.iter();
    let eq = it.next()?;
    if it.all(|x| x == eq) { Some(*eq) } else { None }
}

impl Board {
    /// New empty board
    pub fn new() -> Self {
        Self([[None; 3]; 3])
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GameResult {
    Victory(Player),
    Draw,
}

impl Board {
    // Checks the board for a win or draw condition
    fn check(&self) -> Option<GameResult> {
        // Check rows
        for row in 0..3 {
            if let Some(Some(player)) = all_equal(&self.0[row]) {
                return Some(GameResult::Victory(player));
            }
        }

        // Check columns
        for col in 0..3 {
            if let Some(Some(player)) = all_equal(&[self.0[0][col], self.0[1][col], self.0[2][col]])
            {
                return Some(GameResult::Victory(player));
            }
        }

        // Check main diagonal
        if let Some(Some(player)) = all_equal(&[self.0[0][0], self.0[1][1], self.0[2][2]]) {
            return Some(GameResult::Victory(player));
        }

        // Check second diagonal
        if let Some(Some(player)) = all_equal(&[self.0[0][2], self.0[1][1], self.0[2][0]]) {
            return Some(GameResult::Victory(player));
        }

        // Check for draw
        if self
            .0
            .iter()
            .all(|row| row.iter().all(|&cell| cell.is_some()))
        {
            return Some(GameResult::Draw);
        }

        None
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GameStatus {
    Playing(Player),
    Finished(GameResult),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TicTacToeError {
    OutOfBounds([usize; 2]),
    OccupiedCell([usize; 2]),
    GameFinished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicTacToeGame {
    board: Board,
    status: GameStatus,
}

impl TicTacToeGame {
    pub fn new() -> Self {
        Self {
            // Empty board
            board: Board::new(),
            // Nought starts the game
            status: GameStatus::Playing(Player::Nought),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn status(&self) -> &GameStatus {
        &self.status
    }

    pub fn make_move(
        &mut self,
        row: usize,
        col: usize,
    ) -> Result<Option<GameResult>, TicTacToeError> {
        if let GameStatus::Playing(current_player) = self.status {
            // Check bounds
            if row >= 3 || col >= 3 {
                return Err(TicTacToeError::OutOfBounds([row, col]));
            }
            // Check occupancy
            if self.board.0[row][col].is_some() {
                return Err(TicTacToeError::OccupiedCell([row, col]));
            }
            // Make the move
            self.board.0[row][col] = Some(current_player);
            // Check game
            if let Some(result) = self.board.check() {
                self.status = GameStatus::Finished(result);
                Ok(Some(result))
            } else {
                self.status = GameStatus::Playing(current_player.opposite());
                Ok(None)
            }
        } else {
            Err(TicTacToeError::GameFinished)
        }
    }
}

impl Default for TicTacToeGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for TicTacToeGame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.board)?;
        match self.status {
            GameStatus::Playing(player) => write!(f, "Current player: {player}"),
            GameStatus::Finished(GameResult::Victory(player)) => {
                write!(f, "Game finished! {player} wins!")
            }
            GameStatus::Finished(GameResult::Draw) => write!(f, "Game finished! It's a draw!"),
        }
    }
}
