# Pececito

A desktop companion for fishing in WoW Forever. It finds the running game client and fishes for you: it presses your fishing key to cast, listens to the game's sound for the splash of a bite, and presses the key again to hook.

Status: fishing works on Linux, at quiet spots and next to other players fishing. Windows isn't supported yet.

## How to fish

1. In the game, bind a key to fishing (with an addon such as Forever Fishing, or a macro) and turn on auto loot. Keep the game's sound effects on.
2. Install Pececito's addon: the app offers it when the game is open, or use Settings. Then type `/reload` in the game.
3. Equip your fishing pole and stand at the water, facing it.
4. Press Start fishing, then click the game window. Pececito only presses keys while the game window has focus.

If you end a cast yourself, or a key press doesn't cast, the session pauses until you press Resume or cast by hand. The hook press comes after a random delay that you can set in Settings.

Pececito can't tell what you caught. It counts casts and the bites it hooked.

## The addon

The Pececito addon (`addon/Pececito`) draws three 3 by 3 pixel squares in the top-left corner of the game. Their colors tell the app whether you're fishing, casting something else (such as a lure), or holding no fishing pole. The app reads only that corner of the game window. The squares stay visible when the game's UI is hidden.

## Requirements

* Linux with an X11 or XWayland session. The game runs through Proton, so its window is an XWayland window.
* PipeWire, with `pw-dump` and `pw-record` on the `PATH`. Pececito records the game's own audio stream.
* Write access to `/dev/uinput`, for the virtual keyboard that presses the fishing key. Most desktops give it to the logged-in user; check with `getfacl /dev/uinput`.
* Rust; `rust-toolchain.toml` pins the version.
* Build dependencies for Rune. On Arch and CachyOS: `pacman -S libxkbcommon wayland`.

## Run

```sh
cargo run
```

Settings are saved to `~/.config/pececito/prefs.json`.

## Download

Each release on GitHub has a Linux build (`.tar.gz`) and a Windows build (`.zip`). The Windows build opens, but it can't find the game or press keys yet.

## Install from source

```sh
script/install-linux
```

This builds a release binary and installs it to `~/.local/bin`, with the icon and a desktop entry, so
Pececito shows up in your app launcher with its icon. Set `PREFIX` to install somewhere else.

## Layout

| Path | What it does |
| --- | --- |
| `crates/pececito` | The desktop app: window, screens and settings, built with Rune |
| `crates/game` | Finds the WoW Forever client, listens to its sound and presses its keys |
| `crates/fishing` | The fishing loop: when to press the fishing key, from the time, the game's sound and the addon |
| `addon/Pececito` | The game addon the app reads; the app carries a copy to install |

## Develop

```sh
cargo clippy --workspace --all-targets
cargo test --workspace
cargo test -p game -- --ignored --nocapture   # with the game running
```
