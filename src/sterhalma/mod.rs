use anyhow::{Context, Result};
use std::{
    fmt::{Debug, Display},
    ops::{Index, IndexMut},
};
use thiserror::Error;

/// Length of the Sternhalma board
const BOARD_LENGTH: usize = 17;

/// Board position
/// `None`: Invalid position (outside of the board)
/// `Some(None)`: Empty valid position
/// `Some(Some(piece))`: Position occupied by `piece`
type Position<T> = Option<Option<T>>;

/// Sternhalma board
#[derive(Debug)]
pub struct Board<T>([Position<T>; BOARD_LENGTH * BOARD_LENGTH]);

/// Axial index for the hexagonal lattice
pub(crate) type HexIdx = [usize; 2];

impl<T> Index<HexIdx> for Board<T> {
    type Output = Position<T>;

    fn index(&self, index: HexIdx) -> &Self::Output {
        let [i, j] = index;
        debug_assert!(j < BOARD_LENGTH, "Index out of bounds: [{i}, {j}]");
        &self.0[i * BOARD_LENGTH + j]
    }
}

impl<T> IndexMut<HexIdx> for Board<T> {
    fn index_mut(&mut self, index: HexIdx) -> &mut Self::Output {
        let [i, j] = index;
        debug_assert!(j < BOARD_LENGTH, "Index out of bounds: [{i}, {j}]");
        &mut self.0[i * BOARD_LENGTH + j]
    }
}

/// Board indices look up tables
mod lut;

/// Board initialization
impl<T> Board<T> {
    /// Creates an empty board with valid positions initialized
    pub fn empty() -> Self {
        let mut board = Board(std::array::from_fn(|_| None));

        for index in lut::VALID_POSITIONS {
            board[index] = Some(None);
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

/// Errors that can occur when accessing the board
#[derive(Debug, Error, Clone, Copy)]
pub enum BoardIndexError {
    #[error("Position {0:?} outside of the board")]
    Invalid(HexIdx),
    #[error("Position {0:?} is empty")]
    Empty(HexIdx),
    #[error("Position {0:?} is already occupied")]
    Occupied(HexIdx),
}

impl<T> Board<T> {
    /// Returns a reference to the piece at the specified index on the board
    pub fn get(&self, idx: HexIdx) -> Result<&Option<T>, BoardIndexError> {
        self[idx].as_ref().ok_or(BoardIndexError::Invalid(idx))
    }

    pub fn get_mut(&mut self, idx: HexIdx) -> Result<&mut Option<T>, BoardIndexError> {
        self[idx].as_mut().ok_or(BoardIndexError::Invalid(idx))
    }
}

/// Directions in a hexagonal lattice
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexDirection {
       NW,  NE,

    W,          E,

       SW,  SE,
}

impl<T: Copy> Board<T> {
    /// Nearest neighbor NW
    fn next_nw(&self, [i, j]: HexIdx) -> Option<(HexIdx, Option<T>)> {
        let idx = [i.checked_sub(1)?, j];
        Some((idx, self[idx]?))
    }

    /// Nearest neighbor NE
    fn next_ne(&self, [i, j]: HexIdx) -> Option<(HexIdx, Option<T>)> {
        let idx = [
            i.checked_sub(1)?,
            if j + 1 < BOARD_LENGTH {
                Some(j + 1)
            } else {
                None
            }?,
        ];
        Some((idx, self[idx]?))
    }

    /// Nearest neighbor E
    fn next_e(&self, [i, j]: HexIdx) -> Option<(HexIdx, Option<T>)> {
        let idx = [
            i,
            if j + 1 < BOARD_LENGTH {
                Some(j + 1)
            } else {
                None
            }?,
        ];
        Some((idx, self[idx]?))
    }

    /// Nearest neighbor SE
    fn next_se(&self, [i, j]: HexIdx) -> Option<(HexIdx, Option<T>)> {
        let idx = [
            if i + 1 < BOARD_LENGTH {
                Some(i + 1)
            } else {
                None
            }?,
            j,
        ];
        Some((idx, self[idx]?))
    }

    /// Nearest neighbor SW
    fn next_sw(&self, [i, j]: HexIdx) -> Option<(HexIdx, Option<T>)> {
        let idx = [
            if i + 1 < BOARD_LENGTH {
                Some(i + 1)
            } else {
                None
            }?,
            j.checked_sub(1)?,
        ];
        Some((idx, self[idx]?))
    }

    /// Nearest neighbor W
    fn next_w(&self, [i, j]: HexIdx) -> Option<(HexIdx, Option<T>)> {
        let idx = [i, j.checked_sub(1)?];
        Some((idx, self[idx]?))
    }
}

impl<T> Board<T> {
    /// Sets a piece at the specified index on the board
    pub fn set_piece(&mut self, idx: HexIdx, piece: T) -> Result<(), BoardIndexError> {
        let pos = self.get_mut(idx)?;
        match &pos {
            // Position is already occupied
            Some(_) => Err(BoardIndexError::Occupied(idx)),
            // Position is empty, place the piece
            None => {
                *pos = Some(piece);
                Ok(())
            }
        }
    }
}

impl<T: Copy> Board<T> {
    /// Places the given piece at the specified positions on the board
    pub fn place_pieces(&mut self, indices: &[HexIdx], piece: T) -> Result<(), BoardIndexError> {
        for &idx in indices {
            self.set_piece(idx, piece)?;
        }
        Ok(())
    }

    /// Builder
    pub fn with_pieces(mut self, indices: &[HexIdx], piece: T) -> Result<Self, BoardIndexError> {
        self.place_pieces(indices, piece)?;
        Ok(self)
    }
}

/// Possible movements
/// - Move to an adjacent free position
/// - Hop over an occupied position to a free position
#[derive(Debug, Clone, Copy)]
pub enum Movement {
    Move(HexIdx, HexIdx),
    Hop(HexIdx, HexIdx),
}

/// Expands to expression that evaluates to an optional first movement from the specified index in the given direction.
macro_rules! first_movement_from_index_to_direction {
    ($board:ident, $index:ident, $direction:ident, $next_fn:ident) => {
        // Check adjacent position
        $board.$next_fn($index).and_then(|(idx, p)| match p {
            // Free position, can move there
            None => Some(Movement::Move($index, idx)),
            // Occupied position, check if we can hop over it
            Some(_) => $board.$next_fn(idx).and_then(|(idx, p)| match p {
                // Free position, can hop over to it
                None => Some(Movement::Hop($index, idx)),
                // Occupied position, cannot hop over
                Some(_) => None,
            }),
        })
    };
}

impl<T: Copy> Board<T> {
    /// Returns the possible moves for a piece at the given index
    fn first_movements_from(&self, index: HexIdx) -> [Option<Movement>; 6] {
        [
            // NW
            first_movement_from_index_to_direction!(self, index, NW, next_nw),
            // NE
            first_movement_from_index_to_direction!(self, index, NE, next_ne),
            // E
            first_movement_from_index_to_direction!(self, index, E, next_e),
            // SE
            first_movement_from_index_to_direction!(self, index, SE, next_se),
            // SW
            first_movement_from_index_to_direction!(self, index, SW, next_sw),
            // W
            first_movement_from_index_to_direction!(self, index, W, next_w),
        ]
    }
}

/// Expands to expression that evaluates to an optional hop from the specified index in the given direction.
macro_rules! hop_from_index_to_direction {
    ($board:ident, $index:ident, $direction:ident, $next_fn:ident) => {
        // Check adjacent position
        $board.$next_fn($index).and_then(|(idx, p)| match p {
            // Free position, cannot hop over it
            None => None,
            // Occupied position, check if we can hop over it
            Some(_) => $board.$next_fn(idx).and_then(|(idx, p)| match p {
                // Free position, can hop over to it
                None => Some(Movement::Hop($index, idx)),
                // Occupied position, cannot hop over
                Some(_) => None,
            }),
        })
    };
}

impl<T: Copy> Board<T> {
    /// Returns the possible hops for a piece at the given index, given the last direction of movement
    fn available_hops_from(
        &self,
        index: HexIdx,
        last_direction: HexDirection,
    ) -> [Option<Movement>; 5] {
        match last_direction {
            // NW
            HexDirection::NW => [
                // NE
                hop_from_index_to_direction!(self, index, NE, next_ne),
                // E
                hop_from_index_to_direction!(self, index, E, next_e),
                // SE
                hop_from_index_to_direction!(self, index, SE, next_se),
                // SW
                hop_from_index_to_direction!(self, index, SW, next_sw),
                // W
                hop_from_index_to_direction!(self, index, W, next_w),
            ],
            // NE
            HexDirection::NE => [
                // NW
                hop_from_index_to_direction!(self, index, NW, next_nw),
                // E
                hop_from_index_to_direction!(self, index, E, next_e),
                // SE
                hop_from_index_to_direction!(self, index, SE, next_se),
                // SW
                hop_from_index_to_direction!(self, index, SW, next_sw),
                // W
                hop_from_index_to_direction!(self, index, W, next_w),
            ],
            // E
            HexDirection::E => [
                // NW
                hop_from_index_to_direction!(self, index, NW, next_nw),
                // NE
                hop_from_index_to_direction!(self, index, NE, next_ne),
                // SE
                hop_from_index_to_direction!(self, index, SE, next_se),
                // SW
                hop_from_index_to_direction!(self, index, SW, next_sw),
                // W
                hop_from_index_to_direction!(self, index, W, next_w),
            ],
            // SE
            HexDirection::SE => [
                // NW
                hop_from_index_to_direction!(self, index, NW, next_nw),
                // NE
                hop_from_index_to_direction!(self, index, NE, next_ne),
                // E
                hop_from_index_to_direction!(self, index, E, next_e),
                // SW
                hop_from_index_to_direction!(self, index, SW, next_sw),
                // W
                hop_from_index_to_direction!(self, index, W, next_w),
            ],
            // SW
            HexDirection::SW => [
                // NW
                hop_from_index_to_direction!(self, index, NW, next_nw),
                // NE
                hop_from_index_to_direction!(self, index, NE, next_ne),
                // E
                hop_from_index_to_direction!(self, index, E, next_e),
                // SE
                hop_from_index_to_direction!(self, index, SE, next_se),
                // W
                hop_from_index_to_direction!(self, index, W, next_w),
            ],
            // W
            HexDirection::W => [
                // NW
                hop_from_index_to_direction!(self, index, NW, next_nw),
                // NE
                hop_from_index_to_direction!(self, index, NE, next_ne),
                // E
                hop_from_index_to_direction!(self, index, E, next_e),
                // SE
                hop_from_index_to_direction!(self, index, SE, next_se),
                // SW
                hop_from_index_to_direction!(self, index, SW, next_sw),
            ],
        }
    }
}

impl<T: Copy + PartialEq> Board<T> {
    /// Iterate on the indices of the pieces of a given player
    pub fn iter_indices(&self, player: T) -> impl Iterator<Item = HexIdx> {
        self.0.iter().enumerate().filter_map(move |(i, &pos)| {
            if pos?? == player {
                let idx = [i / BOARD_LENGTH, i % BOARD_LENGTH];
                Some(idx)
            } else {
                None
            }
        })
    }

    pub fn possible_first_moves(&self, player: T) -> impl Iterator<Item = (HexIdx, Vec<Movement>)> {
        self.iter_indices(player).filter_map(|idx| {
            let movements = self
                .first_movements_from(idx)
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            if movements.is_empty() {
                None
            } else {
                Some((idx, movements))
            }
        })
    }
}

impl<T> Board<T> {
    pub fn apply_movement(&mut self, movement: Movement) -> Result<(), BoardIndexError> {
        let (idx, idx_new) = match movement {
            Movement::Move(idx, idx_new) => (idx, idx_new),
            Movement::Hop(idx, idx_new) => (idx, idx_new),
        };

        let piece = self
            .get_mut(idx)?
            .take()
            .ok_or(BoardIndexError::Empty(idx))?;
        let target_pos = self.get_mut(idx_new)?;
        match target_pos {
            Some(_) => return Err(BoardIndexError::Occupied(idx_new)),
            None => *target_pos = Some(piece),
        }

        Ok(())
    }
}

/// Sternhalma board with pieces
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

impl Board<Piece> {
    /// Creates a new Sternhalma board with pieces placed in their starting positions
    pub fn two_players() -> Result<Self, BoardIndexError> {
        Self::empty()
            .with_pieces(&lut::PLAYER1_STARTING_POSITIONS, Piece::Player1)?
            .with_pieces(&lut::PLAYER2_STARTING_POSITIONS, Piece::Player2)
    }
}

/// Board display
impl Display for Board<Piece> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for i in 0..BOARD_LENGTH {
            write!(f, "{}", " ".repeat(i))?;
            for j in 0..BOARD_LENGTH {
                match &self[[i, j]] {
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
            board: Board::two_players().context("Failed to initialize board")?,
            state: GameState::Playing(Piece::Player1, None),
        })
    }
}
