//! The fishing loop: cast, wait for the splash, hook, loot, and cast again.
//!
//! [`Run`] decides when the fishing key is due from the time, the game's sound, and what the
//! game shows the player doing; the caller feeds it all three and presses the key. Times are
//! seconds since the session started.

use std::collections::VecDeque;

/// The fishing channel; a bobber that hasn't bitten by then is gone.
pub const CAST_LENGTH: f64 = 30.0;
/// After a cast, the cast sound and the bobber landing play; none of it is a bite.
const SETTLE_TIME: f64 = 2.0;
/// A key press that hasn't started a cast by then didn't cast: the game refused it, as when the
/// player is moving.
const CAST_START_TIME: f64 = 3.0;
/// A bite is a sound over the threshold for at least half of this long. Splashes flicker under
/// it between frames; clicks and hits are shorter.
const SPLASH_WINDOW: f64 = 0.2;
/// After a bite, the fish stays on the hook about this long; longer than any hook delay.
const HOOK_WINDOW: f64 = 2.0;
/// Splashes measured -22 to -30 dBFS at default game volume, with little else above -40.
const SPLASH_THRESHOLD: f32 = -38.0;
/// After the game window gets focus, the shortest and longest wait before the next key press.
const FOCUS_DELAY: (f64, f64) = (0.6, 1.6);
/// Hooking opens the loot, which auto loot then takes.
const LOOT_TIME: f64 = 3.0;
/// The bobber sits in front of the camera, so its splash is close to centered; sounds off to one
/// side come from somewhere else.
const MAX_BALANCE: f32 = 6.0;

/// How loud 20 ms or so of the game's sound is: the mean square of each channel, full scale 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub left: f32,
    pub right: f32,
}

impl Level {
    /// Measures interleaved stereo samples; `None` for an empty slice.
    pub fn of(samples: &[i16]) -> Option<Level> {
        let (frames, _) = samples.as_chunks::<2>();
        if frames.is_empty() {
            return None;
        }
        let energy = |channel: usize| {
            frames
                .iter()
                .map(|frame| (f32::from(frame[channel]) / 32768.0).powi(2))
                .sum::<f32>()
                / frames.len() as f32
                + 1e-12
        };
        Some(Level {
            left: energy(0),
            right: energy(1),
        })
    }

    /// RMS loudness in dBFS.
    pub fn loudness(self) -> f32 {
        10.0 * ((self.left + self.right) / 2.0).log10()
    }

    /// Left over right in dB; positive is louder on the left.
    pub fn balance(self) -> f32 {
        10.0 * (self.left / self.right).log10()
    }

    fn mean(levels: &[Level]) -> Option<Level> {
        let count = levels.len() as f32;
        (!levels.is_empty()).then(|| Level {
            left: levels.iter().map(|level| level.left).sum::<f32>() / count,
            right: levels.iter().map(|level| level.right).sum::<f32>() / count,
        })
    }
}

/// How long to wait between hearing the splash and pressing the key: a random time in a range,
/// different for every bite, so presses don't land at the same moment each time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HookDelay {
    Short,
    #[default]
    Medium,
    Long,
}

impl HookDelay {
    pub const ALL: [HookDelay; 3] = [HookDelay::Short, HookDelay::Medium, HookDelay::Long];

    /// The shortest and longest wait, in seconds.
    pub fn range(self) -> (f64, f64) {
        match self {
            HookDelay::Short => (0.3, 0.6),
            HookDelay::Medium => (0.4, 0.9),
            HookDelay::Long => (0.6, 1.3),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HookDelay::Short => "Short",
            HookDelay::Medium => "Medium",
            HookDelay::Long => "Long",
        }
    }

    /// The name settings store.
    pub fn name(self) -> &'static str {
        match self {
            HookDelay::Short => "short",
            HookDelay::Medium => "medium",
            HookDelay::Long => "long",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        HookDelay::ALL
            .into_iter()
            .find(|delay| delay.name() == name)
    }
}

/// A xorshift generator: enough to spread out key presses, with no dependency.
#[derive(Clone, Debug, PartialEq)]
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        // Splitmix64 spreads similar seeds apart; xorshift's first outputs from a small seed are
        // small too. Xorshift never leaves zero, so zero is ruled out.
        let mut mixed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Random((mixed ^ (mixed >> 31)) | 1)
    }

    /// A number in `[0, 1)`.
    fn next(&mut self) -> f64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        (state >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The key is due to cast.
    Ready,
    /// The key was pressed; the fishing channel hasn't started yet.
    Casting,
    /// The key press started some other cast, such as applying a lure; the key is due again once
    /// it ends.
    Busy,
    /// The fishing channel runs; listening for the splash once the bobber settles.
    Waiting,
    /// A splash was heard; the key is due to hook.
    Hooking,
    /// The catch is being looted.
    Looting,
    /// The player took over; no key is pressed until they resume or cast by hand.
    Paused(PauseReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    /// The player ended a cast before a bite.
    Interrupted,
    /// A key press didn't start a cast.
    NoCast,
    /// The player has no fishing pole in hand.
    NoPole,
    /// The player moved, or was moved.
    Moved,
}

/// What the game shows the player doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Idle,
    /// Idle, with no fishing pole in hand.
    NoPole,
    /// Casting something other than fishing, such as applying a lure.
    Busy,
    Fishing,
}

impl Phase {
    /// The steps a cast goes through, for showing progress; `Ready` is the start of `Casting`.
    pub const STEPS: [Phase; 4] = [
        Phase::Casting,
        Phase::Waiting,
        Phase::Hooking,
        Phase::Looting,
    ];

    pub fn step(self) -> Phase {
        match self {
            Phase::Ready | Phase::Busy | Phase::Paused(_) => Phase::Casting,
            phase => phase,
        }
    }

    pub fn step_label(self) -> &'static str {
        match self.step() {
            Phase::Waiting => "Wait",
            Phase::Hooking => "Hook",
            Phase::Looting => "Loot",
            _ => "Cast",
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            Phase::Ready | Phase::Casting | Phase::Busy => "Casting",
            Phase::Waiting => "Waiting for a bite",
            Phase::Hooking => "Hooking",
            Phase::Looting => "Looting",
            Phase::Paused(_) => "Paused",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// The key was pressed on the splash.
    Hooked { after: f64 },
    /// The cast ran out with no splash.
    NoBite,
    /// A splash was heard, but the key couldn't be pressed in time.
    Missed,
    /// The player ended the cast before a bite.
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cast {
    pub outcome: Outcome,
    /// When the cast started.
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub phase: Phase,
    pub phase_started: f64,
    pub cast_started: f64,
    /// Finished casts, newest last.
    pub history: Vec<Cast>,
    hook_delay: HookDelay,
    random: Random,
    /// How long after the splash this bite's hook is due.
    reaction: f64,
    /// No cast is due before this, after the game window got focus.
    not_before: f64,
    /// Whether the fishing channel has been seen not running. A channel already running when the
    /// run began started at an unknown time, so its end can't be told from an interruption.
    channel_seen_off: bool,
    /// Whether `cast_started` is when the cast in progress really started.
    cast_start_known: bool,
    /// When the fishing channel that is running was first seen.
    channel_started: Option<f64>,
    /// What was heard over the last `SPLASH_WINDOW`, oldest first.
    recent: VecDeque<(f64, Level)>,
}

impl Run {
    /// `seed` picks the random hook delays; give each session a different one.
    pub fn new(now: f64, hook_delay: HookDelay, seed: u64) -> Self {
        Run {
            phase: Phase::Ready,
            phase_started: now,
            cast_started: now,
            history: Vec::new(),
            hook_delay,
            random: Random::new(seed),
            reaction: hook_delay.range().0,
            not_before: now,
            channel_seen_off: false,
            cast_start_known: true,
            channel_started: None,
            recent: VecDeque::new(),
        }
    }

    /// Casts so far, the one in progress included.
    pub fn casts(&self) -> usize {
        let in_progress = matches!(self.phase, Phase::Casting | Phase::Waiting | Phase::Hooking);
        self.history.len() + usize::from(in_progress)
    }

    pub fn hooked(&self) -> usize {
        self.history
            .iter()
            .filter(|cast| matches!(cast.outcome, Outcome::Hooked { .. }))
            .count()
    }

    /// Seconds since the cast in progress started, while one is.
    pub fn waited(&self, now: f64) -> Option<f64> {
        matches!(self.phase, Phase::Casting | Phase::Waiting).then(|| now - self.cast_started)
    }

    /// Whether the fishing key should be pressed now.
    pub fn key_due(&self, now: f64) -> bool {
        // The focus wait holds back casting only; a hook can't wait that long.
        match self.phase {
            Phase::Ready => now >= self.not_before,
            Phase::Hooking => now >= self.phase_started + self.reaction,
            _ => false,
        }
    }

    /// The caller pressed the key that was due.
    pub fn key_pressed(&mut self, now: f64) {
        match self.phase {
            Phase::Ready => {
                self.cast_started = now;
                self.enter(Phase::Casting, now);
            }
            Phase::Hooking => {
                let after = self.phase_started - self.cast_started;
                self.finish(Outcome::Hooked { after });
                self.enter(Phase::Looting, now);
            }
            _ => {}
        }
    }

    /// The game window just got focus; the next key press waits a random moment, as a person's
    /// would after clicking into the game.
    pub fn focus_gained(&mut self, now: f64) {
        let (shortest, longest) = FOCUS_DELAY;
        self.not_before = now + shortest + (longest - shortest) * self.random.next();
    }

    /// Picks up again after a pause; the key is due to cast.
    pub fn resume(&mut self, now: f64) {
        if let Phase::Paused(_) = self.phase {
            self.enter(Phase::Ready, now);
        }
    }

    /// The player moved, or was moved; a cast in progress is over.
    pub fn moved(&mut self, now: f64) {
        match self.phase {
            Phase::Paused(_) => return,
            Phase::Waiting | Phase::Hooking => self.finish(Outcome::Interrupted),
            Phase::Ready | Phase::Casting | Phase::Busy | Phase::Looting => {}
        }
        self.enter(Phase::Paused(PauseReason::Moved), now);
    }

    /// Takes in what the game shows the player doing.
    pub fn see(&mut self, now: f64, activity: Activity) {
        let fishing = activity == Activity::Fishing;
        let channel_started = match self.channel_started {
            _ if !fishing => None,
            None => Some(now),
            started => started,
        };
        self.channel_started = channel_started;
        // A channel older than the phase is the last cast's: a missed bite leaves it running, and
        // a hook shows it a moment longer.
        let new_channel = channel_started.filter(|started| *started >= self.phase_started);
        match self.phase {
            Phase::Casting if fishing => {
                self.cast_start_known = true;
                self.enter(Phase::Waiting, now);
            }
            // A cast made by hand, paused or not, is followed like one of ours.
            Phase::Ready | Phase::Busy | Phase::Paused(_) | Phase::Looting
                if new_channel.is_some() =>
            {
                self.cast_started = new_channel.unwrap_or(now);
                self.cast_start_known = self.channel_seen_off;
                self.enter(Phase::Waiting, now);
            }
            Phase::Ready | Phase::Casting | Phase::Busy if activity == Activity::NoPole => {
                self.enter(Phase::Paused(PauseReason::NoPole), now);
            }
            Phase::Casting if activity == Activity::Busy => self.enter(Phase::Busy, now),
            Phase::Busy if activity == Activity::Idle => self.enter(Phase::Ready, now),
            Phase::Waiting if !fishing => {
                if !self.cast_start_known || now - self.cast_started >= CAST_LENGTH - 1.0 {
                    self.finish(Outcome::NoBite);
                    self.enter(Phase::Ready, now);
                } else {
                    self.finish(Outcome::Interrupted);
                    self.enter(Phase::Paused(PauseReason::Interrupted), now);
                }
            }
            Phase::Hooking if !fishing => {
                self.finish(Outcome::Missed);
                self.enter(Phase::Ready, now);
            }
            _ => {}
        }
        if !fishing {
            self.channel_seen_off = true;
        }
    }

    /// Takes in the sound that just played.
    pub fn hear(&mut self, now: f64, level: Level) {
        let listening_from = self.cast_started + SETTLE_TIME;
        if self.phase != Phase::Waiting || now < listening_from {
            return;
        }
        self.recent.push_back((now, level));
        while self
            .recent
            .front()
            .is_some_and(|(at, _)| *at <= now - SPLASH_WINDOW)
        {
            self.recent.pop_front();
        }
        if now - listening_from < SPLASH_WINDOW {
            return;
        }
        let loud: Vec<Level> = self
            .recent
            .iter()
            .map(|(_, level)| *level)
            .filter(|level| level.loudness() > SPLASH_THRESHOLD)
            .collect();
        let centered = Level::mean(&loud).is_some_and(|heard| heard.balance().abs() <= MAX_BALANCE);
        if loud.len() * 2 >= self.recent.len() && centered {
            let (shortest, longest) = self.hook_delay.range();
            self.reaction = shortest + (longest - shortest) * self.random.next();
            self.enter(Phase::Hooking, now);
        }
    }

    /// Moves on from phases that ran their time.
    pub fn tick(&mut self, now: f64) {
        match self.phase {
            Phase::Casting if now >= self.phase_started + CAST_START_TIME => {
                self.enter(Phase::Paused(PauseReason::NoCast), now);
            }
            Phase::Waiting if now >= self.cast_started + CAST_LENGTH + SETTLE_TIME => {
                self.finish(Outcome::NoBite);
                self.enter(Phase::Ready, now);
            }
            Phase::Hooking if now >= self.phase_started + HOOK_WINDOW => {
                self.finish(Outcome::Missed);
                self.enter(Phase::Ready, now);
            }
            Phase::Looting if now >= self.phase_started + LOOT_TIME => {
                self.enter(Phase::Ready, now);
            }
            _ => {}
        }
    }

    fn enter(&mut self, phase: Phase, now: f64) {
        self.phase = phase;
        self.phase_started = now;
        self.recent.clear();
    }

    fn finish(&mut self, outcome: Outcome) {
        self.history.push(Cast {
            outcome,
            at: self.cast_started,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f64 = 0.02;

    fn quiet() -> Level {
        level(-90.0, 0.0)
    }

    fn splash() -> Level {
        level(-26.0, 1.0)
    }

    /// A level `loudness` dBFS loud, `balance` dB louder on the left.
    fn level(loudness: f32, balance: f32) -> Level {
        let mean = 10f32.powf(loudness / 10.0);
        let ratio = 10f32.powf(balance / 10.0);
        let right = 2.0 * mean / (1.0 + ratio);
        Level {
            left: right * ratio,
            right,
        }
    }

    /// Plays `level` for `seconds` from `from`, with the player doing `activity`, frame by
    /// frame; returns the end time.
    fn play(run: &mut Run, from: f64, seconds: f64, level: Level, activity: Activity) -> f64 {
        let mut now = from;
        while now < from + seconds {
            now += FRAME;
            run.see(now, activity);
            run.hear(now, level);
            run.tick(now);
        }
        now
    }

    /// Presses the key to cast at `now` and lets the channel start; returns the time after.
    fn cast(run: &mut Run, now: f64) -> f64 {
        assert!(run.key_due(now));
        run.key_pressed(now);
        assert_eq!(run.phase, Phase::Casting);
        let now = play(run, now, 0.3, quiet(), Activity::Idle);
        assert_eq!(run.phase, Phase::Casting, "the channel hasn't started");
        let now = play(run, now, FRAME, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);
        now
    }

    fn last_outcome(run: &Run) -> Option<Outcome> {
        run.history.last().map(|cast| cast.outcome)
    }

    #[test]
    fn measures_loudness_and_balance() {
        let full_left: Vec<i16> = [i16::MAX, 0].repeat(100);
        let Some(level) = Level::of(&full_left) else {
            panic!("no level")
        };
        assert!(
            level.loudness() > -4.0 && level.loudness() < -2.0,
            "{level:?}"
        );
        assert!(level.balance() > 60.0, "{level:?}");
        assert_eq!(Level::of(&[]), None);
    }

    #[test]
    fn hooks_on_a_splash_then_casts_again() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 9.0, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);

        let now = play(&mut run, now, 0.2, splash(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Hooking);
        assert!(!run.key_due(now), "waits for the hook delay");
        let now = play(
            &mut run,
            now,
            HookDelay::Medium.range().1,
            splash(),
            Activity::Fishing,
        );
        assert!(run.key_due(now));
        run.key_pressed(now);
        assert_eq!(run.phase, Phase::Looting);
        assert_eq!(run.hooked(), 1);
        let Some(Outcome::Hooked { after }) = last_outcome(&run) else {
            panic!("expected a hook, got {:?}", run.history)
        };
        assert!((after - 9.4).abs() < 0.1, "{after}");

        play(&mut run, now, LOOT_TIME + FRAME, quiet(), Activity::Idle);
        assert_eq!(run.phase, Phase::Ready);
        assert_eq!(run.casts(), 1);
    }

    #[test]
    fn ignores_the_cast_itself_short_sounds_and_sounds_off_to_the_side() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 1.5, splash(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting, "the cast's own sound");

        let now = play(&mut run, now, 3.0, quiet(), Activity::Fishing);
        let now = play(&mut run, now, 0.06, splash(), Activity::Fishing);
        let now = play(&mut run, now, 1.0, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting, "a click");

        play(&mut run, now, 1.0, level(-24.0, -13.0), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting, "a splash off to the side");
    }

    #[test]
    fn hook_delays_vary_within_their_range() {
        let mut run = Run::new(0.0, HookDelay::Long, 42);
        let mut delays = Vec::new();
        let mut now = 0.0;
        for _ in 0..20 {
            now = cast(&mut run, now);
            now = play(&mut run, now, 3.0, quiet(), Activity::Fishing);
            now = play(&mut run, now, 0.2, splash(), Activity::Fishing);
            assert_eq!(run.phase, Phase::Hooking);
            let heard = run.phase_started;
            while !run.key_due(now) {
                now = play(&mut run, now, FRAME, splash(), Activity::Fishing);
            }
            delays.push(now - heard);
            run.key_pressed(now);
            now = play(&mut run, now, LOOT_TIME + FRAME, quiet(), Activity::Idle);
        }
        let (shortest, longest) = HookDelay::Long.range();
        assert!(
            delays
                .iter()
                .all(|delay| *delay >= shortest && *delay < longest + FRAME * 2.0),
            "{delays:?}"
        );
        let spread = delays.iter().cloned().fold(0.0, f64::max)
            - delays.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread > (longest - shortest) / 2.0, "{delays:?}");
    }

    #[test]
    fn the_first_press_after_focus_waits_a_random_moment() {
        let (shortest, longest) = FOCUS_DELAY;
        let mut waits = Vec::new();
        for seed in 1..10 {
            let mut run = Run::new(0.0, HookDelay::Medium, seed);
            run.focus_gained(0.0);
            assert!(!run.key_due(shortest - FRAME));
            let mut now = 0.0;
            while !run.key_due(now) {
                now += FRAME;
            }
            assert!(now <= longest + FRAME, "{now}");
            waits.push(now);
        }
        waits.dedup();
        assert!(waits.len() > 1, "{waits:?}");
    }

    #[test]
    fn a_quiet_sound_isnt_a_bite() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 5.0, quiet(), Activity::Fishing);
        play(&mut run, now, 0.5, level(-41.0, 0.0), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);
    }

    #[test]
    fn no_fishing_pole_pauses() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = play(&mut run, 0.0, FRAME, quiet(), Activity::NoPole);
        assert_eq!(run.phase, Phase::Paused(PauseReason::NoPole));
        assert!(!run.key_due(now));
    }

    #[test]
    fn a_cast_with_no_bite_ends_with_the_channel() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, CAST_LENGTH - 0.3, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);
        play(&mut run, now, FRAME, quiet(), Activity::Idle);
        assert_eq!(run.phase, Phase::Ready);
        assert_eq!(last_outcome(&run), Some(Outcome::NoBite));
    }

    #[test]
    fn ending_a_cast_early_pauses_until_resumed() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 12.0, quiet(), Activity::Fishing);
        let now = play(&mut run, now, 10.0, quiet(), Activity::Idle);
        assert_eq!(run.phase, Phase::Paused(PauseReason::Interrupted));
        assert!(!run.key_due(now), "the player took over");
        assert_eq!(last_outcome(&run), Some(Outcome::Interrupted));

        run.resume(now);
        assert!(run.key_due(now));
    }

    #[test]
    fn a_press_that_doesnt_cast_pauses() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        run.key_pressed(0.0);
        let now = play(
            &mut run,
            0.0,
            CAST_START_TIME - 0.1,
            quiet(),
            Activity::Idle,
        );
        assert_eq!(run.phase, Phase::Casting);
        let now = play(&mut run, now, 5.0, quiet(), Activity::Idle);
        assert_eq!(run.phase, Phase::Paused(PauseReason::NoCast));
        assert!(!run.key_due(now));
        assert!(run.history.is_empty(), "no cast happened");
    }

    #[test]
    fn a_lure_is_waited_out_then_the_key_is_pressed_again() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        run.key_pressed(0.0);
        let now = play(&mut run, 0.0, 0.3, quiet(), Activity::Idle);
        let now = play(&mut run, now, 5.0, quiet(), Activity::Busy);
        assert_eq!(
            run.phase,
            Phase::Busy,
            "a lure takes longer than a cast starts"
        );
        assert!(!run.key_due(now));
        let now = play(&mut run, now, FRAME, quiet(), Activity::Idle);
        assert!(run.key_due(now), "cast once the lure is on");
        cast(&mut run, now);
    }

    #[test]
    fn a_cast_made_by_hand_is_followed() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        run.focus_gained(0.0);
        let now = play(&mut run, 0.0, FRAME, quiet(), Activity::Idle);
        let now = play(&mut run, now, FRAME, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);
        assert!(!run.key_due(now), "pressing now would reel the line in");

        let now = play(&mut run, now, 5.0, quiet(), Activity::Idle);
        assert_eq!(run.phase, Phase::Paused(PauseReason::Interrupted));
        play(&mut run, now, FRAME, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting, "casting by hand resumes");
    }

    #[test]
    fn a_cast_already_running_at_the_start_ends_without_a_pause() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = play(&mut run, 0.0, 10.0, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);
        play(&mut run, now, FRAME, quiet(), Activity::Idle);
        assert_eq!(
            run.phase,
            Phase::Ready,
            "its start is unknown, so its end isn't an interruption"
        );
        assert_eq!(last_outcome(&run), Some(Outcome::NoBite));
    }

    #[test]
    fn the_focus_wait_doesnt_hold_back_a_hook() {
        let mut run = Run::new(0.0, HookDelay::Short, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 3.0, quiet(), Activity::Fishing);
        let now = play(&mut run, now, 0.2, splash(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Hooking);
        run.focus_gained(now);
        let (_, longest) = HookDelay::Short.range();
        let now = play(&mut run, now, longest, splash(), Activity::Fishing);
        assert!(run.key_due(now));
    }

    #[test]
    fn a_bite_that_gets_away_is_a_miss() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 5.0, quiet(), Activity::Fishing);
        let now = play(&mut run, now, 0.2, splash(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Hooking);
        play(&mut run, now, FRAME, quiet(), Activity::Idle);
        assert_eq!(run.phase, Phase::Ready);
        assert_eq!(last_outcome(&run), Some(Outcome::Missed));
    }

    #[test]
    fn a_bite_missed_without_focus_recasts_over_the_old_channel() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 5.0, quiet(), Activity::Fishing);
        let now = play(&mut run, now, 0.2, splash(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Hooking);

        let now = play(&mut run, now, HOOK_WINDOW + 1.0, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Ready, "the old channel isn't a new cast");
        assert_eq!(last_outcome(&run), Some(Outcome::Missed));
        assert_eq!(run.casts(), 1);

        assert!(run.key_due(now));
        run.key_pressed(now);
        let now = play(&mut run, now, FRAME, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);
        assert_eq!(run.casts(), 2);
        play(&mut run, now, CAST_LENGTH, quiet(), Activity::Fishing);
        assert_eq!(
            run.history.len(),
            1,
            "no second outcome for the missed cast"
        );
    }

    #[test]
    fn a_cast_made_by_hand_while_looting_is_followed() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 3.0, quiet(), Activity::Fishing);
        let now = play(&mut run, now, 0.2, splash(), Activity::Fishing);
        let now = play(
            &mut run,
            now,
            HookDelay::Medium.range().1,
            splash(),
            Activity::Fishing,
        );
        run.key_pressed(now);
        let now = play(&mut run, now, 0.2, quiet(), Activity::Fishing);
        assert_eq!(
            run.phase,
            Phase::Looting,
            "the hooked channel shows a moment"
        );

        let now = play(&mut run, now, 0.5, quiet(), Activity::Idle);
        let now = play(&mut run, now, LOOT_TIME, quiet(), Activity::Fishing);
        assert_eq!(run.phase, Phase::Waiting);
        assert!(!run.key_due(now), "pressing now would reel the line in");
        assert_eq!(run.casts(), 2);
    }

    #[test]
    fn moving_pauses_from_any_phase() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        run.moved(0.0);
        assert_eq!(run.phase, Phase::Paused(PauseReason::Moved));
        assert!(run.history.is_empty());
        run.resume(0.0);

        let now = cast(&mut run, 0.0);
        let now = play(&mut run, now, 4.0, quiet(), Activity::Fishing);
        run.moved(now);
        assert_eq!(run.phase, Phase::Paused(PauseReason::Moved));
        assert_eq!(last_outcome(&run), Some(Outcome::Interrupted));
        let now = play(&mut run, now, 0.2, quiet(), Activity::Fishing);
        assert_eq!(
            run.phase,
            Phase::Paused(PauseReason::Moved),
            "the old channel shows a moment longer"
        );
        let now = play(&mut run, now, 5.0, quiet(), Activity::Idle);
        assert!(!run.key_due(now));
        assert_eq!(run.history.len(), 1);

        run.moved(now);
        run.resume(now);
        assert!(run.key_due(now));
    }

    #[test]
    fn a_pause_keeps_its_reason_when_the_player_moves() {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        let now = play(&mut run, 0.0, FRAME, quiet(), Activity::NoPole);
        run.moved(now);
        assert_eq!(run.phase, Phase::Paused(PauseReason::NoPole));
    }

    #[test]
    fn hook_delay_names_round_trip() {
        for delay in HookDelay::ALL {
            assert_eq!(HookDelay::from_name(delay.name()), Some(delay));
        }
        assert_eq!(HookDelay::from_name("slow"), None);
    }
}
