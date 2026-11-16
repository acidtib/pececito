// Release builds on Windows open without a console window; debug builds keep it for error output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod angler;
mod settings;
mod ui;
mod update;

mod item_icons {
    include!(concat!(env!("OUT_DIR"), "/item_icons.rs"));
}

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Local};
use rune::prefs::{Prefs, PrefsError};
use rune::{
    AppSpec, Display, Image, ImageCache, Key, KeyEvent, Live, Mods, PhysicalKeyCode, Shortcut, Tx,
    Waker, WindowHandle, WindowOptions, WindowSpec,
};
use rune_kit::ShortcutRecorderState;

use angler::{Angler, Report};
use fishing::{HookDelay, Run};
use game::{AddonState, Detection, GameWindow, Keystroke, Watcher};
use settings::Settings;
use update::Update;

const APP_ID: &str = "pececito";
const LOGO_SVG: &[u8] = include_bytes!("../../../assets/icon/pececito.svg");
const WINDOW_SIZE: (f32, f32) = (400.0, 600.0);
const FISHING_KEY_PREF: &str = "fishing_key";
const HOOK_DELAY_PREF: &str = "hook_delay";
const FISH_FOR_PREF: &str = "fish_for_minutes";
/// The window frame's top-left in physical desktop pixels.
const WINDOW_POSITION_PREF: &str = "window_position";
/// Where "Fish for" was saved when it was called "Stop after".
const STOP_AFTER_PREF: &str = "stop_after_minutes";
const DEFAULT_FISH_FOR_MINUTES: Option<u64> = Some(10);
/// Redraws a running session often enough for its timer and bite bar.
const SESSION_REFRESH: Duration = Duration::from_millis(100);
/// How long closing the window waits for settings to reach the file.
const SAVE_ON_CLOSE_LIMIT: Duration = Duration::from_secs(2);

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

/// The Fish for popover next to Start fishing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FishForPicker {
    #[default]
    Closed,
    Open,
    /// A drag on the slider shows this length; the session limit changes only on release.
    Dragging(usize),
}

#[derive(Clone, Debug, PartialEq)]
struct Fishing {
    /// App clock time the session started; the run's times count from here.
    started: f64,
    /// When the session started on the wall clock, for the times the casts list shows.
    started_at: DateTime<Local>,
    run: Run,
    has_focus: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum StopReason {
    Stopped,
    GameClosed,
    /// The session ran as long as the "Fish for" setting allowed, named by its label.
    TimeUp(&'static str),
    AddonMissing,
    Failed(String),
}

/// Work that writes files or runs other programs, on its own thread.
enum Job {
    Idle,
    Running(Receiver<Result<(), String>>),
    /// The last run failed, until the next one.
    Failed(String),
}

impl Job {
    fn start(
        &mut self,
        waker: Option<Waker>,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        let (sender, result) = mpsc::channel();
        thread::spawn(move || {
            if sender.send(work()).is_ok()
                && let Some(waker) = waker
            {
                waker();
            }
        });
        *self = Job::Running(result);
    }

    /// The result of a run that has just finished.
    fn finish(&mut self) -> Option<Result<(), String>> {
        let Job::Running(result) = self else {
            return None;
        };
        let finished = match result.try_recv() {
            Ok(finished) => finished,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("It stopped unexpectedly.".into()),
        };
        *self = match &finished {
            Ok(()) => Job::Idle,
            Err(error) => Job::Failed(error.clone()),
        };
        Some(finished)
    }

    fn is_running(&self) -> bool {
        matches!(self, Job::Running(_))
    }

    fn error(&self) -> Option<&str> {
        match self {
            Job::Failed(error) => Some(error),
            Job::Idle | Job::Running(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Finished {
    run: Run,
    started_at: DateTime<Local>,
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
    /// Index into `ui::FISH_FOR`.
    fish_for: usize,
    fish_for_picker: FishForPicker,
    clock: f64,
    settings: Settings,
    /// The last settings write that failed, until one succeeds.
    save_error: Option<String>,
    watcher: Option<Watcher>,
    angler: Option<Angler>,
    /// The names of items the addon has seen looted, by item id.
    item_names: HashMap<u32, String>,
    /// The icons of the items fishing can give, by item id; loaded in `setup`.
    item_icons: HashMap<u32, Image>,
    install: Job,
    open_config: Job,
    update: Update,
    /// Set by `setup`; tests have none, so starting a session there presses no keys.
    waker: Option<Waker>,
    /// Pececito's own window, to save where it sits when it closes.
    app_window: Option<WindowHandle>,
    /// The app icon for the About section, decoded off the UI thread.
    logo: Option<Image>,
    cast_list: CastList,
}

/// Where the casts list sits, so a new cast at the top stays in view while the list is there.
struct CastList {
    /// How far the list is scrolled down, in points; set by the list as it scrolls.
    offset: Live<f32>,
    /// Rows the list showed after the last `poll`.
    shown: usize,
}

impl State {
    fn new(prefs: Prefs) -> Self {
        let fishing_key = load_fishing_key(&prefs);
        let hook_delay = prefs
            .get::<String>(HOOK_DELAY_PREF)
            .and_then(|name| HookDelay::from_name(&name))
            .unwrap_or_default();
        let fish_for = load_fish_for(&prefs);
        State {
            game: Game::default(),
            session: Session::default(),
            screen: Screen::default(),
            fishing_key: ShortcutRecorderState::new(fishing_key),
            hook_delay,
            fish_for,
            fish_for_picker: FishForPicker::Closed,
            clock: 0.0,
            settings: Settings::start(prefs),
            save_error: None,
            watcher: None,
            angler: None,
            item_names: HashMap::new(),
            item_icons: HashMap::new(),
            install: Job::Idle,
            open_config: Job::Idle,
            update: Update::Unchecked,
            waker: None,
            app_window: None,
            logo: None,
            cast_list: CastList {
                offset: Live::new(0.0),
                shown: 0,
            },
        }
    }

    /// Indexes into the session's history of the casts the list shows, newest first. A hook
    /// waiting for its loot stays out, so its row shows the item from the start.
    fn shown_casts(&self) -> Vec<usize> {
        match &self.session {
            Session::Fishing(fishing) => {
                let now = self.clock - fishing.started;
                let history = &fishing.run.history;
                (0..history.len())
                    .rev()
                    .filter(|&index| !history[index].awaits_loot(now))
                    .collect()
            }
            Session::Stopped(finished) => (0..finished.run.history.len()).rev().collect(),
            Session::Idle => Vec::new(),
        }
    }

    /// Virtual rows keep the reader's row in place when one is added above, even at the top,
    /// so the list moves back up to show a new cast there.
    fn follow_new_casts(&mut self, tx: &mut Tx) {
        let shown = self.shown_casts().len();
        if shown > self.cast_list.shown && self.cast_list.offset.get() < 1.0 {
            tx.scroll_to_y("casts-scroll", 0.0);
        }
        self.cast_list.shown = shown;
    }

    fn window(&self) -> Option<&GameWindow> {
        match &self.game {
            Game::Detected(Detection::Found(window, _)) => Some(window),
            _ => None,
        }
    }

    fn addon(&self) -> Option<AddonState> {
        match &self.game {
            Game::Detected(Detection::Found(_, addon)) => Some(addon.clone()),
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
                    started_at: Local::now(),
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
            started_at: Local::now(),
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
                started_at: fishing.started_at,
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
            Report::ItemNames(names) => self.item_names.extend(names),
            Report::Focus(has_focus) => fishing.has_focus = has_focus,
            Report::AddonMissing => self.end(StopReason::AddonMissing),
            Report::Failed(error) => self.end(StopReason::Failed(error)),
        }
    }

    /// Ends the session once it has run as long as settings allow; returns whether it did.
    fn check_time_limit(&mut self) -> bool {
        let Session::Fishing(fishing) = &self.session else {
            return false;
        };
        let limit = ui::FISH_FOR
            .get(self.fish_for)
            .and_then(|length| length.minutes.map(|minutes| (length.label, minutes)));
        let Some((label, minutes)) = limit else {
            return false;
        };
        let length = minutes as f64 * 60.0;
        if self.clock - fishing.started < length {
            return false;
        }
        if let Some(angler) = self.angler.take() {
            angler.reel_in();
        }
        self.end(StopReason::TimeUp(label));
        // Polls land a little past the limit; the session ran as long as it was set to.
        if let Session::Stopped(finished) = &mut self.session {
            finished.duration = length;
        }
        true
    }

    fn open_settings(&mut self) {
        self.fish_for_picker = FishForPicker::Closed;
        self.screen = Screen::Settings;
    }

    fn toggle_fish_for(&mut self) {
        self.fish_for_picker = match self.fish_for_picker {
            FishForPicker::Closed => FishForPicker::Open,
            FishForPicker::Open | FishForPicker::Dragging(_) => FishForPicker::Closed,
        };
    }

    fn close_fish_for(&mut self) {
        self.fish_for_picker = FishForPicker::Closed;
    }

    fn is_fish_for_open(&self) -> bool {
        self.fish_for_picker != FishForPicker::Closed
    }

    /// The length the slider shows: a drag's, else the one in use.
    fn shown_fish_for(&self) -> usize {
        match self.fish_for_picker {
            FishForPicker::Dragging(index) => index,
            FishForPicker::Closed | FishForPicker::Open => self.fish_for,
        }
    }

    fn drag_fish_for(&mut self, index: usize) {
        if self.is_fish_for_open() && index < ui::FISH_FOR.len() {
            self.fish_for_picker = FishForPicker::Dragging(index);
        }
    }

    /// Ends a drag: a release keeps the length shown, a cancelled drag drops it.
    fn end_fish_for_drag(&mut self, keep: bool) {
        let FishForPicker::Dragging(index) = self.fish_for_picker else {
            return;
        };
        self.fish_for_picker = FishForPicker::Open;
        if keep {
            self.pick_fish_for(index);
        }
    }

    fn install_addon(&mut self) {
        if self.is_installing() {
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        let process = window.process.clone();
        self.install.start(self.waker.clone(), move || {
            game::install_addon(&process).map(|_| ())
        });
    }

    fn open_config_file(&mut self) {
        if self.open_config.is_running() {
            return;
        }
        let Some(path) = self.settings.path().map(Path::to_path_buf) else {
            return;
        };
        self.open_config
            .start(self.waker.clone(), move || open_file(&path));
    }

    /// Checks for a newer release, or gets the one a check found.
    fn advance_update(&mut self) {
        let waker = self.waker.clone();
        self.update.advance(move || {
            if let Some(waker) = waker {
                waker();
            }
        });
    }

    /// Writes the settings still at their defaults, so the file lists every one, and drops the
    /// older "Stop after" key once its value lives under "Fish for".
    fn write_missing_settings(&mut self) {
        let fishing_key = self.fishing_key.shortcut().map(Shortcut::to_string);
        let hook_delay = self.hook_delay.name();
        let minutes = ui::FISH_FOR
            .get(self.fish_for)
            .and_then(|length| length.minutes);
        self.save(move |prefs| {
            if !prefs.contains(FISHING_KEY_PREF) {
                prefs.set(FISHING_KEY_PREF, &fishing_key)?;
            }
            if !prefs.contains(HOOK_DELAY_PREF) {
                prefs.set(HOOK_DELAY_PREF, hook_delay)?;
            }
            if !prefs.contains(FISH_FOR_PREF) {
                prefs.set(FISH_FOR_PREF, &minutes)?;
            }
            prefs.remove(STOP_AFTER_PREF)
        });
    }

    /// Queues a write for the settings thread; `poll` takes in how it went.
    fn save(&mut self, write: impl FnOnce(&mut Prefs) -> Result<(), PrefsError> + Send + 'static) {
        let queued = self
            .settings
            .write(move |prefs| write(prefs).map_err(|error| error.to_string()));
        if let Err(error) = queued {
            self.save_error = Some(error);
        }
    }

    /// Takes in finished settings writes; returns whether any finished.
    fn finish_saves(&mut self) -> bool {
        let results = self.settings.drain();
        for result in &results {
            self.save_error = result.as_ref().err().cloned();
        }
        !results.is_empty()
    }

    /// Takes in finished background work; returns whether any finished.
    fn finish_jobs(&mut self) -> bool {
        let installed = self.install.finish();
        // The next scan says the same once it runs; the game shows it after /reload.
        if let Some(Ok(())) = installed
            && let Game::Detected(Detection::Found(_, addon)) = &mut self.game
            && matches!(addon, AddonState::NotInstalled | AddonState::Mismatched(_))
        {
            *addon = AddonState::NotShowing;
        }
        let opened = self.open_config.finish();
        let updated = self.update.finish();
        installed.is_some() || opened.is_some() || updated
    }

    fn is_installing(&self) -> bool {
        self.install.is_running()
    }

    fn install_error(&self) -> Option<&str> {
        self.install.error()
    }

    fn close_settings(&mut self) {
        self.fishing_key.cancel();
        self.screen = Screen::Main;
    }

    fn save_fishing_key(&mut self, shortcut: Option<&Shortcut>) {
        let value = shortcut.map(Shortcut::to_string);
        self.save(move |prefs| prefs.set(FISHING_KEY_PREF, &value));
    }

    fn pick_hook_delay(&mut self, index: usize) {
        if let Some(hook_delay) = HookDelay::ALL.get(index) {
            self.hook_delay = *hook_delay;
            let name = hook_delay.name();
            self.save(move |prefs| prefs.set(HOOK_DELAY_PREF, name));
        }
    }

    fn pick_fish_for(&mut self, index: usize) {
        if index == self.fish_for {
            return;
        }
        if let Some(length) = ui::FISH_FOR.get(index) {
            self.fish_for = index;
            let minutes = length.minutes;
            self.save(move |prefs| prefs.set(FISH_FOR_PREF, &minutes));
        }
    }
}

/// Opens `path` in the desktop's default app for it.
fn open_file(path: &Path) -> Result<(), String> {
    match path.try_exists() {
        Ok(true) => {}
        Ok(false) => {
            return Err("The settings file is missing. Change a setting to write it again.".into());
        }
        Err(error) => return Err(format!("Can't read {}: {error}", path.display())),
    }
    update::open(path.as_os_str())
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

/// 10 minutes until the user picks a length; a saved null is Forever, and a saved length no
/// longer offered goes back to 10 minutes. Reads the older "Stop after" key when the newer one
/// isn't saved.
fn load_fish_for(prefs: &Prefs) -> usize {
    let find = |minutes: Option<u64>| {
        ui::FISH_FOR
            .iter()
            .position(|length| length.minutes == minutes)
    };
    let default = find(DEFAULT_FISH_FOR_MINUTES).unwrap_or(0);
    prefs
        .get::<Option<u64>>(FISH_FOR_PREF)
        .or_else(|| prefs.get::<Option<u64>>(STOP_AFTER_PREF))
        .and_then(find)
        .unwrap_or(default)
}

fn init() -> State {
    let mut state = State::new(Prefs::open(APP_ID));
    // The settings file exists from the first run, so Open always has something to show.
    state.write_missing_settings();
    state
}

/// Loads each built-in icon once; items that share an icon share its image.
fn load_item_icons(images: &ImageCache) -> HashMap<u32, Image> {
    let by_name: HashMap<&str, Image> = item_icons::ITEM_ICONS
        .iter()
        .map(|&(name, bytes)| (name, images.load(bytes)))
        .collect();
    game::fishing_loot_icons()
        .filter_map(|(item, icon)| Some((item, by_name.get(icon)?.clone())))
        .collect()
}

fn setup(state: &mut State, waker: Waker) {
    state.waker = Some(waker.clone());
    state.settings.set_waker(waker.clone());
    // Rasterized at 2x so it stays sharp on HiDPI; smaller scales draw from its mips.
    let images = ImageCache::new(waker.clone());
    state.logo = Some(images.svg(LOGO_SVG, ui::LOGO_SIZE, 2.0));
    state.item_icons = load_item_icons(&images);
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
    changed |= state.finish_jobs();
    changed |= state.finish_saves();
    changed |= state.check_time_limit();
    state.follow_new_casts(tx);
    if state.is_fishing() {
        tx.wake_after(SESSION_REFRESH);
        changed = true;
    }
    changed
}

fn on_key(state: &mut State, event: &KeyEvent, _tx: &mut Tx) -> bool {
    if state.is_fish_for_open() && event.key == Key::Escape {
        state.close_fish_for();
        return true;
    }
    if state.screen == Screen::Settings && event.key == Key::Escape {
        state.close_settings();
        return true;
    }
    false
}

/// Saves where the window sits. Wayland keeps window positions from apps, so there it saves
/// nothing and the compositor places the window.
fn save_window_position(state: &mut State, _tx: &mut Tx) {
    let Some(placement) = state.app_window.as_ref().and_then(WindowHandle::placement) else {
        return;
    };
    if placement.minimized || placement.maximized || placement.full_screen {
        return;
    }
    let position = placement.position;
    state.save(move |prefs| prefs.set(WINDOW_POSITION_PREF, &position));
    // The process may end once the window closes; let queued writes land first.
    if let Err(error) = state.settings.flush(SAVE_ON_CLOSE_LIMIT) {
        eprintln!("pececito: {error}");
    }
    for result in state.settings.drain() {
        if let Err(error) = result {
            eprintln!("pececito: couldn't save settings: {error}");
        }
    }
}

/// The saved position, while a display still shows the top of the window to grab it by.
fn saved_window_position(prefs: &Prefs, displays: &[Display]) -> Option<(i32, i32)> {
    let (x, y) = prefs.get::<(i32, i32)>(WINDOW_POSITION_PREF)?;
    let grab = (i64::from(x) + 48, i64::from(y) + 16);
    displays
        .iter()
        .any(|display| {
            let (left, top) = (i64::from(display.position.0), i64::from(display.position.1));
            let (width, height) = (i64::from(display.size.0), i64::from(display.size.1));
            (left..left + width).contains(&grab.0) && (top..top + height).contains(&grab.1)
        })
        .then_some((x, y))
}

fn main() {
    // Desktops match the window to `pececito.desktop`, and through it to the installed icon.
    rune::set_app_id(APP_ID);
    let spec = AppSpec::<State> {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Pececito", WINDOW_SIZE, init, ui::view)
    };
    let app = rune::App::new(spec)
        .on_close(save_window_position)
        .on_quit(save_window_position);
    let handle = app.handle();
    let app = app.on_window(move |window| {
        let window = window.clone();
        handle.update(move |state, _tx| state.app_window = Some(window));
    });
    let prefs = Prefs::open(APP_ID);
    rune::run_windows(move |displays| {
        let options = WindowOptions {
            min_size: Some(WINDOW_SIZE),
            max_size: Some(WINDOW_SIZE),
            resizable: false,
            position: saved_window_position(&prefs, displays),
            ..WindowOptions::default()
        };
        vec![WindowSpec::from_app("main", app).options(options)]
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use game::{GameProcess, Rect};
    use rune::{Cursor, Env, PhysicalInput, Runtime};

    fn runtime_with(prefs: Prefs) -> Runtime<State> {
        let env = Env {
            system_dark: true,
            scale: 2.0,
        };
        let mut runtime = Runtime::new(State::new(prefs), ui::view, WINDOW_SIZE, env);
        runtime.frame(0.0);
        runtime
    }

    /// Waits for queued settings writes and takes in their results, as `poll` does.
    fn settle_saves(runtime: &mut Runtime<State>) {
        assert_eq!(runtime.state.settings.flush(Duration::from_secs(5)), Ok(()));
        runtime.update(|state, _tx| {
            state.finish_saves();
        });
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

    fn element_rect(runtime: &Runtime<State>, key: &str) -> Option<[f32; 4]> {
        runtime
            .elements()
            .into_iter()
            .find(|element| element.key.as_deref() == Some(key) && !element.exiting)
            .map(|element| element.rect)
    }

    /// Whether the row keyed `key` is built and wholly inside the casts panel.
    fn in_view(runtime: &Runtime<State>, key: &str) -> bool {
        let Some(panel) = element_rect(runtime, "casts") else {
            panic!("no casts panel")
        };
        element_rect(runtime, key)
            .is_some_and(|rect| rect[1] >= panel[1] && rect[1] + rect[3] <= panel[1] + panel[3])
    }

    fn built_rows(runtime: &Runtime<State>) -> usize {
        runtime
            .elements()
            .iter()
            .filter(|element| {
                element.key.as_deref().is_some_and(|key| {
                    key.strip_prefix("cast-")
                        .is_some_and(|index| index.parse::<usize>().is_ok())
                })
            })
            .count()
    }

    fn push_cast(state: &mut State) {
        if let Session::Stopped(finished) = &mut state.session {
            let at = finished.run.history.len() as f64 * 20.0;
            finished.run.history.push(fishing::Cast {
                outcome: fishing::Outcome::NoBite,
                at,
                loot: Vec::new(),
            });
        }
    }

    #[test]
    fn recent_casts_scroll_through_the_whole_session() {
        let mut runtime = runtime();
        runtime.update(|state, _tx| {
            state.session = Session::Stopped(Finished {
                run: Run::new(0.0, state.hook_delay, 0),
                started_at: Local::now(),
                duration: 600.0,
                reason: StopReason::Stopped,
            });
            for _ in 0..1_000 {
                push_cast(state);
            }
        });
        runtime.frame(0.1);
        assert!(in_view(&runtime, "cast-999"));
        assert!(!in_view(&runtime, "cast-0"));
        assert!(
            built_rows(&runtime) < 50,
            "only rows near the view are built"
        );

        runtime.update(|state, tx| {
            push_cast(state);
            poll(state, tx.clock(), tx);
        });
        runtime.frame(0.1);
        assert!(
            in_view(&runtime, "cast-1000"),
            "a new cast shows at the top"
        );
        let (Some(newest), Some(previous)) = (
            element_rect(&runtime, "cast-1000"),
            element_rect(&runtime, "cast-999"),
        ) else {
            panic!("the newest casts aren't built")
        };
        assert!(newest[1] < previous[1]);

        let Some((x, y)) = runtime.find("casts") else {
            panic!("no casts panel")
        };
        runtime.wheel(x, y, -100_000.0);
        for _ in 0..20 {
            runtime.frame(0.1);
        }
        assert!(in_view(&runtime, "cast-0"));

        runtime.update(|state, tx| {
            push_cast(state);
            poll(state, tx.clock(), tx);
        });
        runtime.frame(0.1);
        assert!(
            in_view(&runtime, "cast-0"),
            "a new cast leaves older casts in view"
        );
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
        runtime.update(|state, _tx| state.pick_fish_for(0));
        tap(&mut runtime, "fish");
        let started = runtime.state.clock;
        runtime.update(|state, tx| {
            poll(state, started + 4.0 * 60.0, tx);
        });
        assert!(runtime.state.is_fishing());
        let mut redrew = false;
        runtime.update(|state, tx| {
            redrew = poll(state, started + 5.0 * 60.0 + 0.08, tx);
        });
        assert!(redrew, "the screen has to show the session ended");
        assert_eq!(stop_reason(&runtime), Some(StopReason::TimeUp("5 min")));
        let Session::Stopped(finished) = &runtime.state.session else {
            panic!(
                "expected a stopped session, got {:?}",
                runtime.state.session
            )
        };
        assert_eq!(finished.duration, 5.0 * 60.0);
    }

    #[test]
    fn clickable_controls_show_the_hand_cursor() {
        let mut runtime = runtime();
        let cursor_over = |runtime: &mut Runtime<State>, key: &str| {
            let Some((x, y)) = runtime.find(key) else {
                panic!("no element keyed {key}");
            };
            runtime.pointer_move(x, y);
            runtime.frame(0.1);
            runtime.cursor()
        };
        assert_eq!(cursor_over(&mut runtime, "settings"), Cursor::Pointer);
        assert_eq!(
            cursor_over(&mut runtime, "fish-for-button"),
            Cursor::Pointer
        );
        // Nothing to reset before a session, so it keeps the arrow.
        assert_eq!(cursor_over(&mut runtime, "reset"), Cursor::Default);
    }

    #[test]
    fn fish_for_snaps_to_the_nearest_length() {
        assert_eq!(ui::nearest_length(0.0), 0);
        assert_eq!(ui::nearest_length(0.2), 1);
        assert_eq!(
            ui::nearest_length(0.7),
            3,
            "the gap before Forever belongs to 2 hours"
        );
        assert_eq!(ui::nearest_length(0.8), 4);
        assert_eq!(ui::nearest_length(1.0), ui::FISH_FOR.len() - 1);
    }

    #[test]
    fn fish_for_opens_above_the_buttons_and_closes() {
        let mut runtime = runtime();
        assert!(runtime.find("fish-for").is_none());
        tap(&mut runtime, "fish-for-button");
        assert!(runtime.find("fish-for").is_some(), "the slider shows");

        runtime.update(|state, tx| {
            let escape = KeyEvent {
                key: Key::Escape,
                mods: Mods::default(),
            };
            on_key(state, &escape, tx);
        });
        assert!(!runtime.state.is_fish_for_open(), "Escape closes it");
        // The closing popover takes presses until it fades.
        runtime.frame(0.5);

        tap(&mut runtime, "fish-for-button");
        tap(&mut runtime, "fish-for-button");
        assert!(
            !runtime.state.is_fish_for_open(),
            "the button closes it too"
        );
    }

    #[test]
    fn a_saved_forever_stays_forever() {
        let mut prefs = Prefs::memory();
        assert!(prefs.set(FISH_FOR_PREF, &None::<u64>).is_ok());
        assert_eq!(load_fish_for(&prefs), ui::FISH_FOR.len() - 1);
        assert!(prefs.set(FISH_FOR_PREF, &Some(30_u64)).is_ok());
        assert_eq!(load_fish_for(&prefs), 1, "no longer offered");
    }

    #[test]
    fn a_saved_window_position_needs_a_display_to_show_it() {
        let display = |position: (i32, i32)| Display {
            id: None,
            name: None,
            position,
            size: (1920, 1080),
            scale: 1.0,
            primary: false,
        };
        let displays = [display((0, 0)), display((1920, 0))];
        let mut prefs = Prefs::memory();
        assert_eq!(saved_window_position(&prefs, &displays), None, "none saved");

        assert!(prefs.set(WINDOW_POSITION_PREF, &(2400, 300)).is_ok());
        assert_eq!(
            saved_window_position(&prefs, &displays),
            Some((2400, 300)),
            "on the second display"
        );
        assert_eq!(
            saved_window_position(&prefs, &displays[..1]),
            None,
            "that display is gone"
        );
    }

    #[test]
    fn a_saved_stop_after_carries_over() {
        let saved = |minutes: Option<u64>| {
            let mut prefs = Prefs::memory();
            assert!(prefs.set(STOP_AFTER_PREF, &minutes).is_ok());
            load_fish_for(&prefs)
        };
        assert_eq!(ui::FISH_FOR[saved(Some(120))].label, "2 hours");
        assert_eq!(ui::FISH_FOR[saved(None)].label, "Forever");
        assert_eq!(
            ui::FISH_FOR[saved(Some(30))].label,
            "10 min",
            "no 30 min any more"
        );

        let mut prefs = Prefs::memory();
        assert!(prefs.set(STOP_AFTER_PREF, &Some(120_u64)).is_ok());
        assert!(prefs.set(FISH_FOR_PREF, &Some(5_u64)).is_ok());
        assert_eq!(
            ui::FISH_FOR[load_fish_for(&prefs)].label,
            "5 min",
            "the newer key wins"
        );
    }

    #[test]
    fn the_old_stop_after_key_is_dropped_once_carried_over() -> Result<(), String> {
        let directory =
            std::env::temp_dir().join(format!("pececito-legacy-{}", std::process::id()));
        let path = directory.join("prefs.json");
        let mut legacy = Prefs::at(&path);
        legacy
            .set(STOP_AFTER_PREF, &Some(60_u64))
            .map_err(|error| error.to_string())?;
        let mut runtime = runtime_with(Prefs::at(&path));
        runtime.update(|state, _tx| state.write_missing_settings());
        settle_saves(&mut runtime);
        let written = Prefs::at(&path);

        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
        assert!(!written.contains(STOP_AFTER_PREF));
        assert_eq!(written.get::<Option<u64>>(FISH_FOR_PREF), Some(Some(60)));
        Ok(())
    }

    #[test]
    fn a_failed_save_shows_its_error() -> Result<(), std::io::Error> {
        let blocker = std::env::temp_dir().join(format!("pececito-blocked-{}", std::process::id()));
        std::fs::write(&blocker, "a file where a folder should be")?;
        let mut runtime = runtime_with(Prefs::at(blocker.join("prefs.json")));
        runtime.update(|state, _tx| state.pick_hook_delay(0));
        settle_saves(&mut runtime);

        std::fs::remove_file(&blocker)?;
        assert!(runtime.state.save_error.is_some());
        assert_eq!(
            runtime.state.hook_delay,
            HookDelay::Short,
            "kept for this run"
        );
        Ok(())
    }

    #[test]
    fn dragging_fish_for_changes_the_limit_only_on_release() {
        let mut runtime = runtime();
        let before = runtime.state.fish_for;
        tap(&mut runtime, "fish-for-button");
        let Some((x, y)) = runtime.find("fish-for") else {
            panic!("no slider")
        };
        // The slider's left end is 5 min.
        let left = x - 150.0;
        runtime.pointer_down(left, y);
        runtime.pointer_move(left + 2.0, y);
        runtime.frame(1.0 / 60.0);
        assert_eq!(runtime.state.shown_fish_for(), 0, "the drag shows 5 min");
        assert_eq!(
            runtime.state.fish_for, before,
            "the limit holds while dragging"
        );

        runtime.pointer_up(left + 2.0, y);
        runtime.frame(1.0 / 60.0);
        assert_eq!(runtime.state.fish_for, 0, "released on 5 min");
    }

    #[test]
    fn a_cancelled_fish_for_drag_changes_nothing() {
        let mut runtime = runtime();
        let before = runtime.state.fish_for;
        tap(&mut runtime, "fish-for-button");
        runtime.update(|state, _tx| state.drag_fish_for(0));
        runtime.update(|state, _tx| state.end_fish_for_drag(false));
        assert_eq!(runtime.state.fish_for, before);
        assert_eq!(runtime.state.fish_for_picker, FishForPicker::Open);
    }

    #[test]
    fn fish_for_steps_with_the_keyboard() {
        let mut runtime = runtime();
        runtime.update(|state, tx| ui::step_fish_for(state, 1, tx));
        assert_eq!(runtime.state.fish_for, 2, "10 minutes up to 1 hour");
        runtime.update(|state, tx| ui::step_fish_for(state, 10, tx));
        assert_eq!(
            runtime.state.fish_for,
            ui::FISH_FOR.len() - 1,
            "clamped at Forever"
        );
        runtime.update(|state, tx| ui::step_fish_for(state, -10, tx));
        assert_eq!(runtime.state.fish_for, 0, "clamped at the shortest");
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
            state.pick_fish_for(1);
        });
        settle_saves(&mut runtime);
        let restored = State::new(Prefs::at(&path));

        std::fs::remove_dir_all(&directory)?;
        assert_eq!(fresh.hook_delay, HookDelay::Medium);
        assert_eq!(fresh.fish_for, 1, "10 minutes");
        assert_eq!(runtime.state.save_error, None);
        assert_eq!(restored.hook_delay, HookDelay::Long);
        assert_eq!(restored.fish_for, 1);
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
    fn default_settings_are_written_to_the_file() -> Result<(), String> {
        let directory =
            std::env::temp_dir().join(format!("pececito-defaults-{}", std::process::id()));
        let path = directory.join("prefs.json");
        let mut runtime = runtime_with(Prefs::at(&path));
        runtime.update(|state, _tx| state.pick_hook_delay(2));
        runtime.update(|state, _tx| state.write_missing_settings());
        settle_saves(&mut runtime);
        let save_error = runtime.state.save_error.clone();
        let written = Prefs::at(&path);

        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
        assert_eq!(save_error, None);
        assert_eq!(
            written.get::<String>(HOOK_DELAY_PREF),
            Some("long".into()),
            "kept"
        );
        assert_eq!(
            written.get::<Option<String>>(FISHING_KEY_PREF),
            Some(Some("space".into()))
        );
        assert_eq!(written.get::<Option<u64>>(FISH_FOR_PREF), Some(Some(10)));
        Ok(())
    }

    #[test]
    fn settings_kept_in_memory_have_no_file_to_open() {
        let mut runtime = runtime();
        tap(&mut runtime, "settings");
        tap(&mut runtime, "open-config");
        assert!(matches!(runtime.state.open_config, Job::Idle));
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
        settle_saves(&mut runtime);
        let save_error = runtime.state.save_error.clone();
        let restored = load_fishing_key(&Prefs::at(&path));

        record(&mut runtime, PhysicalKeyCode::Backspace, Mods::default());
        settle_saves(&mut runtime);
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
