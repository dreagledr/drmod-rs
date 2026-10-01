//! Paths, sizes and argument parsing — the small things every command needs.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The repository root: this crate's parent directory.
///
/// `xtask/` sits at the repository root by the convention, so `CARGO_MANIFEST_DIR` is `<root>/xtask`
/// and the root is one level up. The check is that the root actually looks like the workspace (it
/// holds the manifest that lists the members) — a moved crate should say so instead of writing
/// artifacts into `<repo>/..`.
pub fn workspace_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.to_path_buf();

    let manifest = root.join("Cargo.toml");
    let contents = std::fs::read_to_string(&manifest).ok()?;
    if !contents.contains("[workspace]") {
        return None;
    }

    Some(root)
}

/// A byte count as a human would write it, for the artifact summary lines.
pub fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const KB: f64 = 1024.0;

    let value = bytes as f64;
    if value >= MB {
        format!("{:.1} MB", value / MB)
    } else {
        format!("{:.0} KB", value / KB)
    }
}

/// A command's tail of arguments, walked by hand.
///
/// Not a clap dependency: there are five commands and their options are a handful of flags, which
/// does not pay for a dependency that every release build would then carry.
pub struct Args<'a> {
    args: &'a [String],
    position: usize,
}

impl<'a> Args<'a> {
    pub fn new(args: &'a [String]) -> Self {
        Self { args, position: 0 }
    }

    /// The next argument, if there is one.
    pub fn next(&mut self) -> Option<&'a str> {
        let arg = self.args.get(self.position)?;
        self.position += 1;
        Some(arg.as_str())
    }

    /// The value that belongs to `flag`, consuming it — or the error saying `flag` needs one.
    ///
    /// The error is what a caller gets for `cargo xtask pack-editor --out-dir` with nothing after it:
    /// a command that silently ignores a flag with a missing value writes artifacts somewhere
    /// unexpected instead of stopping.
    pub fn value(&mut self, flag: &str) -> Result<String> {
        self.next()
            .map(str::to_owned)
            .ok_or_else(|| Error::new(format!("`{flag}` needs a value, e.g. `{flag} <path>`")))
    }
}

/// Reads a whole file, naming it in the error.
pub fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))
}

/// Checks that a file exists, or fails with the hint that says how to produce it.
///
/// The `hint` is the part that matters: "does not exist" is only useful next to the command that
/// creates it, and each artifact here has exactly one.
pub fn require_file(path: &Path, hint: &str) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }

    Err(Error::new(format!(
        "not found: {}\n       {hint}",
        path.display()
    )))
}

/// Checks that a directory exists, or fails with the hint that says what is missing without it.
///
/// Separate from `require_file` because a directory is never a file: asking `is_file` about a tree
/// reports "not found" for something that is plainly there, which is a worse error than none.
pub fn require_dir(path: &Path, hint: &str) -> Result<()> {
    if path.is_dir() {
        return Ok(());
    }

    Err(Error::new(format!(
        "not found: {}\n       {hint}",
        path.display()
    )))
}

/// Empties a directory, creating it if it is not there.
///
/// The staging directories are recreated from scratch on every run, so a file that is no longer
/// produced cannot survive inside an artifact from a previous build.
pub fn clean_dir(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)
            .map_err(|error| Error::new(format!("cannot clear {}: {error}", path.display())))?;
    }
    std::fs::create_dir_all(path)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", path.display())))
}

/// Copies a file, naming both ends in the error.
pub fn copy(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::new(format!("cannot create {}: {error}", parent.display())))?;
    }
    std::fs::copy(from, to).map_err(|error| {
        Error::new(format!(
            "cannot copy {} to {}: {error}",
            from.display(),
            to.display()
        ))
    })?;
    Ok(())
}

/// Copies a directory tree.
pub fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", to.display())))?;

    let entries = std::fs::read_dir(from)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", from.display())))?;

    for entry in entries {
        let entry = entry
            .map_err(|error| Error::new(format!("cannot read {}: {error}", from.display())))?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|error| {
            Error::new(format!("cannot stat {}: {error}", entry.path().display()))
        })?;

        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            copy(&entry.path(), &target)?;
        }
    }

    Ok(())
}

/// Every file under `dir`, sorted — the input to a zip.
pub fn files_recursive(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    collect(dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn collect(dir: &Path, into: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", dir.display())))?;

    for entry in entries {
        let entry = entry
            .map_err(|error| Error::new(format!("cannot read {}: {error}", dir.display())))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| Error::new(format!("cannot stat {}: {error}", path.display())))?;

        if kind.is_dir() {
            collect(&path, into)?;
        } else {
            into.push(path);
        }
    }

    Ok(())
}
