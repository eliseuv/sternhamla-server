use anyhow::Result;
use serde::Serialize;
use std::{
    fmt::{Debug, Display},
    ops::{Index, IndexMut},
};

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

// TODO: Function that calculates distance between two axial indices on the haxagonal lattice
pub fn distance(a: HexIdx, b: HexIdx) -> usize {
    // Using the formula for distance in a hexagonal grid
    ((a[0] as isize - b[0] as isize).abs()
        + (a[1] as isize - b[1] as isize).abs()
        + ((a[0] as isize - b[0] as isize) - (a[1] as isize - b[1] as isize)).abs()) as usize
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

/// Error when trying to access a board index that is outside of the board
#[derive(Debug, Clone, Copy)]
pub struct InvalidBoardIndex(HexIdx);

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
        indices: &[HexIdx],
        piece: T,
    ) -> Result<Self, PiecePlacementError> {
        self.place_pieces(indices, piece)?;
        Ok(self)
    }
}

impl<T: Copy + PartialEq> Board<T> {
    /// Iterate on the indices of the pieces of a given player
    pub fn iter_player_indices(&self, player: T) -> impl Iterator<Item = HexIdx> {
        self.0.iter().enumerate().filter_map(move |(i, pos)| {
            if pos.as_ref()?.as_ref()? == &player {
                let idx = [i / BOARD_LENGTH, i % BOARD_LENGTH];
                Some(idx)
            } else {
                None
            }
        })
    }
}

/// Movements of a player on the board
#[derive(Debug)]
pub enum Movement {
    /// Single move to adjacent cell
    Move { from: HexIdx, to: HexIdx },
    /// Multiple hops
    /// Path taken while hopping
    Hops { path: Vec<HexIdx> },
}

impl Movement {
    /// Check if the movement contains a specific index
    pub fn contains(&self, idx: &HexIdx) -> bool {
        match self {
            Movement::Move { from, to } => from == idx || to == idx,
            Movement::Hops { path } => path.contains(idx),
        }
    }
}

impl Board<Player> {
    /// Print the board with the current movement highlighted
    pub fn print_movement(&self, movement: &Movement) {
        let indices = movement.get_indices();
        for i in 0..BOARD_LENGTH {
            print!("{}", " ".repeat(i));
            for j in 0..BOARD_LENGTH {
                let idx = [i, j];
                if movement.contains(&idx) {
                    match &self[idx] {
                        None => print!("󠀠󠀠󠀠󠀠   "),
                        Some(None) => {
                            if idx == indices.to {
                                print!("🟡 ");
                            } else {
                                print!("🟠 ");
                            }
                        }
                        Some(Some(piece)) => match piece {
                            Player::Player1 => print!("🟣 "),
                            Player::Player2 => print!("🟤 "),
                        },
                    }
                } else {
                    match &self[[i, j]] {
                        None => print!("󠀠󠀠󠀠󠀠   "),
                        Some(None) => print!("⚫ "),
                        Some(Some(piece)) => print!("{piece} "),
                    }
                }
            }
            println!();
        }
    }
}

impl<T> Board<T> {
    /// Iterate over all indices that are possible to hop over to starting from `idx`
    pub fn available_hops_from(&self, idx: HexIdx) -> impl Iterator<Item = HexIdx> {
        HexDirection::variants()
            // For all directions
            .into_iter()
            .filter_map(move |direction| {
                // Check if nearest neighbor is valid index
                let (nn_idx, nn_pos) = self.nearest_neighbor(idx, direction)?;
                // Check if it is occupied
                match nn_pos {
                    // Nearest neighbor is empty => Unable to hop over it
                    None => None,
                    // Nearest neighbor is occupied
                    Some(_) => {
                        // Check if next nearest neighbor is valid index
                        let (nnn_idx, nnn_pos) = self.nearest_neighbor(nn_idx, direction)?;
                        match nnn_pos {
                            // Next nearest neighbor is occupied => Unable to hop
                            Some(_) => None,
                            // Next nearest neighbor is empty => We can hop over there
                            None => Some(nnn_idx),
                        }
                    }
                }
            })
    }

    /// Recursive helper to find all possible hop paths starting from a given position.
    fn collect_hop_paths_from(&self, path: &[HexIdx]) -> Vec<Movement> {
        let mut all_hop_movements = Vec::new();

        for next_hop_idx in self.available_hops_from(*path.last().unwrap()) {
            // Ensure we don't hop back to a previously visited cell within the same path.
            // This prevents infinite loops and ensures unique paths for a game like Chinese Checkers.
            if !path.contains(&next_hop_idx) {
                let mut next_path = path.to_vec().clone();
                next_path.push(next_hop_idx);

                // This new path is itself a valid complete hop movement
                all_hop_movements.push(Movement::Hops {
                    path: next_path.clone(),
                });

                // Recursively find further hops from the new position
                let further_movements = self.collect_hop_paths_from(&next_path);
                all_hop_movements.extend(further_movements);
            }
        }
        all_hop_movements
    }

    /// List all available movements for a piece at index `idx`
    pub fn available_movements_from(&self, idx: HexIdx) -> impl Iterator<Item = Movement> {
        HexDirection::variants()
            .into_iter()
            // For all directions
            .filter_map(move |direction| {
                // Check if nearest neighbor is valid index
                let (nn_idx, nn_pos) = self.nearest_neighbor(idx, direction)?;
                // Check if it is occupied
                match nn_pos {
                    // Nearest neighbor is empty => We can move there
                    None => Some(vec![Movement::Move {
                        from: idx,
                        to: nn_idx,
                    }]),
                    // Nearest neighbor is occupied
                    Some(_) => {
                        // Check if next nearest neighbor is valid index
                        let (nnn_idx, nnn_pos) = self.nearest_neighbor(nn_idx, direction)?;
                        match nnn_pos {
                            // Next nearest neighbor is occupied
                            Some(_) => None,
                            None => {
                                // Next nearest neighbor is empty => We can hop over there
                                // The first hop itself is a valid movement.
                                let initial_hop = vec![idx, nnn_idx];

                                // Find all further hops starting from this first hop destination
                                // These are paths that extend beyond the initial hop
                                let further_hop_movements =
                                    self.collect_hop_paths_from(&initial_hop);

                                Some(
                                    [Movement::Hops { path: initial_hop }]
                                        .into_iter()
                                        .chain(further_hop_movements)
                                        .collect(),
                                )
                            }
                        }
                    }
                }
            })
            .flatten()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MovementError {
    /// Initial position is empty
    EmptyInit,
    /// One of the indices is outside the board
    InvalidIndex(HexIdx),
    /// One of the indices is occupied
    Occupied(HexIdx),
    /// The hopping sequence is too short
    ShortHopping(usize),
}

impl<T> Board<T> {
    /// Check if all intermediate indices of the movement are valid
    pub fn validate_movement<'a>(
        &self,
        movement: &'a Movement,
    ) -> Result<&'a Movement, MovementError> {
        match movement {
            Movement::Move { from, to } => {
                // Check starting position
                self.get(from)
                    .map_err(|_| MovementError::InvalidIndex(*from))?
                    .as_ref()
                    .ok_or(MovementError::EmptyInit)?;

                // Check if the destination position is empty
                if self
                    .get(to)
                    .map_err(|_| MovementError::InvalidIndex(*from))?
                    .as_ref()
                    .is_some()
                {
                    return Err(MovementError::Occupied(*to));
                }

                Ok(movement)
            }
            Movement::Hops { path } => {
                // Check starting position
                let start = path
                    .first()
                    .ok_or(MovementError::ShortHopping(path.len()))?;
                self.get(start)
                    .map_err(|_| MovementError::InvalidIndex(*start))?
                    .as_ref()
                    .ok_or(MovementError::EmptyInit)?;

                // Check all other indices in the path
                path.get(1..)
                    .ok_or(MovementError::ShortHopping(path.len()))?
                    .iter()
                    .find_map(|idx| {
                        self.get(idx)
                            .map_err(|_| MovementError::InvalidIndex(*idx))
                            .and_then(|pos| {
                                if pos.is_some() {
                                    Err(MovementError::Occupied(*idx))
                                } else {
                                    Ok(())
                                }
                            })
                            .err()
                    })
                    .map(Err)
                    .unwrap_or(Ok(movement))
            }
        }
    }
}

/// Indices of the player before and after the movement is done
#[derive(Debug)]
pub struct MovementIndices {
    from: HexIdx,
    to: HexIdx,
}

impl Movement {
    /// Get start and end indices of the movement
    pub fn get_indices(&self) -> MovementIndices {
        match self {
            Movement::Move { from, to } => MovementIndices {
                from: *from,
                to: *to,
            },
            Movement::Hops { path } => MovementIndices {
                from: *path.first().unwrap(),
                to: *path.last().unwrap(),
            },
        }
    }
}

impl<T> Board<T> {
    pub fn apply_movement(&mut self, indices: &MovementIndices) -> Result<(), MovementError> {
        let piece = self
            .get_mut(&indices.from)
            .map_err(|InvalidBoardIndex(idx)| MovementError::InvalidIndex(idx))?
            .take()
            .ok_or(MovementError::EmptyInit)?;
        let target_pos = self
            .get_mut(&indices.to)
            .map_err(|InvalidBoardIndex(idx)| MovementError::InvalidIndex(idx))?;
        match target_pos {
            // Target position is occupied
            Some(_) => {
                // Place it back to the original position
                *self.get_mut(&indices.from).unwrap() = Some(piece);
                // Return error
                Err(MovementError::Occupied(indices.to))
            }
            None => {
                // Make movement
                *target_pos = Some(piece);
                Ok(())
            }
        }
    }

    /// Unsafe apply movement without checking for errors
    pub fn unsafe_apply_movement(&mut self, indices: &MovementIndices) {
        let piece = self.get_mut(&indices.from).unwrap().take().unwrap();
        let target_pos = self.get_mut(&indices.to).unwrap();
        *target_pos = Some(piece);
    }
}

/// Sternhalma board with pieces
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Player {
    Player1,
    Player2,
}

impl Player {
    /// List all player variants
    pub const fn variants() -> [Player; 2] {
        [Player::Player1, Player::Player2]
    }

    /// Number of players
    pub const fn count() -> usize {
        Player::variants().len()
    }

    pub const fn opponent(&self) -> Self {
        match self {
            Player::Player1 => Player::Player2,
            Player::Player2 => Player::Player1,
        }
    }
}

impl Display for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Player1 => write!(f, "🔵"),
            Self::Player2 => write!(f, "🔴"),
        }
    }
}

impl Board<Player> {
    /// Creates a new Sternhalma board with pieces placed in their starting positions
    pub fn new() -> Self {
        Self::empty()
            .with_pieces(&lut::PLAYER1_STARTING_POSITIONS, Player::Player1)
            .expect("Player 1 positions are valid")
            .with_pieces(&lut::PLAYER2_STARTING_POSITIONS, Player::Player2)
            .expect("Player 2 positions are valid")
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
                    Some(Some(piece)) => write!(f, "{piece} ")?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

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
}

impl Display for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.board)?;
        match self.status {
            GameStatus::Playing(player) => write!(f, "Current player: {player}")?,
            GameStatus::Finished(winner) => {
                write!(f, "Game finished!")?;
                write!(f, "Winner: {winner}")?;
            }
        }
        Ok(())
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
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            status: GameStatus::Playing(Player::Player1),
        }
    }

    pub fn board(&self) -> &Board<Player> {
        &self.board
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    pub fn iter_available_moves(&self) -> impl Iterator<Item = Movement> {
        match self.status {
            GameStatus::Finished(_) => todo!(),
            GameStatus::Playing(player) => self
                .board
                .iter_player_indices(player)
                .flat_map(move |idx| self.board.available_movements_from(idx)),
        }
    }

    pub fn apply_movement(&mut self, movement: &Movement) -> Result<GameStatus, GameError> {
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
                self.status = self.updade_status();

                Ok(self.status)
            }
        }
    }

    pub fn unsafe_apply_movement(&mut self, indices: &MovementIndices) -> GameStatus {
        // Unsafe apply movement without checking for errors
        self.board.unsafe_apply_movement(indices);

        // Update game status
        self.status = self.updade_status();

        self.status
    }

    /// Update the game status based on current state of the game
    /// TODO: Check if game is finished
    fn updade_status(&self) -> GameStatus {
        match self.status {
            GameStatus::Finished(_) => self.status,
            GameStatus::Playing(player) => GameStatus::Playing(player.opponent()),
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}
