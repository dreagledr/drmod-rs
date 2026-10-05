//! The script formats, re-exported from `drmod-script`.
//!
//! The DSL text, the API JSON, the frame projections and the record converter are shared with the
//! mod and the CLI, so they live in the `drmod-script` crate; this module is only the editor's
//! window onto it, kept so the editor's own paths (`crate::script::dsl`) do not change.

pub use drmod_script::*;
