//! Running other programs, one function per concern.
//!
//! Everything this tool does that is not file copying is "run cargo somewhere with these arguments",
//! and the two places that matters are `cargo` for the workspace's own builds and `zip` writing for
//! the artifacts.

use std::path::Path;
use std::process::Command;

use crate::{Error, Result};

/// Runs cargo in `dir` with `args`, inheriting the console so the build's own output is the log.
///
/// `dir` is load-bearing rather than cosmetic: cargo resolves `.cargo/config.toml` from the working
/// directory's ancestors, so where the command runs is what decides which target triple a build
/// uses. Each call site says which directory it needs and why.
pub fn cargo(dir: &Path, args: &[&str]) -> Result<()> {
    run(dir, "cargo", args)
}

/// Runs a program in `dir`, failing with the exit status when it does not succeed.
pub fn run(dir: &Path, program: &str, args: &[&str]) -> Result<()> {
    println!("    ({}) {} {}", dir.display(), program, args.join(" "));

    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|error| Error::new(format!("cannot run {program}: {error}")))?;

    if status.success() {
        return Ok(());
    }

    Err(Error::new(format!(
        "`{program} {}` failed ({status}) in {}",
        args.join(" "),
        dir.display()
    )))
}

/// Runs cargo in `dir` with `args` while `env` is set to `value`, then removes it again.
///
/// The editor's packaging needs exactly one variable (`TAS_EDITOR_SKIP_MOD_BUILD`) set for exactly
/// one invocation, and setting it in the parent process — the way the PowerShell script did — leaks
/// it into everything this tool runs afterwards. Here it is set on the child only.
pub fn cargo_with_env(dir: &Path, args: &[&str], env: &str, value: &str) -> Result<()> {
    println!("    ({}) {env}={value} cargo {}", dir.display(), args.join(" "));

    let status = Command::new("cargo")
        .args(args)
        .current_dir(dir)
        .env(env, value)
        .status()
        .map_err(|error| Error::new(format!("cannot run cargo: {error}")))?;

    if status.success() {
        return Ok(());
    }

    Err(Error::new(format!(
        "`cargo {}` failed ({status}) in {}",
        args.join(" "),
        dir.display()
    )))
}
