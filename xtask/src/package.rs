//! The three release artifacts: the mod (in both of its forms) and the tools.
//!
//! Names match the PowerShell scripts this replaces:
//!
//! | command | was | writes |
//! |---|---|---|
//! | `build` | `build.ps1` | `out/drmod-rs.zip`, `out/drmod-asi.zip` |
//! | `build-tools` | `build_tools.ps1` | `out/dbdump.exe`, `out/dump-replay-input.exe`, `out/drmod-tas.exe` + `out/drmod-tas.zip` |
//!
//! # The mod, in both forms
//!
//! The same `drmod_rs_lib.dll` ships two ways, and they must stay the same bytes:
//!
//! * **the launcher** — `drmod.exe`, which embeds the DLL via `include_bytes!` and extracts it into
//!   `%TEMP%` before injecting. One file, no loader needed.
//! * **the ASI plugin** — `plugins/drmod_rs_lib.asi` next to the vendored ASI loader, which the game
//!   loads itself. No second process.
//!
//! ⚠️ **Neither form is compressed, and neither may become compressed.** The `.asi` is the payload
//! the launcher extracts byte for byte, so packing one and not the other would make the two forms
//! ship different mods — `vendor/asi-loader/README.md` says this in the same words. The earlier
//! scripts UPX-packed `drmod.exe` only; that step is gone (the exe is embedded in the launcher's own
//! archive anyway, and UPX is a second toolchain to install), so nothing here packs anything.
//!
//! ⚠️ **The ASI loader's architecture is checked, not trusted.** The game is a 32-bit process and
//! silently never loads a 64-bit `d3d9.dll`, which reaches the user as "the mod installed but does
//! nothing". `drmod-tas-editor/build.rs` performs the same PE32 check for the copy it embeds; this
//! one guards the copy that goes into the zip.

use std::path::{Path, PathBuf};

use crate::process;
use crate::{done, step, util, Error, Result};

/// The mod's DLL, as the root build leaves it. The triple is spelled out because it is a contract:
/// `drmod-injector/src/main.rs` embeds the same path with `include_bytes!`.
const MOD_TARGET: &str = "i686-pc-windows-msvc";
/// The tools' triple. `arrow-rs` and `parquet`, which `dbdump` needs, are 64-bit only.
const TOOLS_TARGET: &str = "x86_64-pc-windows-msvc";

/// `cargo xtask build` — build the mod and package both of its distributions.
pub fn build(root: PathBuf) -> Result<()> {
    crate::announce(&root);

    step("Building drmod-rs (release)...");
    process::cargo(&root, &["build", "--release"])?;

    let target = root.join("target").join(MOD_TARGET).join("release");
    let exe = target.join("drmod.exe");
    let dll = target.join("drmod_rs_lib.dll");

    util::require_file(&exe, "the launcher did not build — check the cargo output above")?;
    util::require_file(&dll, "the library did not build — check the cargo output above")?;

    let out = root.join("out");
    std::fs::create_dir_all(&out)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", out.display())))?;

    // ── the launcher, as a single-file zip ──
    step("Packaging the launcher to out/ ...");
    let launcher_zip = out.join("drmod-rs.zip");
    zip_file(&launcher_zip, &[(exe.clone(), "drmod.exe".to_owned())])?;
    done(&launcher_zip);

    // ── the same mod as an ASI plugin, beside a loader ──
    step("Packaging the ASI distribution...");
    let loader = root.join("vendor").join("asi-loader").join("d3d9.dll");
    util::require_file(
        &loader,
        "the Win32 Ultimate-ASI-Loader is checked in at vendor/asi-loader — see its README.md",
    )?;
    require_pe32(&loader, "vendor/asi-loader/README.md")?;

    // Staged in out/asi/ first, both because a zip needs a tree that looks like the game folder
    // (`d3d9.dll` beside `plugins/`) and because a failed build then leaves the staging area for
    // inspection rather than a half-written archive.
    let stage = out.join("asi");
    util::clean_dir(&stage)?;
    util::copy(&dll, &stage.join("plugins").join("drmod_rs_lib.asi"))?;
    util::copy(&loader, &stage.join("d3d9.dll"))?;
    util::copy(&root.join("docs").join("asi-readme.txt"), &stage.join("readme.txt"))?;

    let asi_zip = out.join("drmod-asi.zip");
    zip_assembled_tree(&asi_zip, &stage)?;
    done(&asi_zip);

    println!(
        "==> ASI payload: drmod_rs_lib.asi = drmod_rs_lib.dll ({})",
        util::human_size(
            std::fs::metadata(&dll)
                .map_err(|error| Error::new(format!("cannot stat {}: {error}", dll.display())))?
                .len()
        )
    );

    Ok(())
}

/// `cargo xtask build-tools` — build `dbdump`, `drmod-tas` (both x64) and `dump-replay-input` (i686)
/// into `out/`.
///
/// ⚠️ **The triples are passed explicitly, which is what lets this run from the repository root.**
/// Cargo resolves `.cargo/config.toml` from the working directory's ancestors, so building
/// `drmod-dbdump` from the root would inherit the root's `i686-pc-windows-msvc` and fail on
/// `arrow-rs` — the reason `build_tools.ps1` wrapped these in `Push-Location`. An explicit `--target`
/// overrides that default, and `--target-dir` keeps both builds' artifacts in the root's `target/`
/// where the CI paths (`target/x86_64-pc-windows-msvc/release/dbdump.exe`) already point.
///
/// `tools/disasm/` is its own workspace with no `.cargo/config.toml` of its own, so its build takes
/// the triple by argument for the same reason — and its artifacts belong beside the other tools in
/// the root's `target/` rather than in a `tools/disasm/target/` nobody looks in.
pub fn build_tools(root: PathBuf) -> Result<()> {
    crate::announce(&root);

    let target_dir = root.join("target");
    let target_dir = target_dir
        .to_str()
        .ok_or_else(|| Error::new("the repository path is not valid UTF-8"))?;

    step("Building dbdump (x64)...");
    process::cargo(
        &root,
        &[
            "build",
            "--release",
            "--target",
            TOOLS_TARGET,
            "--target-dir",
            target_dir,
            "-p",
            "drmod-dbdump",
        ],
    )?;

    step("Building dump-replay-input (disasm)...");
    process::cargo(
        &root,
        &[
            "build",
            "--release",
            "--manifest-path",
            "tools/disasm/Cargo.toml",
            "--target",
            MOD_TARGET,
            "--target-dir",
            target_dir,
            "--bin",
            "dump-replay-input",
        ],
    )?;

    step("Building drmod-tas (x64)...");
    process::cargo(
        &root,
        &[
            "build",
            "--release",
            "--target",
            TOOLS_TARGET,
            "--target-dir",
            target_dir,
            "-p",
            "drmod-cli",
        ],
    )?;

    let out = root.join("out");
    std::fs::create_dir_all(&out)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", out.display())))?;

    for relative in [
        format!("target/{TOOLS_TARGET}/release/dbdump.exe"),
        format!("target/{TOOLS_TARGET}/release/drmod-tas.exe"),
        format!("target/{MOD_TARGET}/release/dump-replay-input.exe"),
    ] {
        let built = root.join(&relative);
        util::require_file(&built, "the tool did not build — check the cargo output above")?;

        let name = built
            .file_name()
            .ok_or_else(|| Error::new(format!("{} has no file name", built.display())))?;
        let staged = out.join(name);
        util::copy(&built, &staged)?;
        done(&staged);
    }

    // The CLI also ships as a single-file archive for the release, the way the editor and the mod
    // do: `out/drmod-tas.zip` is what CI attaches and what a user unzips to get `drmod-tas.exe`.
    // Written to a temp name inside `out/` and moved into place, so an interrupted run cannot leave
    // a half-written archive where a release archive is expected.
    let cli = out.join("drmod-tas.exe");
    let archive = out.join("drmod-tas.zip");
    let temporary = out.join("drmod-tas.zip.tmp");
    step("Packaging out/drmod-tas.zip ...");
    zip_file(&temporary, &[(cli, "drmod-tas.exe".to_owned())])?;
    std::fs::rename(&temporary, &archive).map_err(|error| {
        Error::new(format!(
            "cannot move {} to {}: {error}",
            temporary.display(),
            archive.display()
        ))
    })?;
    done(&archive);

    println!("==> Tools in: {}", out.display());
    Ok(())
}

/// Writes one zip holding the given files under the given names inside it.
fn zip_file(archive: &Path, entries: &[(PathBuf, String)]) -> Result<()> {
    let mut file = create_archive(archive)?;

    for (source, name) in entries {
        add_file(&mut file, source, name)?;
    }

    file.finish().map_err(|error| {
        Error::new(format!("cannot finish {}: {error}", archive.display()))
    })?;

    Ok(())
}

/// Writes a zip of an assembled directory, laid out relative to that directory.
///
/// The directory becomes the zip's root (`out/asi/plugins/x` is `plugins/x` in the archive), which is
/// what both artifacts that are trees need: an ASI package has to have `d3d9.dll` beside `plugins/`,
/// and the editor's zip has to unpack into a folder that runs.
pub fn zip_assembled_tree(archive: &Path, dir: &Path) -> Result<()> {
    let mut file = create_archive(archive)?;

    for path in util::files_recursive(dir)? {
        let relative = path
            .strip_prefix(dir)
            .map_err(|_| Error::new(format!("{} is not under {}", path.display(), dir.display())))?;
        let name = relative.to_string_lossy().replace('\\', "/");
        add_file(&mut file, &path, &name)?;
    }

    file.finish().map_err(|error| {
        Error::new(format!("cannot finish {}: {error}", archive.display()))
    })?;

    Ok(())
}

fn create_archive(archive: &Path) -> Result<zip::ZipWriter<std::fs::File>> {
    if let Some(parent) = archive.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::new(format!("cannot create {}: {error}", parent.display())))?;
    }

    if archive.exists() {
        std::fs::remove_file(archive)
            .map_err(|error| Error::new(format!("cannot replace {}: {error}", archive.display())))?;
    }

    let file = std::fs::File::create(archive)
        .map_err(|error| Error::new(format!("cannot create {}: {error}", archive.display())))?;

    Ok(zip::ZipWriter::new(file))
}

/// Writes one file into the archive under `name`.
///
/// Takes the writer by reference and reborrows it mutably, so the caller keeps ownership of the
/// `ZipWriter` for the next entry (`ZipWriter` is not `Write` through a shared reference).
fn add_file(writer: &mut zip::ZipWriter<std::fs::File>, source: &Path, name: &str) -> Result<()> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    writer
        .start_file(name, options)
        .map_err(|error| Error::new(format!("cannot add {name} to the archive: {error}")))?;

    let mut file = std::fs::File::open(source)
        .map_err(|error| Error::new(format!("cannot open {}: {error}", source.display())))?;

    std::io::copy(&mut file, writer)
        .map_err(|error| Error::new(format!("cannot write {name} into the archive: {error}")))?;

    Ok(())
}

/// Whether the bytes begin with a PE32 (i386) image header.
///
/// `MZ` at the start, `PE\0\0` at the offset the DOS header names, and `0x014C` —
/// `IMAGE_FILE_MACHINE_I386` — at the start of the COFF header that follows. A full PE parse would be
/// more code than the question needs: the question is only whether this is the Win32 build, and the
/// machine field is the field that answers it. The editor's build script asks the same question about
/// the same file for the copy it embeds.
fn require_pe32(path: &Path, hint: &str) -> Result<()> {
    if is_pe32(&util::read(path)?) {
        return Ok(());
    }

    Err(Error::new(format!(
        "{} is not a Win32 image — the game is a 32-bit process and never loads a 64-bit d3d9.dll. \
         See {hint}.",
        path.display()
    )))
}

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
