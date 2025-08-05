//! Tic-Tac-Toe
//! Basic implementation of a very simple game in order to facilitate experimentation on architecture

use std::{
    fmt::Display,
    ops::{Index, IndexMut},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Player {
    #[serde(rename = "x")]
    Cross,
    #[serde(rename = "o")]
    Nought,
}

/// List of all players
pub const PLAYERS_LIST: [Player; 2] = [Player::Cross, Player::Nought];

impl Player {
    pub const fn variants() -> [Player; 2] {
        [Player::Cross, Player::Nought]
    }

    pub const fn count() -> usize {
        Self::variants().len()
    }

    /// Get the opponent of a given player
    pub const fn opposite(&self) -> Self {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position(pub [usize; 2]);

impl Position {
    pub fn is_valid(&self) -> bool {
        let [i, j] = self.0;
        (i <= 3) && (j <= 3)
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

impl Index<Position> for Board {
    type Output = Option<Player>;

    fn index(&self, index: Position) -> &Self::Output {
        let [i, j] = index.0;
        &self.0[i][j]
    }
}

impl IndexMut<Position> for Board {
    fn index_mut(&mut self, index: Position) -> &mut Self::Output {
        let [i, j] = index.0;
        &mut self.0[i][j]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum GameResult {
    Victory { player: Player },
    Draw,
}

impl Board {
    // Checks the board for a win or draw condition
    fn check(&self) -> Option<GameResult> {
        // Check rows
        for row in 0..3 {
            if let Some(Some(player)) = all_equal(&self.0[row]) {
                return Some(GameResult::Victory { player });
            }
        }

        // Check columns
        for col in 0..3 {
            if let Some(Some(player)) = all_equal(&[self.0[0][col], self.0[1][col], self.0[2][col]])
            {
                return Some(GameResult::Victory { player });
            }
        }

        // Check main diagonal
        if let Some(Some(player)) = all_equal(&[self.0[0][0], self.0[1][1], self.0[2][2]]) {
            return Some(GameResult::Victory { player });
        }

        // Check second diagonal
        if let Some(Some(player)) = all_equal(&[self.0[0][2], self.0[1][1], self.0[2][0]]) {
            return Some(GameResult::Victory { player });
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
#[serde(rename_all = "snake_case")]
pub enum GameError {
    OutOfBounds(Position),
    OccupiedCell(Position),
    GameFinished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    board: Board,
    status: GameStatus,
    history: Vec<Position>, // History of moves made
}

impl Game {
    pub fn new() -> Self {
        Self {
            // Empty board
            board: Board::new(),
            // Cross starts
            status: GameStatus::Playing(Player::Cross),
            // History of moves
            history: Vec::new(),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    pub fn available_moves(&self) -> Vec<Position> {
        self.board
            .0
            .iter()
            .enumerate()
            .flat_map(|(row, cols)| {
                cols.iter().enumerate().filter_map(move |(col, &cell)| {
                    if cell.is_none() {
                        Some(Position([row, col]))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub fn make_move(&mut self, position: Position) -> Result<GameStatus, GameError> {
        match self.status {
            GameStatus::Playing(player) => {
                // Check bounds
                if !position.is_valid() {
                    return Err(GameError::OutOfBounds(position));
                }
                // Check occupancy
                if self.board[position].is_some() {
                    return Err(GameError::OccupiedCell(position));
                }
                // Make the move
                self.board[position] = Some(player);
                self.history.push(position);
                // Update game status
                self.status = if let Some(result) = self.board.check() {
                    GameStatus::Finished(result)
                } else {
                    GameStatus::Playing(player.opposite())
                };
                Ok(self.status)
            }
            GameStatus::Finished(_) => Err(GameError::GameFinished),
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.board)?;
        match self.status {
            GameStatus::Playing(player) => write!(f, "Current player: {player}"),
            GameStatus::Finished(GameResult::Victory { player }) => {
                write!(f, "Game finished! {player} wins!")
            }
            GameStatus::Finished(GameResult::Draw) => write!(f, "Game finished! It's a draw!"),
        }
    }
}
