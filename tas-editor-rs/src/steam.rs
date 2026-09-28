//! Where the game is installed, found the way Steam records it.
//!
//! A Steam library path is not guessable: `steamapps\common` under the Steam root is only the
//! default, and anyone with a second drive has the game somewhere else entirely. So the search
//! follows Steam's own bookkeeping instead of a hard-coded path —
//! `HKCU\Software\Valve\Steam\SteamPath` names the installation, and each
//! `steamapps\libraryfolders.vdf` names the libraries it knows about (the file has been both a
//! list of paths and a map of objects over the years, so both spellings are read).
//!
//! Nothing here fails at its caller: a missing key, an unreadable registry hive and a vdf nobody
//! can parse all end the same way — an empty answer, and the pane offers the folder picker
//! instead.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_SUCCESS, MAX_PATH};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW, REG_SZ,
};
use windows::core::w;

/// The game's folder name inside a library's `steamapps\common`. Both the Steam folder and the
/// process's own name differ in case from this one, which is why every comparison is
/// case-insensitive.
pub const GAME_FOLDER_NAME: &str = "METAL GEAR RISING REVENGEANCE";

/// The executable that decides whether a candidate folder really is the game.
pub const GAME_EXE_NAME: &str = "METAL GEAR RISING REVENGEANCE.exe";

/// Every library Steam knows about, the installation root first. Empty when Steam cannot be found
/// at all — the caller treats that as "no game folder", not as a failure.
pub fn libraries() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let Some(steam) = steam_root() else {
        return roots;
    };

    roots.push(steam.clone());

    let vdf = steam.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&vdf) {
        for path in parse_library_paths(&text) {
            // The installation root is already in the list, and one library can be named twice
            // once a second account's config is involved.
            if !contains(&roots, &path) {
                roots.push(path);
            }
        }
    }

    roots
}

/// The folder the game is installed in, or `None`.
pub fn game_folder() -> Option<PathBuf> {
    libraries()
        .into_iter()
        .map(|library| library.join("steamapps").join("common").join(GAME_FOLDER_NAME))
        .find(|candidate| is_game_folder(Some(candidate.as_path())))
}

/// Whether a folder really holds the game.
///
/// The exe is the test rather than the folder name: a user-chosen folder is treated exactly like a
/// discovered one, and an empty `common` entry from a vdf must not pass for an installation.
pub fn is_game_folder(folder: Option<&Path>) -> bool {
    folder.is_some_and(|folder| folder.join(GAME_EXE_NAME).is_file())
}

/// The Steam installation root, from the per-user registry key Steam writes on every run.
pub fn steam_root() -> Option<PathBuf> {
    let path = read_registry_string(HKEY_CURRENT_USER, w!("Software\\Valve\\Steam"), w!("SteamPath"))?;

    // Steam writes forward slashes; Windows accepts them, and `Path` reads the result fine.
    let root = PathBuf::from(path);
    root.is_dir().then_some(root)
}

/// One `REG_SZ` value out of the registry, or `None` for any of the ways that can fail.
fn read_registry_string(root: HKEY, subkey: windows::core::PCWSTR, name: windows::core::PCWSTR) -> Option<String> {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(root, subkey, None, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }

        let mut buffer = [0u16; MAX_PATH as usize];
        let mut size = (buffer.len() * 2) as u32;
        let mut kind = REG_SZ;
        let status = RegQueryValueExW(
            key,
            name,
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        if status != ERROR_SUCCESS {
            return None;
        }

        let length = (size as usize / 2).saturating_sub(1);
        String::from_utf16(&buffer[..length]).ok()
    }
}

/// The library paths inside a `libraryfolders.vdf`.
///
/// The file's shape has changed twice (`"1" "D:\\Games"` once, `"1" { "path" "D:\\Games" }` since),
/// so both are read rather than betting on one.
///
/// ⚠️ **Each shape is searched for by its own key, and that is not a style choice.** A VDF nests: a
/// key that opens an object sits on a line of its own, so the strings of the file do **not** alternate
/// key/value — walking them two at a time pairs `"LibraryFolders"` with `"TimeNextStatsReport"` and
/// offsets everything after it. It survives a modern file by luck (whose first string is the `"path"`
/// the modern branch spots by name) and fails the old list shape, whose first strings are a timestamp
/// and an id (found by porting the C# sibling's tests — one of them fails on exactly that shape).
///
/// The C# sibling encodes the same rule as two regexes, `"path"\s+"..."` and `"\d+"\s+"..."`: an entry
/// is recognised by **the key that names it**, never by where it sits among the strings.
pub fn parse_library_paths(vdf: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();

    let mut cursor = 0usize;
    while let Some(key) = read_string(vdf, cursor) {
        cursor = key.end;

        // The value is the next string, if one follows this key across nothing but whitespace.
        let Some(value) = read_string(vdf, skip_whitespace(vdf, cursor)) else {
            continue;
        };

        // Only if it really is the next thing: a key that opens an object (`"0" {`) has no value, and
        // taking the *next* key as one would invent entries out of the nesting.
        if !is_whitespace_only(vdf, cursor, value_span_start(vdf, cursor)) {
            continue;
        }

        let modern = key.text.eq_ignore_ascii_case("path");

        // The older shape: `"1" "D:\\Games"` — the key is a number, which is what separates a library
        // from every other `"key" "value"` pair in the file. A *named* key with a numeric value
        // (`"TimeNextStatsReport" "1234567890"`) is not one of them, which is why the test is on the
        // key and never on the value.
        let numbered = !key.text.is_empty() && key.text.chars().all(|c| c.is_ascii_digit());

        if modern || numbered {
            add(&mut found, unescape(&value.text));
            cursor = value.end;
        }
    }

    found
}

/// One quoted string as the file spells it: its contents, and where it ended.
struct Str {
    text: String,
    end: usize,
}

/// Where the next string starts at or after `from`, without reading it.
///
/// Used only to check that nothing but whitespace sits between a key and its value.
fn value_span_start(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut open = from;
    while open < bytes.len() && bytes[open] != b'"' {
        open += 1;
    }

    open
}

/// The first index at or after `from` that is not a space or a tab.
fn skip_whitespace(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut cursor = from;

    while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
        cursor += 1;
    }

    cursor
}

/// Whether everything between `from` and `to` is spaces and tabs.
///
/// A newline counts as a separator here only if it is the *only* thing crossing; a `{` or `}` does
/// not, and that is the point — it is what tells a `"key" "value"` pair from a `"key" {` object.
fn is_whitespace_only(text: &str, from: usize, to: usize) -> bool {
    text[from..to]
        .chars()
        .all(|c| c == ' ' || c == '\t' || c == '\r' || c == '\n')
}

/// The next quoted string at or after `from`, with the escapes left as written.
///
/// The contents come back exactly as the file wrote them, escapes included: `unescape` is what
/// removes them, and it has to see the doubled separators to know which ones were escapes.
fn read_string(text: &str, from: usize) -> Option<Str> {
    let bytes = text.as_bytes();

    // Skip to the opening quote.
    let mut open = from;
    while open < bytes.len() && bytes[open] != b'"' {
        open += 1;
    }

    if open >= bytes.len() {
        return None;
    }

    let mut cursor = open + 1;
    let mut content = String::new();

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => {
                // Keep the backslash and whatever follows it; `unescape` decides what it meant.
                content.push('\\');
                if let Some(next) = text[cursor + 1..].chars().next() {
                    content.push(next);
                    cursor += 1 + next.len_utf8();
                } else {
                    cursor += 1;
                }
            }
            b'"' => {
                return Some(Str {
                    text: content,
                    end: cursor + 1,
                });
            }
            _ => {
                if let Some(c) = text[cursor..].chars().next() {
                    content.push(c);
                    cursor += c.len_utf8();
                } else {
                    break;
                }
            }
        }
    }

    // An unterminated string: the file was truncated mid-write. Nothing after it can be trusted, so
    // the scan is over.
    None
}

fn add(found: &mut Vec<PathBuf>, path: String) {
    if path.is_empty() {
        return;
    }

    let candidate = PathBuf::from(&path);
    if candidate.is_dir() && !contains(found, &candidate) {
        found.push(candidate);
    }
}

fn contains(paths: &[PathBuf], path: &Path) -> bool {
    paths.iter().any(|known| same_path(known, path))
}

/// Whether two paths name the same folder. Windows paths are case-insensitive, and Steam's own
/// records are written with whatever case happened to be on the drive.
pub fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
}

/// VDF escapes a backslash, and Windows paths are nothing but backslashes.
fn unescape(value: &str) -> String {
    value
        .replace("\\\\", "\\")
        .replace("\\\"", "\"")
        .replace("\\t", "\t")
        .trim()
        .to_owned()
}

/// Every drive letter a `libraryfolders.vdf` could name, for the diagnostic line.
#[allow(dead_code)]
pub fn library_drives() -> BTreeSet<char> {
    libraries()
        .iter()
        .filter_map(|path| path.to_string_lossy().chars().next())
        .collect()
}
