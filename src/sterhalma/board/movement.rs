use std::fmt::Debug;

use anyhow::Result;

use crate::sterhalma::board::{
    BOARD_LENGTH, Board, HexDirection, HexIdx, InvalidBoardIndex, player::Player,
};

/// Movements of a player on the board
#[derive(Debug)]
pub enum MovementFull {
    /// Single move to adjacent cell
    Move { from: HexIdx, to: HexIdx },
    /// Multiple hops
    /// Path taken while hopping
    Hops { path: Vec<HexIdx> },
}

impl MovementFull {
    /// Get the origin index of the movement
    fn origin(&self) -> HexIdx {
        match self {
            MovementFull::Move { from, .. } => *from,
            MovementFull::Hops { path } => *path.first().unwrap(),
        }
    }

    /// Get the destination index of the movement
    fn destination(&self) -> HexIdx {
        match self {
            MovementFull::Move { to, .. } => *to,
            MovementFull::Hops { path } => *path.last().unwrap(),
        }
    }

    /// Check if the movement contains a specific index
    fn contains(&self, idx: &HexIdx) -> bool {
        match self {
            MovementFull::Move { from, to } => from == idx || to == idx,
            MovementFull::Hops { path } => path.contains(idx),
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
    fn collect_hop_paths_from(&self, path: &[HexIdx]) -> Vec<MovementFull> {
        let mut all_hop_movements = Vec::new();

        for next_hop_idx in self.available_hops_from(*path.last().unwrap()) {
            // Ensure we don't hop back to a previously visited cell within the same path.
            // This prevents infinite loops and ensures unique paths for a game like Chinese Checkers.
            if !path.contains(&next_hop_idx) {
                let mut next_path = path.to_vec().clone();
                next_path.push(next_hop_idx);

                // This new path is itself a valid complete hop movement
                all_hop_movements.push(MovementFull::Hops {
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
    pub fn available_movements_from(&self, idx: HexIdx) -> impl Iterator<Item = MovementFull> {
        HexDirection::variants()
            .into_iter()
            // For all directions
            .filter_map(move |direction| {
                // Check if nearest neighbor is valid index
                let (nn_idx, nn_pos) = self.nearest_neighbor(idx, direction)?;
                // Check if it is occupied
                match nn_pos {
                    // Nearest neighbor is empty => We can move there
                    None => Some(vec![MovementFull::Move {
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
                                    [MovementFull::Hops { path: initial_hop }]
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

impl<T: PartialEq> Board<T> {
    /// Iterate over all available movements for a player
    pub fn iter_player_movements(&self, player: &T) -> impl Iterator<Item = MovementFull> {
        // Iterate over all indices of the player
        self.iter_player_indices(player)
            // For each index, get all available movements
            .flat_map(move |idx| self.available_movements_from(idx))
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
        movement: &'a MovementFull,
    ) -> Result<&'a MovementFull, MovementError> {
        match movement {
            MovementFull::Move { from, to } => {
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
            MovementFull::Hops { path } => {
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Movement {
    pub from: HexIdx,
    pub to: HexIdx,
}

impl MovementFull {
    /// Get start and end indices of the movement
    pub fn get_indices(&self) -> Movement {
        match self {
            MovementFull::Move { from, to } => Movement {
                from: *from,
                to: *to,
            },
            MovementFull::Hops { path } => Movement {
                from: *path.first().unwrap(),
                to: *path.last().unwrap(),
            },
        }
    }
}

impl<T> Board<T> {
    pub fn apply_movement(&mut self, indices: &Movement) -> Result<(), MovementError> {
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
    pub fn unsafe_apply_movement(&mut self, indices: &Movement) {
        let piece = self.get_mut(&indices.from).unwrap().take().unwrap();
        let target_pos = self.get_mut(&indices.to).unwrap();
        *target_pos = Some(piece);
    }
}

impl Board<Player> {
    /// Print the board with the current movement highlighted
    pub fn print_movement(&self, movement: &MovementFull) {
        let indices = movement.get_indices();
        for i in 0..BOARD_LENGTH {
            print!("{}", " ".repeat(i));
            for j in 0..BOARD_LENGTH {
                let idx = [i, j];
                match movement.contains(&idx) {
                    true => match &self[idx] {
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
                    },
                    false => match &self[[i, j]] {
                        None => print!("󠀠󠀠󠀠󠀠   "),
                        Some(None) => print!("⚫ "),
                        Some(Some(piece)) => print!("{piece} "),
                    },
                }
            }
            println!();
        }
    }
}
