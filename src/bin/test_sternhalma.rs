use sternhalma_server::sterhalma::{Board, Player};

fn main() {
    let board = Board::new();

    println!("{board}");

    for (_, movements) in board.possible_first_moves(Player::Player1) {
        for movement in movements {
            let mut new_board = Board::new();
            new_board.apply_movement(movement).unwrap();
            println!("{new_board}");
        }
    }
}
