//! Reading Steam's library list.
//!
//! The parser is the one part of the install path that reads somebody else's file format, and it has
//! to survive the format's age: `libraryfolders.vdf` has been a list of paths and is now a map of
//! objects, and both spellings are still in the wild. The paths themselves are the interesting part —
//! VDF escapes a backslash, and Windows paths are nothing but backslashes.
//!
//! A port of the C# sibling's `SteamLibraryTests.cs`, assertion for assertion.
//!
//! ⚠️ `parse_library_paths` filters by "is it a directory" — a library on an unplugged drive is
//! dropped — so a test asserting on a path has to use one that is really there. These use the temp
//! directory, which is, rather than a plausible-looking `D:\Games` a build machine may not have.

use std::path::{Path, PathBuf};

use drmod_tas_editor::steam;

/// The temp directory, without its trailing separator — the shape a VDF path is written in.
fn temp() -> String {
    std::env::temp_dir()
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_owned()
}

/// A path as VDF spells it: every backslash doubled.
fn escaped(path: &str) -> String {
    path.replace('\\', "\\\\")
}

/// The temp directory with a trailing separator — for the unescape test, where one more separator
/// has to survive the escape/unescape round trip.
fn temp_with_separator() -> String {
    format!("{}\\\\", temp())
}

#[test]
fn the_modern_object_shape_names_its_libraries() {
    let library = temp();
    let vdf = format!(
        r#""libraryfolders"
{{
    "0"
    {{
        "path"		"C:\\Program Files (x86)\\Steam"
        "label"		""
        "apps"
        {{
            "235460"		"123"
        }}
    }}
    "1"
    {{
        "path"		"{}"
        "label"		""
        "apps"
        {{
            "235460"		"456"
        }}
    }}
}}"#,
        escaped(&library)
    );

    assert!(steam::parse_library_paths(&vdf).contains(&PathBuf::from(&library)));
}

#[test]
fn the_older_list_shape_still_names_its_libraries() {
    // The format before the objects: a numbered key and a bare path. A file that old is a file a user
    // still has if Steam has not rewritten it — and one Steam no longer writes.
    let library = temp();
    let vdf = format!(
        r#""LibraryFolders"
{{
    "TimeNextStatsReport"	"1234567890"
    "ContentStatsID"		"-1234567890123456789"
    "1"		"{}"
}}"#,
        escaped(&library)
    );

    assert!(steam::parse_library_paths(&vdf).contains(&PathBuf::from(&library)));
}

#[test]
fn timestamps_and_ids_are_not_mistaken_for_paths() {
    // This is the trap the two patterns exist for: `"TimeNextStatsReport" "1234567890"` is a
    // `"key" "value"` pair like any other, and the old list format's keys are *numbers*. Only a
    // numeric key counts, and a numeric value under a named key must not.
    let vdf = r#""LibraryFolders"
{
    "TimeNextStatsReport"	"1234567890"
    "ContentStatsID"		"1"
}"#;

    assert!(steam::parse_library_paths(vdf).is_empty());
}

#[test]
fn a_library_that_is_not_there_is_dropped() {
    // A path in the file is not a folder on the disk: a drive that is unplugged, a library that was
    // deleted, a path edited by hand. Handing one on would make the search look in a folder that
    // cannot hold the game and then report "not installed".
    let vdf = r#""libraryfolders"
{
    "0"
    {
        "path"		"Z:\\nowhere\\at\\all"
    }
}"#;

    assert!(steam::parse_library_paths(vdf).is_empty());
}

#[test]
fn a_path_is_unescaped_on_the_way_out() {
    // VDF writes a backslash as `\\`. Left that way the path is a folder that does not exist — and
    // would be silently dropped rather than reported, which is the failure that hides.
    let wanted = format!("{}\\", temp());
    let vdf = format!(
        "\"libraryfolders\"\n{{\n    \"0\"\n    {{\n        \"path\"\t\t\"{}\"\n    }}\n}}\n",
        escaped(&temp_with_separator())
    );

    assert!(
        steam::parse_library_paths(&vdf).contains(&PathBuf::from(&wanted)),
        "the doubled separators have to come back as one"
    );
}

#[test]
fn one_library_named_twice_is_kept_once() {
    let library = temp();
    let vdf = format!(
        r#""libraryfolders"
{{
    "0"
    {{
        "path"		"{}"
    }}
    "1"
    {{
        "path"		"{}"
    }}
}}"#,
        escaped(&library),
        escaped(&library)
    );

    assert_eq!(steam::parse_library_paths(&vdf).len(), 1);
}

#[test]
fn garbage_is_no_libraries_rather_than_a_crash() {
    // The file is on somebody else's disk, in somebody else's format, and may be truncated halfway
    // through a write. An answer of "no extra libraries" costs the search one library; a panic costs
    // the window.
    assert!(steam::parse_library_paths("").is_empty());
    assert!(steam::parse_library_paths("\0\0\0 not a vdf at all").is_empty());
    assert!(steam::parse_library_paths("\"libraryfolders\" { \"0\" { \"path\"").is_empty());
}

/// A folder of its own under the temp directory, removed when the test ends.
struct TempFolder {
    path: PathBuf,
}

impl TempFolder {
    fn new() -> Self {
        // The same shape the C# sibling uses: the process id stands in for a GUID, which is enough
        // because the name only has to be unique among the tests running at once.
        let path = std::env::temp_dir().join(format!("drmod-tas-editor-game-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the temp folder can be made");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn a_folder_counts_as_the_game_only_with_its_exe() {
    let folder = TempFolder::new();

    // The folder name is the game's, but nothing is in it: a library entry can outlive the
    // installation, and the name is not evidence.
    assert!(!steam::is_game_folder(Some(folder.path())));
    assert!(!steam::is_game_folder(None));

    std::fs::write(folder.path().join(steam::GAME_EXE_NAME), "stub").expect("the stub exe writes");
    assert!(steam::is_game_folder(Some(folder.path())));
}
