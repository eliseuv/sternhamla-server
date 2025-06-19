//! Sternhalma
//!
#![feature(array_try_map, generic_const_exprs)]

use anyhow::{Context, Result};
use std::{
    fmt::Display,
    ops::{Index, IndexMut},
};
use thiserror::Error;

/// Board indices look up tables
mod lut;

/// Length of the Sternhalma board
const BOARD_LENGTH: usize = 17;

/// Axial index for the hexagonal lattice
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

impl HexIndex {
    pub(crate) fn next_nw(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i.checked_sub(1)?, j]))
    }

    pub(crate) fn next_ne(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i.checked_sub(1)?, {
            let j_next = j + 1;
            if j_next < BOARD_LENGTH {
                Some(j_next)
            } else {
                None
            }
        }?]))
    }

    pub(crate) fn next_e(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i, {
            let j_next = j + 1;
            if j_next < BOARD_LENGTH {
                Some(j_next)
            } else {
                None
            }
        }?]))
    }

    pub(crate) fn next_se(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([
            {
                let i_next = i + 1;
                if i_next < BOARD_LENGTH {
                    Some(i_next)
                } else {
                    None
                }
            }?,
            j,
        ]))
    }

    pub(crate) fn next_sw(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([
            {
                let i_next = i + 1;
                if i_next < BOARD_LENGTH {
                    Some(i_next)
                } else {
                    None
                }
            }?,
            j.checked_sub(1)?,
        ]))
    }

    pub(crate) fn next_w(&self) -> Option<Self> {
        let [i, j] = self.0;
        Some(HexIndex([i, j.checked_sub(1)?]))
    }
}

/// Board position
/// `None`: Invalid position (outside of the board)
/// `Some(None)`: Empty valid position
/// `Some(Some(piece))`: Position occupied by `piece`
type Position<T> = Option<Option<T>>;

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
    /// Sets a piece at the specified index on the board
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
#[derive(Debug, Error, Clone, Copy)]
pub enum BoardIndexError {
    #[error("Position {0:?} outside of the board")]
    Invalid(HexIndex),
    #[error("Position {0:?} is already occupied")]
    Occupied(HexIndex),
}

// TODO: Remove `Copy` bound
impl<T: Copy> Board<T> {
    /// Creates an empty board with valid positions initialized
    pub(crate) fn empty() -> Self {
        let mut board = Self {
            lattice: BoardLattice([None; BOARD_LENGTH * BOARD_LENGTH]),
        };
        for index in lut::VALID_POSITIONS.map(HexIndex) {
            board.lattice[index] = Some(None);
        }

        board
    }
}

impl<T: Copy> Default for Board<T> {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Piece {
    Player1,
    Player2,
}

impl Piece {
    /// Returns the character representation of the piece
    pub(crate) fn char(&self) -> char {
        match self {
            Self::Player1 => '🔵',
            Self::Player2 => '🔴',
        }
    }
}

impl Display for Board<Piece> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for i in 0..BOARD_LENGTH {
            write!(f, "{}", " ".repeat(i))?;
            for j in 0..BOARD_LENGTH {
                match &self.lattice[HexIndex([i, j])] {
                    None => write!(f, "󠀠󠀠󠀠󠀠   ")?,
                    Some(None) => write!(f, "⚫ ")?,
                    Some(Some(piece)) => write!(f, "{} ", piece.char())?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
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

#[derive(Debug, Clone, Copy)]
pub enum Movement {
    Move(HexDirection, HexIndex),
    Hop(HexDirection, HexIndex),
}

/// Expands to expression that evaluates to an optional first movement from the specified index in the given direction.
macro_rules! possible_first_move_direction {
    ($board:ident, $direction:ident, $index:ident, $next_fn:ident) => {
        $index
            // Check nearest neighbor
            .$next_fn()
            .and_then(|idx_nn| match &$board.lattice[idx_nn]? {
                // Position is empty, can move there
                None => Some(Movement::Move(HexDirection::$direction, idx_nn)),
                // Position is occupied, check if we can hop over it
                Some(_) => idx_nn
                    .$next_fn()
                    .and_then(|idx_nnn| match &$board.lattice[idx_nnn]? {
                        // Position is occupied, cannot hop over to it
                        Some(_) => None,
                        // Position is empty, can hop over to it
                        None => Some(Movement::Hop(HexDirection::$direction, idx_nnn)),
                    }),
            })
    };
}

/// Expands to expression that evaluates to an optional hop from the specified index in the given direction.
macro_rules! possible_hop_direction {
    ($board:ident, $direction:ident, $index:ident, $next_fn:ident) => {
        $index
            // Check nearest neighbor
            .$next_fn()
            .and_then(|idx_nn| match &$board.lattice[idx_nn]? {
                // Position is empty, cannot hop over
                None => None,
                // Position is occupied, check if we can hop over it
                Some(_) => idx_nn
                    .$next_fn()
                    .and_then(|idx_nnn| match &$board.lattice[idx_nnn]? {
                        // Position is occupied, cannot hop over to it
                        Some(_) => None,
                        // Position is empty, can hop over to it
                        None => Some(Movement::Hop(HexDirection::$direction, idx_nnn)),
                    }),
            })
    };
}

impl<T: Copy> Board<T> {
    /// Returns the possible moves for a piece at the given index
    fn possible_first_movements(&self, index: HexIndex) -> [Option<Movement>; 6] {
        [
            // NW
            possible_first_move_direction!(self, NW, index, next_nw),
            // NE
            possible_first_move_direction!(self, NE, index, next_ne),
            // E
            possible_first_move_direction!(self, E, index, next_e),
            // SE
            possible_first_move_direction!(self, SE, index, next_se),
            // SW
            possible_first_move_direction!(self, SW, index, next_sw),
            // W
            possible_first_move_direction!(self, W, index, next_w),
        ]
    }

    /// Returns the possible hops for a piece at the given index, given the last direction of movement
    fn possible_hops(
        &self,
        index: HexIndex,
        last_direction: HexDirection,
    ) -> [Option<Movement>; 5] {
        match last_direction {
            // NW
            HexDirection::NW => [
                // NE
                possible_hop_direction!(self, NE, index, next_ne),
                // E
                possible_hop_direction!(self, E, index, next_e),
                // SE
                possible_hop_direction!(self, SE, index, next_se),
                // SW
                possible_hop_direction!(self, SW, index, next_sw),
                // W
                possible_hop_direction!(self, W, index, next_w),
            ],
            // NE
            HexDirection::NE => [
                // NW
                possible_hop_direction!(self, NW, index, next_nw),
                // E
                possible_hop_direction!(self, E, index, next_e),
                // SE
                possible_hop_direction!(self, SE, index, next_se),
                // SW
                possible_hop_direction!(self, SW, index, next_sw),
                // W
                possible_hop_direction!(self, W, index, next_w),
            ],
            // E
            HexDirection::E => [
                // NW
                possible_hop_direction!(self, NW, index, next_nw),
                // NE
                possible_hop_direction!(self, NE, index, next_ne),
                // SE
                possible_hop_direction!(self, SE, index, next_se),
                // SW
                possible_hop_direction!(self, SW, index, next_sw),
                // W
                possible_hop_direction!(self, W, index, next_w),
            ],
            // SE
            HexDirection::SE => [
                // NW
                possible_hop_direction!(self, NW, index, next_nw),
                // NE
                possible_hop_direction!(self, NE, index, next_ne),
                // E
                possible_hop_direction!(self, E, index, next_e),
                // SW
                possible_hop_direction!(self, SW, index, next_sw),
                // W
                possible_hop_direction!(self, W, index, next_w),
            ],
            // SW
            HexDirection::SW => [
                // NW
                possible_hop_direction!(self, NW, index, next_nw),
                // NE
                possible_hop_direction!(self, NE, index, next_ne),
                // E
                possible_hop_direction!(self, E, index, next_e),
                // SE
                possible_hop_direction!(self, SE, index, next_se),
                // W
                possible_hop_direction!(self, W, index, next_w),
            ],
            // W
            HexDirection::W => [
                // NW
                possible_hop_direction!(self, NW, index, next_nw),
                // NE
                possible_hop_direction!(self, NE, index, next_ne),
                // E
                possible_hop_direction!(self, E, index, next_e),
                // SE
                possible_hop_direction!(self, SE, index, next_se),
                // SW
                possible_hop_direction!(self, SW, index, next_sw),
            ],
        }
    }
}

impl Board<Piece> {
    pub fn list_possible_first_movements(&self, piece: Piece) -> Vec<(HexIndex, Vec<Movement>)> {
        self.lattice
            .0
            .iter()
            .enumerate()
            .filter_map(|(i, &pos)| {
                // If the position is valid and the piece matches, check for possible movements
                if pos?? == piece {
                    let index = HexIndex([i / BOARD_LENGTH, i % BOARD_LENGTH]);
                    let movements = self
                        .possible_first_movements(index)
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>();
                    if !movements.is_empty() {
                        Some((index, movements))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn apply_movement(
        &mut self,
        index: HexIndex,
        movement: Movement,
    ) -> Result<(), BoardIndexError> {
        match movement {
            Movement::Move(_, target_index) => {
                let piece = self.lattice[index];
                self.lattice[index] = Some(None);
                self.lattice[target_index] = piece;
            }
            Movement::Hop(_, target_index) => {
                let piece = self.lattice[index];
                self.lattice[index] = Some(None);
                self.lattice[target_index] = piece;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum GameState {
    /// Which piece is currently playing and the last movement made
    Playing(Piece, Option<Movement>),
    /// Game finished
    Finished(Option<Piece>),
}

#[derive(Debug)]
pub struct Game {
    board: Board<Piece>,
    state: GameState,
}

impl Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.board)?;
        match self.state {
            GameState::Playing(piece, _) => write!(f, "Current player: {}", piece.char()),
            GameState::Finished(winner) => {
                write!(f, "Game finished!")?;
                match winner {
                    Some(piece) => write!(f, "Winner: {}", piece.char()),
                    None => write!(f, "It's a draw!"),
                }
            }
        }
    }
}

impl Game {
    pub fn new() -> Result<Self> {
        Ok(Self {
            board: Board::new().context("Failed to initialize board")?,
            state: GameState::Playing(Piece::Player1, None),
        })
    }
}
