#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod color;
mod library;
mod settings;
mod ui;
mod window_menu;

use eframe::egui;
use ui::PudduScrollApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PudduScroll")
            .with_position([80.0, 60.0])
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1100.0, 720.0]),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "PudduScroll",
        options,
        Box::new(|cc| Ok(Box::new(PudduScrollApp::new(cc)))),
    )
}
