# Pececito

A desktop companion app for fishing in WoW Forever, a private World of Warcraft server. Built in Rust
with Rune (github.com/anomalyco/rune), a GPU UI framework. The dev machine is CachyOS with KDE Plasma 6
on Wayland, and the game runs through Proton (Faugus launcher, umu, Proton-GE).

Work in small goals. Build only the current goal; don't start on later fishing steps (casting, bobber
detection, input) unless asked.

# Workspace

* `crates/pececito` is the app; `crates/game` and `crates/fishing` are libraries. Crates are named by what
  they do, without a `pececito-` prefix; they are never published.
* `game` and `fishing` must not depend on Rune or `rune-kit`. Keep UI types out of them so they build and
  test without a window; the app adapts between them and Rune (for example, passing Rune's `Waker` to
  `game::Watcher` as a plain closure).
* Dependency versions and the Rune patches live in the root `Cargo.toml` (`[workspace.dependencies]`);
  crates use `foo.workspace = true`. Rust is pinned in `rust-toolchain.toml` to the version Rune pins.

# The game on this machine

* The client process is `Wow.exe` (from `/proc/<pid>/comm`), with an argv[0] under
  `World of Warcraft\_forever_\`. Retail or classic clients may run from the same Battle.net prefix,
  so match `_forever_`, not just the exe name.
* Proton runs the game under XWayland (`PROTON_ENABLE_WAYLAND=0`), so its window is an X11 window. Find
  it through `_NET_CLIENT_LIST` and match `_NET_WM_PID` to the client's pid. The title is
  `World of Warcraft`; the `WM_CLASS` is a launcher-generated `steam_app_*` name, so don't match on it.
* Window coordinates from X11 are XWayland logical coordinates, the same space as a full Spectacle capture.

# Rune

* Rune is pinned to one `dev` rev in the root `Cargo.toml`. Cargo doesn't inherit `[patch]` from a dependency, so
  every crate in Rune's own `[patch.crates-io]` table is patched here at the same rev. When bumping Rune,
  check its root `Cargo.toml` at the new rev and update the rev and the patch list together.
* Rune's docs live in its repo: `crates/rune/README.md` (API and rules) and `examples/`. Read them from
  the cargo git checkout (`~/.cargo/git/checkouts/rune-*/<rev>/`) so they match the pinned rev.
* Blocking work (scanning `/proc`, X11 round trips, screen capture) never runs in a view, handler or
  `poll`. Run it on a worker thread, hand results over a channel, call the `Waker` from `setup`, and
  drain the channel in `poll`. `poll` returns true only when visible state changed.
* Behaviour with modes or phases is one `enum` whose variants carry only the data valid in them, not a
  cluster of booleans and `Option`s.
* `AppSpec` and `Env` are built as literals; end them with `..AppSpec::new(..)`.
* Check UI headlessly before claiming it works: `target/debug/pececito --snap out.png`, and
  `--script s.json --snap out.png --at <seconds>` to tap elements by `key`. Give interactive elements a
  `key` so tests and scripts can find them.
* Settings go through `rune::prefs::Prefs` (`~/.config/pececito/prefs.json`). Set `RUNE_PREFS_DIR` to a
  scratch folder for headless runs, or a scripted key change overwrites the user's real settings. Tests
  use `Prefs::memory()` or `Prefs::at(<temp path>)`.
* Tests that change state directly go through `Runtime::update`, so the view rebuilds as it does after
  `poll`; assigning to `runtime.state` leaves the old buttons, with their old disabled state, on screen.
  `Runtime::new` doesn't run the app's `poll` either: a test that needs time to pass calls
  `runtime.update(|state, tx| { poll(state, tx.clock(), tx); })` between frames.

# Rust coding guidelines

* Prioritize correctness and clarity. Speed is secondary unless specified.
* Avoid functions that panic, like `unwrap()` and `expect()`; propagate errors with `?`. Be careful
  with indexing that can go out of bounds.
* Never silently discard errors with `let _ =` on fallible operations. Propagate, log, or handle them
  explicitly with `match` or `if let Err(..)`.
* Errors from background work reach the UI so the user sees what failed.
* Prefer adding to existing files unless the code is a new logical component. Avoid many small files.
* Never create `mod.rs` files; use `src/some_module.rs`.
* Use full words for names, no abbreviations like `q` for `queue`.
* No `unsafe`.
* Avoid creative additions unless asked.

# Build and verify

* `cargo clippy --workspace --all-targets` with no warnings, and `cargo fmt --all`.
* `cargo test --workspace`. With the game running, `cargo test -p game -- --ignored --nocapture` checks
  live detection.
* `cargo run` (from the root) opens the window.

# Writing

### Comments
* Default to no comment. Comment only what is not obvious: a platform constraint, a subtle invariant,
  a workaround. One clause, not a paragraph.
* Don't narrate changes or summarize code in comments, and don't point to docs from code.

### Style
* No emojis and no em dashes anywhere: code, comments, docs, UI text, commits. Use a hyphen, comma, or
  semicolon.
* Don't over-summarize. No closing paragraph restating the change.

### Voice
These apply to docs, UI text, commit messages, and replies to the user.
* Practical over promotional: say what the user can do. No marketing words like "powerful",
  "seamless", "robust", or "best-in-class".
* Direct and concise: short sentences, get to the point. Second person ("you"), present tense.
* Honest about limits: when something doesn't work yet, say so plainly, with a workaround if there is one.
* No hedging or filler: "simply", "just", "easily", "entirely", "certainly", "definitely", "deeply",
  "actually".
* No meta-commentary ("the honest take is", "to be frank") and no apologetic tone.
* Flag uncertainty instead of guessing, and don't describe unbuilt features as if they exist.

### Commits and pull requests
* Only commit when asked, and only after the work builds and is verified.
* Release Please builds the changelog and version from commit messages, so commit messages and PR
  titles follow Conventional Commits: `type(scope): summary`. Squash merges use the PR title as
  the commit message, so a PR title needs the format as much as a commit does.
  * Types: `feat` (a user-visible feature, minor bump), `fix` (a user-visible bug fix, patch bump),
    and `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore` for the rest, which don't bump
    the version.
  * The scope is optional: a crate (`pececito`, `game`, `fishing`) or `addon`. Leave it out when a
    change spans several.
  * The summary is lowercase, imperative and has no trailing period:
    `fix(game): find the audio stream after the game restarts`.
  * Mark a breaking change with `!` after the type or scope (`feat(addon)!: ...`) and a
    `BREAKING CHANGE:` footer saying what breaks.
* Small commits, one idea each. Never commit secrets or `.env` files.
* Use the body only for the why a future reader needs. No debugging narration.
* No co-author trailers and no tool attribution: no `Co-authored-by`, `Generated with`, agent names, or
  session links in commits, PRs, issues, or comments.

# Rules hygiene

Keep this file high-signal: rules are traps to avoid, not maps of the code. Add a rule only when it is
non-obvious, came up more than once, and is specific enough to act on. Propose additions to the user
instead of editing this file during feature work.
