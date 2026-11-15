use std::fs;
use std::path::{Path, PathBuf};

use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
use x11rb::rust_connection::RustConnection;

use crate::{GameProcess, GameWindow};

const TOC: &str = include_str!("../../../addon/Pececito/Pececito.toc");
const LUA: &str = include_str!("../../../addon/Pececito/Pececito.lua");
const ICON: &[u8] = include_bytes!("../../../addon/Pececito/Icon.png");

/// The corner the addon draws its strip in, with room to spare for larger cells.
const REGION_WIDTH: u16 = 256;
const REGION_HEIGHT: u16 = 16;
/// The two cells that open the strip. A cell is a square of one color carrying three bits: red
/// is 4, green is 2 and blue is 1, each on or off.
const MAGENTA: u32 = 5;
const CYAN: u32 = 3;
const VERSION_CELLS: usize = 2;
const STATE_CELLS: usize = 1;
const MOVES_CELLS: usize = 1;
const CHECKSUM_CELLS: usize = 2;

/// What the Pececito addon shows in the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub state: PlayerState,
    /// Goes up by one, wrapping around, each time the player starts moving.
    pub moves: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
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

/// The addon's files in the game's folder, against the ones this build installs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddonFiles {
    Missing,
    Different(AddonMismatch),
    Current,
}

/// How installed addon files differ from the ones this build installs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddonMismatch {
    /// An older release of the app wrote them.
    Older { installed: u32, shipped: u32 },
    /// A newer release of the app wrote them.
    Newer { installed: u32, shipped: u32 },
    /// The version is this build's, or can't be read, but the files aren't: they were changed
    /// by hand.
    Edited,
}

/// The version of the addon this build installs.
pub fn addon_version() -> Option<u32> {
    toc_version(TOC.as_bytes())
}

/// The `## Version` line of a `.toc` file.
fn toc_version(toc: &[u8]) -> Option<u32> {
    String::from_utf8_lossy(toc)
        .lines()
        .find_map(|line| line.strip_prefix("## Version:"))
        .and_then(|version| version.trim().parse().ok())
}

pub fn addon_files(process: &GameProcess) -> AddonFiles {
    match fs::read_link(format!("/proc/{}/cwd", process.pid)) {
        Ok(game) => addon_files_in(&game),
        Err(_) => AddonFiles::Missing,
    }
}

fn addon_files_in(game: &Path) -> AddonFiles {
    let folder = game.join("Interface").join("AddOns").join("Pececito");
    let files = [
        ("Pececito.toc", TOC.as_bytes()),
        ("Pececito.lua", LUA.as_bytes()),
        ("Icon.png", ICON),
    ];
    let mut current = true;
    let mut installed = None;
    for (name, expected) in files {
        match fs::read(folder.join(name)) {
            Ok(contents) => {
                current &= contents == expected;
                if name.ends_with(".toc") {
                    installed = toc_version(&contents);
                }
            }
            Err(_) => return AddonFiles::Missing,
        }
    }
    if current {
        return AddonFiles::Current;
    }
    AddonFiles::Different(match (installed, addon_version()) {
        (Some(installed), Some(shipped)) if installed < shipped => {
            AddonMismatch::Older { installed, shipped }
        }
        (Some(installed), Some(shipped)) if installed > shipped => {
            AddonMismatch::Newer { installed, shipped }
        }
        _ => AddonMismatch::Edited,
    })
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

    /// What the addon shows; `None` when its strip isn't on screen, or is drawn by another
    /// version of the addon, as it is until the game reloads after an update.
    pub fn read(&self) -> Result<Option<Frame>, String> {
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
        Ok(decode(&image.data, usize::from(width)))
    }
}

/// Reads the strip's cells from the top-left corner of rows of 32-bit pixels, stored blue,
/// green, red, unused.
struct Cells<'a> {
    pixels: &'a [[u8; 4]],
    /// Pixels in a row.
    width: usize,
    /// A cell's side in pixels.
    side: usize,
    next: usize,
    /// The checksum so far: each cell after the markers, times its place among them.
    sum: u32,
}

impl<'a> Cells<'a> {
    /// `None` unless the corner opens with the magenta and cyan cells.
    fn find(data: &'a [u8], width: usize) -> Option<Self> {
        let (pixels, _) = data.as_chunks::<4>();
        let side = pixels
            .iter()
            .take(width)
            .take_while(|pixel| bits(pixel) == MAGENTA)
            .count();
        let mut cells = Cells {
            pixels,
            width,
            side,
            next: 1,
            sum: 0,
        };
        (side > 0 && cells.cell()? == CYAN).then_some(cells)
    }

    /// The next cell, sampled at its middle, where neighbors and scaling blur it least.
    fn cell(&mut self) -> Option<u32> {
        let x = self.next * self.side + self.side / 2;
        let y = self.side / 2;
        self.next += 1;
        (x < self.width)
            .then(|| self.pixels.get(y * self.width + x))
            .flatten()
            .map(bits)
    }

    /// The number in the next `count` cells, most significant first.
    fn number(&mut self, count: usize) -> Option<u32> {
        let mut number = 0;
        for _ in 0..count {
            let cell = self.cell()?;
            let place = self.next as u32 - 2;
            self.sum += place * cell;
            number = number * 8 + cell;
        }
        Some(number)
    }

    /// Whether the cells that follow hold the checksum of the ones read so far.
    fn checksum_matches(&mut self) -> bool {
        let expected = self.sum % 8u32.pow(CHECKSUM_CELLS as u32);
        self.number(CHECKSUM_CELLS) == Some(expected)
    }
}

/// A pixel as a cell's three bits; a channel is on when it's over half.
fn bits([blue, green, red, _]: &[u8; 4]) -> u32 {
    u32::from(*red >= 128) * 4 + u32::from(*green >= 128) * 2 + u32::from(*blue >= 128)
}

fn decode(data: &[u8], width: usize) -> Option<Frame> {
    let mut cells = Cells::find(data, width)?;
    // Later cells mean what this version says they do, so another version's aren't read.
    if Some(cells.number(VERSION_CELLS)?) != addon_version() {
        return None;
    }
    let state = match cells.number(STATE_CELLS)? {
        0 => PlayerState::Idle,
        1 => PlayerState::NoPole,
        2 => PlayerState::Busy,
        3 => PlayerState::Fishing,
        _ => return None,
    };
    let moves = cells.number(MOVES_CELLS)?;
    cells.checksum_matches().then_some(Frame { state, moves })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIDE: usize = 3;

    fn color(cell: u32) -> [u8; 3] {
        [4, 2, 1].map(|bit| if cell & bit != 0 { 255 } else { 0 })
    }

    /// The cells of a number, most significant first.
    fn digits(number: u32, count: u32) -> Vec<u32> {
        (0..count)
            .rev()
            .map(|place| number / 8u32.pow(place) % 8)
            .collect()
    }

    /// The strip's cells for a state, as this build's addon draws them.
    fn strip(state: u32) -> Vec<u32> {
        strip_with(state, 0)
    }

    fn strip_with(state: u32, moves: u32) -> Vec<u32> {
        let mut payload = digits(addon_version().unwrap_or(0), 2);
        payload.push(state);
        payload.push(moves);
        let sum: u32 = payload
            .iter()
            .zip(1..)
            .map(|(cell, place)| cell * place)
            .sum();
        payload.extend(digits(sum % 64, 2));
        [vec![MAGENTA, CYAN], payload].concat()
    }

    /// A corner of the game window with `cells` drawn `side` pixels large over grass, and its
    /// width in pixels.
    fn corner(cells: &[u32], side: usize, drift: u8) -> (Vec<u8>, usize) {
        let width = cells.len() * side + 5;
        let mut data = Vec::new();
        for y in 0..side + 2 {
            for x in 0..width {
                let [red, green, blue] = match cells.get(x / side) {
                    Some(cell) if y < side => color(*cell)
                        .map(|channel| if channel == 0 { drift } else { channel - drift }),
                    _ => [40, 90, 30],
                };
                data.extend([blue, green, red, 0]);
            }
        }
        (data, width)
    }

    fn read(cells: &[u32], side: usize, drift: u8) -> Option<Frame> {
        let (data, width) = corner(cells, side, drift);
        decode(&data, width)
    }

    #[test]
    fn reads_the_player_state() {
        let states = [
            PlayerState::Idle,
            PlayerState::NoPole,
            PlayerState::Busy,
            PlayerState::Fishing,
        ];
        for (number, state) in (0..).zip(states) {
            assert_eq!(
                read(&strip(number), SIDE, 0),
                Some(Frame { state, moves: 0 })
            );
        }
        assert_eq!(
            read(&strip_with(0, 6), SIDE, 0).map(|frame| frame.moves),
            Some(6)
        );
        assert_eq!(read(&strip(4), SIDE, 0), None, "no such state");
    }

    #[test]
    fn reads_drifted_colors_and_other_cell_sizes() {
        let frame = Some(Frame {
            state: PlayerState::Fishing,
            moves: 0,
        });
        assert_eq!(read(&strip(3), SIDE, 100), frame);
        assert_eq!(read(&strip(3), 1, 0), frame);
        assert_eq!(read(&strip(3), 6, 40), frame);
    }

    #[test]
    fn needs_both_markers() {
        let mut cells = strip(3);
        cells[1] = MAGENTA;
        assert_eq!(read(&cells, SIDE, 0), None);
        let mut cells = strip(3);
        cells[0] = CYAN;
        assert_eq!(read(&cells, SIDE, 0), None);
        assert_eq!(decode(&[], 0), None);
    }

    #[test]
    fn rejects_a_bad_checksum_and_a_cut_off_strip() {
        let mut cells = strip(3);
        cells[4] = 2;
        assert_eq!(read(&cells, SIDE, 0), None, "a cell changed");
        let cells = strip(3);
        assert_eq!(read(&cells[..cells.len() - 1], SIDE, 0), None);
    }

    #[test]
    fn ignores_another_version_of_the_addon() {
        let mut cells = strip(3);
        cells[3] = (cells[3] + 1) % 8;
        assert_eq!(read(&cells, SIDE, 0), None);
        let green = 2;
        assert_eq!(
            read(&[MAGENTA, green, CYAN], SIDE, 0),
            None,
            "the first release's three squares"
        );
    }

    #[test]
    fn the_addon_draws_the_version_in_its_toc() {
        let Some(version) = addon_version() else {
            panic!("Pececito.toc has no version")
        };
        assert!(LUA.contains(&format!("local VERSION = {version}\n")));
    }

    #[test]
    fn finds_installed_files() -> Result<(), std::io::Error> {
        let game = std::env::temp_dir().join(format!("pececito-game-{}", std::process::id()));
        let folder = game.join("Interface").join("AddOns").join("Pececito");
        fs::create_dir_all(&folder)?;
        let before = addon_files_in(&game);
        fs::write(folder.join("Pececito.toc"), TOC)?;
        fs::write(folder.join("Pececito.lua"), "-- changed by hand")?;
        fs::write(folder.join("Icon.png"), ICON)?;
        let edited = addon_files_in(&game);
        fs::write(folder.join("Pececito.toc"), "## Version: 0\n")?;
        let older = addon_files_in(&game);
        fs::write(
            folder.join("Pececito.toc"),
            "## Title: Pececito\n## Version: 4000000000\n",
        )?;
        let newer = addon_files_in(&game);
        fs::write(folder.join("Pececito.toc"), TOC)?;
        fs::write(folder.join("Pececito.lua"), LUA)?;
        let after = addon_files_in(&game);
        fs::remove_dir_all(&game)?;
        let Some(shipped) = addon_version() else {
            panic!("Pececito.toc has no version")
        };
        assert_eq!(before, AddonFiles::Missing);
        assert_eq!(edited, AddonFiles::Different(AddonMismatch::Edited));
        assert_eq!(
            older,
            AddonFiles::Different(AddonMismatch::Older {
                installed: 0,
                shipped
            })
        );
        assert_eq!(
            newer,
            AddonFiles::Different(AddonMismatch::Newer {
                installed: 4_000_000_000,
                shipped
            })
        );
        assert_eq!(after, AddonFiles::Current);
        Ok(())
    }

    /// The app tells an old addon from its own by the version, so a changed addon needs a new
    /// one: bump `## Version` in the `.toc`, then update both numbers here.
    #[test]
    fn a_changed_addon_has_a_new_version() {
        let fingerprint = [TOC.as_bytes(), LUA.as_bytes(), ICON]
            .iter()
            .flat_map(|file| file.iter())
            // A Windows checkout may turn line ends into CRLF.
            .filter(|byte| **byte != b'\r')
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
            });
        assert_eq!(
            (addon_version(), fingerprint),
            (Some(2), 10_802_587_795_968_827_633)
        );
    }
}
