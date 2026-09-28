//! What the editor remembers between launches.
//!
//! A port of the C# sibling's `EditorSettingsTests` (and the settings half of its `WorkspaceTests`).
//! The file format is the contract: the two editors deliberately share one settings file, so a value
//! one writes has to read back in the other — including the spellings a user's existing file already
//! has.
//!
//! ⚠️ Everything here goes through a temp path, never `%LOCALAPPDATA%`: the seam exists for exactly
//! that reason, and a test that wrote the real file would destroy an author's workspace to check a
//! parse.

use std::path::{Path, PathBuf};

use tas_editor_rs::api::{FpsCapMode, PlaybackRules};
use tas_editor_rs::settings::Settings;

/// A folder of its own under the temp directory, removed when the test ends.
struct TempFolder {
    path: PathBuf,
}

impl TempFolder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tas-editor-settings-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the temp folder can be made");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// The settings file inside the folder — the path every test reads and writes through.
    fn file(&self) -> PathBuf {
        self.path.join("settings")
    }

    fn write(&self, text: &str) -> PathBuf {
        let file = self.file();
        std::fs::write(&file, text).expect("the fixture writes");
        file
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn the_folder_and_the_run_rules_survive_a_restart() {
    let folder = TempFolder::new("roundtrip");
    let file = folder.file();

    let rules = PlaybackRules {
        fixed_tick: false,
        cap: FpsCapMode::Custom,
        custom_fps: 144,
        pin_seed: true,
        seed: 0x55555555,
        headless: true,
    };

    let settings = Settings {
        folder: Some(folder.path().to_path_buf()),
        game_folder: None,
        rules,
        seed_text: "0x55555555".to_owned(),
        light_theme: false,
    };
    settings.save_to(Some(&file));

    let loaded = Settings::load_from(Some(&file));

    assert_eq!(loaded.folder.as_deref(), Some(folder.path()));
    assert_eq!(loaded.rules, rules);
    // The spelling travels with the value: the notes spell seeds in hex (`docs/API.md` §3.10), and a
    // round trip through the settings file must not turn `0x55555555` into a decimal nobody
    // recognises.
    assert_eq!(loaded.seed_text, "0x55555555");
}

#[test]
fn nothing_remembered_and_a_file_that_cannot_be_read_answer_the_same_way() {
    let folder = TempFolder::new("absent");
    let file = folder.file();

    // `None` is "no file at all" — a fresh launch.
    assert!(Settings::load_from(None).folder.is_none());

    // An editor that opens on no folder beats one that refuses to start over its own settings.
    assert!(Settings::load_from(Some(&file)).folder.is_none());

    // And a file whose folder line is empty names no folder.
    let written = folder.write("folder=\ndark_theme=false\n");
    assert!(Settings::load_from(Some(&written)).folder.is_none());
}

#[test]
fn a_frame_cap_is_remembered_by_the_panels_own_word() {
    let folder = TempFolder::new("caps");
    let file = folder.file();

    for cap in [FpsCapMode::Default, FpsCapMode::Unlimited, FpsCapMode::Custom] {
        let settings = Settings {
            rules: PlaybackRules {
                cap,
                ..PlaybackRules::default()
            },
            ..Settings::default()
        };
        settings.save_to(Some(&file));

        assert_eq!(Settings::load_from(Some(&file)).rules.cap, cap);
    }
}

#[test]
fn the_mods_own_cap_spellings_stay_readable() {
    // The panel used to write the mod's words before the two vocabularies were separated, and a
    // settings file is not worth an upgrade migration over a word.
    let folder = TempFolder::new("legacy-caps");

    let file = folder.write("cap=off\n");
    assert_eq!(Settings::load_from(Some(&file)).rules.cap, FpsCapMode::Unlimited);

    let file = folder.write("cap=game\n");
    assert_eq!(Settings::load_from(Some(&file)).rules.cap, FpsCapMode::Default);
}

#[test]
fn a_settings_file_from_before_the_run_rules_still_names_its_folder() {
    // The previous format was one line of path and nothing else — no `=`, no keys. Reading it as a
    // settings file nobody can parse would cost the author their workspace after an upgrade.
    let folder = TempFolder::new("bare-path");
    let workspace = folder.path().join("scripts");
    std::fs::create_dir_all(&workspace).expect("the workspace folder can be made");

    let file = folder.write(&workspace.display().to_string());
    let loaded = Settings::load_from(Some(&file));

    assert_eq!(loaded.folder.as_deref(), Some(workspace.as_path()));
    assert_eq!(loaded.rules, PlaybackRules::default());
}

#[test]
fn a_seed_that_does_not_read_falls_back_to_the_default_rather_than_breaking_the_file() {
    let folder = TempFolder::new("bad-seed");
    let file = folder.write("folder=\nfixed_tick=false\nseed=not-a-number\n\n");

    let loaded = Settings::load_from(Some(&file));

    assert!(loaded.folder.is_none());
    assert!(!loaded.rules.fixed_tick, "a `false` is read as written");
    assert_eq!(
        loaded.rules.seed,
        PlaybackRules::default().seed,
        "the default seed stands in"
    );
    // Half a number is still what the field held: it is not the editor's place to rewrite what the
    // author is in the middle of typing.
    assert_eq!(loaded.seed_text, "not-a-number");
}

#[test]
fn an_unknown_key_is_ignored_rather_than_refused() {
    // The two editors share one settings file, so a key this one does not know can only mean the
    // other one wrote it. Refusing the file would lose the folder over somebody else's preference.
    let folder = TempFolder::new("unknown-key");
    let workspace = folder.path().join("scripts");
    std::fs::create_dir_all(&workspace).expect("the workspace folder can be made");

    let file = folder.write(&format!(
        "folder={}\nsomething_the_csharp_editor_wrote=yes\n",
        workspace.display()
    ));

    let loaded = Settings::load_from(Some(&file));
    assert_eq!(loaded.folder.as_deref(), Some(workspace.as_path()));
}
