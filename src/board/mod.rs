use std::{
    fmt::Display,
    ops::{Index, IndexMut},
};
use thiserror::Error;

use crate::piece::Piece;

/// Board indices look up tables
mod lut;

/// Index in the lattice
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HexIndex(pub [usize; 2]);

/// Directions in a hexagonal lattice
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexDirection {
        NW,  NE,

    W,            E,

        SW,  SE,
}

const HEX_DIRECTIONS: [HexDirection; 6] = [
    HexDirection::NW,
    HexDirection::NE,
    HexDirection::E,
    HexDirection::SE,
    HexDirection::SW,
    HexDirection::W,
];

impl HexIndex {
    fn next_nw(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i.checked_sub(1)?, j]))
    }

    fn next_ne(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i.checked_sub(1)?, j + 1]))
    }

    fn next_e(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i, j + 1]))
    }

    fn next_se(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i + 1, j]))
    }

    fn next_sw(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i + 1, j.checked_sub(1)?]))
    }

    fn next_w(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i, j.checked_sub(1)?]))
    }
}

/// Board position
/// `None`: Invalid position (outside of the board)
/// `Some(None)`: Empty valid position
/// `Some(Some(piece))`: Position occupied by `piece`
type Position<T> = Option<Option<T>>;

/// Length of the Sternhalma board
const BOARD_LENGTH: usize = 17;

/// Lattice to store the state of the board
#[derive(Debug)]
struct BoardLattice<T>([Position<T>; BOARD_LENGTH * BOARD_LENGTH]);

impl<T> Index<HexIndex> for BoardLattice<T> {
    type Output = Position<T>;

    fn index(&self, index: HexIndex) -> &Self::Output {
        let [i, j] = index.0;
        assert!(j < BOARD_LENGTH, "Index out of bounds: [{i}, {j}]");
        &self.0[i * BOARD_LENGTH + j]
    }
}

impl<T> IndexMut<HexIndex> for BoardLattice<T> {
    fn index_mut(&mut self, index: HexIndex) -> &mut Self::Output {
        let [i, j] = index.0;
        assert!(j < BOARD_LENGTH, "Index out of bounds: [{i}, {j}]");
        &mut self.0[i * BOARD_LENGTH + j]
    }
}

/// Sternhalma board
#[derive(Debug)]
pub struct Board<T> {
    /// Lattice storing the state of the board.
    lattice: BoardLattice<T>,
}

impl<T> Board<T> {
    pub fn set_piece(&mut self, index: HexIndex, piece: T) -> Result<(), BoardIndexError> {
        match &mut self.lattice[index] {
            // Position is outside the board
            None => Err(BoardIndexError::Invalid(index)),
            // Position is inside the board
            Some(pos) => match pos {
                // Position is already occupied
                Some(_) => Err(BoardIndexError::Occupied(index)),
                // Position is empty, place the piece
                None => {
                    *pos = Some(piece);
                    Ok(())
                }
            },
        }
    }
}

/// Errors that can occur when accessing the board
#[derive(Debug, Error, Clone, Copy, Eq, PartialEq)]
pub enum BoardIndexError {
    #[error("Position {0:?} outside of the board")]
    Invalid(HexIndex),
    #[error("Position {0:?} is already occupied")]
    Occupied(HexIndex),
}

impl Display for Board<Piece> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for i in 0..BOARD_LENGTH {
            write!(f, "{}", " ".repeat(i))?;
            for j in 0..BOARD_LENGTH {
                match &self.lattice[HexIndex([i, j])] {
                    None => write!(f, "  ")?,
                    Some(None) => write!(f, "• ")?,
                    Some(Some(piece)) => write!(f, "{} ", piece.char())?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

impl<P> Board<P> {
    /// Creates an empty board with valid positions initialized
    pub(crate) fn empty() -> Self {
        let mut board = Self {
            lattice: BoardLattice([const { None }; BOARD_LENGTH * BOARD_LENGTH]),
        };
        for index in lut::VALID_POSITIONS.map(HexIndex) {
            board.lattice[index] = Some(None);
        }

        board
    }
}

impl<T> Default for Board<T> {
    /// Default board is an empty board
    fn default() -> Self {
        Self::empty()
    }
}

impl<T: Copy> Board<T> {
    /// Places the given piece at the specified positions on the board
    fn place_pieces(mut self, piece: T, indices: &[HexIndex]) -> Result<Self, BoardIndexError> {
        for &index in indices {
            match &mut self.lattice[index] {
                // Position is outside the board
                None => return Err(BoardIndexError::Invalid(index)),
                // Position is inside the board
                Some(pos) => match pos {
                    // Position is already occupied
                    Some(_) => return Err(BoardIndexError::Occupied(index)),
                    // Position is empty, place the piece
                    None => {
                        *pos = Some(piece);
                    }
                },
            }
        }
        Ok(self)
    }
}

impl Board<Piece> {
    /// Creates a new Sternhalma board with pieces placed in their starting positions
    pub fn new() -> Result<Self, BoardIndexError> {
        Self::empty()
            .place_pieces(
                Piece::Player1,
                &lut::PLAYER1_STARTING_POSITIONS.map(HexIndex),
            )?
            .place_pieces(
                Piece::Player2,
                &lut::PLAYER2_STARTING_POSITIONS.map(HexIndex),
            )
    }
}

enum Movement {
    Simple(HexDirection),
    Hop(HexDirection),
}

macro_rules! first_move_direction {
    ($board:ident, $index:ident, $next_fn:ident) => {
        $index
            // Check nearest neighbor
            .$next_fn()
            .and_then(|idx_nn| match &$board.lattice[idx_nn]? {
                // Position is empty, can move there
                None => Some(idx_nn),
                // Position is occupied, check if we can hop over it
                Some(_) => idx_nn
                    .$next_fn()
                    .and_then(|idx_nnn| match &$board.lattice[idx_nnn]? {
                        // Position is empty, can hop over the occupied position
                        None => Some(idx_nnn),
                        // Position is occupied, cannot hop
                        Some(_) => None,
                    }),
            })
    };
}

impl<T: Copy> Board<T> {
    /// Returns the possible moves for a piece at the given index
    // TODO: Hot part of the algorithm, must be optimized
    pub fn possible_first_moves(&self, index: HexIndex) -> [Option<HexIndex>; 6] {
        [
            // NW
            first_move_direction!(self, index, next_nw),
            // NE
            first_move_direction!(self, index, next_ne),
            // E
            first_move_direction!(self, index, next_e),
            // SE
            first_move_direction!(self, index, next_se),
            // SW
            first_move_direction!(self, index, next_sw),
            // W
            first_move_direction!(self, index, next_w),
        ]
    }

    // TODO: Ignore the direction the player just came from
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum GameState {
    Playing(Piece),
    Finished(Option<Piece>),
}
