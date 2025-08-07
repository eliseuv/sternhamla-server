use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Circle,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Black,
    White,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    Small,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hole {
    Close,
    Open,
}

#[derive(Debug)]
pub struct Piece {
    shape: Shape,
    color: Color,
    size: Size,
    hole: Hole,
}

impl Display for Piece {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.size {
            Size::Small => write!(f, "S")?,
            Size::Large => write!(f, "L")?,
        };
        match self.shape {
            Shape::Circle => match self.color {
                Color::Black => match self.hole {
                    Hole::Close => write!(f, ""),
                    Hole::Open => write!(f, ""),
                },
                Color::White => match self.hole {
                    Hole::Close => write!(f, ""),
                    Hole::Open => write!(f, ""),
                },
            },
            Shape::Square => match self.color {
                Color::Black => match self.hole {
                    Hole::Close => write!(f, ""),
                    Hole::Open => write!(f, ""),
                },
                Color::White => match self.hole {
                    Hole::Close => write!(f, ""),
                    Hole::Open => write!(f, ""),
                },
            },
        }
    }
}

impl Piece {
    /// List all pieces
    pub fn list_all() -> [Self; 16] {
        [Shape::Circle, Shape::Square]
            .into_iter()
            .flat_map(move |shape| {
                [Color::Black, Color::White]
                    .into_iter()
                    .flat_map(move |color| {
                        [Size::Small, Size::Large]
                            .into_iter()
                            .flat_map(move |size| {
                                [Hole::Close, Hole::Open]
                                    .into_iter()
                                    .map(move |hole| Piece {
                                        shape,
                                        color,
                                        size,
                                        hole,
                                    })
                            })
                    })
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }

    pub fn is_similar(&self, other: &Self) -> [bool; 4] {
        [
            self.shape == other.shape,
            self.color == other.color,
            self.size == other.size,
            self.hole == other.hole,
        ]
    }
}

#[derive(Debug)]
pub struct Board([[Option<Piece>; 4]; 4]);

impl Display for Board {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for row in &self.0 {
            for cell in row {
                match cell {
                    Some(piece) => write!(f, "{piece} ")?,
                    None => write!(f, "⬜ ")?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum Player {
    Player1,
    Player2,
}

impl Player {
    pub fn other(&self) -> Self {
        match self {
            Player::Player1 => Player::Player2,
            Player::Player2 => Player::Player1,
        }
    }
}

#[derive(Debug)]
pub enum GameResult {
    Win(Player),
    Draw,
}

#[derive(Debug)]
pub enum GameStatus {
    Playing(Player),
    Finished(GameResult),
}

#[derive(Debug)]
pub struct Game {
    _board: Board,
    _available: Vec<Piece>,
    _status: GameStatus,
}
