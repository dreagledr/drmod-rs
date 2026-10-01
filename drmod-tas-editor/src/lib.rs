//! TAS Editor (Rust): the C# editor's functionality, on the `dear-app` + `dear-imgui-cte` stack.
//!
//! A library as well as a binary so the script formats can be tested without a window: everything
//! the editor's correctness rests on — the DSL text, the API JSON, the two frame views — is here
//! and has no dependency on the UI.
//!
//! The binary (`src/main.rs`) is the application: the window, the frame loop and the text editor.
//! Everything it draws lives here, which is what `cargo test` exercises headlessly — including
//! `tests/golden.rs`, which checks this port's written scripts against the format's own fixtures
//! byte for byte.

pub mod api;
pub mod buffers;
pub mod editor;
pub mod game_window;
pub mod menu_settler;
pub mod mod_install;
pub mod script;
pub mod settings;
pub mod steam;
pub mod workspace;
