//! A stand-in for the real fishing loop, so the UI can be designed against a session that moves.
//! Times are seconds on the app clock that `poll` receives.

/// The longest a cast waits for a bite before the bobber despawns.
pub const BITE_WINDOW: f64 = 20.0;

const CAST_TIME: f64 = 1.5;
const HOOK_TIME: f64 = 0.6;
const LOOT_TIME: f64 = 1.2;
const FISH: [&str; 6] = [
    "Raw Brilliant Smallfish",
    "Raw Longjaw Mud Snapper",
    "Oily Blackmouth",
    "Raw Bristle Whisker Catfish",
    "Firefin Snapper",
    "Raw Rainbow Fin Albacore",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Casting,
    Waiting,
    Hooking,
    Looting,
}

impl Phase {
    pub const ALL: [Phase; 4] = [
        Phase::Casting,
        Phase::Waiting,
        Phase::Hooking,
        Phase::Looting,
    ];

    pub fn step_label(self) -> &'static str {
        match self {
            Phase::Casting => "Cast",
            Phase::Waiting => "Wait",
            Phase::Hooking => "Hook",
            Phase::Looting => "Loot",
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            Phase::Casting => "Casting",
            Phase::Waiting => "Waiting for a bite",
            Phase::Hooking => "Hooking",
            Phase::Looting => "Looting",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Caught(&'static str),
    GotAway,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cast {
    pub outcome: Outcome,
    /// Seconds into the session.
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub started: f64,
    pub phase: Phase,
    pub phase_started: f64,
    /// Finished casts, newest last.
    pub history: Vec<Cast>,
}

impl Run {
    pub fn new(now: f64) -> Self {
        Run {
            started: now,
            phase: Phase::Casting,
            phase_started: now,
            history: Vec::new(),
        }
    }

    /// The cast in progress counts too.
    pub fn casts(&self) -> usize {
        self.history.len() + 1
    }

    pub fn catches(&self) -> usize {
        self.history
            .iter()
            .filter(|cast| matches!(cast.outcome, Outcome::Caught(_)))
            .count()
    }

    fn wait_time(&self) -> f64 {
        3.0 + (self.history.len() * 7 % 9) as f64
    }

    fn phase_length(&self) -> f64 {
        match self.phase {
            Phase::Casting => CAST_TIME,
            Phase::Waiting => self.wait_time(),
            Phase::Hooking => HOOK_TIME,
            Phase::Looting => LOOT_TIME,
        }
    }

    /// Advances through every phase that ended by `now`.
    pub fn tick(&mut self, now: f64) {
        while now >= self.phase_started + self.phase_length() {
            let ended = self.phase_started + self.phase_length();
            self.phase = match self.phase {
                Phase::Casting => Phase::Waiting,
                Phase::Waiting => Phase::Hooking,
                Phase::Hooking => Phase::Looting,
                Phase::Looting => {
                    let number = self.history.len();
                    let outcome = if number % 6 == 5 {
                        Outcome::GotAway
                    } else {
                        Outcome::Caught(FISH[number * 5 % FISH.len()])
                    };
                    self.history.push(Cast {
                        outcome,
                        at: ended - self.started,
                    });
                    Phase::Casting
                }
            };
            self.phase_started = ended;
        }
    }

    /// How far into the bite window the bobber is, while waiting.
    pub fn bite_progress(&self, now: f64) -> Option<(f64, f32)> {
        if self.phase != Phase::Waiting {
            return None;
        }
        let waited = (now - self.phase_started).max(0.0);
        Some((waited, (waited / BITE_WINDOW).clamp(0.0, 1.0) as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cast_runs_through_every_phase_and_lands_a_catch() {
        let mut run = Run::new(10.0);
        run.tick(10.0 + CAST_TIME);
        assert_eq!(run.phase, Phase::Waiting);
        let bite = 10.0 + CAST_TIME + run.wait_time();
        run.tick(bite);
        assert_eq!(run.phase, Phase::Hooking);
        run.tick(bite + HOOK_TIME + LOOT_TIME);
        assert_eq!(run.phase, Phase::Casting);
        assert_eq!(run.catches(), 1);
        assert_eq!(run.casts(), 2);
    }

    #[test]
    fn some_fish_get_away() {
        let mut run = Run::new(0.0);
        run.tick(600.0);
        assert!(
            run.history
                .iter()
                .any(|cast| cast.outcome == Outcome::GotAway)
        );
        assert!(run.catches() < run.history.len());
    }
}
