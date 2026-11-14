# Pececito

A desktop companion for fishing in WoW Forever. It finds the running game client and will run the fishing loop: cast, wait for a bite, hook and loot.

Status: game detection and the fishing key setting work. The fishing loop is a mock that drives the UI.

## Requirements

* Linux with an X11 or XWayland session. The game runs through Proton, so its window is an XWayland window.
* Rust; `rust-toolchain.toml` pins the version.
* Build dependencies for Rune. On Arch and CachyOS: `pacman -S libxkbcommon wayland`.

## Run

```sh
cargo run
```

Settings are saved to `~/.config/pececito/prefs.json`.

## Install

```sh
script/install-linux
```

This builds a release binary and installs it to `~/.local/bin`, with the icon and a desktop entry, so
Pececito shows up in your app launcher with its icon. Set `PREFIX` to install somewhere else.

## Layout

| Crate | What it does |
| --- | --- |
| `crates/pececito` | The desktop app: window, screens and settings, built with Rune |
| `crates/game` | Finds the WoW Forever client: its process and its window |
| `crates/fishing` | The fishing loop: casts, bites, catches and sessions |

## Develop

```sh
cargo clippy --workspace --all-targets
cargo test --workspace
cargo test -p game -- --ignored --nocapture   # with the game running
```
