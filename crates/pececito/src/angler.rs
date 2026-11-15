use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fishing::{Activity, HookDelay, Level, Loot, Run};
use game::{FocusReader, GameAudio, GameWindow, Keyboard, Keystroke, PlayerState, SignalReader};

/// The longest the loop goes without a tick when the game sends no sound.
const TICK: Duration = Duration::from_millis(50);
/// How often focus is checked while no key is due, so the UI can ask for it early.
const FOCUS_CHECK: Duration = Duration::from_secs(1);
/// How often the addon's squares are read.
const SIGNAL_CHECK: Duration = Duration::from_millis(100);
/// How long the addon's squares may be gone, as during a loading screen, before fishing stops.
const SIGNAL_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum Report {
    Run(Run),
    /// The names of items the addon has seen looted, by item id.
    ItemNames(HashMap<u32, String>),
    /// Whether the game window has focus, which the loop needs to press keys.
    Focus(bool),
    /// The Pececito addon isn't showing in the game, so fishing stopped.
    AddonMissing,
    Failed(String),
}

#[derive(Debug)]
enum Command {
    /// Pick up again after a pause.
    Resume,
}

/// Fishes on its own thread: listens to the game and presses the fishing key until dropped.
/// The thread stops within a tick and cleans up after itself, so dropping doesn't wait for it.
pub struct Angler {
    /// Dropping it is also what tells the thread to stop.
    commands: Sender<Command>,
    reports: Receiver<Report>,
}

impl Angler {
    /// `wake` runs on the angler's thread after each report, so the caller can drain them.
    pub fn start(
        window: GameWindow,
        keystroke: Keystroke,
        hook_delay: HookDelay,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let (commands, command_receiver) = mpsc::channel();
        let (sender, reports) = mpsc::channel();
        thread::spawn(move || {
            let report = |report| {
                if sender.send(report).is_ok() {
                    wake();
                }
            };
            let fishing = Fishing {
                window: &window,
                keystroke: &keystroke,
                commands: &command_receiver,
            };
            if let Err(error) = fishing.fish(hook_delay, &report) {
                report(Report::Failed(error));
            }
        });
        Angler { commands, reports }
    }

    pub fn resume(&self) {
        if self.commands.send(Command::Resume).is_err() {
            eprintln!("pececito: can't resume, the fishing thread already ended");
        }
    }

    /// The reports that arrived since the last call, oldest first.
    pub fn drain(&self) -> Vec<Report> {
        self.reports.try_iter().collect()
    }
}

struct Fishing<'a> {
    window: &'a GameWindow,
    keystroke: &'a Keystroke,
    commands: &'a Receiver<Command>,
}

impl Fishing<'_> {
    fn fish(&self, hook_delay: HookDelay, report: &dyn Fn(Report)) -> Result<(), String> {
        let started = Instant::now();
        let signal = SignalReader::new(self.window)?;
        let focus_reader = FocusReader::new(self.window)?;
        if signal.read()?.is_none() {
            report(Report::AddonMissing);
            return Ok(());
        }
        report(Report::ItemNames(game::item_names(&self.window.process)));
        let mut keyboard = Keyboard::new()?;
        let audio = GameAudio::open(self.window.process.pid)?;
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        let mut run = Run::new(0.0, hook_delay, seed);
        report(Report::Run(run.clone()));
        let mut focus: Option<(bool, Instant)> = None;
        let mut activity = Activity::Idle;
        // A change counts once two reads in a row agree; single reads flicker as the channel ends.
        let mut previous_read = None;
        let mut moves = None;
        let mut loot_serial = None;
        let mut signal_read = Instant::now();
        let mut signal_seen = Instant::now();
        loop {
            let chunk = audio.next(TICK)?;
            let now = started.elapsed().as_secs_f64();
            let before = (run.phase, run.history.len());
            let mut looted = false;
            let Some(commands) = self.commands() else {
                return Ok(());
            };
            for command in commands {
                match command {
                    Command::Resume => run.resume(now),
                }
            }
            if signal_read.elapsed() >= SIGNAL_CHECK {
                signal_read = Instant::now();
                // A minimized or partly off-screen window can't be read; it counts as the
                // squares being gone, so the grace period applies.
                let result = signal.read();
                let read = *result.as_ref().unwrap_or(&None);
                match read {
                    None if signal_seen.elapsed() >= SIGNAL_GRACE => {
                        if let Err(error) = result {
                            return Err(format!(
                                "Pececito can't see the game window. Keep it on screen and not minimized. ({error})"
                            ));
                        }
                        report(Report::AddonMissing);
                        return Ok(());
                    }
                    None => {}
                    Some(frame) => {
                        if read == previous_read {
                            if moves.is_some_and(|moves| moves != frame.moves) {
                                run.moved(now);
                            }
                            moves = Some(frame.moves);
                            if loot_serial.is_some_and(|serial| serial != frame.loot.serial) {
                                looted |= run.looted(
                                    now,
                                    Loot {
                                        item: frame.loot.item,
                                        count: frame.loot.count,
                                    },
                                );
                            }
                            loot_serial = Some(frame.loot.serial);
                            activity = match frame.state {
                                PlayerState::Fishing => Activity::Fishing,
                                PlayerState::Busy => Activity::Busy,
                                PlayerState::NoPole => Activity::NoPole,
                                PlayerState::Idle => Activity::Idle,
                            };
                        }
                        signal_seen = signal_read;
                    }
                }
                previous_read = read;
            }
            run.see(now, activity);
            if let Some(level) = chunk.as_deref().and_then(Level::of) {
                run.hear(now, level);
            }
            run.tick(now);

            let key_due = run.key_due(now);
            let checked_recently =
                focus.is_some_and(|(_, checked)| checked.elapsed() < FOCUS_CHECK);
            if key_due || !checked_recently {
                let has_focus = focus_reader.has_focus()?;
                if focus.map(|(had_focus, _)| had_focus) != Some(has_focus) {
                    report(Report::Focus(has_focus));
                    if has_focus {
                        run.focus_gained(now);
                    }
                }
                focus = Some((has_focus, Instant::now()));
                if has_focus && run.key_due(now) {
                    keyboard.press(self.keystroke)?;
                    run.key_pressed(now);
                }
            }
            if looted || (run.phase, run.history.len()) != before {
                report(Report::Run(run.clone()));
            }
        }
    }

    /// The commands sent since the last call, or `None` once the `Angler` is gone.
    fn commands(&self) -> Option<Vec<Command>> {
        let mut commands = Vec::new();
        loop {
            match self.commands.try_recv() {
                Ok(command) => commands.push(command),
                Err(TryRecvError::Empty) => return Some(commands),
                Err(TryRecvError::Disconnected) => return None,
            }
        }
    }
}
