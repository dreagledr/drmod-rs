//! Getting the game back into gameplay before a run — the editor's port of the python tools'
//! menu work (`drmod_api.ensure_gameplay` / `recover_fail`).
//!
//! Why it is needed at all: a script's own `restart` plays the pause menu, and a menu that is
//! already open swallows those keys — so a run started from the pause menu, or after a death left
//! the fail menu up, would arm and then not move. The python tools answer that by driving the menu
//! with short mod scripts (a `pause` bit toggles the pause menu, a `confirm` takes the fail menu's
//! preselected Retry) rather than by synthesising input, and the editor does the same.
//!
//! ⚠️ This only reads as far as the *mod's* status names. A menu the mod has no script for — the
//! game's front end, a mission that has not finished loading — is not something a blind confirm
//! should be pointed at, so those come back as a message naming the state instead.
//!
//! ⚠️ The steps need the game's window in the foreground ([`crate::game_window`]): the menu reads
//! its keyboard through the game's own poll, which gives up when the window is not foreground. The
//! caller brings it forward before asking.

use std::time::{Duration, Instant};

use crate::api::{GameScriptPhase, ModApi};
use crate::script::{json, ScriptCommand, ScriptDocument};
use crate::script::model::ScriptInput;

/// Long enough for a script of a handful of frames to be played and the pause to leave, short
/// enough that a stuck menu does not hold the caller forever.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

/// After the menu has been asked to close, the status does not turn `In Game` in the same tick —
/// the transition runs through `ProcessOutOfPause`.
const SETTLE: Duration = Duration::from_secs(3);

/// How often the status is re-read while waiting.
const POLL: Duration = Duration::from_millis(100);

/// The pause menu — the one a `pause` bit toggles.
const PAUSE_MENU: &str = "Pause Menu";

/// Menus the player is dead in: the game does not leave them on its own, and their preselected
/// entry is Retry, so a single confirm is the whole way out.
const FAIL_MENUS: [&str; 3] = ["Mission Fail", "Mission Failed", "Game Over"];

/// The statuses the game spends a mission load in. A run cannot start here — and it does not need
/// to be forced out of here either, because it is on its way out.
const LOADING_MENUS: [&str; 3] = [
    "Loading Into Mission",
    "Loading Into Boss Mission",
    "Main Menu Load",
];

/// Brings the game into gameplay, whatever menu it is in.
///
/// Returns why not, or `None` when it is playing.
///
/// The order is the tools': already playing is nothing to do; a fail menu is a confirm; anything
/// else the mod knows how to press is the pause toggle. A status that is none of those is a
/// refusal, because the alternative is guessing at a menu this client has never driven.
pub fn ensure_gameplay(api: &ModApi) -> Option<String> {
    let current = status(api)?;

    if current == crate::api::GameStatus::IN_GAME {
        return None;
    }

    if FAIL_MENUS
        .iter()
        .any(|menu| menu.eq_ignore_ascii_case(&current))
    {
        return if recover(api) {
            None
        } else {
            Some(format!(
                "the game is stuck in \u{201c}{current}\u{201d} — the retry confirm did not bring it back"
            ))
        };
    }

    if PAUSE_MENU.eq_ignore_ascii_case(&current) {
        return if close_pause(api) {
            None
        } else {
            let now = status(api).unwrap_or_else(|| "no answer".to_owned());
            Some(format!("the pause menu did not close (still \u{201c}{now}\u{201d})"))
        };
    }

    if LOADING_MENUS
        .iter()
        .any(|menu| menu.eq_ignore_ascii_case(&current))
    {
        return Some(format!(
            "the game is loading (\u{201c}{current}\u{201d}) — a run needs gameplay"
        ));
    }

    Some(format!(
        "the game is in \u{201c}{current}\u{201d}, and this panel only knows how to leave the pause and fail menus"
    ))
}

/// Toggles the pause menu shut — the same one-frame `pause` bit the tools send, played by the mod
/// itself.
pub fn close_pause(api: &ModApi) -> bool {
    if !play(api, "close-menu", |input| ScriptInput {
        pause: true,
        ..input
    }) {
        return false;
    }

    wait_for_menu(api, crate::api::GameStatus::IN_GAME, SETTLE)
}

/// Takes the fail menu's preselected Retry — `recover_fail` in the tools.
pub fn recover(api: &ModApi) -> bool {
    if !play(api, "fail-retry", |input| ScriptInput {
        confirm: true,
        ..input
    }) {
        return false;
    }

    wait_for_menu(api, crate::api::GameStatus::IN_GAME, SCRIPT_TIMEOUT)
}

/// Runs a one-command script through the mod and waits for it to finish (or be stopped).
///
/// It goes out as a script rather than as a keystroke because that is how the mod delivers menu
/// input at all — the pause menu does not tick the input unit, so frames are read through the
/// `isKeyDown`/`isKeyPressed` detours, which is exactly what a script's menu bits feed.
fn play(api: &ModApi, name: &str, input: fn(ScriptInput) -> ScriptInput) -> bool {
    let script = ScriptDocument {
        name: name.to_owned(),
        trigger: None,
        restart: None,
        commands: vec![ScriptCommand {
            // Three frames, as in the tools: a menu bit has to be down across the frame the menu
            // reads, and one frame is what the poll can miss.
            t: 0,
            duration: 3,
            input: input(ScriptInput::default()),
            when_enemy: None,
        }],
    };

    let Ok(body) = json::write(&script) else {
        return false;
    };

    let mut started = api.run(&body);
    if started.conflict {
        // The slot is held by whatever is running. Only a script that is *not* the menu step is
        // stopped here — and the caller has already refused a run, so there is nothing else a held
        // slot could be about.
        let _ = api.stop();
        started = api.run(&body);
    }

    let Some(response) = started.value else {
        return false;
    };

    wait_for_script(api, response.script_id, SCRIPT_TIMEOUT)
}

/// The current menu status, or `None` when the mod did not answer.
fn status(api: &ModApi) -> Option<String> {
    api.state().value.map(|state| state.menu_status)
}

/// Waits until the menu status is `menu`, or until the timeout runs out.
fn wait_for_menu(api: &ModApi, menu: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(state) = api.state().value
            && state.menu_status.eq_ignore_ascii_case(menu)
        {
            return true;
        }

        std::thread::sleep(POLL);
    }

    false
}

/// Waits until the script under `id` is over — done, stopped, or gone from the slot.
fn wait_for_script(api: &ModApi, id: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match api.state().value {
            // No answer is not a failure to report here: the game blocks its render loop while a
            // mission loads, and the caller's own poll will say so if it stays that way.
            None => {}
            Some(state) => match &state.script {
                // The mod has no script under that id any more: it finished, or it was stopped.
                None => return true,
                Some(script) if script.id != id => return true,
                Some(script) => {
                    if matches!(
                        script.phase,
                        GameScriptPhase::Done | GameScriptPhase::Stopped
                    ) {
                        return true;
                    }
                }
            },
        }

        std::thread::sleep(POLL);
    }

    false
}
