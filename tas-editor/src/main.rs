#![windows_subsystem = "windows"]

mod editor;
mod matrix;
mod mock;
mod model;
mod panels;

#[cfg(test)]
mod layout_test;

use editor::Editor;
use windows_reactor::*;

fn main() {
    App::run_component::<Editor>(()).unwrap();
}
