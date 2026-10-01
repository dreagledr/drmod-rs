//! What the game already has, and what installing would change.
//!
//! The question the pane asks before offering a button, and the answer it paints: a whole-folder
//! presence check by content, so "installed" means *these* bytes rather than a file that happens to
//! have the right name.

use std::path::{Path, PathBuf};

use super::payload::{ASI_NAME, LOADER_NAME, Payload};
use crate::steam;

/// What the game folder holds, by content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModState {
    /// No game folder to look in — discovered or picked. Everything else is unknown until one is.
    NoGameFolder,

    /// The game folder is there and holds no plugin.
    NotInstalled,

    /// The plugin is there and is the one this build carries.
    Installed,

    /// The plugin is there but is a different build (older, newer, or hand-placed).
    OtherVersion,
}

/// What an install did, in a line the pane can paint. Never an error: every caller is a click
/// handler or a render, and the only thing either can do with a failure is show it.
#[derive(Clone, Debug)]
pub struct InstallResult {
    pub ok: bool,
    pub message: String,
}

impl InstallResult {
    pub fn done(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

/// Where the loader goes inside the game folder — DX9 picked up by the game's own `d3d9.dll`
/// import, which is the whole mechanism of an ASI loader.
pub const LOADER_PATH: &str = LOADER_NAME;

/// Where the plugin goes: an ASI loader scans `plugins\` beside itself.
pub const PLUGIN_DIR: &str = "plugins";

/// The upstream loader, for the message about an existing `d3d9.dll` that is not ours.
pub const LOADER_URL: &str = "https://github.com/ThirteenAG/Ultimate-ASI-Loader/releases";

/// The full path of the plugin inside a game folder.
pub fn plugin_path(game_folder: &Path) -> PathBuf {
    game_folder.join(PLUGIN_DIR).join(ASI_NAME)
}

/// What the game folder holds now, by content.
///
/// The payload is what "installed by us" means: same bytes → this build; different bytes →
/// somebody else's, and worth asking about.
pub fn detect(game_folder: Option<&Path>, payload: &Payload) -> ModState {
    let Some(folder) = game_folder.filter(|folder| steam::is_game_folder(Some(folder))) else {
        return ModState::NoGameFolder;
    };

    let plugin = plugin_path(folder);
    if !plugin.is_file() {
        return ModState::NotInstalled;
    }

    if same_bytes(safe_read(&plugin).as_deref(), &payload.asi) {
        ModState::Installed
    } else {
        ModState::OtherVersion
    }
}

/// Whether the game already has a `d3d9.dll` at all — and whether the one it has is ours.
///
/// Two separate questions on purpose: a foreign loader is not a reason to refuse the install (the
/// plugin does not need *our* loader), it is a reason not to touch that file and to say so.
pub fn loader(game_folder: Option<&Path>, payload: &Payload) -> (bool, bool) {
    let Some(folder) = game_folder.filter(|folder| steam::is_game_folder(Some(folder))) else {
        return (false, false);
    };

    let path = folder.join(LOADER_PATH);
    if !path.is_file() {
        return (false, false);
    }

    (true, same_bytes(safe_read(&path).as_deref(), &payload.loader))
}

/// Writes the mod into the game folder.
///
/// The order is deliberate — the loader first, the plugin last. A half-finished install must not
/// leave a plugin whose loader is missing (the game would silently load nothing), and the plugin is
/// the file whose presence [`detect`] reads, so writing it last means an interrupted install reads
/// as "not installed" rather than as "installed and broken".
///
/// ⚠️ Both writes go through a temporary file beside the target and a move over it. A direct write
/// onto the plugin can be interrupted by a full disk or a crash mid-write and leave a truncated DLL
/// that the game fails to load — the one failure that looks like a broken mod rather than like a
/// failed install.
pub fn install(game_folder: Option<&Path>, payload: &Payload) -> InstallResult {
    let Some(folder) = game_folder.filter(|folder| steam::is_game_folder(Some(folder))) else {
        return InstallResult::failed(format!(
            "No game folder — point the editor at the folder holding {}.",
            steam::GAME_EXE_NAME
        ));
    };

    let mut notes: Vec<String> = Vec::new();
    let (loader_present, loader_ours) = loader(Some(folder), payload);

    if !loader_present {
        if let Some(error) = write(&folder.join(LOADER_PATH), &payload.loader) {
            return InstallResult::failed(error);
        }

        notes.push(format!("added {LOADER_PATH}"));
    } else if !loader_ours {
        // Not ours, and not ours to replace: almost certainly ReShade, an ENB, or another mod's
        // loader. Any ASI loader loads this plugin, so the install is complete without ours.
        notes.push(format!(
            "kept the existing {LOADER_PATH} — this plugin works with any ASI loader"
        ));
    }

    let plugin_dir = folder.join(PLUGIN_DIR);
    if let Err(error) = std::fs::create_dir_all(&plugin_dir) {
        return InstallResult::failed(format!("Cannot create {}: {error}", plugin_dir.display()));
    }

    if let Some(error) = write(&plugin_path(folder), &payload.asi) {
        return InstallResult::failed(error);
    }

    notes.insert(0, format!("installed {PLUGIN_DIR}\\{ASI_NAME}"));
    let mut message = format!(
        "{}. Restart the game for the mod to load.",
        notes.join("; ")
    );

    if loader_present && !loader_ours {
        message.push_str(&format!(
            " No ASI loader of its own? Take the Win32 d3d9.dll from {LOADER_URL}"
        ));
    }

    InstallResult::done(message)
}

/// Whether there is anything of ours to take out: the plugin, or a loader that is ours.
///
/// The caller uses this to decide whether the uninstall is a live action — a button that can only
/// report "nothing to remove" is a click that should not have been offered.
pub fn can_remove(game_folder: Option<&Path>, payload: &Payload) -> bool {
    matches!(
        detect(game_folder, payload),
        ModState::Installed | ModState::OtherVersion
    ) || loader(game_folder, payload).1
}

/// Takes the mod out of the game folder.
///
/// The mirror of [`install`], and the same rule about somebody else's file: the plugin is ours and
/// always goes, but a `d3d9.dll` is only removed when it is byte-for-byte the one we would have
/// written.
///
/// ⚠️ The plugin goes **first**, the loader second, which is the reverse of the install order and
/// deliberate for the same reason: if the removal is interrupted, what is left is a loader with no
/// plugin (harmless) rather than a plugin the game still tries to load.
pub fn remove(game_folder: Option<&Path>, payload: &Payload) -> InstallResult {
    let Some(folder) = game_folder.filter(|folder| steam::is_game_folder(Some(folder))) else {
        return InstallResult::failed(format!(
            "No game folder — point the editor at the folder holding {}.",
            steam::GAME_EXE_NAME
        ));
    };

    let mut notes: Vec<String> = Vec::new();

    let plugin = plugin_path(folder);
    if plugin.is_file() {
        if let Some(error) = delete(&plugin) {
            return InstallResult::failed(error);
        }

        notes.push(format!("removed {PLUGIN_DIR}\\{ASI_NAME}"));
    }

    let (loader_present, loader_ours) = loader(Some(folder), payload);
    if loader_present && loader_ours {
        if let Some(error) = delete(&folder.join(LOADER_PATH)) {
            return InstallResult::failed(error);
        }

        notes.push(format!("removed {LOADER_PATH}"));
    } else if loader_present {
        notes.push(format!(
            "kept {LOADER_PATH} — it is not this build's, and another mod may need it"
        ));
    }

    if notes.is_empty() {
        return InstallResult::done("Nothing to remove — the mod was not installed.");
    }

    InstallResult::done(format!(
        "{}. The game no longer loads the mod on its next start.",
        notes.join("; ")
    ))
}

/// One file, written through a temporary beside it.
///
/// The temporary shares the target's directory on purpose: a move across volumes is a copy, and
/// would lose the atomicity this is for.
fn write(path: &Path, bytes: &[u8]) -> Option<String> {
    let temporary = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .map(|extension| format!("{}.", extension.to_string_lossy()))
            .unwrap_or_default()
    ));

    if let Err(error) = std::fs::write(&temporary, bytes) {
        return Some(format!("Cannot write {}: {error}", path.display()));
    }

    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Some(format!("Cannot write {}: {error}", path.display()));
    }

    None
}

fn delete(path: &Path) -> Option<String> {
    std::fs::remove_file(path)
        .err()
        .map(|error| format!("Cannot delete {}: {error}", path.display()))
}

/// Unreadable reads as "not ours", which is the safe side: the install will overwrite it only after
/// the user has been asked.
fn safe_read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn same_bytes(left: Option<&[u8]>, right: &[u8]) -> bool {
    match left {
        Some(bytes) => bytes == right,
        None => false,
    }
}
