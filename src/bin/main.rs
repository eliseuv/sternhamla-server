use sternhalma_rs::{
    board::{Board, HexIndex},
    piece::Piece,
};

fn main() {
    let mut board = Board::new().unwrap();

    board.set_piece(HexIndex([9, 8]), Piece::Player1).unwrap();
    board.set_piece(HexIndex([10, 8]), Piece::Player1).unwrap();

    board
        .possible_first_moves(HexIndex([8, 8]))
        .into_iter()
        .flatten()
        .for_each(|idx| board.set_piece(idx, Piece::MARKER).unwrap());

    println!("{board}");
}
