use rand::{SeedableRng, seq::IteratorRandom};
use rand_xoshiro::Xoshiro256PlusPlus;
use sternhalma_server::sterhalma::{Game, GameStatus};

const N_TURNS: usize = 8;

fn main() {
    let mut game = Game::new();
    println!("{game}");

    let mut rng = Xoshiro256PlusPlus::from_os_rng();

    for _ in 0..N_TURNS {
        if let GameStatus::Playing(_) = game.status() {
            let movement = game.iter_available_moves().choose(&mut rng).unwrap();
            match game.board().validate_movement(&movement) {
                Err(e) => panic!("{e:?}"),
                Ok(movement) => {
                    game.board().print_movement(movement);
                    let indices = movement.get_indices();
                    game.unsafe_apply_movement(&indices);
                }
            }
        } else {
            break;
        }
    }
}
