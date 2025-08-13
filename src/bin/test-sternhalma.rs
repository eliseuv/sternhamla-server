use itertools::Itertools;
use rand::seq::{IndexedRandom, IteratorRandom};
use rand_distr::{Distribution, Poisson};
use rand_xoshiro::{Xoshiro256PlusPlus, rand_core::SeedableRng};

use sternhalma_server::sterhalma::{
    Game, GameStatus,
    board::{Board, HexIdx, hex_distance, lut, movement::Movement, player::Player},
};

trait Agent {
    /// Create new agent for given player
    fn new(player: Player) -> Self
    where
        Self: Sized;

    /// Select a movement based on the current state of the board
    fn select_movement(&mut self, board: &Board<Player>) -> Movement;
}

struct AgentBrownian {
    player: Player,
    rng: Xoshiro256PlusPlus,
}

impl Agent for AgentBrownian {
    fn new(player: Player) -> Self {
        Self {
            player,
            rng: Xoshiro256PlusPlus::from_os_rng(),
        }
    }

    fn select_movement(&mut self, board: &Board<Player>) -> Movement {
        board
            .iter_player_movements(&self.player)
            .map(|mv| mv.get_indices())
            .choose(&mut self.rng)
            .unwrap()
    }
}

struct AgentMin {
    player: Player,
    rng: Xoshiro256PlusPlus,
    dist: Poisson<f32>,
    goal: [HexIdx; 15],
}

impl Agent for AgentMin {
    fn new(player: Player) -> Self {
        let goal = match player {
            Player::Player1 => lut::PLAYER2_STARTING_POSITIONS,
            Player::Player2 => lut::PLAYER1_STARTING_POSITIONS,
        };
        Self {
            player,
            rng: Xoshiro256PlusPlus::from_os_rng(),
            dist: Poisson::new(3.0).unwrap(),
            goal,
        }
    }

    fn select_movement(&mut self, board: &Board<Player>) -> Movement {
        let movements = board
            .iter_player_movements(&self.player)
            .map(|mv| mv.get_indices())
            .collect::<Vec<_>>();
        let mv_min = movements
            .iter()
            .filter(|mv| !self.goal.contains(&mv.from))
            .sorted_unstable_by_key(|mv| {
                self.goal.iter().map(|idx| hex_distance(mv.to, *idx)).min()
            })
            .nth(self.dist.sample(&mut self.rng) as usize);
        match mv_min {
            Some(mv) => mv.clone(),
            None => movements.choose(&mut self.rng).unwrap().clone(),
        }
    }
}

fn main() {
    env_logger::init();

    let mut game = Game::new();
    println!("{game}");

    let mut agent1 = AgentMin::new(Player::Player1);
    let mut agent2 = AgentBrownian::new(Player::Player2);

    while let GameStatus::Playing { player, turns } = game.status()
        && turns < 1024
    {
        let agent: &mut dyn Agent = match player {
            Player::Player1 => &mut agent1,
            Player::Player2 => &mut agent2,
        };

        let movement = agent.select_movement(game.board());
        game.unsafe_apply_movement(&movement);
        println!("{game}");
    }
}
