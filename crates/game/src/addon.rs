use std::fs;
use std::path::{Path, PathBuf};

use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
use x11rb::rust_connection::RustConnection;

use crate::{GameProcess, GameWindow};

const TOC: &str = include_str!("../../../addon/Pececito/Pececito.toc");
const LUA: &str = include_str!("../../../addon/Pececito/Pececito.lua");
const ICON: &[u8] = include_bytes!("../../../addon/Pececito/Icon.png");

/// The corner the addon draws its 9 by 3 pixel strip in, with room to spare.
const REGION_WIDTH: u16 = 48;
const REGION_HEIGHT: u16 = 16;
/// How far a pixel may stray from the addon's colors and still count.
const TOLERANCE: u8 = 40;
const MAGENTA: [u8; 3] = [255, 0, 255];
const CYAN: [u8; 3] = [0, 255, 255];
const FISHING: [u8; 3] = [0, 255, 0];
const IDLE: [u8; 3] = [0, 0, 255];
const BUSY: [u8; 3] = [255, 255, 0];
const NO_POLE: [u8; 3] = [255, 0, 0];

/// What the Pececito addon shows in the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// The addon's squares aren't on screen: it isn't installed or loaded.
    Missing,
    Idle,
    /// Idle, with no fishing pole in hand.
    NoPole,
    /// Casting something other than fishing, such as applying a lure.
    Busy,
    Fishing,
}

/// Writes the addon into the game's `Interface/AddOns`, returning its folder.
pub fn install_addon(process: &GameProcess) -> Result<PathBuf, String> {
    // Proton starts the client in its own folder, so the working directory is the game folder.
    let game = fs::read_link(format!("/proc/{}/cwd", process.pid))
        .map_err(|error| format!("Can't find the game's folder: {error}"))?;
    if !game.join("Wow.exe").is_file() {
        return Err(format!(
            "Can't find the game's folder: {} has no Wow.exe",
            game.display()
        ));
    }
    let folder = game.join("Interface").join("AddOns").join("Pececito");
    let write = |name: &str, contents: &[u8]| -> Result<(), String> {
        let path = folder.join(name);
        fs::write(&path, contents)
            .map_err(|error| format!("Can't write {}: {error}", path.display()))
    };
    fs::create_dir_all(&folder)
        .map_err(|error| format!("Can't create {}: {error}", folder.display()))?;
    write("Pececito.toc", TOC.as_bytes())?;
    write("Pececito.lua", LUA.as_bytes())?;
    write("Icon.png", ICON)?;
    Ok(folder)
}

/// Whether the addon is installed in the game's folder.
pub fn addon_installed(process: &GameProcess) -> bool {
    fs::read_link(format!("/proc/{}/cwd", process.pid)).is_ok_and(|game| addon_files_present(&game))
}

fn addon_files_present(game: &Path) -> bool {
    let folder = game.join("Interface").join("AddOns").join("Pececito");
    ["Pececito.toc", "Pececito.lua", "Icon.png"]
        .iter()
        .all(|name| folder.join(name).is_file())
}

/// Reads the addon's squares from the game window, keeping one X connection for repeated reads.
pub struct SignalReader {
    connection: RustConnection,
    window: u32,
}

impl SignalReader {
    pub fn new(window: &GameWindow) -> Result<Self, String> {
        let (connection, _) =
            x11rb::connect(None).map_err(|error| format!("connecting to the X server: {error}"))?;
        Ok(SignalReader {
            connection,
            window: window.window_id,
        })
    }

    pub fn read(&self) -> Result<Signal, String> {
        let failed = |error: &dyn std::fmt::Display| format!("reading the game window: {error}");
        let geometry = self
            .connection
            .get_geometry(self.window)
            .map_err(|error| failed(&error))?
            .reply()
            .map_err(|error| failed(&error))?;
        let width = REGION_WIDTH.min(geometry.width);
        let height = REGION_HEIGHT.min(geometry.height);
        let image = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                0,
                0,
                width,
                height,
                u32::MAX,
            )
            .map_err(|error| failed(&error))?
            .reply()
            .map_err(|error| failed(&error))?;
        let pixels = usize::from(width) * usize::from(height);
        if image.data.len() != pixels * 4 {
            return Err(format!(
                "reading the game window: expected 32-bit pixels, got {} bytes for {pixels}",
                image.data.len()
            ));
        }
        Ok(classify(&image.data))
    }
}

/// Finds the addon's colors among 32-bit pixels, stored blue, green, red, unused.
fn classify(data: &[u8]) -> Signal {
    let (pixels, _) = data.as_chunks::<4>();
    let count = |color: [u8; 3]| {
        pixels
            .iter()
            .filter(|[blue, green, red, _]| {
                [*red, *green, *blue]
                    .iter()
                    .zip(color)
                    .all(|(channel, target)| channel.abs_diff(target) <= TOLERANCE)
            })
            .count()
    };
    if count(MAGENTA) == 0 || count(CYAN) == 0 {
        return Signal::Missing;
    }
    [
        (Signal::Fishing, count(FISHING)),
        (Signal::Busy, count(BUSY)),
        (Signal::NoPole, count(NO_POLE)),
        (Signal::Idle, count(IDLE)),
    ]
    .into_iter()
    .filter(|(_, pixels)| *pixels > 0)
    .max_by_key(|(_, pixels)| *pixels)
    .map_or(Signal::Missing, |(signal, _)| signal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of pixels in the window's byte order.
    fn pixels(colors: &[[u8; 3]]) -> Vec<u8> {
        colors
            .iter()
            .flat_map(|[red, green, blue]| [*blue, *green, *red, 0])
            .collect()
    }

    #[test]
    fn reads_the_middle_square() {
        let grass = [40, 90, 30];
        assert_eq!(
            classify(&pixels(&[grass, MAGENTA, FISHING, CYAN])),
            Signal::Fishing
        );
        assert_eq!(
            classify(&pixels(&[MAGENTA, IDLE, CYAN, grass])),
            Signal::Idle
        );
        assert_eq!(classify(&pixels(&[MAGENTA, BUSY, CYAN])), Signal::Busy);
        assert_eq!(classify(&pixels(&[MAGENTA, NO_POLE, CYAN])), Signal::NoPole);
        assert_eq!(
            classify(&pixels(&[[250, 10, 245], [10, 240, 8], [5, 250, 250]])),
            Signal::Fishing,
            "close colors count"
        );
    }

    #[test]
    fn needs_both_markers() {
        assert_eq!(classify(&pixels(&[FISHING, CYAN])), Signal::Missing);
        assert_eq!(classify(&pixels(&[MAGENTA, FISHING])), Signal::Missing);
        assert_eq!(classify(&pixels(&[MAGENTA, CYAN])), Signal::Missing);
        assert_eq!(classify(&[]), Signal::Missing);
    }

    #[test]
    fn finds_installed_files() -> Result<(), std::io::Error> {
        let game = std::env::temp_dir().join(format!("pececito-game-{}", std::process::id()));
        let folder = game.join("Interface").join("AddOns").join("Pececito");
        fs::create_dir_all(&folder)?;
        let before = addon_files_present(&game);
        fs::write(folder.join("Pececito.toc"), TOC)?;
        fs::write(folder.join("Pececito.lua"), LUA)?;
        fs::write(folder.join("Icon.png"), ICON)?;
        let after = addon_files_present(&game);
        fs::remove_dir_all(&game)?;
        assert!(!before);
        assert!(after);
        Ok(())
    }
}
