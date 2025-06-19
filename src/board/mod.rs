use ndarray::Array2;
use std::{
    fmt::Display,
    ops::{Index, IndexMut},
};
use thiserror::Error;

use crate::piece::Piece;

/// Sternhalma board
#[derive(Debug)]
pub struct Board<P> {
    /// Lattice storing the state of the board.
    /// `None`: Position outside of the board
    /// `Some(None)`: Position inside the board but empty
    /// `Some(Some(piece))`: Position inside the board and occupied by `piece`
    lattice: Array2<Option<Option<P>>>,
}

/// Index of a position on the board
pub type BoardIndex = [usize; 2];

impl<P> Index<BoardIndex> for Board<P> {
    type Output = Option<Option<P>>;

    fn index(&self, index: BoardIndex) -> &Self::Output {
        &self.lattice[index]
    }
}

impl<P> IndexMut<BoardIndex> for Board<P> {
    fn index_mut(&mut self, index: BoardIndex) -> &mut Self::Output {
        &mut self.lattice[index]
    }
}

/// Errors that can occur when accessing the board
#[derive(Debug, Error, Clone, Copy, Eq, PartialEq)]
pub enum BoardIndexError {
    #[error("Position {0:?} outside of the board")]
    Invalid(BoardIndex),
    #[error("Position {0:?} is already occupied")]
    Occupied(BoardIndex),
}

/// Board indices look up tables
pub(crate) mod lut;

impl<P> Board<P> {
    /// Creates an empty board with valid positions initialized
    pub(crate) fn empty() -> Self {
        let mut lattice = Array2::from_shape_simple_fn((17, 17), || None);
        for idx in lut::VALID_POSITIONS {
            lattice[idx] = Some(None);
        }

        Self { lattice }
    }
}

impl<P> Default for Board<P> {
    /// Default board is an empty board
    fn default() -> Self {
        Self::empty()
    }
}

impl<P: Copy> Board<P> {
    /// Places the given piece at the specified positions on the board
    fn place_pieces(&mut self, piece: P, indices: &[[usize; 2]]) -> Result<(), BoardIndexError> {
        for &idx in indices {
            match &mut self[idx] {
                // Position is outside the board
                None => return Err(BoardIndexError::Invalid(idx)),
                // Position is inside the board
                Some(pos) => match pos {
                    // Position is already occupied
                    Some(_) => return Err(BoardIndexError::Occupied(idx)),
                    // Position is empty, place the piece
                    None => {
                        *pos = Some(piece);
                    }
                },
            }
        }
        Ok(())
    }
}

impl Board<Piece> {
    pub fn new() -> Self {
        let mut board = Self::empty();

        board
            .place_pieces(Piece::Player1, &lut::PLAYER1_STARTING_POSITIONS)
            .expect("Error in indices from LUT");
        board
            .place_pieces(Piece::Player2, &lut::PLAYER2_STARTING_POSITIONS)
            .expect("Error in indices from LUT");

        board
    }
}

impl Display for Board<Piece> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for i in 0..17 {
            write!(f, "{}", " ".repeat(i))?;
            for j in 0..17 {
                match &self.lattice[[i, j]] {
                    None => write!(f, "  ")?,
                    Some(None) => write!(f, "• ")?,
                    // TODO: Padding
                    Some(Some(piece)) => write!(f, "{} ", piece.char())?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

// #[derive(Clone, Copy, Debug, Eq, PartialEq)]
// pub(crate) enum GameState {
//     Progress(Piece),
//     Finished(Option<Piece>),
// }
