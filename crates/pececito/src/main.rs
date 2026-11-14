mod ui;

use std::time::Duration;

use rune::prefs::Prefs;
use rune::{AppSpec, Key, KeyEvent, Mods, PhysicalKeyCode, Shortcut, Tx, Waker};
use rune_kit::ShortcutRecorderState;

use fishing::mock::Run;
use game::{Detection, GameWindow, Watcher};

const APP_ID: &str = "pececito";
const FISHING_KEY_PREF: &str = "fishing_key";
/// Redraws a running session often enough for its timers and bite bar.
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
struct Finished {
    run: Run,
    duration: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
enum Session {
    #[default]
    Idle,
    Fishing(Run),
    /// Idle again, keeping the last session's results on screen.
    Stopped(Finished),
}

struct State {
    game: Game,
    session: Session,
    screen: Screen,
    /// The key bound to fishing in the game's own keybindings.
    fishing_key: ShortcutRecorderState,
    /// Index into `ui::SENSITIVITIES`; not wired to detection yet.
    sensitivity: usize,
    /// Index into `ui::STOP_AFTER`; not wired to the session yet.
    stop_after: usize,
    clock: f64,
    prefs: Prefs,
    save_error: Option<String>,
    watcher: Option<Watcher>,
}

impl State {
    fn new(prefs: Prefs) -> Self {
        let fishing_key = load_fishing_key(&prefs);
        State {
            game: Game::default(),
            session: Session::default(),
            screen: Screen::default(),
            fishing_key: ShortcutRecorderState::new(fishing_key),
            sensitivity: 1,
            stop_after: ui::STOP_AFTER.len() - 1,
            clock: 0.0,
            prefs,
            save_error: None,
            watcher: None,
        }
    }

    fn window(&self) -> Option<&GameWindow> {
        match &self.game {
            Game::Detected(Detection::Found(window)) => Some(window),
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
            && self.window().is_some()
    }

    fn start(&mut self, tx: &mut Tx) {
        if self.can_fish() {
            self.clock = tx.clock();
            self.session = Session::Fishing(Run::new(self.clock));
            tx.wake_after(SESSION_REFRESH);
        }
    }

    fn stop(&mut self) {
        if let Session::Fishing(run) = std::mem::take(&mut self.session) {
            let duration = self.clock - run.started;
            self.session = Session::Stopped(Finished { run, duration });
        }
    }

    fn apply(&mut self, result: Result<Detection, String>) {
        self.game = match result {
            Ok(detection) => Game::Detected(detection),
            Err(error) => Game::Error(error),
        };
        if self.window().is_none() {
            self.stop();
        }
    }

    fn open_settings(&mut self) {
        self.screen = Screen::Settings;
    }

    fn close_settings(&mut self) {
        self.fishing_key.cancel();
        self.screen = Screen::Main;
    }

    fn save_fishing_key(&mut self, shortcut: Option<&Shortcut>) {
        let value = shortcut.map(Shortcut::to_string);
        self.save_error = self
            .prefs
            .set(FISHING_KEY_PREF, &value)
            .err()
            .map(|error| error.to_string());
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

fn init() -> State {
    State::new(Prefs::open(APP_ID))
}

fn setup(state: &mut State, waker: Waker) {
    state.watcher = Some(Watcher::start(move || waker()));
}

fn poll(state: &mut State, time: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(watcher) = &state.watcher {
        let results = watcher.drain();
        changed = !results.is_empty();
        for result in results {
            state.apply(result);
        }
    }
    state.clock = time;
    if let Session::Fishing(run) = &mut state.session {
        run.tick(time);
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
        let process = GameProcess {
            pid: 42,
            executable: r"C:\World of Warcraft\_forever_\Wow.exe".into(),
        };
        Detection::Found(GameWindow {
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
        })
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

    #[test]
    fn fish_starts_and_stop_keeps_the_results() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        assert!(runtime.state.is_fishing());
        assert!(runtime.find("fish").is_none(), "the button turns into Stop");

        for _ in 0..(10 * 30) {
            runtime.update(|state, tx| {
                poll(state, tx.clock(), tx);
            });
            runtime.frame(0.1);
        }
        tap(&mut runtime, "stop");
        let Session::Stopped(finished) = &runtime.state.session else {
            panic!(
                "expected a stopped session, got {:?}",
                runtime.state.session
            )
        };
        assert!(
            finished.run.catches() > 0,
            "the mock lands fish in 30 seconds"
        );
        assert!(finished.duration > 29.0, "{}", finished.duration);
    }

    #[test]
    fn fishing_stops_when_the_game_goes_away() {
        let mut runtime = ready();
        tap(&mut runtime, "fish");
        assert!(runtime.state.is_fishing());

        runtime.update(|state, _tx| state.apply(Ok(Detection::NotRunning)));
        assert!(matches!(runtime.state.session, Session::Stopped(_)));
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
