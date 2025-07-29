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
