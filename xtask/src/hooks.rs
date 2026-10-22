//! Installing the repository's git hooks.
//!
//! The hooks themselves live in `.githooks/` so they are versioned with the code; git only runs
//! them once `core.hooksPath` points there. That is a per-clone setting, so it cannot be committed
//! — `cargo xtask install-hooks` is the one command that writes it.
//!
//! This is deliberately not `cargo-husky` or a `build.rs` side effect: a hook that appears because
//! a crate was built is a surprise, and the repository already has an automation entry point.

use std::path::Path;

use crate::process;
use crate::{Error, Result};

/// Points `core.hooksPath` at the committed `.githooks` directory.
///
/// Idempotent: running it again just rewrites the same value, which is what makes it safe to put in
/// onboarding notes.
pub fn install(root: &Path) -> Result<()> {
    let hook = root.join(".githooks").join("pre-commit");
    if !hook.is_file() {
        return Err(Error::new(format!(
            "not found: {}\n       the checkout is incomplete: .githooks/pre-commit is committed",
            hook.display()
        )));
    }

    process::run(root, "git", &["config", "core.hooksPath", ".githooks"])?;

    println!("    pre-commit: cargo fmt --check for the workspaces a commit touches");
    println!("    skip once with `git commit --no-verify`");
    Ok(())
}
