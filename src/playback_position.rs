use std::time::{Duration, Instant};

pub struct Position {
    generation: u64,
    confirmed: Duration,
    observed: Instant,
    sampled: bool,
}

impl Default for Position {
    fn default() -> Self {
        Self {
            generation: 0,
            confirmed: Duration::ZERO,
            observed: Instant::now(),
            sampled: false,
        }
    }
}

impl Position {
    pub fn observe(&mut self, generation: u64, position: Duration, now: Instant) {
        self.generation = generation;
        self.confirmed = position;
        self.observed = now;
        self.sampled = true;
    }

    pub fn target(&mut self, generation: u64, position: Duration, now: Instant) {
        self.observe(generation, position, now);
        self.sampled = false;
    }

    pub fn is_confirmed(&self, generation: u64) -> bool {
        self.sampled && self.generation == generation
    }

    pub fn read(&self, generation: u64, seconds: u64, advancing: bool, now: Instant) -> Duration {
        if generation != self.generation || seconds != self.confirmed.as_secs() {
            return Duration::from_secs(seconds);
        }
        if advancing && self.is_confirmed(generation) {
            self.confirmed.saturating_add(
                now.saturating_duration_since(self.observed)
                    .min(Duration::from_millis(250)),
            )
        } else {
            self.confirmed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precision_extrapolation_and_stale_generation_are_bounded() {
        let now = Instant::now();
        let mut clock = Position::default();
        clock.observe(4, Duration::from_millis(12_345), now);
        assert_eq!(
            clock.read(4, 12, false, now + Duration::from_secs(9)),
            Duration::from_millis(12_345)
        );
        assert_eq!(
            clock.read(4, 12, true, now + Duration::from_millis(100)),
            Duration::from_millis(12_445)
        );
        assert_eq!(
            clock.read(4, 12, true, now + Duration::from_secs(9)),
            Duration::from_millis(12_595)
        );
        assert_eq!(clock.read(5, 12, true, now), Duration::from_secs(12));
        assert_eq!(clock.read(4, 7, true, now), Duration::from_secs(7));
    }
}
