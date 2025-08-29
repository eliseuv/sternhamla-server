use std::{
    fmt::{Debug, Display},
    ops::{Index, IndexMut},
};

use anyhow::Result;

use crate::sterhalma::board::player::Player;

/// Length of the Sternhalma board
const BOARD_LENGTH: usize = 17;

/// Board position:
/// `None`: Invalid position (outside of the board)
/// `Some(None)`: Valid position.Empty.
/// `Some(Some(_))`: Valid position. Occupied
pub type Position<T> = Option<Option<T>>;

/// Sternhalma board
#[derive(Debug)]
pub struct Board<T>([Position<T>; BOARD_LENGTH * BOARD_LENGTH]);

/// Axial index for the hexagonal lattice
pub type HexIdx = [usize; 2];

/// Hexagonal distance between two cells in the hexagonal grid
/// How many steps it takes to get from one cell to another
pub fn hex_distance([q1, r1]: HexIdx, [q2, r2]: HexIdx) -> usize {
    let dq = q1.abs_diff(q2);
    let dr = r1.abs_diff(r2);
    unsafe {
        [dq, dr, (q1 + r1).abs_diff(q2 + r2)]
            .into_iter()
            .max()
            .unwrap_unchecked()
    }
}

/// Square macro
macro_rules! square {
    ($n:expr) => {
        $n * $n
    };
}

/// Euclidean distance between two cells in the hexagonal grid
pub fn dist_euclidean([q1, r1]: HexIdx, [q2, r2]: HexIdx) -> f64 {
    let dq = q1.abs_diff(q2);
    let dr = r1.abs_diff(r2);
    ((square!(dq + dr) - (dq * dr)) as f64).sqrt()
}

/// Board indexing
/// Returns `Option<Position<T>>` with `None` meaning an index outside of the board
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
pub mod lut;

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

/// Error when trying to access a board index that is outside of the board
#[derive(Debug, Clone, Copy)]
pub struct InvalidBoardIndex(pub HexIdx);

impl<T> Board<T> {
    /// Returns a reference to the piece at the specified index on the board
    pub fn get(&self, idx: &HexIdx) -> Result<&Option<T>, InvalidBoardIndex> {
        self[*idx].as_ref().ok_or(InvalidBoardIndex(*idx))
    }

    pub fn get_mut(&mut self, idx: &HexIdx) -> Result<&mut Option<T>, InvalidBoardIndex> {
        self[*idx].as_mut().ok_or(InvalidBoardIndex(*idx))
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

impl HexDirection {
    /// List all possible hexagonal grid directions
    pub const fn variants() -> [HexDirection; 6] {
        [
            HexDirection::NW,
            HexDirection::NE,
            HexDirection::W,
            HexDirection::E,
            HexDirection::SW,
            HexDirection::SE,
        ]
    }
}

impl<T> Board<T> {
    /// Nearest neighbor in a given direction
    fn nearest_neighbor(
        &self,
        idx: HexIdx,
        direction: HexDirection,
    ) -> Option<(HexIdx, &Option<T>)> {
        match direction {
            HexDirection::NW => self.next_nw(idx),
            HexDirection::NE => self.next_ne(idx),
            HexDirection::W => self.next_w(idx),
            HexDirection::E => self.next_e(idx),
            HexDirection::SW => self.next_sw(idx),
            HexDirection::SE => self.next_se(idx),
        }
    }

    /// Nearest neighbor NW
    #[inline(always)]
    fn next_nw(&self, [i, j]: HexIdx) -> Option<(HexIdx, &Option<T>)> {
        let idx = [i.checked_sub(1)?, j];
        Some((idx, self[idx].as_ref()?))
    }

    /// Nearest neighbor NE
    #[inline(always)]
    fn next_ne(&self, [i, j]: HexIdx) -> Option<(HexIdx, &Option<T>)> {
        let idx = [
            i.checked_sub(1)?,
            if j + 1 < BOARD_LENGTH {
                Some(j + 1)
            } else {
                None
            }?,
        ];
        Some((idx, self[idx].as_ref()?))
    }

    /// Nearest neighbor E
    #[inline(always)]
    fn next_e(&self, [i, j]: HexIdx) -> Option<(HexIdx, &Option<T>)> {
        let idx = [
            i,
            if j + 1 < BOARD_LENGTH {
                Some(j + 1)
            } else {
                None
            }?,
        ];
        Some((idx, self[idx].as_ref()?))
    }

    /// Nearest neighbor SE
    #[inline(always)]
    fn next_se(&self, [i, j]: HexIdx) -> Option<(HexIdx, &Option<T>)> {
        let idx = [
            if i + 1 < BOARD_LENGTH {
                Some(i + 1)
            } else {
                None
            }?,
            j,
        ];
        Some((idx, self[idx].as_ref()?))
    }

    /// Nearest neighbor SW
    #[inline(always)]
    fn next_sw(&self, [i, j]: HexIdx) -> Option<(HexIdx, &Option<T>)> {
        let idx = [
            if i + 1 < BOARD_LENGTH {
                Some(i + 1)
            } else {
                None
            }?,
            j.checked_sub(1)?,
        ];
        Some((idx, self[idx].as_ref()?))
    }

    /// Nearest neighbor W
    #[inline(always)]
    fn next_w(&self, [i, j]: HexIdx) -> Option<(HexIdx, &Option<T>)> {
        let idx = [i, j.checked_sub(1)?];
        Some((idx, self[idx].as_ref()?))
    }
}

/// Error when trying to place a piece on the board
#[derive(Debug, Clone, Copy)]
pub enum PiecePlacementError {
    /// Trying to place a piece on an invalid position
    InvalidIndex(HexIdx),
    /// Trying to place a piece on an occupied position
    Occupied(HexIdx),
}

impl<T> Board<T> {
    /// Sets a piece at the specified index on the board
    pub fn set_piece(&mut self, idx: HexIdx, piece: T) -> Result<(), PiecePlacementError> {
        let pos = self
            .get_mut(&idx)
            .map_err(|InvalidBoardIndex(idx)| PiecePlacementError::InvalidIndex(idx))?;
        match pos {
            // Position is already occupied
            Some(_) => Err(PiecePlacementError::Occupied(idx)),
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
    pub fn place_pieces(
        &mut self,
        indices: &[HexIdx],
        piece: T,
    ) -> Result<(), PiecePlacementError> {
        for &idx in indices {
            self.set_piece(idx, piece)?;
        }
        Ok(())
    }

    /// Builder
    pub fn with_pieces(
        mut self,
        piece: T,
        indices: &[HexIdx],
    ) -> Result<Self, PiecePlacementError> {
        self.place_pieces(indices, piece)?;
        Ok(self)
    }
}

impl<T: PartialEq> Board<T> {
    /// Iterate on the indices of the pieces of a given player
    pub fn iter_player_indices(&self, player: &T) -> impl Iterator<Item = HexIdx> {
        self.0.iter().enumerate().filter_map(move |(i, pos)| {
            if pos.as_ref()?.as_ref()? == player {
                let idx = [i / BOARD_LENGTH, i % BOARD_LENGTH];
                Some(idx)
            } else {
                None
            }
        })
    }
}

/// Movements on the board
pub mod movement;

/// Player pieces
pub mod player;

impl Board<Player> {
    /// Creates a new Sternhalma board with pieces placed in their starting positions
    pub fn new() -> Self {
        unsafe {
            Self::empty()
                .with_pieces(Player::Player1, &lut::PLAYER1_STARTING_POSITIONS)
                .unwrap_unchecked()
                .with_pieces(Player::Player2, &lut::PLAYER2_STARTING_POSITIONS)
                .unwrap_unchecked()
        }
    }
}

/// Board display
impl Display for Board<Player> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for i in 0..BOARD_LENGTH {
            write!(f, "{}", " ".repeat(i))?;
            for j in 0..BOARD_LENGTH {
                match &self[[i, j]] {
                    None => write!(f, "󠀠󠀠󠀠󠀠   ")?,
                    Some(None) => write!(f, "⚫ ")?,
                    Some(Some(player)) => write!(f, "{piece} ", piece = player.piece())?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// Get the indices of the goal positions for a given player
pub(crate) const fn goal_indices(player: &Player) -> [[usize; 2]; 15] {
    match player {
        Player::Player1 => lut::PLAYER2_STARTING_POSITIONS,
        Player::Player2 => lut::PLAYER1_STARTING_POSITIONS,
    }
}

impl Board<Player> {
    /// Check if a player has won the game
    /// A player that occupied all its goal positions
    pub(crate) fn check_winner(&self) -> Option<Player> {
        Player::variants().into_iter().find(|player| {
            goal_indices(player)
                .into_iter()
                .all(|idx| unsafe { self.get(&idx).unwrap_unchecked() == &Some(*player) })
        })
    }

    /// Calculate the score for a given player
    /// Number of goal positions occupied
    pub fn score(&self, player: &Player) -> usize {
        goal_indices(player)
            .into_iter()
            .filter(|idx| unsafe { self.get(idx).unwrap_unchecked() == &Some(*player) })
            .count()
    }

    /// Calculate the scores of all players
    pub fn get_scores(&self) -> [usize; 2] {
        unsafe {
            Player::variants()
                .into_iter()
                .map(|player| self.score(&player))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap_unchecked()
        }
    }
}
