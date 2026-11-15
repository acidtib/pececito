use std::thread;
use std::time::Duration;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, KeyCode, KeyEvent};

/// Long enough for the compositor to add a new device before it gets keys from it.
const DEVICE_SETTLE: Duration = Duration::from_millis(500);
/// How long a key stays down; a press with no hold can land between the game's input polls.
const KEY_HOLD: Duration = Duration::from_millis(40);

const MODIFIERS: [(&str, KeyCode); 4] = [
    ("ctrl", KeyCode::KEY_LEFTCTRL),
    ("alt", KeyCode::KEY_LEFTALT),
    ("shift", KeyCode::KEY_LEFTSHIFT),
    ("super", KeyCode::KEY_LEFTMETA),
];

/// Key names as Rune writes a shortcut (`shift+f`), each at its US layout position.
const KEYS: [(&str, KeyCode); 86] = [
    ("a", KeyCode::KEY_A),
    ("b", KeyCode::KEY_B),
    ("c", KeyCode::KEY_C),
    ("d", KeyCode::KEY_D),
    ("e", KeyCode::KEY_E),
    ("f", KeyCode::KEY_F),
    ("g", KeyCode::KEY_G),
    ("h", KeyCode::KEY_H),
    ("i", KeyCode::KEY_I),
    ("j", KeyCode::KEY_J),
    ("k", KeyCode::KEY_K),
    ("l", KeyCode::KEY_L),
    ("m", KeyCode::KEY_M),
    ("n", KeyCode::KEY_N),
    ("o", KeyCode::KEY_O),
    ("p", KeyCode::KEY_P),
    ("q", KeyCode::KEY_Q),
    ("r", KeyCode::KEY_R),
    ("s", KeyCode::KEY_S),
    ("t", KeyCode::KEY_T),
    ("u", KeyCode::KEY_U),
    ("v", KeyCode::KEY_V),
    ("w", KeyCode::KEY_W),
    ("x", KeyCode::KEY_X),
    ("y", KeyCode::KEY_Y),
    ("z", KeyCode::KEY_Z),
    ("0", KeyCode::KEY_0),
    ("1", KeyCode::KEY_1),
    ("2", KeyCode::KEY_2),
    ("3", KeyCode::KEY_3),
    ("4", KeyCode::KEY_4),
    ("5", KeyCode::KEY_5),
    ("6", KeyCode::KEY_6),
    ("7", KeyCode::KEY_7),
    ("8", KeyCode::KEY_8),
    ("9", KeyCode::KEY_9),
    ("f1", KeyCode::KEY_F1),
    ("f2", KeyCode::KEY_F2),
    ("f3", KeyCode::KEY_F3),
    ("f4", KeyCode::KEY_F4),
    ("f5", KeyCode::KEY_F5),
    ("f6", KeyCode::KEY_F6),
    ("f7", KeyCode::KEY_F7),
    ("f8", KeyCode::KEY_F8),
    ("f9", KeyCode::KEY_F9),
    ("f10", KeyCode::KEY_F10),
    ("f11", KeyCode::KEY_F11),
    ("f12", KeyCode::KEY_F12),
    ("f13", KeyCode::KEY_F13),
    ("f14", KeyCode::KEY_F14),
    ("f15", KeyCode::KEY_F15),
    ("f16", KeyCode::KEY_F16),
    ("f17", KeyCode::KEY_F17),
    ("f18", KeyCode::KEY_F18),
    ("f19", KeyCode::KEY_F19),
    ("f20", KeyCode::KEY_F20),
    ("f21", KeyCode::KEY_F21),
    ("f22", KeyCode::KEY_F22),
    ("f23", KeyCode::KEY_F23),
    ("f24", KeyCode::KEY_F24),
    ("escape", KeyCode::KEY_ESC),
    ("enter", KeyCode::KEY_ENTER),
    ("space", KeyCode::KEY_SPACE),
    ("tab", KeyCode::KEY_TAB),
    ("backspace", KeyCode::KEY_BACKSPACE),
    ("delete", KeyCode::KEY_DELETE),
    ("insert", KeyCode::KEY_INSERT),
    ("home", KeyCode::KEY_HOME),
    ("end", KeyCode::KEY_END),
    ("pageup", KeyCode::KEY_PAGEUP),
    ("pagedown", KeyCode::KEY_PAGEDOWN),
    ("left", KeyCode::KEY_LEFT),
    ("right", KeyCode::KEY_RIGHT),
    ("up", KeyCode::KEY_UP),
    ("down", KeyCode::KEY_DOWN),
    ("minus", KeyCode::KEY_MINUS),
    ("equal", KeyCode::KEY_EQUAL),
    ("comma", KeyCode::KEY_COMMA),
    ("period", KeyCode::KEY_DOT),
    ("slash", KeyCode::KEY_SLASH),
    ("semicolon", KeyCode::KEY_SEMICOLON),
    ("quote", KeyCode::KEY_APOSTROPHE),
    ("bracketleft", KeyCode::KEY_LEFTBRACE),
    ("bracketright", KeyCode::KEY_RIGHTBRACE),
    ("backslash", KeyCode::KEY_BACKSLASH),
    ("backquote", KeyCode::KEY_GRAVE),
];

/// A key and the modifiers held with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keystroke {
    modifiers: Vec<KeyCode>,
    key: KeyCode,
}

impl Keystroke {
    /// Parses names joined by `+`, modifiers first: `space`, `shift+f`, `ctrl+alt+1`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let invalid = || format!("can't press {text:?}");
        let mut parts: Vec<String> = text
            .split('+')
            .map(|part| part.trim().to_ascii_lowercase())
            .collect();
        let key_name = parts.pop().ok_or_else(invalid)?;
        let modifiers = parts
            .iter()
            .map(|name| lookup(&MODIFIERS, name).ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let key = lookup(&KEYS, &key_name).ok_or_else(invalid)?;
        Ok(Keystroke { modifiers, key })
    }
}

fn lookup(table: &[(&str, KeyCode)], name: &str) -> Option<KeyCode> {
    table
        .iter()
        .find(|(entry, _)| *entry == name)
        .map(|(_, code)| *code)
}

/// A virtual keyboard through `/dev/uinput`. Its keys go to whichever window has focus.
pub struct Keyboard {
    device: VirtualDevice,
}

impl Keyboard {
    pub fn new() -> Result<Self, String> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for (_, code) in MODIFIERS.iter().chain(&KEYS) {
            keys.insert(*code);
        }
        let device = VirtualDevice::builder()
            .and_then(|builder| builder.name("Pececito keyboard").with_keys(&keys))
            .and_then(|builder| builder.build())
            .map_err(|error| format!("Can't create a virtual keyboard in /dev/uinput: {error}"))?;
        thread::sleep(DEVICE_SETTLE);
        Ok(Keyboard { device })
    }

    pub fn press(&mut self, keystroke: &Keystroke) -> Result<(), String> {
        for modifier in &keystroke.modifiers {
            self.emit(*modifier, 1)?;
        }
        self.emit(keystroke.key, 1)?;
        thread::sleep(KEY_HOLD);
        self.emit(keystroke.key, 0)?;
        for modifier in keystroke.modifiers.iter().rev() {
            self.emit(*modifier, 0)?;
        }
        Ok(())
    }

    fn emit(&mut self, code: KeyCode, value: i32) -> Result<(), String> {
        self.device
            .emit(&[*KeyEvent::new(code, value)])
            .map_err(|error| format!("pressing a key through /dev/uinput: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_as_rune_writes_them() {
        assert_eq!(
            Keystroke::parse("space"),
            Ok(Keystroke {
                modifiers: vec![],
                key: KeyCode::KEY_SPACE
            })
        );
        assert_eq!(
            Keystroke::parse("ctrl+shift+f"),
            Ok(Keystroke {
                modifiers: vec![KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT],
                key: KeyCode::KEY_F
            })
        );
        assert!(Keystroke::parse("shift+nope").is_err());
        assert!(Keystroke::parse("f+shift").is_err());
        assert!(Keystroke::parse("").is_err());
    }
}
