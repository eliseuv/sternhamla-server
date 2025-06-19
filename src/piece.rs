#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Piece {
    MARKER,
    Player1,
    Player2,
}

impl Piece {
    /// Returns the character representation of the piece
    pub(crate) fn char(&self) -> char {
        match self {
            Self::Player1 => '1',
            Self::Player2 => '2',
            Self::MARKER => '❌',
        }
    }
}
