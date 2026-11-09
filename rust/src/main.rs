// A release build on Windows opens no console window next to the program.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use eframe::egui;
use linewright::ui::app::{LinewrightApp, WINDOW_NAME};

fn main() -> eframe::Result {
    let first = std::env::args_os().nth(1);
    // `linewright --mcp` runs no window: it serves the Model Context Protocol on standard input and output.
    if first.as_deref() == Some(std::ffi::OsStr::new("--mcp")) {
        if let Err(error) = linewright::mcp::serve() {
            eprintln!("linewright --mcp: {error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let path = first.map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_NAME)
            .with_app_id("linewright")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 560.0])
            .with_icon(linewright::ui::logo::window_icon(64))
            .with_decorations(false), // the header is the title bar
        ..Default::default()
    };
    eframe::run_native(WINDOW_NAME, options, Box::new(move |cc| Ok(Box::new(LinewrightApp::new(cc, path)))))
}
