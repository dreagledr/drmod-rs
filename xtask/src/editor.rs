//! `cargo xtask pack-editor` — the TAS editor distribution (was `drmod-tas-editor/pack.ps1`).
//!
//! The editor is one exe: its build script embeds the mod payload (`drmod_rs_lib.asi` and
//! `d3d9.dll`) into the binary, so there is no runtime folder to assemble. What a distribution adds
//! is the one thing a zip needs in order to be a distribution rather than a bare binary: the
//! `examples/` scripts, so a first launch opens on a workspace instead of an empty pane
//! (`EditorSettings::first_folder` looks for them beside the exe first).
//!
//! The archive goes to `<root>/out/drmod-tas-editor.zip`, beside the mod's two zips: everything this
//! tool publishes lands in `out/`, and one layout is one thing to explain.
//!
//! ⚠️ **The exe is not slimmed.** It carries the mod and the ASI loader as embedded bytes — ~22 MB of
//! which ~9 MB is the payload — and that is the point: a user who unzips this and clicks Install gets
//! the mod the same build produced. Stripping it would break that, so nothing here does. The temp zip
//! goes to `out/` and not to the system temp directory, so that a failed run's leftovers are as
//! disposable as everything else in `out/` and stay on the same filesystem as the final archive.
//!
//! ⚠️ **Release only.** A debug build links a different runtime and expects a debugger's world; a zip
//! made from one is a zip that fails on a machine with no Rust toolchain.

use std::path::PathBuf;

use crate::process;
use crate::{done, step, util, Error, Result};

/// The editor's own triple. Its `.cargo/config.toml` says x64 because the window stack does not build
/// for the i686 the root config forces, which is why the path names the triple rather than the host.
const TARGET: &str = "x86_64-pc-windows-msvc";

/// The editor crate, relative to the repository root. It is a standalone workspace of its own.
const CRATE: &str = "drmod-tas-editor";

const USAGE: &str = "\
Usage: cargo xtask pack-editor [options]

Builds the editor, assembles out/drmod-tas-editor and packages it as out/drmod-tas-editor.zip.

Options:
  --skip-cargo         Reuse the mod DLL the root's `cargo xtask build` left in target/, instead of
                       letting the editor's build script build it again
  -h, --help           Show this message

The editor is always built — the release archives a build, not whatever exe happens to be lying in
target/ from last week. The release job runs `cargo xtask build` first and then passes --skip-cargo:
that is what keeps one release from compiling the same mod twice.";

pub fn pack(root: PathBuf, args: &[String]) -> Result<()> {
    let mut skip_cargo = false;

    let mut args = util::Args::new(args);
    while let Some(arg) = args.next() {
        match arg {
            "--skip-cargo" => skip_cargo = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(Error::new(format!("unknown option `{other}`\n\n{USAGE}"))),
        }
    }

    crate::announce(&root);

    let crate_dir = root.join(CRATE);
    let exe = crate_dir.join("target").join(TARGET).join("release").join("drmod-tas-editor.exe");
    let examples = crate_dir.join("examples");

    step("cargo build --release (editor)");

    // ⚠️ The nested mod build is skipped for this one invocation and no other: setting the
    // variable in the parent process — the way the PowerShell script did — would leak it into
    // everything this tool ran afterwards.
    if skip_cargo {
        println!(
            "    --skip-cargo: reusing the mod DLL already in the root's target/ — \
             TAS_EDITOR_SKIP_MOD_BUILD=1 for this build only"
        );
        process::cargo_with_env(
            &crate_dir,
            &["build", "--release"],
            "TAS_EDITOR_SKIP_MOD_BUILD",
            "1",
        )?;
    } else {
        process::cargo(&crate_dir, &["build", "--release"])?;
    }

    // The build either produced the exe or failed; this catches a path that has drifted apart from
    // what cargo writes, rather than a missing build.
    util::require_file(
        &exe,
        &format!(
            "the editor's build did not produce it — check the cargo output above, or run \
             `cargo build --release` in {} by hand",
            crate_dir.display()
        ),
    )?;

    // ⚠️ The payload is embedded, so its presence inside the exe is what a release rests on. A build
    // that somehow produced an editor without it would install nothing, and that failure would surface
    // in front of a user — so it is checked here, where the answer is still cheap. The build script
    // prints `mod payload embedded from ...` when it stages one, and refuses a missing DLL outright.
    let exe_size = file_size(&exe)?;
    if exe_size < 10 * 1024 * 1024 {
        return Err(Error::new(format!(
            "{} is only {} — too small to carry the mod payload.\n       \
             The test that catches this is in the build script's output: it prints \
             `mod payload embedded from ...`. A build made with TAS_EDITOR_SKIP_MOD_BUILD and no mod \
             DLL anywhere else would fail it.",
            exe.display(),
            util::human_size(exe_size)
        )));
    }

    util::require_dir(
        &examples,
        "a distribution without examples/ opens on an empty workspace",
    )?;

    let out = root.join("out");
    let staged = out.join("drmod-tas-editor");
    let archive = out.join("drmod-tas-editor.zip");

    step(&format!("Assembling {}", staged.display()));
    util::clean_dir(&staged)?;
    util::copy(&exe, &staged.join("drmod-tas-editor.exe"))?;
    // The scripts are copies of `tools/demo/`, staged here by hand rather than linked: a link into a
    // Rust tool directory would break the moment this tree is built on its own.
    util::copy_dir(&examples, &staged.join("examples"))?;

    let files = util::files_recursive(&staged)?;
    let total: u64 = files
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();

    step(&format!("Packaging {}", archive.display()));
    // Written to a temp name inside `out/` and moved into place, so an interrupted run cannot leave a
    // half-written archive where a release archive is expected.
    let temporary = out.join("drmod-tas-editor.zip.tmp");
    crate::package::zip_assembled_tree(&temporary, &staged)?;
    std::fs::rename(&temporary, &archive).map_err(|error| {
        Error::new(format!(
            "cannot move {} to {}: {error}",
            temporary.display(),
            archive.display()
        ))
    })?;

    println!();
    step(&format!("Done: {}", staged.display()));
    println!("      files: {}, size: {}", files.len(), util::human_size(total));
    println!("      exe: {} (payload embedded)", util::human_size(exe_size));
    done(&archive);
    println!("      unpack and run: drmod-tas-editor.exe");

    Ok(())
}

fn file_size(path: &std::path::Path) -> Result<u64> {
    std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|error| Error::new(format!("cannot stat {}: {error}", path.display())))
}
