//! Experimental TAS editor on Rune.
//!
//! The shipping editor (`drmod-tas-editor`) is the tool: it is ImGui, Windows-only, and carries the
//! mod's install. This crate asks a different question — can Rune, a retained GPU UI, carry the
//! same editor — so it shares the script formats (`drmod-script`) and the mod's HTTP client, and
//! owns only the view.
//!
//! ```sh
//! cargo run -- --snap out.png     # headless, no window
//! cargo run                       # the window
//! ```

mod api;
mod app;
mod buffers;
mod highlight;
mod menu_settler;
mod panels;
mod platform;
mod poller;
mod settings;
mod work;
mod workspace;

use rune::AppSpec;

fn main() {
    // The compositor's name for the window: Wayland `app_id`, X11 `WM_CLASS`.
    rune::set_app_id("drmod-rune-editor");

    rune::run(AppSpec {
        families: ("Inter", "ui-monospace"),
        poll: Some(app::App::poll),
        on_key: Some(app::App::on_key),
        min_size: Some((980.0, 620.0)),
        ..AppSpec::new(panels::TITLE, (1440.0, 900.0), app::App::new, panels::view)
    });
}
