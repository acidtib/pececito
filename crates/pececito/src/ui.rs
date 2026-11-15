use std::sync::OnceLock;

use rune::{Color, Cx, El, Icon, TextStyle, col, el, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, SegmentedControl,
    ShortcutRecorder,
};

use crate::{Fishing, Game, Screen, Session, State, StopReason};
use fishing::{CAST_LENGTH, HookDelay, Outcome, PauseReason, Phase};
use game::{AddonState, Detection};

/// Session lengths to pick from, in minutes; the last is no limit.
pub const STOP_AFTER: [(&str, Option<u64>); 4] = [
    ("30 min", Some(30)),
    ("1 hour", Some(60)),
    ("2 hours", Some(120)),
    ("Never", None),
];
const RECENT_CASTS: usize = 6;

const STAT_VALUE: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(20.0, 600.0)
};
const HEADLINE: TextStyle = TextStyle::new(17.0, 600.0);
const TABULAR: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..TextStyle::new(12.0, 400.0)
};

static GEAR: Icon = Icon {
    name: "pececito-gear",
    view: 24.0,
    body: r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="1.75"/>"#,
};

static ROTATE_CCW: Icon = Icon {
    name: "pececito-rotate-ccw",
    view: 24.0,
    body: r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/><path d="M3 3v5h5" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

static ARROW_LEFT: Icon = Icon {
    name: "pececito-arrow-left",
    view: 24.0,
    body: r#"<path d="M12 19l-7-7 7-7M19 12H5" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

fn kit(dark: bool) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| Kit::neutral(dark))
}

pub fn view(state: &State, cx: &mut Cx<State>) -> El {
    let kit = kit(cx.system_dark());
    let screen = match state.screen {
        Screen::Main => main_screen(state, kit),
        Screen::Settings => settings_screen(state, kit),
    };
    screen
        .w_full()
        .h_full()
        .gap(12.0)
        .padding(16.0)
        .fill(kit.c.page)
}

fn panel(kit: &Kit) -> El {
    col()
        .w_full()
        .padding(14.0)
        .radius(kit.radius.lg)
        .fill(kit.c.bg_base)
        .border(1.0, kit.c.border_muted)
}

fn section_label(kit: &Kit, label: &'static str) -> El {
    text(label)
        .font(kit.ty.small_medium)
        .color(kit.c.text_faint)
}

fn minutes_seconds(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

// Main screen

fn main_screen(state: &State, kit: &Kit) -> El {
    col()
        .child(main_header(state, kit))
        .child(status_card(state, kit))
        .child(stats(state, kit))
        .child(casts(state, kit))
        .child(action_button(state, kit))
}

fn main_header(state: &State, kit: &Kit) -> El {
    let reset_help = match state.session {
        Session::Stopped(_) => "Clear the last session",
        Session::Fishing(_) => "Stop fishing to reset",
        Session::Idle => "Nothing to reset yet",
    };
    let reset = IconButton::new(&ROTATE_CCW)
        .variant(IconButtonVariant::Ghost)
        .label("Reset session")
        .disabled(!matches!(state.session, Session::Stopped(_)))
        .on_press(State::reset_session)
        .build(kit)
        .key("reset")
        .help(reset_help);
    let settings = IconButton::new(&GEAR)
        .variant(IconButtonVariant::Ghost)
        .label("Settings")
        .on_press(State::open_settings)
        .build(kit)
        .key("settings")
        .help("Settings");
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text("Pececito")
                .font(kit.ty.body_medium)
                .color(kit.c.text_base)
                .grow(1.0),
        )
        .child(connection_pill(state, kit))
        .child(reset)
        .child(settings)
}

fn connection_pill(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let (label, dot, help) = match &state.game {
        Game::Detected(Detection::Found(..)) => (
            "Connected",
            c.success.icon,
            "Found the WoW Forever window".to_string(),
        ),
        Game::Detected(Detection::NoWindow(_)) => (
            "Game loading",
            c.warning.icon,
            "Waiting for the game window".to_string(),
        ),
        Game::Detected(Detection::NotRunning) => (
            "Game closed",
            c.icon_faint,
            "Pececito connects when WoW Forever opens".to_string(),
        ),
        Game::Searching => (
            "Searching",
            c.icon_faint,
            "Looking for WoW Forever".to_string(),
        ),
        Game::Error(error) => ("Error", c.danger.icon, error.clone()),
    };
    row()
        .key("connection")
        .role("status")
        .label(label)
        .gap(6.0)
        .center_y()
        .h(22.0)
        .px(8.0)
        .radius(11.0)
        .fill(c.layer_01)
        .border(1.0, c.border_muted)
        .help(help)
        .child(el().size(6.0, 6.0).radius(3.0).fill(dot).a11y_hidden())
        .child(
            text(label)
                .font(kit.ty.small_medium)
                .color(c.text_muted)
                .a11y_hidden(),
        )
}

/// What the bot is doing now, or why it can't start.
fn status_card(state: &State, kit: &Kit) -> El {
    let card = panel(kit).key("status").gap(12.0).h(144.0);
    if let Session::Fishing(fishing) = &state.session {
        return fishing_status(card, fishing, state.clock, kit);
    }
    let c = &kit.c;
    let message = |headline: &str, detail: String, color: Color| {
        col()
            .gap(4.0)
            .child(text(headline.to_string()).font(HEADLINE).color(c.text_base))
            .child(text(detail).font(kit.ty.ui).color(color).wrap().w_full())
    };
    let blocked = match &state.game {
        Game::Searching => Some(message(
            "Looking for WoW Forever",
            "This takes a second.".into(),
            c.text_muted,
        )),
        Game::Detected(Detection::NotRunning) => Some(message(
            "WoW Forever isn't running",
            "Pececito connects when the game opens.".into(),
            c.text_muted,
        )),
        Game::Detected(Detection::NoWindow(_)) => Some(message(
            "WoW Forever is loading",
            "Waiting for the game window.".into(),
            c.text_muted,
        )),
        Game::Error(error) => Some(message("Can't see the game", error.clone(), c.danger.fg)),
        Game::Detected(Detection::Found(..)) => None,
    };
    if let Some(blocked) = blocked {
        return card
            .child(blocked)
            .child(el().grow(1.0))
            .child(steps(None, kit));
    }
    let open_settings = || {
        Button::new("Open settings")
            .variant(ButtonVariant::Neutral)
            .on_press(State::open_settings)
            .build(kit)
            .key("open-settings")
    };
    if state.fishing_key.shortcut().is_none() {
        return card
            .child(message(
                "Set your fishing key",
                "Pececito presses it to cast.".into(),
                c.text_muted,
            ))
            .child(row().child(open_settings()));
    }
    match state.addon() {
        Some(AddonState::NotInstalled) => {
            let (detail, color) = match state.install_error() {
                Some(error) => (format!("Couldn't install: {error}"), c.danger.fg),
                None => (
                    "Pececito reads it in the game to see when you're fishing.".to_string(),
                    c.text_muted,
                ),
            };
            return card
                .child(message("Install Pececito's addon", detail, color))
                .child(row().child(install_button(state, kit)));
        }
        Some(AddonState::NotShowing) => {
            return card
                .child(message(
                    "Load Pececito's addon",
                    "Type /reload in the game, or turn Pececito on in the AddOns list.".into(),
                    c.text_muted,
                ))
                .child(el().grow(1.0))
                .child(steps(None, kit));
        }
        Some(AddonState::NoPole) => {
            return card
                .child(message(
                    "Equip your fishing pole",
                    "Pececito casts once a fishing pole is in your hands.".into(),
                    c.text_muted,
                ))
                .child(el().grow(1.0))
                .child(steps(None, kit));
        }
        Some(AddonState::Showing) | None => {}
    }
    if let Session::Stopped(finished) = &state.session {
        let ended = match &finished.reason {
            StopReason::Failed(error) => {
                Some(message("Fishing stopped", error.clone(), c.danger.fg))
            }
            StopReason::AddonMissing => Some(message(
                "Fishing stopped",
                "Pececito lost sight of its addon. Check that it's still turned on in the game."
                    .into(),
                c.warning.fg,
            )),
            StopReason::TimeUp(limit) => Some(message(
                "Session over",
                format!("Stopped after {limit}, as set in settings."),
                c.text_muted,
            )),
            StopReason::Stopped | StopReason::GameClosed => None,
        };
        if let Some(ended) = ended {
            return card
                .child(ended)
                .child(el().grow(1.0))
                .child(steps(None, kit));
        }
    }
    let detail = match &state.fishing_key.shortcut() {
        Some(key) => format!(
            "Face the water and press Start. Pececito presses {} to cast and to hook.",
            key.keycaps().join("+")
        ),
        None => String::new(),
    };
    card.child(message("Ready to fish", detail, c.text_muted))
        .child(el().grow(1.0))
        .child(steps(None, kit))
}

fn fishing_status(card: El, fishing: &Fishing, clock: f64, kit: &Kit) -> El {
    let c = &kit.c;
    let run = &fishing.run;
    if let Phase::Paused(reason) = run.phase {
        let detail = match reason {
            PauseReason::Interrupted => {
                "Your cast ended early. Cast by hand or press Resume to keep fishing."
            }
            PauseReason::NoCast => {
                "Your fishing key didn't start a cast. Face the water, then cast by hand or press Resume."
            }
            PauseReason::NoPole => {
                "You don't have a fishing pole in your hands. Equip one, then cast by hand or press Resume."
            }
        };
        let resume = Button::new("Resume")
            .variant(ButtonVariant::Neutral)
            .on_press(State::resume)
            .build(kit)
            .key("resume");
        return card
            .child(
                col()
                    .gap(4.0)
                    .child(text("Paused").font(HEADLINE).color(c.text_base))
                    .child(
                        text(detail)
                            .font(kit.ty.ui)
                            .color(c.text_muted)
                            .wrap()
                            .w_full(),
                    ),
            )
            .child(el().grow(1.0))
            .child(row().child(resume));
    }
    let now = clock - fishing.started;
    let waited = run.waited(now);
    let progress = waited.map_or(0.0, |waited| (waited / CAST_LENGTH).clamp(0.0, 1.0) as f32);
    let (headline, timer) = if fishing.has_focus {
        let timer = waited.map_or(String::new(), |waited| {
            format!("{:.1}s of {CAST_LENGTH:.0}s", waited.min(CAST_LENGTH))
        });
        (run.phase.headline(), timer)
    } else {
        ("Click the game window", "Paused".to_string())
    };
    let top = row()
        .w_full()
        .center_y()
        .child(
            text(format!("Cast {}", run.casts().max(1)))
                .font(kit.ty.small_medium)
                .color(c.text_faint)
                .grow(1.0),
        )
        .child(text(timer).font(TABULAR).color(c.text_muted));
    let bar = el()
        .key("bite-bar")
        .w_full()
        .h(4.0)
        .radius(2.0)
        .fill(c.layer_02)
        .child(
            el().h(4.0)
                .radius(2.0)
                .w_frac(progress)
                .fill(c.success.icon),
        );
    card.child(top)
        .child(text(headline).font(HEADLINE).color(c.text_base))
        .child(el().grow(1.0))
        .child(bar)
        .child(steps(Some(run.phase.step()), kit))
}

/// The loop's four steps; with no current step, all of them dimmed as a preview.
fn steps(current: Option<Phase>, kit: &Kit) -> El {
    let c = &kit.c;
    let current_index =
        current.and_then(|current| Phase::STEPS.iter().position(|phase| *phase == current));
    Phase::STEPS
        .iter()
        .enumerate()
        .fold(row().w_full().gap(6.0), |steps, (index, phase)| {
            let (dot, ink) = match current_index {
                Some(at) if at == index => (c.success.icon, c.text_base),
                Some(at) if at > index => (c.icon_muted, c.text_muted),
                _ => (c.layer_03, c.text_faint),
            };
            steps.child(
                row()
                    .grow(1.0)
                    .gap(6.0)
                    .center_y()
                    .child(el().size(6.0, 6.0).radius(3.0).fill(dot))
                    .child(text(phase.step_label()).font(kit.ty.small).color(ink)),
            )
        })
}

fn stats(state: &State, kit: &Kit) -> El {
    let (label, run, duration) = match &state.session {
        Session::Fishing(fishing) => (
            "This session",
            Some(&fishing.run),
            state.clock - fishing.started,
        ),
        Session::Stopped(finished) => ("Last session", Some(&finished.run), finished.duration),
        Session::Idle => ("This session", None, 0.0),
    };
    let (hooked, casts) = match run {
        Some(run) => (run.hooked().to_string(), run.casts().to_string()),
        None => ("0".into(), "0".into()),
    };
    let tile = |value: String, name: &'static str| {
        panel(kit)
            .grow(1.0)
            .gap(2.0)
            .child(text(value).font(STAT_VALUE).color(kit.c.text_base))
            .child(text(name).font(kit.ty.small).color(kit.c.text_muted))
    };
    col()
        .key("stats")
        .w_full()
        .gap(8.0)
        .child(section_label(kit, label))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(tile(hooked, "Hooked"))
                .child(tile(casts, "Casts"))
                .child(tile(minutes_seconds(duration), "Time")),
        )
}

fn casts(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let history = match &state.session {
        Session::Fishing(fishing) => fishing.run.history.as_slice(),
        Session::Stopped(finished) => finished.run.history.as_slice(),
        Session::Idle => &[],
    };
    let list = panel(kit)
        .key("casts")
        .grow(1.0)
        .gap(10.0)
        .clip_rounded(kit.radius.lg);
    if history.is_empty() {
        return col()
            .w_full()
            .grow(1.0)
            .gap(8.0)
            .child(section_label(kit, "Casts"))
            .child(
                list.center().child(
                    text("Your casts show up here.")
                        .font(kit.ty.ui)
                        .color(c.text_faint),
                ),
            );
    }
    let rows = history.iter().rev().take(RECENT_CASTS).map(|cast| {
        let (name, ink) = match cast.outcome {
            Outcome::Hooked { after } => (format!("Hooked after {after:.1}s"), c.text_base),
            Outcome::NoBite => ("No bite".to_string(), c.text_faint),
            Outcome::Missed => ("Missed the bite".to_string(), c.text_muted),
            Outcome::Interrupted => ("Cast ended early".to_string(), c.text_faint),
        };
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(text(name).font(kit.ty.ui).color(ink).grow(1.0).shrink())
            .child(
                text(minutes_seconds(cast.at))
                    .font(TABULAR)
                    .color(c.text_faint),
            )
    });
    col()
        .w_full()
        .grow(1.0)
        .gap(8.0)
        .child(section_label(kit, "Casts"))
        .child(list.children(rows))
}

fn action_button(state: &State, kit: &Kit) -> El {
    match state.session {
        Session::Fishing(_) => Button::new("Stop")
            .variant(ButtonVariant::Danger)
            .control_size(ControlSize::ExtraLarge)
            .on_press(State::stop)
            .build(kit)
            .key("stop"),
        Session::Idle | Session::Stopped(_) => Button::new("Start fishing")
            .variant(ButtonVariant::Submit)
            .control_size(ControlSize::ExtraLarge)
            .disabled(!state.can_fish())
            .on_press_tx(State::start)
            .build(kit)
            .key("fish"),
    }
    .w_full()
}

// Settings screen

fn settings_screen(state: &State, kit: &Kit) -> El {
    let back = IconButton::new(&ARROW_LEFT)
        .variant(IconButtonVariant::Ghost)
        .label("Back")
        .on_press(State::close_settings)
        .build(kit)
        .key("back");
    let header = row().w_full().gap(8.0).center_y().child(back).child(
        text("Settings")
            .font(kit.ty.body_medium)
            .color(kit.c.text_base),
    );

    col()
        .child(header)
        .child(settings_section(
            kit,
            "Game",
            vec![fishing_key_setting(state, kit), addon_setting(state, kit)],
        ))
        .child(settings_section(
            kit,
            "Hooking",
            vec![setting(
                kit,
                "Hook delay",
                if state.is_fishing() {
                    "Changes apply from the next session."
                } else {
                    "A random wait between the splash and the hook, different for every bite."
                },
                SegmentedControl::new(
                    &HookDelay::ALL.map(HookDelay::label),
                    HookDelay::ALL
                        .iter()
                        .position(|delay| *delay == state.hook_delay),
                )
                .label("Hook delay")
                .width(None)
                .on_pick(|state: &mut State, index, _tx| state.pick_hook_delay(index))
                .build(kit)
                .key("hook-delay"),
            )],
        ))
        .child(settings_section(
            kit,
            "Session",
            vec![setting(
                kit,
                "Stop after",
                "Ends the session on its own.",
                SegmentedControl::new(&STOP_AFTER.map(|(label, _)| label), Some(state.stop_after))
                    .label("Stop after")
                    .width(None)
                    .on_pick(|state: &mut State, index, _tx| state.pick_stop_after(index))
                    .build(kit)
                    .key("stop-after"),
            )],
        ))
}

fn settings_section(kit: &Kit, label: &'static str, settings: Vec<El>) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(section_label(kit, label))
        .child(panel(kit).gap(14.0).children(settings))
}

/// A setting with its name and explanation above, and its control below when it's wide.
fn setting(kit: &Kit, name: &'static str, description: &'static str, control: El) -> El {
    col()
        .w_full()
        .gap(10.0)
        .child(setting_text(
            kit,
            name,
            description.to_string(),
            kit.c.text_muted,
        ))
        .child(control)
}

fn setting_text(kit: &Kit, name: &'static str, description: String, color: Color) -> El {
    col()
        .grow(1.0)
        .shrink()
        .gap(2.0)
        .child(text(name).font(kit.ty.ui_medium).color(kit.c.text_base))
        .child(
            text(description)
                .font(kit.ty.small)
                .color(color)
                .wrap()
                .w_full(),
        )
}

fn fishing_key_setting(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let recorder = ShortcutRecorder::new("fishing-key", &state.fishing_key, |state: &mut State| {
        &mut state.fishing_key
    })
    .label("Fishing key")
    .allow_unmodified()
    .width(132.0)
    .disabled(state.is_fishing())
    .on_change(|state: &mut State, shortcut, _tx| {
        state.save_fishing_key(shortcut);
        Ok(())
    })
    .build(kit);
    let (description, color) = match (&state.save_error, state.fishing_key.shortcut()) {
        (Some(error), _) => (format!("Couldn't save: {error}"), c.danger.fg),
        (None, None) => ("Required to start fishing.".to_string(), c.warning.fg),
        (None, Some(_)) if state.is_fishing() => {
            ("Stop fishing to change it.".to_string(), c.text_muted)
        }
        (None, Some(_)) => (
            "The key bound to Fishing in game.".to_string(),
            c.text_muted,
        ),
    };
    row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(setting_text(kit, "Fishing key", description, color))
        .child(recorder)
}

fn addon_setting(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let (description, color) = match (state.install_error(), state.addon()) {
        (Some(error), _) => (format!("Couldn't install: {error}"), c.danger.fg),
        (None, None) => ("Open the game to install it.".to_string(), c.text_muted),
        (None, Some(AddonState::NotInstalled)) => (
            "Pececito reads it in the game to see when you're fishing.".to_string(),
            c.warning.fg,
        ),
        (None, Some(AddonState::NotShowing)) => (
            "Installed. Type /reload in the game to load it.".to_string(),
            c.text_muted,
        ),
        (None, Some(AddonState::Showing | AddonState::NoPole)) => {
            ("Installed and running.".to_string(), c.text_muted)
        }
    };
    row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(setting_text(kit, "Pececito addon", description, color))
        .child(install_button(state, kit))
}

fn install_button(state: &State, kit: &Kit) -> El {
    let label = match state.addon() {
        _ if state.is_installing() => "Installing",
        Some(AddonState::NotShowing | AddonState::NoPole | AddonState::Showing) => "Reinstall",
        Some(AddonState::NotInstalled) | None => "Install",
    };
    Button::new(label)
        .variant(ButtonVariant::Neutral)
        .disabled(state.window().is_none() || state.is_installing())
        .on_press(State::install_addon)
        .build(kit)
        .key("install-addon")
}
