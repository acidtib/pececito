//! Finds the WoW Forever client on this machine: its process, then its window.

mod process;
mod watcher;
mod window;

use std::path::Path;

pub use watcher::Watcher;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detection {
    NotRunning,
    /// The client is running but has no mapped top-level window yet (still loading).
    NoWindow(GameProcess),
    Found(GameWindow),
}

pub fn detect() -> Result<Detection, String> {
    let Some(process) = process::find_process(Path::new("/proc"))? else {
        return Ok(Detection::NotRunning);
    };
    match window::find_window(&process)? {
        Some(window) => Ok(Detection::Found(window)),
        None => Ok(Detection::NoWindow(process)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the game running: `cargo test -p game -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn detects_the_running_client() -> Result<(), String> {
        let detection = detect()?;
        println!("{detection:#?}");
        assert!(
            matches!(detection, Detection::Found(_)),
            "expected a window, got {detection:?}"
        );
        Ok(())
    }
}
