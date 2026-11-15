use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, GetPropertyReply, Window};
use x11rb::rust_connection::RustConnection;

use crate::{GameProcess, GameWindow, Rect};

/// Proton runs the client under XWayland, so the window is found through EWMH: the top-level
/// whose `_NET_WM_PID` is the client's pid.
pub(crate) fn find_window(process: &GameProcess) -> Result<Option<GameWindow>, String> {
    let (connection, root) = connect()?;
    let atoms = Atoms::intern(&connection)?;

    let clients = get_property(
        &connection,
        root,
        atoms.client_list,
        AtomEnum::WINDOW.into(),
        u32::MAX,
    )?;
    for window in clients.value32().into_iter().flatten() {
        let failed = |error: &dyn std::fmt::Display| {
            format!("reading property of window {window:#x}: {error}")
        };
        let window_pid = match connection
            .get_property(false, window, atoms.wm_pid, AtomEnum::CARDINAL, 0, 1)
            .map_err(|error| failed(&error))?
            .reply()
        {
            Ok(reply) => reply.value32().and_then(|mut values| values.next()),
            // A window closed since the list was read; it isn't the game's.
            Err(ReplyError::X11Error(_)) => continue,
            Err(error) => return Err(failed(&error)),
        };
        if window_pid != Some(process.pid) {
            continue;
        }
        let title = get_property(
            &connection,
            window,
            atoms.wm_name,
            atoms.utf8_string,
            u32::MAX,
        )?;
        let class = get_property(
            &connection,
            window,
            AtomEnum::WM_CLASS.into(),
            AtomEnum::STRING.into(),
            u32::MAX,
        )?;
        return Ok(Some(GameWindow {
            process: process.clone(),
            window_id: window,
            title: String::from_utf8_lossy(&title.value).into_owned(),
            class: wm_class_name(&class.value),
            geometry: root_geometry(&connection, root, window)?,
        }));
    }
    Ok(None)
}

/// Checks whether a window has keyboard focus, keeping one X connection for repeated checks.
pub struct FocusReader {
    connection: RustConnection,
    root: Window,
    active_window: u32,
    window: Window,
}

impl FocusReader {
    pub fn new(window: &GameWindow) -> Result<Self, String> {
        let (connection, root) = connect()?;
        let active_window = Atoms::intern(&connection)?.active_window;
        Ok(FocusReader {
            connection,
            root,
            active_window,
            window: window.window_id,
        })
    }

    /// The X11 window with keyboard focus. While a native Wayland window has focus, XWayland
    /// reports none.
    pub fn active_window(&self) -> Result<Option<u32>, String> {
        let active = get_property(
            &self.connection,
            self.root,
            self.active_window,
            AtomEnum::WINDOW.into(),
            1,
        )?
        .value32()
        .and_then(|mut values| values.next());
        Ok(active.filter(|window| *window != 0))
    }

    /// Whether keystrokes go to the window now.
    pub fn has_focus(&self) -> Result<bool, String> {
        Ok(self.active_window()? == Some(self.window))
    }
}

fn connect() -> Result<(RustConnection, Window), String> {
    let (connection, screen_number) =
        x11rb::connect(None).map_err(|error| format!("connecting to the X server: {error}"))?;
    let root = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or_else(|| format!("X screen {screen_number} does not exist"))?
        .root;
    Ok((connection, root))
}

struct Atoms {
    active_window: u32,
    client_list: u32,
    wm_pid: u32,
    wm_name: u32,
    utf8_string: u32,
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, String> {
        let intern = |name: &str| -> Result<u32, String> {
            let cookie = connection
                .intern_atom(false, name.as_bytes())
                .map_err(|error| format!("{name}: {error}"))?;
            Ok(cookie
                .reply()
                .map_err(|error| format!("{name}: {error}"))?
                .atom)
        };
        Ok(Atoms {
            active_window: intern("_NET_ACTIVE_WINDOW")?,
            client_list: intern("_NET_CLIENT_LIST")?,
            wm_pid: intern("_NET_WM_PID")?,
            wm_name: intern("_NET_WM_NAME")?,
            utf8_string: intern("UTF8_STRING")?,
        })
    }
}

fn get_property(
    connection: &RustConnection,
    window: Window,
    property: u32,
    kind: u32,
    length: u32,
) -> Result<GetPropertyReply, String> {
    connection
        .get_property(false, window, property, kind, 0, length)
        .map_err(|error| format!("reading property of window {window:#x}: {error}"))?
        .reply()
        .map_err(|error| format!("reading property of window {window:#x}: {error}"))
}

fn root_geometry(
    connection: &RustConnection,
    root: Window,
    window: Window,
) -> Result<Rect, String> {
    let geometry = connection
        .get_geometry(window)
        .map_err(|error| format!("geometry of window {window:#x}: {error}"))?
        .reply()
        .map_err(|error| format!("geometry of window {window:#x}: {error}"))?;
    let origin = connection
        .translate_coordinates(window, root, 0, 0)
        .map_err(|error| format!("position of window {window:#x}: {error}"))?
        .reply()
        .map_err(|error| format!("position of window {window:#x}: {error}"))?;
    Ok(Rect {
        x: origin.dst_x.into(),
        y: origin.dst_y.into(),
        width: geometry.width.into(),
        height: geometry.height.into(),
    })
}

/// `WM_CLASS` is two NUL-terminated strings, instance then class; the class is the stable one.
fn wm_class_name(value: &[u8]) -> String {
    value
        .split(|byte| *byte == 0)
        .rfind(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_class_half_of_wm_class() {
        assert_eq!(wm_class_name(b"steam_app_1\0steam_app_1\0"), "steam_app_1");
        assert_eq!(wm_class_name(b""), "");
    }
}
