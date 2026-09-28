//! Installing the mod into the game — two files copied into the game's root, and the game loads
//! them on its next start.
//!
//! ```text
//! Metal Gear Rising REVENGEANCE\
//! |-- METAL GEAR RISING REVENGEANCE.exe
//! |-- d3d9.dll                   <- the ASI loader
//! |-- plugins\
//!      |-- drmod_rs_lib.asi      <- the mod itself
//! ```
//!
//! Both files are **embedded in the editor's binary** ([`payload`]), staged there by the crate's
//! build script: `cargo build` builds the mod and carries its bytes, so the editor never installs a
//! version it was not built from. Nothing here launches, kills or injects anything — the files are the
//! whole install, which is why the panel says "restart the game" rather than acting.
//!
//! ⚠️ A `d3d9.dll` that is already there belongs to somebody else until proven otherwise. Almost
//! every machine with a ReShade, an ENB or another ASI mod has one, and overwriting it would break
//! that mod to install this one. The plugin is loaded by any ASI loader, so the loader is only ever
//! *added* when the folder has none — and only *removed* when it is byte-for-byte ours.

pub mod installer;
pub mod payload;

pub use installer::{ModState, LOADER_PATH, LOADER_URL, PLUGIN_DIR};
pub use payload::{ASI_NAME, LOADER_NAME, Payload};
