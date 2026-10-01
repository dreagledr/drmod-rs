//! The script controls region: Save, the run, the rules the run is configured by, and what the game
//! is doing.
//!
//! This is the editor's one window onto the mod. The levers here are the ones the python tools set
//! before a script goes out (`drmod_api.fixed_dt` / `fps_cap` / `rng`), and what they are set to is
//! read back from the game rather than remembered from the click — a panel that shows what it
//! *asked for* while the game says something else is worse than no panel.
//!
//! ⚠️ The rules are **not part of the script**. They are the mod's state for one run, which the text
//! format says itself (`docs/SCRIPT_DSL.md` §6), so they live in the editor's settings and no `.tas`
//! file is rewritten to hold them.

use dear_app::imgui::{Condition, Ui};

use crate::api::{FpsCapMode, GameScriptPhase, GameStatus, PlaybackRules};
use crate::script::ScriptTextStatus;

/// What a frame of this region decided.
#[derive(Clone, Debug, Default)]
pub struct ControlsActions {
    pub save: bool,
    pub run: bool,
    pub apply: bool,
    pub cancel: bool,
    /// A rule was edited.
    pub rules_changed: bool,
    /// The seed field was typed into; the new text is [`ControlsActions::seed_out`].
    pub seed_changed: bool,
    pub seed_out: Option<String>,
}

/// Everything the panel paints, so the drawing code takes values rather than the shell.
pub struct ControlsView<'a> {
    pub status: &'a ScriptTextStatus,
    pub dirty: bool,
    pub rules: PlaybackRules,
    pub seed_text: &'a str,
    pub game: &'a GameStatus,
    pub preparing: bool,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
}

pub fn draw(ui: &Ui, view: &mut ControlsView<'_>, actions: &mut ControlsActions) {
    buttons(ui, view, actions);
    ui.separator();
    rules(ui, view, actions);
    ui.separator();

    if view.game.online {
        ui.text_wrapped(&status_line(view.game));
    } else {
        ui.text_wrapped(OFFLINE);
    }

    if let Some(error) = view.error {
        ui.text_wrapped(error);
    } else if let Some(message) = view.message {
        ui.text_wrapped(message);
    } else {
        ui.text_wrapped(&starts(view.status));
    }
}

fn buttons(ui: &Ui, view: &ControlsView<'_>, actions: &mut ControlsActions) {
    {
        let _disabled = ui.begin_disabled_with_cond(!view.dirty);
        if ui.button("Save") {
            actions.save = true;
        }
    }

    ui.same_line();

    // Run is live only with something to run, the mod answering, and no run in flight — the mod
    // holds one script slot and answers a second one `409`. Filling in the panel does not need a
    // game, but running does, and a disabled button is a better answer than a click that can only
    // fail.
    let can_run = view.game.online && !view.preparing && !view.game.script_active();
    {
        let _disabled = ui.begin_disabled_with_cond(!can_run);
        if ui.button("Run") {
            actions.run = true;
        }
    }

    ui.same_line();

    // Apply sets the levers on their own, without a script: an extreme one (1 fps, a lifted cap) is
    // how a run is made cheap, and it also outlives the run. It needs the game answering and nothing
    // else.
    {
        let _disabled = ui.begin_disabled_with_cond(!(view.game.online && !view.preparing));
        if ui.button("Apply") {
            actions.apply = true;
        }
    }

    ui.same_line();

    // Cancel is live exactly while the mod says a script is active, whoever started it (a run from
    // the python tools is a run this panel can stop).
    {
        let _disabled = ui.begin_disabled_with_cond(!view.game.script_active());
        if ui.button("Cancel") {
            actions.cancel = true;
        }
    }

    ui.same_line();
    if view.preparing {
        ui.text("working…");
    } else if view.dirty {
        ui.text("unsaved changes");
    } else {
        ui.text("saved");
    }
}

fn rules(ui: &Ui, view: &mut ControlsView<'_>, actions: &mut ControlsActions) {
    let mut fixed_tick = view.rules.fixed_tick;
    if ui.checkbox("Fixed tick 1/60", &mut fixed_tick) {
        view.rules.fixed_tick = fixed_tick;
        actions.rules_changed = true;
    }

    ui.same_line();

    let cap_names = ["default", "unlimited", "custom"];
    let selected_cap = crate::settings::cap_index(view.rules.cap);
    ui.set_next_item_width(120.0);
    if let Some(combo) = ui.begin_combo("Frame cap", cap_names[selected_cap]) {
        for (index, name) in cap_names.iter().enumerate() {
            if ui
                .selectable_config(*name)
                .selected(index == selected_cap)
                .build()
            {
                view.rules.cap = crate::settings::cap_from_index(index);
                actions.rules_changed = true;
            }
        }

        combo.end();
    }

    ui.same_line();
    {
        let _disabled = ui.begin_disabled_with_cond(view.rules.cap != FpsCapMode::Custom);
        let mut fps = view.rules.custom_fps as i32;
        ui.set_next_item_width(80.0);
        if ui.input_int("FPS", &mut fps) {
            view.rules.custom_fps =
                (fps.max(0) as u32).clamp(PlaybackRules::MIN_FPS, PlaybackRules::MAX_FPS);
            actions.rules_changed = true;
        }
    }

    ui.same_line();
    let mut pin_seed = view.rules.pin_seed;
    if ui.checkbox("Freeze seed", &mut pin_seed) {
        view.rules.pin_seed = pin_seed;
        actions.rules_changed = true;
    }

    ui.same_line();
    let mut seed = view.seed_text.to_owned();
    ui.set_next_item_width(110.0);
    if ui.input_text("Seed (0x for hex)", &mut seed).build() {
        actions.seed_changed = true;
        // The caller reads the field's new text back out of `seed_out`; the borrowed view cannot
        // hold the new string, and leaking one per keystroke would be worse than the indirection.
        actions.seed_out = Some(seed);
    }

    ui.same_line();
    let mut headless = view.rules.headless;
    if ui.checkbox("Headless run", &mut headless) {
        view.rules.headless = headless;
        actions.rules_changed = true;
    }
}

/// The live status line: the game, then what the mod says about the levers — read from `/state`,
/// never from what the panel asked for.
pub fn status_line(game: &GameStatus) -> String {
    let mut parts = vec![game.menu_status.clone()];

    if !game.mission_name.is_empty() {
        parts.push(game.mission_name.clone());
    } else if game.mission_id != 0 {
        parts.push(format!("mission {}", game.mission_id));
    }

    parts.push(format!("{:.1} fps", game.fps));

    if let Some(script) = &game.script {
        parts.push(format!(
            "{} {}/{} ({})",
            script.name,
            script.frame,
            script.total_frames,
            phase_word(script.phase)
        ));
    }

    parts.push(if game.fixed_tick {
        "tick fixed 1/60".to_owned()
    } else {
        "tick as in the game".to_owned()
    });

    parts.push(match game.cap_limit {
        Some(limit) => format!("cap {limit} fps"),
        None => "cap unlimited".to_owned(),
    });

    parts.push(if game.rng_pin == "off" {
        "rng off".to_owned()
    } else {
        format!("rng {} {}", game.rng_pin, game.rng_seed)
    });

    if game.headless {
        parts.push("headless".to_owned());
    }

    parts.join(" · ")
}

/// What the region says when the mod does not answer at all.
pub const OFFLINE: &str = "Offline — the game is not running the mod, or the API is not reachable";

/// What a run of the text on screen would be. Read from the document the pane parsed rather than
/// from the file: the rules line is edited as text, and the trigger and the restart are what the run
/// is about.
pub fn starts(status: &ScriptTextStatus) -> String {
    let Some(document) = &status.document else {
        return "Not a script the mod would run — the text region below says why".to_owned();
    };

    let start = match &document.trigger {
        Some(trigger) if trigger.ticks.is_some() => format!(
            "starts after {} ticks of gameplay",
            trigger.ticks.unwrap_or(0)
        ),
        Some(trigger) if trigger.pos.is_some() => {
            let at = trigger.pos.unwrap_or([0.0, 0.0, 0.0]);
            format!("starts at {:.2}, {:.2}, {:.2}", at[0], at[1], at[2])
        }
        _ => "starts at once".to_owned(),
    };

    let restart = if document.restart.is_some() {
        " · restarts the mission first"
    } else {
        ""
    };

    format!("\u{201c}{}\u{201d} {start}{restart}", document.name)
}

fn phase_word(phase: GameScriptPhase) -> &'static str {
    match phase {
        GameScriptPhase::Restarting => "restarting",
        GameScriptPhase::Armed => "armed",
        GameScriptPhase::Running => "running",
        GameScriptPhase::Done => "done",
        GameScriptPhase::Stopped => "stopped",
        GameScriptPhase::Unknown => "unknown",
    }
}

/// The window the combo's popup needs; `Condition::Always` because the popup is positioned by ImGui
/// itself and the value only says it is wanted.
#[allow(dead_code)]
fn popup_condition() -> Condition {
    Condition::Always
}
