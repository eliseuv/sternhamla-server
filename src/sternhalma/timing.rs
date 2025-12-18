use tokio::time::Instant;

pub struct GameTimer<const N_TURNS: usize> {
    /// Timer to measure the time elapsed for each interval
    timer: Instant,
    /// Number of turns per second for the interval
    turns_rate: f64,
}

impl<const N_TURNS: usize> GameTimer<N_TURNS> {
    pub fn new() -> Self {
        Self {
            timer: Instant::now(),
            turns_rate: f64::NAN,
        }
    }

    #[inline(always)]
    pub const fn turns_rate(&self) -> f64 {
        self.turns_rate
    }
}

impl<const N_TURNS: usize> Default for GameTimer<N_TURNS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N_TURNS: usize> GameTimer<N_TURNS> {
    #[inline(always)]
    pub fn update(&mut self, game: &super::Game) {
        if game.status.turns() % N_TURNS == 0 {
            self.turns_rate = N_TURNS as f64 / self.timer.elapsed().as_secs_f64();
            self.timer = Instant::now();
        }
    }

    #[inline(always)]
    pub fn on_trigger<F>(&mut self, game: &super::Game, func: F)
    where
        F: Fn(&Self),
    {
        if game.status.turns() % N_TURNS == 0 {
            self.turns_rate = N_TURNS as f64 / self.timer.elapsed().as_secs_f64();

            func(&*self);

            self.timer = Instant::now();
        }
    }
}
