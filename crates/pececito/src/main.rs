mod angler;
mod ui;

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use rune::prefs::Prefs;
use rune::{AppSpec, Key, KeyEvent, Mods, PhysicalKeyCode, Shortcut, Tx, Waker};
use rune_kit::ShortcutRecorderState;

use angler::{Angler, Report};
use fishing::{HookDelay, Run};
use game::{AddonState, Detection, GameWindow, Keystroke, Watcher};

const APP_ID: &str = "pececito";
const FISHING_KEY_PREF: &str = "fishing_key";
const HOOK_DELAY_PREF: &str = "hook_delay";
const STOP_AFTER_PREF: &str = "stop_after_minutes";
/// Redraws a running session often enough for its timer and bite bar.
const SESSION_REFRESH: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default, PartialEq)]
enum Game {
    #[default]
    Searching,
    Detected(Detection),
    Error(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Screen {
    #[default]
    Main,
    Settings,
}

#[derive(Clone, Debug, PartialEq)]
struct Fishing {
    /// App clock time the session started; the run's times count from here.
    started: f64,
    run: Run,
    has_focus: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum StopReason {
    Stopped,
    GameClosed,
    /// The session ran as long as the "Stop after" setting allowed, named by its label.
    TimeUp(&'static str),
    AddonMissing,
    Failed(String),
}

/// Installing the addon writes into the game's folder, so it runs on its own thread.
enum Install {
    Idle,
    Running(Receiver<Result<(), String>>),
    /// The last install failed, until the next one.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
struct Finished {
    run: Run,
    duration: f64,
    reason: StopReason,
}

#[derive(Clone, Debug, Default, PartialEq)]
enum Session {
    #[default]
    Idle,
    Fishing(Fishing),
    /// Idle again, keeping the last session's results on screen.
    Stopped(Finished),
}

struct State {
    game: Game,
    session: Session,
    screen: Screen,
    /// The key bound to fishing in the game's own keybindings.
    fishing_key: ShortcutRecorderState,
    hook_delay: HookDelay,
    /// Index into `ui::STOP_AFTER`.
    stop_after: usize,
    clock: f64,
    prefs: Prefs,
    save_error: Option<String>,
    watcher: Option<Watcher>,
    angler: Option<Angler>,
    install: Install,
    /// Set by `setup`; tests have none, so starting a session there presses no keys.
    waker: Option<Waker>,
}

impl State {
    fn new(prefs: Prefs) -> Self {
        let fishing_key = load_fishing_key(&prefs);
        let hook_delay = prefs
            .get::<String>(HOOK_DELAY_PREF)
            .and_then(|name| HookDelay::from_name(&name))
            .unwrap_or_default();
        let stop_after = load_stop_after(&prefs);
        State {
            game: Game::default(),
            session: Session::default(),
            screen: Screen::default(),
            fishing_key: ShortcutRecorderState::new(fishing_key),
            hook_delay,
            stop_after,
            clock: 0.0,
            prefs,
            save_error: None,
            watcher: None,
            angler: None,
            install: Install::Idle,
            waker: None,
        }
    }

    fn window(&self) -> Option<&GameWindow> {
        match &self.game {
            Game::Detected(Detection::Found(window, _)) => Some(window),
            _ => None,
        }
    }

    fn addon(&self) -> Option<AddonState> {
        match &self.game {
            Game::Detected(Detection::Found(_, addon)) => Some(*addon),
            _ => None,
        }
    }

    fn is_fishing(&self) -> bool {
        matches!(self.session, Session::Fishing(_))
    }

    fn can_fish(&self) -> bool {
        !self.is_fishing()
            && self.fishing_key.shortcut().is_some()
            && !self.fishing_key.is_recording()
            && self.addon() == Some(AddonState::Showing)
    }

    fn start(&mut self, tx: &mut Tx) {
        if !self.can_fish() {
            return;
        }
        let (Some(window), Some(shortcut)) = (self.window().cloned(), self.fishing_key.shortcut())
        else {
            return;
        };
        self.clock = tx.clock();
        let keystroke = match Keystroke::parse(&shortcut.to_string()) {
            Ok(keystroke) => keystroke,
            Err(error) => {
                self.session = Session::Stopped(Finished {
                    run: Run::new(0.0, self.hook_delay, 0),
                    duration: 0.0,
                    reason: StopReason::Failed(error),
                });
                return;
            }
        };
        if let Some(waker) = self.waker.clone() {
            self.angler = Some(Angler::start(
                window,
                keystroke,
                self.hook_delay,
                move || waker(),
            ));
        }
        self.session = Session::Fishing(Fishing {
            started: self.clock,
            run: Run::new(0.0, self.hook_delay, 0),
            has_focus: true,
        });
        tx.wake_after(SESSION_REFRESH);
    }

    fn stop(&mut self) {
        self.end(StopReason::Stopped);
    }

    /// Clears a finished session's results; a running session has to be stopped first.
    fn reset_session(&mut self) {
        if let Session::Stopped(_) = self.session {
            self.session = Session::Idle;
        }
    }

    fn resume(&mut self) {
        let Session::Fishing(fishing) = &mut self.session else {
            return;
        };
        // The thread reports the same change; updating here keeps the button from lingering.
        fishing.run.resume(self.clock - fishing.started);
        if let Some(angler) = &self.angler {
            angler.resume();
        }
    }

    fn end(&mut self, reason: StopReason) {
        self.angler = None;
        if let Session::Fishing(fishing) = std::mem::take(&mut self.session) {
            self.session = Session::Stopped(Finished {
                run: fishing.run,
                duration: self.clock - fishing.started,
                reason,
            });
        }
    }

    fn apply(&mut self, result: Result<Detection, String>) {
        self.game = match result {
            Ok(detection) => Game::Detected(detection),
            Err(error) => Game::Error(error),
        };
        if self.window().is_none() {
            self.end(StopReason::GameClosed);
        }
    }

    fn receive(&mut self, report: Report) {
        let Session::Fishing(fishing) = &mut self.session else {
            return;
        };
        match report {
            Report::Run(run) => fishing.run = run,
            Report::Focus(has_focus) => fishing.has_focus = has_focus,
            Report::AddonMissing => self.end(StopReason::AddonMissing),
            Report::Failed(error) => self.end(StopReason::Failed(error)),
        }
    }

    /// Ends the session once it has run as long as settings allow.
    fn check_time_limit(&mut self) {
        let Session::Fishing(fishing) = &self.session else {
            return;
        };
        let limit = ui::STOP_AFTER
            .get(self.stop_after)
            .and_then(|(label, minutes)| minutes.map(|minutes| (*label, minutes)));
        if let Some((label, minutes)) = limit
            && self.clock - fishing.started >= minutes as f64 * 60.0
        {
            self.end(StopReason::TimeUp(label));
        }
    }

    fn open_settings(&mut self) {
        self.screen = Screen::Settings;
    }

    fn install_addon(&mut self) {
        if self.is_installing() {
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        let process = window.process.clone();
        let waker = self.waker.clone();
        let (sender, result) = mpsc::channel();
        thread::spawn(move || {
            let installed = game::install_addon(&process).map(|_| ());
            if sender.send(installed).is_ok()
                && let Some(waker) = waker
            {
                waker();
            }
        });
        self.install = Install::Running(result);
    }

    /// Takes in a finished install; returns whether one finished.
    fn finish_install(&mut self) -> bool {
        let Install::Running(result) = &self.install else {
            return false;
        };
        let installed = match result.try_recv() {
            Ok(installed) => installed,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("the install stopped unexpectedly".into()),
        };
        self.install = match installed {
            Ok(()) => {
                // The next scan says the same once it runs; the game shows it after /reload.
                if let Game::Detected(Detection::Found(_, addon)) = &mut self.game
                    && *addon == AddonState::NotInstalled
                {
                    *addon = AddonState::NotShowing;
                }
                Install::Idle
            }
            Err(error) => Install::Failed(error),
        };
        true
    }

    fn is_installing(&self) -> bool {
        matches!(self.install, Install::Running(_))
    }

    fn install_error(&self) -> Option<&str> {
        match &self.install {
            Install::Failed(error) => Some(error),
            Install::Idle | Install::Running(_) => None,
        }
    }

    fn close_settings(&mut self) {
        self.fishing_key.cancel();
        self.screen = Screen::Main;
    }

    fn save_fishing_key(&mut self, shortcut: Option<&Shortcut>) {
        let value = shortcut.map(Shortcut::to_string);
        let result = self.prefs.set(FISHING_KEY_PREF, &value);
        self.saved(result);
    }

    fn pick_hook_delay(&mut self, index: usize) {
        if let Some(hook_delay) = HookDelay::ALL.get(index) {
            self.hook_delay = *hook_delay;
            let result = self.prefs.set(HOOK_DELAY_PREF, hook_delay.name());
            self.saved(result);
        }
    }

    fn pick_stop_after(&mut self, index: usize) {
        if let Some((_, minutes)) = ui::STOP_AFTER.get(index) {
            self.stop_after = index;
            let result = self.prefs.set(STOP_AFTER_PREF, minutes);
            self.saved(result);
        }
    }

    fn saved(&mut self, result: Result<(), impl std::fmt::Display>) {
        self.save_error = result.err().map(|error| error.to_string());
    }
}

/// Space until the user records a key; a cleared key stays cleared.
fn load_fishing_key(prefs: &Prefs) -> Option<Shortcut> {
    let space = || Shortcut::from_physical_key(PhysicalKeyCode::Space, Mods::default()).ok();
    match prefs.get::<Option<String>>(FISHING_KEY_PREF) {
        None => space(),
        Some(None) => None,
        Some(Some(saved)) => match saved.parse() {
            Ok(shortcut) => Some(shortcut),
            Err(error) => {
                eprintln!("pececito: ignoring saved fishing key {saved:?}: {error}");
                space()
            }
        },
    }
}

/// "Never" until the user picks a limit; a saved value no longer offered also means "Never".
fn load_stop_after(prefs: &Prefs) -> usize {
    let never = ui::STOP_AFTER.len() - 1;
    let saved = prefs.get::<Option<u64>>(STOP_AFTER_PREF).flatten();
    ui::STOP_AFTER
        .iter()
        .position(|(_, minutes)| *minutes == saved)
        .unwrap_or(never)
}

fn init() -> State {
    State::new(Prefs::open(APP_ID))
}

fn setup(state: &mut State, waker: Waker) {
    state.waker = Some(waker.clone());
    state.watcher = Some(Watcher::start(move || waker()));
}

fn poll(state: &mut State, time: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    state.clock = time;
    if let Some(watcher) = &state.watcher {
        let results = watcher.drain();
        changed |= !results.is_empty();
        for result in results {
            state.apply(result);
        }
    }
    if let Some(angler) = &state.angler {
        let reports = angler.drain();
        changed |= !reports.is_empty();
        for report in reports {
            state.receive(report);
        }
    }
    changed |= state.finish_install();
    state.check_time_limit();
    if state.is_fishing() {
        tx.wake_after(SESSION_REFRESH);
        changed = true;
    }
    changed
}

fn on_key(state: &mut State, event: &KeyEvent, _tx: &mut Tx) -> bool {
    if state.screen == Screen::Settings && event.key == Key::Escape {
        state.close_settings();
        return true;
    }
    false
}

fn main() {
    // Desktops match the window to `pececito.desktop`, and through it to the installed icon.
    rune::set_app_id(APP_ID);
    rune::run(AppSpec::<State> {
        min_size: Some((380.0, 560.0)),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Pececito", (400.0, 600.0), init, ui::view)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use game::{GameProcess, Rect};
    use rune::{Env, PhysicalInput, Runtime};

    const SIZE: (f32, f32) = (400.0, 600.0);

    fn runtime_with(prefs: Prefs) -> Runtime<State> {
        let env = Env {
            system_dark: true,
            scale: 2.0,
        };
        let mut runtime = Runtime::new(State::new(prefs), ui::view, SIZE, env);
        runtime.frame(0.0);
        runtime
    }

    fn runtime() -> Runtime<State> {
        runtime_with(Prefs::memory())
    }

    fn tap(runtime: &mut Runtime<State>, key: &str) {
        let Some((x, y)) = runtime.find(key) else {
            panic!("no element {key}")
        };
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0 / 60.0);
    }

    fn press(runtime: &mut Runtime<State>, key: PhysicalKeyCode, mods: Mods) {
        for pressed in [true, false] {
            runtime.physical_input(PhysicalInput::Key {
                key,
                mods,
                pressed,
                repeat: false,
            });
        }
        runtime.frame(1.0 / 60.0);
    }

    fn fishing_key(runtime: &Runtime<State>) -> Option<Shortcut> {
        runtime.state.fishing_key.shortcut().cloned()
    }

    fn shortcut(key: PhysicalKeyCode, mods: Mods) -> Option<Shortcut> {
        Shortcut::from_physical_key(key, mods).ok()
    }

    fn shift() -> Mods {
        Mods {
            shift: true,
            ..Mods::default()
        }
    }

    fn found() -> Detection {
        found_with(AddonState::Showing)
    }

    fn found_with(addon: AddonState) -> Detection {
        let process = GameProcess {
            pid: 42,
            executable: r"C:\World of Warcraft\_forever_\Wow.exe".into(),
        };
        Detection::Found(
            GameWindow {
                process,
                window_id: 1,
                title: "World of Warcraft".into(),
                class: "steam_app_1".into(),
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
            },
            addon,
        )
    }

    fn ready() -> Runtime<State> {
        let mut runtime = runtime();
        runtime.update(|state, _tx| state.apply(Ok(found())));
        runtime.frame(1.0 / 60.0);
        runtime
    }

    /// Records a key from the main screen, through settings and back.
    fn record(runtime: &mut Runtime<State>, key: PhysicalKeyCode, mods: Mods) {
        tap(runtime, "settings");
        tap(runtime, "fishing-key");
        press(runtime, key, mods);
        tap(runtime, "back");
    }

    #[test]
    fn fish_is_locked_until_the_game_window_is_found() {
        let mut runtime = runtime();
        tap(&mut runtime, "fish");
        assert_eq!(runtime.state.session, Session::Idle, "still searching");

        runtime.update(|state, _tx| state.apply(Ok(Detection::NotRunning)));
        tap(&mut runtime, "fish");
        assert_eq!(runtime.state.session, Session::Idle, "game not running");

        runtime.update(|state, _tx| state.apply(Err("no X server".into())));
        tap(&mut runtime, "fish");
        assert_eq!(runtime.state.session, Session::Idle, "detection failed");
    }

    /// A run partway through its second cast, with the first one hooked.
    fn second_cast() -> Run {
        let mut run = Run::new(0.0, HookDelay::Medium, 7);
        run.key_pressed(0.0);
        run.see(0.5, fishing::Activity::Fishing);
        let splash = fishing::Level {
            left: 0.003,
            right: 0.003,
        };
        for frame in 0..30 {
            run.hear(9.0 + f64::from(frame) * 0.02, splash);
        }
        run.key_pressed(9.8);
        run.tick(13.0);
        run.key_pressed(13.0);
        run.see(13.5, fishing::Activity::Fishing);
        run
    }

    fn stop_reason(runtime: &Runtime<State>) -> Option<StopReason> {
        match &runtime.state.session {
            Session::Stopped(finished) => Some(finished.reason.clone()),
            _ => None,
        }
    }

    #[test]
    fn fish_starts_and_stop_keeps_the_results() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        assert!(runtime.state.is_fishing());
        assert!(runtime.find("fish").is_none(), "the button turns into Stop");

        runtime.update(|state, _tx| state.receive(Report::Run(second_cast())));
        let Session::Fishing(fishing) = &runtime.state.session else {
            panic!("expected fishing, got {:?}", runtime.state.session)
        };
        assert_eq!((fishing.run.hooked(), fishing.run.casts()), (1, 2));

        tap(&mut runtime, "stop");
        let Session::Stopped(finished) = &runtime.state.session else {
            panic!(
                "expected a stopped session, got {:?}",
                runtime.state.session
            )
        };
        assert_eq!(finished.run.hooked(), 1, "the results stay on screen");
        assert_eq!(finished.reason, StopReason::Stopped);
    }

    #[test]
    fn reset_clears_a_finished_session_only() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        runtime.update(|state, _tx| state.receive(Report::Run(second_cast())));
        tap(&mut runtime, "reset");
        assert!(runtime.state.is_fishing(), "disabled while fishing");

        tap(&mut runtime, "stop");
        tap(&mut runtime, "reset");
        assert_eq!(runtime.state.session, Session::Idle);
    }

    #[test]
    fn fishing_stops_when_the_game_goes_away() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        assert!(runtime.state.is_fishing());

        runtime.update(|state, _tx| state.apply(Ok(Detection::NotRunning)));
        assert_eq!(stop_reason(&runtime), Some(StopReason::GameClosed));
    }

    #[test]
    fn fish_is_locked_until_the_addon_shows() {
        let mut runtime = runtime();
        runtime.update(|state, _tx| state.apply(Ok(found_with(AddonState::NotInstalled))));
        runtime.frame(1.0 / 60.0);
        tap(&mut runtime, "fish");
        assert_eq!(runtime.state.session, Session::Idle);
        assert!(
            runtime.find("install-addon").is_some(),
            "offered on the main screen"
        );

        // The fake game has no folder to install into.
        tap(&mut runtime, "install-addon");
        assert!(runtime.state.is_installing());
        for _ in 0..200 {
            if !runtime.state.is_installing() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
            runtime.update(|state, tx| {
                poll(state, tx.clock(), tx);
            });
        }
        assert!(runtime.state.install_error().is_some());

        runtime.update(|state, _tx| state.apply(Ok(found_with(AddonState::NotShowing))));
        runtime.frame(1.0 / 60.0);
        tap(&mut runtime, "fish");
        assert_eq!(
            runtime.state.session,
            Session::Idle,
            "installed, not loaded yet"
        );

        runtime.update(|state, _tx| state.apply(Ok(found())));
        runtime.frame(1.0 / 60.0);
        tap(&mut runtime, "fish");
        assert!(runtime.state.is_fishing());
    }

    #[test]
    fn a_paused_session_resumes() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        let mut run = second_cast();
        run.see(20.0, fishing::Activity::Idle);
        assert!(matches!(run.phase, fishing::Phase::Paused(_)));
        runtime.update(|state, _tx| state.receive(Report::Run(run)));
        runtime.frame(1.0 / 60.0);

        tap(&mut runtime, "resume");
        let Session::Fishing(fishing) = &runtime.state.session else {
            panic!("expected fishing, got {:?}", runtime.state.session)
        };
        assert_eq!(fishing.run.phase, fishing::Phase::Ready);
    }

    #[test]
    fn losing_the_addon_stops_fishing() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        runtime.update(|state, _tx| state.receive(Report::AddonMissing));
        assert_eq!(stop_reason(&runtime), Some(StopReason::AddonMissing));
    }

    #[test]
    fn a_failure_stops_fishing_and_says_why() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        runtime.update(|state, _tx| state.receive(Report::Failed("no sound".into())));
        runtime.frame(1.0 / 60.0);
        assert_eq!(
            stop_reason(&runtime),
            Some(StopReason::Failed("no sound".into()))
        );
        assert!(runtime.find("fish").is_some(), "fishing can start again");
    }

    #[test]
    fn losing_focus_shows_on_the_session() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        runtime.update(|state, _tx| state.receive(Report::Focus(false)));
        assert!(matches!(
            &runtime.state.session,
            Session::Fishing(Fishing {
                has_focus: false,
                ..
            })
        ));
    }

    #[test]
    fn the_session_ends_after_the_time_limit() {
        let mut runtime = ready();
        runtime.update(|state, _tx| state.pick_stop_after(0));
        tap(&mut runtime, "fish");
        let started = runtime.state.clock;
        runtime.update(|state, tx| {
            poll(state, started + 29.0 * 60.0, tx);
        });
        assert!(runtime.state.is_fishing());
        runtime.update(|state, tx| {
            poll(state, started + 30.0 * 60.0, tx);
        });
        assert_eq!(stop_reason(&runtime), Some(StopReason::TimeUp("30 min")));
    }

    #[test]
    fn hooking_and_session_settings_are_saved() -> Result<(), std::io::Error> {
        let directory =
            std::env::temp_dir().join(format!("pececito-settings-{}", std::process::id()));
        let path = directory.join("prefs.json");

        let fresh = State::new(Prefs::at(&path));
        let mut runtime = runtime_with(Prefs::at(&path));
        runtime.update(|state, _tx| {
            state.pick_hook_delay(2);
            state.pick_stop_after(1);
        });
        let restored = State::new(Prefs::at(&path));

        std::fs::remove_dir_all(&directory)?;
        assert_eq!(fresh.hook_delay, HookDelay::Medium);
        assert_eq!(fresh.stop_after, ui::STOP_AFTER.len() - 1, "Never");
        assert_eq!(runtime.state.save_error, None);
        assert_eq!(restored.hook_delay, HookDelay::Long);
        assert_eq!(restored.stop_after, 1);
        Ok(())
    }

    #[test]
    fn settings_open_and_close() {
        let mut runtime = runtime();
        assert!(
            runtime.find("fishing-key").is_none(),
            "the key lives in settings"
        );
        tap(&mut runtime, "settings");
        assert_eq!(runtime.state.screen, Screen::Settings);
        assert!(runtime.find("fishing-key").is_some());

        tap(&mut runtime, "back");
        assert_eq!(runtime.state.screen, Screen::Main);

        tap(&mut runtime, "settings");
        runtime.update(|state, tx| {
            let escape = KeyEvent {
                key: Key::Escape,
                mods: Mods::default(),
            };
            on_key(state, &escape, tx);
        });
        assert_eq!(runtime.state.screen, Screen::Main, "Escape goes back");
    }

    #[test]
    fn a_missing_key_points_to_settings() {
        let mut runtime = ready();
        record(&mut runtime, PhysicalKeyCode::Backspace, Mods::default());
        assert_eq!(fishing_key(&runtime), None);

        tap(&mut runtime, "fish");
        assert_eq!(runtime.state.session, Session::Idle);
        tap(&mut runtime, "open-settings");
        assert_eq!(runtime.state.screen, Screen::Settings);
    }

    #[test]
    fn the_fishing_key_starts_as_space() {
        let runtime = runtime();
        assert_eq!(
            fishing_key(&runtime),
            shortcut(PhysicalKeyCode::Space, Mods::default())
        );
    }

    #[test]
    fn records_a_bare_key_and_a_chord() {
        let mut runtime = runtime();
        record(&mut runtime, PhysicalKeyCode::KeyF, Mods::default());
        assert_eq!(
            fishing_key(&runtime),
            shortcut(PhysicalKeyCode::KeyF, Mods::default())
        );

        record(&mut runtime, PhysicalKeyCode::Digit1, shift());
        assert_eq!(
            fishing_key(&runtime),
            shortcut(PhysicalKeyCode::Digit1, shift())
        );
    }

    #[test]
    fn the_fishing_key_is_locked_while_fishing() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        tap(&mut runtime, "settings");
        tap(&mut runtime, "fishing-key");
        assert!(!runtime.state.fishing_key.is_recording());
    }

    #[test]
    fn the_fishing_key_is_saved_and_restored() -> Result<(), std::io::Error> {
        let directory = std::env::temp_dir().join(format!("pececito-prefs-{}", std::process::id()));
        let path = directory.join("prefs.json");

        let mut runtime = runtime_with(Prefs::at(&path));
        record(&mut runtime, PhysicalKeyCode::KeyF, shift());
        let save_error = runtime.state.save_error.clone();
        let restored = load_fishing_key(&Prefs::at(&path));

        record(&mut runtime, PhysicalKeyCode::Backspace, Mods::default());
        let cleared = load_fishing_key(&Prefs::at(&path));

        std::fs::remove_dir_all(&directory)?;
        assert_eq!(save_error, None);
        assert_eq!(restored, shortcut(PhysicalKeyCode::KeyF, shift()));
        assert_eq!(cleared, None, "a cleared key stays cleared");
        Ok(())
    }

    #[test]
    fn a_broken_saved_key_falls_back_to_space() {
        let mut prefs = Prefs::memory();
        assert!(prefs.set(FISHING_KEY_PREF, &Some("not a key")).is_ok());
        assert_eq!(
            load_fishing_key(&prefs),
            shortcut(PhysicalKeyCode::Space, Mods::default())
        );
    }
}
