//! The build script: build the mod the editor installs, and stage it where the editor embeds it.
//!
//! Two files, and they are the whole of what "installing the mod" writes into the game:
//!
//! ```text
//! <OUT_DIR>/Mod/
//! |-- drmod_rs_lib.asi   <- the mod: the root workspace's `drmod_rs_lib.dll`, renamed
//! |-- d3d9.dll           <- the ASI loader, from ../vendor/asi-loader/
//! ```
//!
//! They go to `OUT_DIR` because the binary **embeds** them (`include_bytes!` in
//! `src/mod_install/payload.rs`) rather than looking for them at runtime. That is what makes the
//! payload's currency structural instead of hoped-for: an embedded copy is, by construction, the one
//! this build staged, and there is no directory that can be stale, deleted, or missing beside a
//! shipped exe. `OUT_DIR` is cargo's own per-build directory, so it is also the one place a build
//! script may write without touching the source tree.
//!
//! # Why this is a build script and not a PowerShell file
//!
//! The editor *installs* the mod but does not implement it: the mod is the root workspace
//! (`drmod_rs_lib`, i686, injected into the game), and the editor carries it as a payload. Those are
//! one product, so they are built together — one `cargo build` here builds the mod from the same
//! sources its own build would use and embeds the result. There is no second script to remember, and
//! no payload that is quietly a version behind (the failure that matters: an editor whose Install
//! button writes a stale DLL into the game looks like a broken mod, not like a stale build).
//!
//! The two sources, one line each:
//!
//! * `drmod_rs_lib.dll` — `cargo build --release --lib` in the repository root (the root's
//!   `.cargo/config.toml` makes that i686, which is what a 32-bit game needs);
//! * `d3d9.dll` — the vendored Ultimate-ASI-Loader, copied from `../vendor/asi-loader/`.
//!
//! ⚠️ Win32, not Win64. The game is a 32-bit process and silently never loads a 64-bit `d3d9.dll`, so
//! a wrong-architecture loader installs and then does nothing — the worst failure to debug. The
//! loader is checked for the PE32 signature before it is staged.
//!
//! # When the mod cannot be built
//!
//! The editor needs *a* mod to embed, but not necessarily a mod that compiles this second: a mod
//! mid-edit is a normal state to build the editor in. So a failed nested build falls back to the DLL
//! the last successful build left in the root's `target/`, with a warning that says the embedded
//! payload is that older one. Only with no DLL anywhere does this script stop the build — and then
//! it says why, and `TAS_EDITOR_SKIP_MOD_BUILD=1` is the way to build the editor without trying the
//! mod at all.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for a build script"));

    let Some(repo_root) = crate_dir.parent().map(Path::to_path_buf) else {
        fail("the editor crate has no parent directory to find the mod in");
    };

    let payload_dir = out_dir.join("Mod");
    if let Err(reason) = std::fs::create_dir_all(&payload_dir) {
        fail(&format!("cannot create {}: {reason}", payload_dir.display()));
    }

    // Watch the mod so the payload cannot go stale: a change to the mod's own sources — or to the
    // crates it shares with the editor, or to the loader — re-runs this script. Cargo then runs it
    // only when one of these has actually changed, which is what makes the embedded bytes current
    // without rebuilding the world on every `cargo build`.
    //
    // ⚠️ The staged files are deliberately **not** watched: cargo caches a build script's output and
    // replays it when nothing watched changed, and a script that watches what it writes rewrites it
    // on every run — which changes the very mtime cargo is watching, and the build never settles
    // (measured: a permanent recompile loop).
    for watched in [
        "../drmod-core/src",
        "../Cargo.toml",
        "../Cargo.lock",
        "../.cargo/config.toml",
        "../drmod-protocol/src",
        "../drmod-replay-types/src",
        "../vendor/asi-loader/d3d9.dll",
    ] {
        println!("cargo:rerun-if-changed={watched}");
    }

    let loader = repo_root.join("vendor").join("asi-loader").join("d3d9.dll");
    let dll = repo_root
        .join("target")
        .join("i686-pc-windows-msvc")
        .join("release")
        .join("drmod_rs_lib.dll");

    stage_loader(&loader, &payload_dir);
    stage_mod(&repo_root, &dll, &payload_dir);

    println!("cargo:warning=mod payload embedded from {} and {}", dll.display(), loader.display());
}

/// Copies the ASI loader out of `vendor/`, where it is checked in as a binary.
///
/// The check that matters is the architecture: see the module docs. A wrong-arch loader is worse than
/// no loader, so this one stops the build rather than warning.
fn stage_loader(loader: &Path, payload_dir: &Path) {
    let bytes = match std::fs::read(loader) {
        Ok(bytes) => bytes,
        Err(error) => fail(&format!(
            "cannot read {}: {error}. See vendor/asi-loader/README.md.",
            loader.display()
        )),
    };

    if !is_pe32(&bytes) {
        fail(&format!(
            "{} is not a Win32 image — the game is a 32-bit process and never loads a 64-bit \
             d3d9.dll. See vendor/asi-loader/README.md.",
            loader.display()
        ));
    }

    let target = payload_dir.join("d3d9.dll");
    if let Err(error) = std::fs::write(&target, bytes) {
        fail(&format!("cannot write {}: {error}", target.display()));
    }
}

/// Builds the mod and stages its DLL as the `.asi` the loader expects.
///
/// `--lib` and `-p` keep this to the mod itself: the root workspace also holds the multiplayer server
/// and the replay tools, and none of them belongs in a payload.
fn stage_mod(repo_root: &Path, dll: &Path, payload_dir: &Path) {
    let skip = std::env::var("TAS_EDITOR_SKIP_MOD_BUILD").is_ok_and(|value| !value.is_empty());

    // Whether *this* run produced a fresh DLL. Only a failed build falls back to the previous one, and
    // only that fallback is worth a warning: warning on a successful build would train the reader to
    // ignore the line that matters.
    let mut freshly_built = false;

    if !skip {
        // ⚠️ The cargo that invoked this script, by its own path, and with an explicit target dir: the
        // editor's build must never send the mod's artifacts into the editor's `target/`, and the
        // root's `target/` is where the mod's own `cargo build` puts them, so the two share one cache.
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let mut command = Command::new(&cargo);
        command
            .current_dir(repo_root)
            .args(["build", "--release", "--lib", "-p", "drmod-rs", "--target-dir"])
            .arg(repo_root.join("target"));

        // ⚠️ **The inherited build environment is stripped, and this is not tidiness.**
        // Cargo hands a build script the flags *it* was invoked with (`RUSTFLAGS`,
        // `CARGO_ENCODED_RUSTFLAGS`, and a jobserver in `CARGO_MAKEFLAGS`), and a nested cargo
        // inherits them. Those flags then differ from the ones a plain `cargo build` at the root uses,
        // and cargo fingerprints rustflags — so the two builds fight over one `target/`: each one
        // finds the other's artifacts dirty and rebuilds imgui, hudhook and sqlite3 (measured: 31 s
        // and `Dirty drmod-rs: the rustflags changed`, on every editor build, and it left the root's
        // cache in a state the user's own `cargo build` had to rebuild again).
        //
        // Removing them makes the nested invocation byte-for-byte the command the user would type in
        // the root, so the two share the cache the way they should.
        for leaked in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "RUSTDOCFLAGS",
            "RUSTC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_TARGET_DIR",
            "CARGO_MAKEFLAGS",
            "CARGO_INCREMENTAL",
        ] {
            command.env_remove(leaked);
        }

        let result = command.status();

        match result {
            Ok(status) if status.success() => freshly_built = true,
            Ok(status) => println!(
                "cargo:warning=`cargo build --release --lib -p drmod-rs` failed ({status}) in {}",
                repo_root.display()
            ),
            Err(error) => println!("cargo:warning=cannot run {cargo}: {error}"),
        }
    }

    if !dll.is_file() {
        fail(&format!(
            "the mod is not built: {} does not exist, so there is nothing to embed. Run \
             `cargo build --release` at the repository root, or install the i686-pc-windows-msvc \
             target. (TAS_EDITOR_SKIP_MOD_BUILD skips the build, not this check.)",
            dll.display()
        ));
    }

    if !skip && !freshly_built {
        println!(
            "cargo:warning=the mod did not build, so the embedded payload is the previous one from {}",
            dll.display()
        );
    }

    // The ASI form of the mod is the DLL renamed — nothing is repacked, so the editor installs the
    // same bytes the launcher extracts.
    let target = payload_dir.join("drmod_rs_lib.asi");
    if let Err(error) = std::fs::copy(dll, &target) {
        fail(&format!("cannot write {}: {error}", target.display()));
    }
}

/// Whether the bytes begin with a PE32 (i386) image header.
///
/// `MZ` at the start, `PE\0\0` at the offset the DOS header names, and `0x014C` — IMAGE_FILE_MACHINE_I386
/// — at the start of the COFF header that follows the signature. A full PE parse would be more code
/// than this needs: the question is only whether this is the Win32 build, and the machine field is the
/// field that answers it.
fn is_pe32(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return false;
    }

    let offset = u32::from_le_bytes([bytes[0x3C], bytes[0x3D], bytes[0x3E], bytes[0x3F]]) as usize;

    // ⚠️ The machine field sits *after* the four-byte `PE\0\0` signature, not at the header's start.
    let Some(header) = bytes.get(offset..offset + 6) else {
        return false;
    };

    &header[0..4] == b"PE\0\0" && header[4..6] == [0x4C, 0x01]
}

/// Stops the build with a message that says what to do.
///
/// The payload is not optional to *this* crate — the binary embeds it — so there is no partial state
/// to fall back into. A panic is how a build script says that, and cargo prints the message.
fn fail(reason: &str) -> ! {
    panic!("mod payload: {reason}");
}
