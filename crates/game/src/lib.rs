//! Finds the WoW Forever client on this machine, listens to its sound and presses its keys.

mod addon;
mod audio;
#[cfg(target_os = "linux")]
mod keyboard;
#[cfg(not(target_os = "linux"))]
mod keyboard {
    /// Key presses go through `/dev/uinput`, which only Linux has.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Keystroke;

    impl Keystroke {
        pub fn parse(_text: &str) -> Result<Self, String> {
            Ok(Keystroke)
        }
    }

    pub struct Keyboard;

    impl Keyboard {
        pub fn new() -> Result<Self, String> {
            Err("Pressing keys isn't supported on this system yet".into())
        }

        pub fn press(&mut self, _keystroke: &Keystroke) -> Result<(), String> {
            Err("Pressing keys isn't supported on this system yet".into())
        }
    }
}
mod process;
mod watcher;
mod window;

use std::path::Path;

use addon::addon_installed;
pub use addon::{Signal, SignalReader, install_addon};
pub use audio::{CHUNK_FRAMES, GameAudio, SAMPLE_RATE};
pub use keyboard::{Keyboard, Keystroke};
pub use watcher::Watcher;
pub use window::FocusReader;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameProcess {
    pub pid: u32,
    pub executable: String,
}

/// A window's frame in X11 root coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameWindow {
    pub process: GameProcess,
    pub window_id: u32,
    pub title: String,
    pub class: String,
    pub geometry: Rect,
}

/// The Pececito addon, as seen in a running game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddonState {
    NotInstalled,
    /// Installed, but its squares aren't on screen: the game hasn't loaded it since it was
    /// installed, or it's turned off.
    NotShowing,
    /// Showing, and the player has no fishing pole in hand.
    NoPole,
    Showing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detection {
    NotRunning,
    /// The client is running but has no mapped top-level window yet (still loading).
    NoWindow(GameProcess),
    Found(GameWindow, AddonState),
}

pub fn detect() -> Result<Detection, String> {
    if cfg!(not(target_os = "linux")) {
        return Err("Finding the game isn't supported on this system yet".into());
    }
    let Some(process) = process::find_process(Path::new("/proc"))? else {
        return Ok(Detection::NotRunning);
    };
    let Some(window) = window::find_window(&process)? else {
        return Ok(Detection::NoWindow(process));
    };
    let addon = if addon_installed(&process) {
        // A minimized or partly off-screen window can't be read; the squares count as hidden.
        match SignalReader::new(&window).and_then(|reader| reader.read()) {
            Ok(Signal::Missing) => AddonState::NotShowing,
            Ok(Signal::NoPole) => AddonState::NoPole,
            Ok(Signal::Idle | Signal::Busy | Signal::Fishing) => AddonState::Showing,
            Err(error) => {
                eprintln!("pececito: {error}");
                AddonState::NotShowing
            }
        }
    } else {
        AddonState::NotInstalled
    };
    Ok(Detection::Found(window, addon))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the game running: prints what the Pececito addon shows.
    #[test]
    #[ignore]
    fn reads_the_addon() -> Result<(), String> {
        let Detection::Found(window, _) = detect()? else {
            return Err("the game window isn't open".into());
        };
        println!("addon: {:?}", SignalReader::new(&window)?.read()?);
        Ok(())
    }

    /// Needs the game running: prints whether it has focus, for checking against the desktop.
    #[test]
    #[ignore]
    fn reports_focus() -> Result<(), String> {
        let Detection::Found(window, _) = detect()? else {
            return Err("the game window isn't open".into());
        };
        let focus = FocusReader::new(&window)?;
        println!(
            "active window: {:?}, game window: {:#x}, has focus: {}",
            focus.active_window()?,
            window.window_id,
            focus.has_focus()?
        );
        Ok(())
    }

    /// Needs the game running: `cargo test -p game -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn detects_the_running_client() -> Result<(), String> {
        let detection = detect()?;
        println!("{detection:#?}");
        assert!(
            matches!(detection, Detection::Found(..)),
            "expected a window, got {detection:?}"
        );
        Ok(())
    }
}
