use std::path::Path;
use std::sync::OnceLock;

use rune::{
    Color, Cursor, Cx, Drag, El, Icon, Phase as DragPhase, ScrollSpec, TextStyle, Tx, col, el, row,
    text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Placement,
    SegmentedControl, ShortcutRecorder, popover, scrim,
};

use crate::{Fishing, Game, Screen, Session, State, StopReason};
use fishing::{CAST_LENGTH, HookDelay, Outcome, PauseReason, Phase};
use game::{AddonMismatch, AddonState, Detection};

/// Points square; the logo is rasterized at this size.
pub const LOGO_SIZE: f32 = 56.0;

pub struct SessionLength {
    pub label: &'static str,
    /// None fishes until the user stops.
    pub minutes: Option<u64>,
    /// Where the stop sits along the slider, from 0 to 1.
    position: f32,
}

/// Session lengths to pick from; Forever sits apart at the far end.
pub const FISH_FOR: [SessionLength; 5] = [
    SessionLength {
        label: "5 min",
        minutes: Some(5),
        position: 0.0,
    },
    SessionLength {
        label: "10 min",
        minutes: Some(10),
        position: 0.18,
    },
    SessionLength {
        label: "1 hour",
        minutes: Some(60),
        position: 0.36,
    },
    SessionLength {
        label: "2 hours",
        minutes: Some(120),
        position: 0.54,
    },
    SessionLength {
        label: "Forever",
        minutes: None,
        position: 1.0,
    },
];
/// The main screen's panels in the fixed 400 pt window, inside its 16 pt padding.
const CONTENT_WIDTH: f32 = 368.0;
const POPOVER_PADDING: f32 = 14.0;
const FISH_FOR_BUTTON_WIDTH: f32 = 96.0;
const KNOB: f32 = 18.0;
const TRACK: f32 = 4.0;
const STOP_LABEL_WIDTH: f32 = 60.0;
const RECENT_CASTS: usize = 6;
const TOOLBAR_HEIGHT: f32 = 44.0;

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
    let content = |screen: El| screen.w_full().gap(12.0).padding(16.0);
    let mut layer = Vec::new();
    let (bar, body) = match state.screen {
        Screen::Main => (
            main_toolbar(state, kit),
            content(main_screen(state, kit, &mut layer)).grow(1.0),
        ),
        // Settings run taller than the fixed window.
        Screen::Settings => (
            settings_toolbar(kit),
            col()
                .key("settings-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(content(settings_screen(state, kit))),
        ),
    };
    let base = col()
        .fill(kit.c.page)
        .child(bar)
        .child(el().w_full().h(1.0).fill(kit.c.border_muted))
        .child(body);
    rune_kit::root(base, layer)
}

fn toolbar(kit: &Kit) -> El {
    row()
        .key("toolbar")
        .w_full()
        .h(TOOLBAR_HEIGHT)
        .px(8.0)
        .gap(4.0)
        .center_y()
        .fill(kit.c.bg_base)
}

/// Shows the hand cursor over a control while it's clickable; rune-kit keeps the arrow.
trait Clickable {
    fn pointer(self, enabled: bool) -> Self;
}

impl Clickable for El {
    fn pointer(self, enabled: bool) -> El {
        if enabled {
            self.cursor(Cursor::Pointer)
        } else {
            self
        }
    }
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

fn main_screen(state: &State, kit: &Kit, layer: &mut Vec<El>) -> El {
    col()
        .child(status_card(state, kit))
        .child(stats(state, kit))
        .child(casts(state, kit))
        .child(
            row()
                .key("actions")
                .w_full()
                .gap(8.0)
                .child(action_button(state, kit).grow(1.0))
                .child(fish_for_button(state, kit, layer)),
        )
}

fn main_toolbar(state: &State, kit: &Kit) -> El {
    let reset_help = match state.session {
        Session::Stopped(_) => "Clear the last session's results",
        Session::Fishing(_) => "Stop fishing to reset",
        Session::Idle => "Nothing to reset yet",
    };
    let reset = IconButton::new(&ROTATE_CCW)
        .variant(IconButtonVariant::Ghost)
        .label("Reset session")
        .disabled(!matches!(state.session, Session::Stopped(_)))
        .on_press(State::reset_session)
        .build(kit)
        .pointer(matches!(state.session, Session::Stopped(_)))
        .key("reset")
        .help(reset_help);
    let settings = IconButton::new(&GEAR)
        .variant(IconButtonVariant::Ghost)
        .label("Settings")
        .on_press(State::open_settings)
        .build(kit)
        .pointer(true)
        .key("settings")
        .help("Settings");
    toolbar(kit)
        .child(connection(state, kit).grow(1.0))
        .child(reset)
        .child(settings)
}

fn connection(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let (label, dot, help) = match &state.game {
        Game::Detected(Detection::Found(..)) => (
            "Connected",
            c.success.icon,
            "Connected to the WoW Forever window".to_string(),
        ),
        Game::Detected(Detection::NoWindow(_)) => (
            "Game loading",
            c.warning.icon,
            "Waiting for the game window".to_string(),
        ),
        Game::Detected(Detection::NotRunning) => (
            "Game not running",
            c.icon_faint,
            "Pececito connects when you open WoW Forever".to_string(),
        ),
        Game::Searching => (
            "Looking for game",
            c.icon_faint,
            "Looking for WoW Forever".to_string(),
        ),
        Game::Error(error) => ("Can't connect", c.danger.icon, error.clone()),
    };
    row()
        .key("connection")
        .role("status")
        .label(label)
        .gap(8.0)
        .center_y()
        .pl(8.0)
        .help(help)
        .child(el().size(8.0, 8.0).radius(4.0).fill(dot).a11y_hidden())
        .child(
            text(label)
                .font(kit.ty.ui_medium)
                .color(c.text_base)
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
            "Open the game and Pececito connects on its own.".into(),
            c.text_muted,
        )),
        Game::Detected(Detection::NoWindow(_)) => Some(message(
            "WoW Forever is loading",
            "Waiting for the game window.".into(),
            c.text_muted,
        )),
        Game::Error(error) => Some(message(
            "Can't connect to the game",
            error.clone(),
            c.danger.fg,
        )),
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
            .pointer(true)
            .key("open-settings")
    };
    if state.fishing_key.shortcut().is_none() {
        return card
            .child(message(
                "Set your fishing key",
                "Pick the key bound to Fishing in the game. Pececito presses it to cast and hook."
                    .into(),
                c.text_muted,
            ))
            .child(row().child(open_settings()));
    }
    match state.addon() {
        Some(AddonState::NotInstalled) => {
            let (detail, color) = match state.install_error() {
                Some(error) => (format!("Couldn't install: {error}"), c.danger.fg),
                None => (
                    "The addon lets Pececito see when you're fishing.".to_string(),
                    c.text_muted,
                ),
            };
            return card
                .child(message("Install Pececito's addon", detail, color))
                .child(row().child(install_button(state, kit)));
        }
        Some(AddonState::Mismatched(mismatch)) => {
            let (headline, advice) = match mismatch {
                AddonMismatch::Older { installed, shipped } => (
                    "Update Pececito's addon",
                    format!(
                        "The game has version {installed} and this Pececito reads version {shipped}. Update it, then type /reload in the game chat."
                    ),
                ),
                AddonMismatch::Newer { installed, shipped } => (
                    "Update Pececito",
                    format!(
                        "The game has addon version {installed}, newer than the version {shipped} this Pececito reads. Update Pececito, or reinstall the addon to go back to version {shipped}."
                    ),
                ),
                AddonMismatch::Edited => (
                    "Reinstall Pececito's addon",
                    "The addon's files in the game were changed. Reinstall it, then type /reload in the game chat."
                        .to_string(),
                ),
            };
            let (detail, color) = match state.install_error() {
                Some(error) => (format!("Couldn't install: {error}"), c.danger.fg),
                None => (advice, c.text_muted),
            };
            return card
                .child(message(headline, detail, color))
                .child(row().child(install_button(state, kit)));
        }
        Some(AddonState::Unreadable(error)) => {
            return card
                .child(message(
                    "Pececito can't see the game window",
                    format!("Keep it on screen and not minimized. ({error})"),
                    c.warning.fg,
                ))
                .child(el().grow(1.0))
                .child(steps(None, kit));
        }
        Some(AddonState::NotShowing) => {
            return card
                .child(message(
                    "Load Pececito's addon",
                    "Type /reload in the game chat, or turn on Pececito in the AddOns list.".into(),
                    c.text_muted,
                ))
                .child(el().grow(1.0))
                .child(steps(None, kit));
        }
        Some(AddonState::NoPole) => {
            return card
                .child(message(
                    "Equip a fishing pole",
                    "Pececito can start once a fishing pole is equipped.".into(),
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
                "Pececito lost contact with its addon. Check that it's turned on, then type /reload."
                    .into(),
                c.warning.fg,
            )),
            StopReason::TimeUp(limit) => Some(message(
                "Session over",
                format!(
                    "Stopped after {limit}. Change it with the button next to Start fishing."
                ),
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
            "Face the water, then press Start fishing. Pececito presses {} to cast and hook.",
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
                "Your fishing key didn't cast. Check that it's bound to Fishing and you're facing water."
            }
            PauseReason::NoPole => {
                "No fishing pole is equipped. Equip one, then cast by hand or press Resume."
            }
        };
        let resume = Button::new("Resume")
            .variant(ButtonVariant::Neutral)
            .on_press(State::resume)
            .build(kit)
            .pointer(true)
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
    let left = run
        .waited(now)
        .map(|waited| (CAST_LENGTH - waited).clamp(0.0, CAST_LENGTH));
    let progress = left.map_or(0.0, |left| (left / CAST_LENGTH) as f32);
    let (headline, timer) = if fishing.has_focus {
        let timer = left.map_or(String::new(), |left| format!("{left:.1}s"));
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
            .child(section_label(kit, "Recent casts"))
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
        .child(section_label(kit, "Recent casts"))
        .child(list.children(rows))
}

fn action_button(state: &State, kit: &Kit) -> El {
    match state.session {
        Session::Fishing(_) => Button::new("Stop fishing")
            .variant(ButtonVariant::Danger)
            .control_size(ControlSize::ExtraLarge)
            .on_press(State::stop)
            .build(kit)
            .pointer(true)
            .key("stop"),
        Session::Idle | Session::Stopped(_) => Button::new("Start fishing")
            .variant(ButtonVariant::Submit)
            .control_size(ControlSize::ExtraLarge)
            .disabled(!state.can_fish())
            .on_press_tx(State::start)
            .build(kit)
            .pointer(state.can_fish())
            .key("fish"),
    }
}

/// How long to fish, picked in a popover above the buttons.
fn fish_for_button(state: &State, kit: &Kit, layer: &mut Vec<El>) -> El {
    let picked = FISH_FOR.get(state.fish_for);
    let trigger = Button::new(picked.map_or("", |length| length.label))
        .variant(ButtonVariant::Neutral)
        .control_size(ControlSize::ExtraLarge)
        .on_press(State::toggle_fish_for)
        .build(kit)
        .pointer(true)
        .key("fish-for-button")
        .w(FISH_FOR_BUTTON_WIDTH)
        .help("How long to fish")
        .expanded(state.is_fish_for_open());
    if !state.is_fish_for_open() {
        return trigger;
    }
    let shown = FISH_FOR.get(state.shown_fish_for());
    let description = if shown.is_some_and(|length| length.minutes.is_none()) {
        "Pececito fishes until you press Stop."
    } else {
        "Pececito stops on its own after this long."
    };
    let content = col()
        .key("fish-for-popover")
        .role("dialog")
        .label("Fish for")
        .description(description)
        .focus_return("fish-for-button")
        .w(CONTENT_WIDTH)
        .padding(POPOVER_PADDING)
        .gap(12.0)
        .child(
            col()
                .gap(2.0)
                .child(
                    text("Fish for")
                        .font(kit.ty.body_medium)
                        .color(kit.c.text_strong),
                )
                .child(text(description).font(kit.ty.small).color(kit.c.text_muted)),
        )
        .child(fish_for_slider(
            state,
            kit,
            CONTENT_WIDTH - 2.0 * POPOVER_PADDING,
        ));
    layer.push(scrim("fish-for-scrim", State::close_fish_for));
    layer.push(popover::anchored(
        kit,
        popover::raised(kit, content, kit.radius.lg),
        "actions",
        Placement::Top,
        8.0,
    ));
    trigger
}

// Settings screen

fn settings_toolbar(kit: &Kit) -> El {
    let back = IconButton::new(&ARROW_LEFT)
        .variant(IconButtonVariant::Ghost)
        .label("Back")
        .on_press(State::close_settings)
        .build(kit)
        .pointer(true)
        .key("back");
    toolbar(kit).child(back).child(
        text("Settings")
            .font(kit.ty.ui_medium)
            .color(kit.c.text_base),
    )
}

fn settings_screen(state: &State, kit: &Kit) -> El {
    col()
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
                    "Changes apply to your next session."
                } else {
                    "How long Pececito waits after the splash before it hooks. Each bite gets a slightly different wait."
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
                .pointer(true)
                .key("hook-delay"),
            )],
        ))
        .child(settings_section(
            kit,
            "Config file",
            vec![config_file_setting(state, kit)],
        ))
        .child(about(state, kit))
}

fn about(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let logo = el().size(LOGO_SIZE, LOGO_SIZE);
    let logo = match &state.logo {
        Some(image) => logo.pixels(image.pixels()),
        None => logo,
    };
    col()
        .key("about")
        .w_full()
        .center_y()
        .gap(4.0)
        .pt(16.0)
        .pb(8.0)
        .child(logo.mb(8.0))
        .child(text("Pececito").font(kit.ty.body_medium).color(c.text_base))
        .child(
            text(concat!("Version ", env!("CARGO_PKG_VERSION")))
                .font(kit.ty.small)
                .color(c.text_muted),
        )
        .child(
            text("A fishing companion for WoW Forever.")
                .font(kit.ty.small)
                .color(c.text_faint),
        )
}

fn config_file_setting(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let path = state.settings.path();
    let (note, color) = match (&state.save_error, state.open_config.error(), path) {
        (Some(error), _, _) => (format!("Couldn't save your settings: {error}"), c.danger.fg),
        (None, Some(error), _) => (error.to_string(), c.danger.fg),
        (None, None, Some(_)) => (
            "Pececito reads it when it starts. Edit it while Pececito is closed.".to_string(),
            c.text_muted,
        ),
        (None, None, None) => (
            "Settings aren't saved to a file in this run.".to_string(),
            c.text_muted,
        ),
    };
    let location = path.map_or_else(|| "No file".to_string(), display_path);
    let open = Button::new("Open")
        .variant(ButtonVariant::Neutral)
        .disabled(path.is_none())
        .on_press(State::open_config_file)
        .build(kit)
        .pointer(path.is_some())
        .key("open-config");
    col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(setting_text(kit, "Settings file", location, c.text_muted))
                .child(open),
        )
        .child(text(note).font(kit.ty.small).color(color).wrap().w_full())
}

/// The stop closest to `value`, a point along the slider from 0 to 1.
pub fn nearest_length(value: f32) -> usize {
    FISH_FOR
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (a.position - value)
                .abs()
                .total_cmp(&(b.position - value).abs())
        })
        .map_or(0, |(index, _)| index)
}

pub fn step_fish_for(state: &mut State, steps: i32, _tx: &mut Tx) {
    let last = FISH_FOR.len() as i64 - 1;
    let index = (state.fish_for as i64 + i64::from(steps)).clamp(0, last);
    state.pick_fish_for(index as usize);
}

fn fish_for_slider(state: &State, kit: &Kit, width: f32) -> El {
    let c = &kit.c;
    let travel = width - KNOB;
    let center = move |position: f32| KNOB / 2.0 + position * travel;
    let shown_index = state.shown_fish_for();
    let shown = FISH_FOR.get(shown_index);
    let position = shown.map_or(1.0, |length| length.position);
    let to_value = move |x: f32| ((x - KNOB / 2.0) / travel).clamp(0.0, 1.0);
    let track_top = (KNOB - TRACK) / 2.0;
    let ticks = FISH_FOR.iter().map(|length| {
        el().size(TRACK, TRACK)
            .radius(TRACK / 2.0)
            .absolute(center(length.position) - TRACK / 2.0, track_top)
            .fill(c.icon_faint)
            .a11y_hidden()
    });
    let slider = el()
        .key("fish-for")
        .role("slider")
        .label("Fish for")
        .range_value(shown_index as f64, 0.0, (FISH_FOR.len() - 1) as f64)
        .range_step(1.0)
        .a11y_value(shown.map_or("", |length| length.label))
        .on_step(step_fish_for)
        .on_step_limit(|state: &mut State, end, _tx| {
            state.pick_fish_for(if end { FISH_FOR.len() - 1 } else { 0 })
        })
        .size(width, KNOB)
        .pointer(true)
        .motion(rune::tok::motion::SNAPPY)
        .on_drag(
            move |state: &mut State, drag: &mut Drag, _tx| match drag.phase {
                DragPhase::Down | DragPhase::Move => {
                    state.drag_fish_for(nearest_length(to_value(drag.x)))
                }
                DragPhase::Up => state.end_fish_for_drag(!drag.cancelled()),
            },
        )
        .child(
            el().size(width, TRACK)
                .absolute(0.0, track_top)
                .radius(TRACK / 2.0)
                .fill(c.layer_02)
                .a11y_hidden(),
        )
        .children(ticks)
        .child(
            el().size(center(position), TRACK)
                .absolute(0.0, track_top)
                .radius(TRACK / 2.0)
                .fill(c.success.icon)
                .a11y_hidden(),
        )
        .child(
            el().size(KNOB, KNOB)
                .absolute(position * travel, 0.0)
                .radius(KNOB / 2.0)
                .fill(c.text_base)
                .border(1.0, c.border_base)
                .a11y_hidden(),
        );
    let last = FISH_FOR.len() - 1;
    let labels = FISH_FOR.iter().enumerate().map(|(index, length)| {
        // The end labels line up with the slider's edges.
        let left = match index {
            0 => 0.0,
            _ if index == last => width - STOP_LABEL_WIDTH,
            _ => center(length.position) - STOP_LABEL_WIDTH / 2.0,
        };
        let label = text(length.label)
            .font(kit.ty.small)
            .color(if index == shown_index {
                c.text_base
            } else {
                c.text_faint
            });
        let cell = row().w(STOP_LABEL_WIDTH).absolute(left, 0.0).a11y_hidden();
        match index {
            0 => cell.child(label),
            _ if index == last => cell.child(el().grow(1.0)).child(label),
            _ => cell
                .child(el().grow(1.0))
                .child(label)
                .child(el().grow(1.0)),
        }
    });
    col()
        .w(width)
        .gap(6.0)
        .child(slider)
        .child(el().w(width).h(16.0).children(labels))
}

/// The path with the home folder written as `~`.
fn display_path(path: &Path) -> String {
    std::env::var_os("HOME")
        .and_then(|home| path.strip_prefix(home).ok())
        .map_or_else(
            || path.display().to_string(),
            |inside| format!("~/{}", inside.display()),
        )
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
    .build(kit)
    .pointer(!state.is_fishing());
    let (description, color) = match state.fishing_key.shortcut() {
        None => ("Set a key to start fishing.".to_string(), c.warning.fg),
        Some(_) if state.is_fishing() => ("Stop fishing to change it.".to_string(), c.text_muted),
        Some(_) => (
            "Your Fishing keybind in the game.".to_string(),
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
    let installed = game::addon_version().map_or("Installed".to_string(), |version| {
        format!("Version {version} installed")
    });
    let (description, color) = match (state.install_error(), state.addon()) {
        (Some(error), _) => (format!("Couldn't install: {error}"), c.danger.fg),
        (None, None) => ("Open the game to install it.".to_string(), c.text_muted),
        (None, Some(AddonState::NotInstalled)) => (
            "Not installed. The addon lets Pececito see when you're fishing.".to_string(),
            c.warning.fg,
        ),
        (None, Some(AddonState::Mismatched(AddonMismatch::Older { installed, shipped }))) => (
            format!(
                "Version {installed} installed. Update it to version {shipped}, then type /reload in the game chat."
            ),
            c.warning.fg,
        ),
        (None, Some(AddonState::Mismatched(AddonMismatch::Newer { installed, shipped }))) => (
            format!(
                "Version {installed} installed, newer than this Pececito's version {shipped}. Update Pececito, or reinstall to go back."
            ),
            c.warning.fg,
        ),
        (None, Some(AddonState::Mismatched(AddonMismatch::Edited))) => (
            "Installed, but its files were changed. Reinstall it, then type /reload in the game chat."
                .to_string(),
            c.warning.fg,
        ),
        (None, Some(AddonState::NotShowing)) => (
            format!("{installed}. Type /reload in the game chat to load it."),
            c.text_muted,
        ),
        (None, Some(AddonState::Unreadable(_))) => (
            format!("{installed}. Pececito can't see the game window to check that it's running."),
            c.text_muted,
        ),
        (None, Some(AddonState::Showing | AddonState::NoPole)) => {
            (format!("{installed} and running."), c.text_muted)
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
        Some(AddonState::Mismatched(AddonMismatch::Older { .. })) => "Update",
        Some(
            AddonState::Mismatched(AddonMismatch::Newer { .. } | AddonMismatch::Edited)
            | AddonState::NotShowing
            | AddonState::Unreadable(_)
            | AddonState::NoPole
            | AddonState::Showing,
        ) => "Reinstall",
        Some(AddonState::NotInstalled) | None => "Install",
    };
    Button::new(label)
        .variant(ButtonVariant::Neutral)
        .disabled(state.window().is_none() || state.is_installing())
        .on_press(State::install_addon)
        .build(kit)
        .pointer(state.window().is_some() && !state.is_installing())
        .key("install-addon")
}
