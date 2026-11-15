# Pececito

A desktop companion for fishing in WoW Forever. It finds the running game client and fishes for you: it presses your fishing key to cast, listens to the game's sound for the splash of a bite, and presses the key again to hook.

Fishing works on Linux, at quiet spots and next to other players fishing. The Windows build opens, but it can't find the game or press keys yet. There's no macOS build yet.

## Install

On Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/acidtib/pececito/main/script/install.sh | sh
```

This installs the latest release to `~/.local/pececito.app`, links `pececito` into `~/.local/bin`, and adds Pececito to your app launcher. Run it again to update. On KDE it also adds a window rule so KWin remembers where you put the window. Set `PECECITO_VERSION=0.2.0` to install a specific release.

On Windows, download and run `Pececito-x86_64.exe` from the [latest release](https://github.com/acidtib/pececito/releases/latest). It installs for your user only, with a Start menu shortcut and an uninstaller.

To fish, Pececito needs:

* PipeWire, with `pw-dump` and `pw-record` on the `PATH`. Pececito records the game's own audio stream.
* Write access to `/dev/uinput`, for the virtual keyboard that presses the fishing key. Most desktops give it to the logged-in user; check with `getfacl /dev/uinput`.

Settings are saved to `~/.config/pececito/prefs.json`.

## How to fish

1. In the game, bind a key to fishing (with an addon such as Forever Fishing, or a macro) and turn on auto loot. Keep the game's sound effects on.
2. Install Pececito's addon: the app offers it when the game is open, or use Settings. Then type `/reload` in the game.
3. Equip your fishing pole and stand at the water, facing it.
4. Press Start fishing, then click the game window. Pececito only presses keys while the game window has focus.

If you end a cast yourself, or a key press doesn't cast, the session pauses until you press Resume or cast by hand. The hook press comes after a random delay that you can set in Settings.

Pececito can't tell what you caught. It counts casts and the bites it hooked.

## The addon

The Pececito addon (`addon/Pececito`) draws three 3 by 3 pixel squares in the top-left corner of the game. Their colors tell the app whether you're fishing, casting something else (such as a lure), or holding no fishing pole. The app reads only that corner of the game window. The squares stay visible when the game's UI is hidden.

## Layout

| Path | What it does |
| --- | --- |
| `crates/pececito` | The desktop app: window, screens and settings, built with Rune |
| `crates/game` | Finds the WoW Forever client, listens to its sound and presses its keys |
| `crates/fishing` | The fishing loop: when to press the fishing key, from the time, the game's sound and the addon |
| `addon/Pececito` | The game addon the app reads; the app carries a copy to install |

## Develop

```sh
cargo run
cargo clippy --workspace --all-targets
cargo test --workspace
cargo test -p game -- --ignored --nocapture   # with the game running
```
