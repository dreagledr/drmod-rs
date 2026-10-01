//! The two files the editor installs into the game, embedded in the editor's own binary.
//!
//! They are `include_bytes!` of what [`build`](crate) staged, and that is the whole reason the
//! payload cannot go stale: the bytes in a running editor are, by construction, the ones the build it
//! was compiled by produced. An editor that *looks for* its payload beside the exe can find an older
//! one, or none; an editor that carries it cannot.
//!
//! ⚠️ It also means the build has a hard requirement, not a soft one: `cargo build` here builds the
//! mod (`../drmod_rs_lib`, i686) and embeds the result, and a machine with no mod DLL to embed cannot
//! compile this crate at all. That is deliberate — see `build.rs` for the fallback it takes when the
//! mod is mid-edit — and it is the same shape the Reactor sibling has, where the csproj fails loudly
//! on a missing payload rather than shipping an Install button with nothing behind it.
//!
//! The visible consequence is that `ModPanel` has no "payload missing" state to paint: the payload is
//! a fact of the build.

/// The mod itself, as the ASI loader will load it: `drmod_rs_lib.dll` renamed to `.asi`. The loader
/// does not care what the file is called — `DllMain` on process attach is the entry point either way
/// — so this is the same bytes the launcher extracts.
pub const ASI_NAME: &str = "drmod_rs_lib.asi";

/// The ASI loader, as `d3d9.dll` in the game's root.
///
/// ⚠️ Win32: the game is a 32-bit process and never loads a 64-bit `d3d9.dll`, which `build.rs`
/// checks for before staging it.
pub const LOADER_NAME: &str = "d3d9.dll";

/// The mod's bytes.
const ASI: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Mod/drmod_rs_lib.asi"));

/// The loader's bytes.
const LOADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Mod/d3d9.dll"));

/// The two files, as the build staged them.
#[derive(Clone, Copy, Debug)]
pub struct Payload {
    pub asi: &'static [u8],
    pub loader: &'static [u8],
}

impl Payload {
    /// What this build embedded. There is nothing to read and nothing that can fail.
    pub const fn embedded() -> Self {
        Self {
            asi: ASI,
            loader: LOADER,
        }
    }

    /// The mod's size in kibibytes, for the install panel to show — the number an author checks when
    /// they want to know whether the payload they just built made it in.
    pub fn asi_kib(&self) -> usize {
        self.asi.len() / 1024
    }

    /// The loader's size in kibibytes.
    pub fn loader_kib(&self) -> usize {
        self.loader.len() / 1024
    }
}

impl Default for Payload {
    fn default() -> Self {
        Self::embedded()
    }
}
