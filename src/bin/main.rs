use sternhalma_rs::board::Board;

fn main() {
    let board = Board::new();

    println!("{board}");
    println!("{x:?}", x = board[[17, 0]]);
}
