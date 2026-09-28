//! What survives a restart: the workspace folder, the game folder, and the run rules.
//!
//! A plain `key=value` file in `%LOCALAPPDATA%\tas-editor-rs\settings` — the same shape and the same
//! place as the Reactor sibling's, so a user moving between the two editors keeps their workspace.
//! There is no settings library here on purpose: there are five values, and a serialization
//! framework would be more code than the file it writes.
//!
//! Every failure reads as "nothing remembered": a settings file nobody can parse must not stop the
//! editor from opening, and a fresh launch is a perfectly good state to fall back to.

use std::path::{Path, PathBuf};

use crate::api::{FpsCapMode, PlaybackRules};

/// The folder name under `%LOCALAPPDATA%`.
const APP_DIR: &str = "tas-editor-rs";
const FILE: &str = "settings";

/// What the editor remembers between launches.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub folder: Option<PathBuf>,
    pub game_folder: Option<PathBuf>,
    pub rules: PlaybackRules,
    /// The seed as the field spells it — the documented seeds are hex (`0x55555555`), and the
    /// spelling is what an author recognises.
    pub seed_text: String,
    /// The light theme. A preference, and the only setting that is neither a folder nor a run rule:
    /// the editor ships dark, as the C# sibling does.
    pub light_theme: bool,
}

impl Settings {
    /// Reads the settings file, or answers with the defaults.
    pub fn load() -> Self {
        Self::load_from(file_path().as_deref())
    }

    /// Reads a settings file by path.
    ///
    /// The path is a parameter rather than being read inside for one reason: `load` from
    /// `%LOCALAPPDATA%` is not testable — a test that wrote there would touch the author's real
    /// settings — and the parsing is the part with the decisions in it. `None` is "no file", which is
    /// the fresh launch.
    pub fn load_from(path: Option<&Path>) -> Self {
        let mut settings = Self {
            seed_text: default_seed_text(),
            ..Self::default()
        };

        let Some(path) = path else {
            return settings;
        };

        let Ok(text) = std::fs::read_to_string(path) else {
            return settings;
        };

        // A file written before there were keys — a bare path, no `=` anywhere — is read as the
        // folder rather than as a settings file nobody can parse.
        let mut saw_key = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            saw_key = true;
            let key = key.trim();
            let value = value.trim();

            match key {
                "folder" => settings.folder = non_empty(value).map(PathBuf::from),
                "game_folder" => settings.game_folder = non_empty(value).map(PathBuf::from),
                "fixed_tick" => {
                    if let Some(on) = read_bool(value) {
                        settings.rules.fixed_tick = on;
                    }
                }
                "cap" => {
                    if let Some(cap) = PlaybackRules::read_cap(value) {
                        settings.rules.cap = cap;
                    }
                }
                "custom_fps" => {
                    if let Ok(fps) = value.parse::<u32>() {
                        settings.rules.custom_fps = fps.clamp(
                            PlaybackRules::MIN_FPS,
                            PlaybackRules::MAX_FPS,
                        );
                    }
                }
                "pin_seed" => {
                    if let Some(on) = read_bool(value) {
                        settings.rules.pin_seed = on;
                    }
                }
                "seed" => {
                    if let Some(seed) = PlaybackRules::try_seed(value) {
                        settings.rules.seed = seed;
                    }

                    // The spelling is kept whether or not it parsed: a seed typed as hex is what
                    // the field shows next time, and a value that did not parse is still what the
                    // user wrote.
                    settings.seed_text = value.to_owned();
                }
                "seed_text" => settings.seed_text = value.to_owned(),
                "headless" => {
                    if let Some(on) = read_bool(value) {
                        settings.rules.headless = on;
                    }
                }
                "light_theme" => {
                    if let Some(on) = read_bool(value) {
                        settings.light_theme = on;
                    }
                }
                _ => {}
            }
        }

        if !saw_key {
            let bare = text.trim();
            if !bare.is_empty() {
                settings.folder = Some(PathBuf::from(bare));
            }
        }

        settings
    }

    /// Writes the settings file. A failure to write is not worth a dialog — the editor keeps
    /// working, it just forgets.
    pub fn save(&self) {
        self.save_to(file_path().as_deref());
    }

    /// Writes a settings file by path, for the same reason [`Settings::load_from`] reads one by path.
    pub fn save_to(&self, path: Option<&Path>) {
        let Some(path) = path else {
            return;
        };

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let _ = std::fs::write(path, self.contents());
    }

    /// The file's own text. Split out so it can be asserted on without a disk.
    pub fn contents(&self) -> String {
        let mut lines = Vec::new();

        if let Some(folder) = &self.folder {
            lines.push(format!("folder={}", folder.display()));
        }

        if let Some(game_folder) = &self.game_folder {
            lines.push(format!("game_folder={}", game_folder.display()));
        }

        lines.push(format!("fixed_tick={}", self.rules.fixed_tick));
        lines.push(format!("cap={}", self.rules.cap_spelling()));
        lines.push(format!("custom_fps={}", self.rules.custom_fps));
        lines.push(format!("pin_seed={}", self.rules.pin_seed));
        lines.push(format!("seed={}", self.seed_text));
        lines.push(format!("headless={}", self.rules.headless));
        lines.push(format!("light_theme={}", self.light_theme));

        lines.join("\n") + "\n"
    }

    /// The folder a first launch opens on: the example scripts staged next to the exe, so the pane
    /// is not empty and the `Open folder…` picker has something to point at.
    ///
    /// `None` when no candidate holds a script — the dev loop and a build that skipped staging them
    /// open on no workspace and say so, which is a state the pane already handles.
    ///
    /// ⚠️ A candidate has to hold **at least one `.tas`**, not merely exist: `cargo run` puts a
    /// binary in `target\debug\`, and cargo creates an (empty) `examples\` directory beside every
    /// binary it builds. A plain "is it a directory" test would therefore stop at cargo's own empty
    /// folder and never look at the crate's — which is exactly the folder the examples are in.
    pub fn first_folder() -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Ok(exe) = std::env::current_exe()
            && let Some(directory) = exe.parent()
        {
            candidates.push(directory.join("examples"));
        }

        candidates.push(PathBuf::from("examples"));

        candidates.into_iter().find(|path| holds_scripts(path))
    }
}

/// Whether a folder holds a script — the test that makes [`Settings::first_folder`] skip cargo's
/// empty `examples\`.
fn holds_scripts(folder: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };

    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tas"))
    })
}

/// `%LOCALAPPDATA%\tas-editor-rs\settings`.
fn file_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join(APP_DIR).join(FILE))
}

fn default_seed_text() -> String {
    PlaybackRules::default().seed.to_string()
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn read_bool(value: &str) -> Option<bool> {
    match value.to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// The seed field's own text, kept as typed: `0x55555555` stays hex instead of being rewritten as
/// the decimal a parse would produce.
pub fn seed_text_for(seed: u32) -> String {
    seed.to_string()
}

/// A cap's index in the panel's own order, so a combo box and the value cannot drift apart.
pub fn cap_index(cap: FpsCapMode) -> usize {
    match cap {
        FpsCapMode::Default => 0,
        FpsCapMode::Unlimited => 1,
        FpsCapMode::Custom => 2,
    }
}

/// The inverse of [`cap_index`], with an out-of-range index read as the default rather than
/// panicking a render.
pub fn cap_from_index(index: usize) -> FpsCapMode {
    match index {
        1 => FpsCapMode::Unlimited,
        2 => FpsCapMode::Custom,
        _ => FpsCapMode::Default,
    }
}
