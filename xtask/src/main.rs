//! Build automation for drmod-rs.
//!
//! This is the project's [cargo-xtask]: a crate that exists only to hold the commands that used to
//! be PowerShell scripts (`build.ps1`, `build_tools.ps1`, `drmod-tas-editor/pack.ps1`,
//! `test_api.ps1`, `test_connect.ps1`). Run it through the alias in the root `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask build          # the launcher exe + the ASI zip
//! cargo xtask build-tools    # dbdump.exe and dump-replay-input.exe -> out/
//! cargo xtask pack-editor    # the TAS editor distribution (and its zip with --zip)
//! cargo xtask test-api       # the mod's HTTP API smoke test (game running, mod injected)
//! cargo xtask test-connect   # the multiplayer server smoke test
//! ```
//!
//! # Why this is a Rust crate and not a script
//!
//! The scripts it replaces had to know things the build already knows — which triple each crate
//! builds for, where cargo puts the artifacts, that the x64 crates must not be built with the root's
//! i686 config, that the mod must be built before the editor embeds it. Every one of those was a
//! comment in a `.ps1` that nothing checked. Here the paths are computed from the workspace layout
//! and the triples come from the build, so a wrong assumption is a compile error or a missing file
//! rather than a release artifact built from last month's DLL.
//!
//! ⚠️ The crate is not a member of the root workspace (it carries its own `[workspace]`), so a plain
//! `cargo build --release` at the root never builds it and the mod's build is untouched.
//!
//! ⚠️ **The x64 crates are built from here with an explicit `--target x86_64-pc-windows-msvc`.**
//! Cargo merges `.cargo/config.toml` by the *working directory's* ancestors, not the manifest path,
//! so building `drmod-dbdump` from the repository root would pick up the root config and try to
//! compile arrow-rs for i686 — which does not exist. Passing the triple explicitly overrides that
//! `[build] target` without the `pushd` the PowerShell script needed.

mod editor;
mod package;
mod process;
mod test_api;
mod test_connect;
mod util;

use std::process::ExitCode;

/// What the tool prints for `cargo xtask` and `cargo xtask help`.
const USAGE: &str = "\
drmod-rs build automation

Usage: cargo xtask <command> [options]

Commands:
  build                Build the mod and package it: out/drmod-rs.zip and out/drmod-asi.zip
  build-tools          Build the x64/32-bit tools and copy them into out/
  pack-editor          Assemble the TAS editor distribution (add --zip to archive it)
  test-api             Smoke-test the mod's HTTP API (game running with the mod injected)
  test-connect         Smoke-test the multiplayer server (dashboard, TCP connect/disconnect)

Options:
  -h, --help           Show this message (also: cargo xtask help)

Run one of the commands to see its own options.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let Some(command) = args.first() else {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    };

    // The workspace root is this crate's parent: `xtask/` sits at the repository root, the way the
    // convention describes, so every path below is derived from `<root>/xtask/..` rather than from
    // the caller's current directory.
    let Some(root) = util::workspace_root() else {
        eprintln!("xtask: cannot find the repository root from {}", env!("CARGO_MANIFEST_DIR"));
        return ExitCode::FAILURE;
    };

    let rest = &args[1..];
    let result = match command.as_str() {
        "build" => package::build(root),
        "build-tools" => package::build_tools(root),
        "pack-editor" => editor::pack(root, rest),
        "test-api" => test_api::run(root, rest),
        "test-connect" => test_connect::run(root, rest),
        "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => {
            eprintln!("xtask: unknown command `{other}`\n");
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("\nxtask: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A command's failure, with the message the user needs to act on it.
///
/// Steps that are worth retrying or checking by hand carry their own words; this is the shape that
/// lets `?` carry them out of the command.
pub type Result<T> = std::result::Result<T, Error>;

/// Something a command could not do, and why — printed by `main` and nothing else.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// The repository root, reported by every command in its first line so a failure names the tree it
/// was looking at.
pub fn announce(root: &std::path::Path) {
    println!("==> drmod-rs at {}", root.display());
}

/// Shorthand used throughout: a step header in the same shape the PowerShell scripts used, so the CI
/// log stays readable at a glance.
pub fn step(message: &str) {
    println!("==> {message}");
}

/// The closing line of an artifact-producing step: what was written, and how big it is.
///
/// The size is printed because these artifacts are downloads — a payload that quietly loses its
/// bytes (say, the editor's embedded mod) is the failure worth noticing here rather than in front of
/// a user.
pub fn done(path: &std::path::Path) {
    match std::fs::metadata(path) {
        Ok(meta) => println!("==> Done: {} ({})", path.display(), util::human_size(meta.len())),
        Err(_) => println!("==> Done: {}", path.display()),
    }
}
