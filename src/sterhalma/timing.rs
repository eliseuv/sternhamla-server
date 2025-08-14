use tokio::time::Instant;

pub struct GameTimer {
    /// Number of turns between each update
    n_turns: usize,
    /// Timer to measure the time elapsed for each interval
    timer: Instant,
    /// Number of turns per second for the interval
    turns_rate: f64,
}

impl GameTimer {
    pub fn new(n_turns: usize) -> Self {
        Self {
            n_turns,
            timer: Instant::now(),
            turns_rate: 0.0,
        }
    }

    pub fn turns_rate(&self) -> f64 {
        self.turns_rate
    }

    #[inline(always)]
    pub fn update(&mut self, game: &super::Game) {
        if game.status.turns() % self.n_turns == 0 {
            self.turns_rate = self.n_turns as f64 / self.timer.elapsed().as_secs_f64();
            self.timer = Instant::now();
        }
    }

    #[inline(always)]
    pub fn on_trigger<F: Fn(&Self)>(&mut self, game: &super::Game, f: F) {
        if game.status.turns() % self.n_turns == 0 {
            self.turns_rate = self.n_turns as f64 / self.timer.elapsed().as_secs_f64();

            f(&*self);

            self.timer = Instant::now();
        }
    }
}
