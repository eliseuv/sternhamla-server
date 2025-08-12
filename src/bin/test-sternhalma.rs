use std::collections::HashMap;

use rand::{Rng, SeedableRng, seq::IteratorRandom};
use rand_xoshiro::Xoshiro256PlusPlus;

use sternhalma_server::sterhalma::{
    Game, GameStatus,
    board::{Board, movement::MovementFull, player::Player},
};

struct Agent {
    player: Player,
}

impl Agent {
    fn new(player: Player) -> Self {
        Self { player }
    }

    fn select_movement(&mut self, board: &Board<Player>, rng: &mut impl Rng) -> MovementFull {
        board
            .iter_player_movements(&self.player)
            .choose(rng)
            .expect("No available movements")
    }
}

fn main() {
    env_logger::init();

    let mut game = Game::new();
    println!("{game}");

    let mut agents = HashMap::from([
        (Player::Player1, Agent::new(Player::Player1)),
        (Player::Player2, Agent::new(Player::Player2)),
    ]);

    let mut rng = Xoshiro256PlusPlus::from_os_rng();

    while let GameStatus::Playing(player) = game.status() {
        game.unsafe_apply_movement(
            &agents
                .get_mut(&player)
                .unwrap()
                .select_movement(game.board(), &mut rng)
                .get_indices(),
        );
        println!("{game}");
    }
}
