use rand::{SeedableRng, seq::IteratorRandom};
use rand_xoshiro::Xoshiro256PlusPlus;
use sternhalma_server::sterhalma::{Game, GameStatus};

const N_TURNS: usize = 8;

fn main() {
    let mut game = Game::new();
    println!("{board}", board = game.board());

    let mut rng = Xoshiro256PlusPlus::from_os_rng();

    for _ in 0..N_TURNS {
        if let GameStatus::Playing(player) = game.status() {
            let movement = game
                .board()
                .iter_player_indices(&player)
                .flat_map(|idx| game.board().available_movements_from(idx))
                .choose(&mut rng)
                .unwrap();

            game.apply_movement(&movement).unwrap();
            println!("{board}", board = game.board());
        }
    }
}
