use sternhalma_server::sterhalma::{Board, Piece};

fn main() {
    let board = Board::two_players().unwrap();

    println!("{board}");

    for (_, movements) in board.possible_first_moves(Piece::Player1) {
        for movement in movements {
            let mut new_board = Board::two_players().unwrap();
            new_board.apply_movement(movement).unwrap();
            println!("{new_board}");
        }
    }
}
