use sternhalma_server::sterhalma::{Board, Player};

fn main() {
    let board = Board::new().unwrap();

    println!("{board}");

    for (_, movements) in board.possible_first_moves(Player::Player1) {
        for movement in movements {
            let mut new_board = Board::new().unwrap();
            new_board.apply_movement(movement).unwrap();
            println!("{new_board}");
        }
    }
}
