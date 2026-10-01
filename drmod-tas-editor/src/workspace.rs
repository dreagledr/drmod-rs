//! The on-disk workspace: the `.tas` files of one folder, and the file operations the script list
//! offers over them.
//!
//! Nothing here throws at its caller. Every call comes either from a click handler or from a
//! render, and the only thing either can do with an error is paint it — so a folder that is gone,
//! a file that cannot be read or written, comes back as a message instead.
//!
//! Only `.tas` is a script, and only at the folder's top level: the format has three
//! representations and the text is the one that lives on disk here.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::script::{ScriptTextStatus, dsl};

/// A script text is a `.tas` file — the DSL's own extension, so the two cannot drift apart.
pub const EXTENSION: &str = dsl::EXTENSION;

/// What a new file is called when there is nothing in the folder to name it after, and what a
/// copy gets appended to the name it copies.
const NEW_STEM: &str = "script";
const COPY_SUFFIX: &str = "-copy";

/// A script as the workspace sees it: one `.tas` file of the open folder.
///
/// `path` is the identity — the list rows, the selection and the text buffers key on it —
/// because a file is the one thing here with a stable, equatable name: the text format's own
/// `name` may repeat across files and is edited freely.
///
/// `text` is what the file held when the folder was listed, and a save writes the buffer back
/// over it. `frames` is the last frame that text touches, and `error` the reason it does not read
/// as a script at all — I/O or the parser's own refusal. A file the mod would turn down is listed
/// with its error rather than dropped, so a typo shows up in the row instead of a file that
/// silently is not there.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptEntry {
    pub path: PathBuf,
    pub name: String,
    pub text: String,
    pub frames: u32,
    pub error: Option<String>,
}

impl ScriptEntry {
    /// Whether the text reads as a script the mod would take — a row with an error is still a
    /// file the editor opens and saves, it just is not one the game would run.
    pub fn reads(&self) -> bool {
        self.error.is_none()
    }
}

/// The scripts of a folder, and why the folder itself could not be read.
#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub scripts: Vec<ScriptEntry>,
    pub error: Option<String>,
}

/// The folder's scripts by name, each one read and parsed.
///
/// Read on listing rather than on selection because a row has to show a frame count, and that
/// count only exists once the text has been parsed. A workspace is a handful of files, so the cost
/// is one pass over them per listing — a listing that is memoized, not one per frame.
pub fn list(folder: Option<&Path>) -> Listing {
    let Some(folder) = folder else {
        return Listing::default();
    };

    if !folder.is_dir() {
        return Listing {
            scripts: Vec::new(),
            error: Some(format!("The workspace folder is gone: {}", folder.display())),
        };
    }

    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) => {
            return Listing {
                scripts: Vec::new(),
                error: Some(format!("Cannot read {}: {error}", folder.display())),
            };
        }
    };

    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if is_script(&path) {
            scripts.push(read(&path));
        }
    }

    scripts.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
    });

    Listing {
        scripts,
        error: None,
    }
}

/// A new, empty script in the folder.
///
/// Empty on purpose: the file name is not the script's name (the text never renames the file), so
/// there is nothing to write into it that the author would not immediately have to take back.
pub fn create(folder: &Path) -> (Option<PathBuf>, Option<String>) {
    let path = unique(folder, NEW_STEM);
    match write(&path, "") {
        None => (Some(path), None),
        Some(error) => (None, Some(error)),
    }
}

/// A copy of a script, beside it, under `<name>-copy`. What is copied is the file, not the
/// document: the two are the same thing here, and the copy is a place to start editing from.
pub fn duplicate(path: &Path) -> (Option<PathBuf>, Option<String>) {
    let (Some(folder), Some(stem)) = (path.parent(), stem_of(path)) else {
        return (
            None,
            Some(format!("Cannot tell where {} lives", path.display())),
        );
    };

    let target = unique(folder, &format!("{stem}{COPY_SUFFIX}"));
    match std::fs::copy(path, &target) {
        Ok(_) => (Some(target), None),
        Err(error) => (
            None,
            Some(format!("Cannot copy {}: {error}", path.display())),
        ),
    }
}

/// Renames a script's file inside its own folder: `<stem>.tas` → `<new name>.tas`, the file itself
/// moved and nothing else touched.
///
/// The name is the *file's*, not the script's — the `name=` a rules line carries is a different
/// thing and is edited in the text.
///
/// A name that is already taken is refused with a message rather than suffixed: **New** and
/// **Duplicate** make up a free name because they were asked for "a new file", but a name that
/// was typed is a name the author wants, and `-2` would hand them a file they did not ask for.
pub fn rename(path: &Path, name: &str) -> (Option<PathBuf>, Option<String>) {
    let stem = name.trim();

    if stem.is_empty() {
        return (None, Some("A script needs a file name".to_owned()));
    }

    if stem.to_lowercase().ends_with(EXTENSION) {
        return (
            None,
            Some(format!(
                "Leave {EXTENSION} out of the name — the workspace adds it"
            )),
        );
    }

    if let Some(invalid) = stem.chars().find(|c| is_invalid_file_char(*c)) {
        return (
            None,
            Some(format!("A file name cannot hold '{invalid}'")),
        );
    }

    let Some(folder) = path.parent() else {
        return (
            None,
            Some(format!("Cannot tell where {} lives", path.display())),
        );
    };

    let target = folder.join(format!("{stem}{EXTENSION}"));
    if target.to_string_lossy().to_lowercase() == path.to_string_lossy().to_lowercase() {
        // The same name, however it was spelled: nothing to do, and nothing to complain about.
        return (Some(path.to_path_buf()), None);
    }

    if target.exists() {
        return (
            None,
            Some(format!("{stem}{EXTENSION} is already in this folder")),
        );
    }

    match std::fs::rename(path, &target) {
        Ok(()) => (Some(target), None),
        Err(error) => (
            None,
            Some(format!("Cannot rename {}: {error}", path.display())),
        ),
    }
}

/// Overwrites a script's file with a text.
///
/// The whole file, because the whole file is the script: there is no part of it the editor holds
/// somewhere else. The text is written in the format's own separator (`\n`) whatever the caller's
/// text uses — a `.tas` that came from elsewhere may carry `\r\n` or a lone `\r` between its lines
/// ([`dsl::lines`]), and a file written through here therefore still reads as the script it was.
pub fn write(path: &Path, text: &str) -> Option<String> {
    let normalised = dsl::lines(text);
    match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .and_then(|mut file| file.write_all(normalised.as_bytes()))
    {
        Ok(()) => None,
        Err(error) => Some(format!("Cannot write {}: {error}", path.display())),
    }
}

pub fn delete(path: &Path) -> Option<String> {
    match std::fs::remove_file(path) {
        Ok(()) => None,
        Err(error) => Some(format!("Cannot delete {}: {error}", path.display())),
    }
}

/// A free `<stem>.tas` in the folder: `stem-2`, `stem-3`, … — a file that is already there is
/// never overwritten silently.
fn unique(folder: &Path, stem: &str) -> PathBuf {
    let mut candidate = folder.join(format!("{stem}{EXTENSION}"));
    let mut counter = 2;
    while candidate.exists() {
        candidate = folder.join(format!("{stem}-{counter}{EXTENSION}"));
        counter += 1;
    }

    candidate
}

fn is_script(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.to_string_lossy().to_lowercase() == EXTENSION.trim_start_matches('.')
    })
}

fn stem_of(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// One file as an entry. Both the read and the parse are wrapped, not just the parse: the listing
/// is built during a render, and a render that fails takes the window with it — a file nobody can
/// read is a row that says so.
///
/// The text is kept as the format reads it ([`dsl::lines`]): a file whose lines are separated by
/// something other than `\n` would otherwise be listed as one broken line, while the editor —
/// which normalises before parsing — shows it as the script it is. Two answers for the same file
/// is the one thing this must not do.
fn read(path: &Path) -> ScriptEntry {
    let name = stem_of(path).unwrap_or_else(|| path.display().to_string());

    match std::fs::read(path) {
        Ok(bytes) => {
            // Not `read_to_string`: a script with a stray non-UTF-8 byte is still worth listing,
            // and the lossy form keeps the row readable instead of throwing the file away.
            let text = dsl::lines(&String::from_utf8_lossy(&bytes));
            let status = ScriptTextStatus::of(&text);
            match status.document {
                Some(document) => ScriptEntry {
                    path: path.to_path_buf(),
                    name,
                    frames: ScriptTextStatus::last_frame(&document),
                    text,
                    error: None,
                },
                None => ScriptEntry {
                    path: path.to_path_buf(),
                    name,
                    text,
                    frames: 0,
                    error: status.error,
                },
            }
        }
        Err(error) => ScriptEntry {
            path: path.to_path_buf(),
            name,
            text: String::new(),
            frames: 0,
            error: Some(format!("Cannot read: {error}")),
        },
    }
}

/// The characters Windows refuses in a file name.
fn is_invalid_file_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 0x20
}
