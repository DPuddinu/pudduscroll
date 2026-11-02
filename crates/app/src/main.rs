#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod color;
mod disk;
mod host;
mod library;
mod settings;
mod ui;
#[cfg(not(target_arch = "wasm32"))]
mod update;
#[cfg(target_arch = "wasm32")]
mod update {
    use std::sync::mpsc::Receiver;

    #[derive(Clone, Debug)]
    #[allow(
        dead_code,
        reason = "the desktop updater is the only constructor of these events"
    )]
    pub(crate) enum Event {
        Installing(String),
        Restart,
        Failed(String),
    }

    pub(crate) fn start() -> Option<Receiver<Event>> {
        None
    }
}
mod window_menu;

#[cfg(not(target_arch = "wasm32"))]
use eframe::egui;
use ui::PudduScrollApp;

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;

    wasm_bindgen_futures::spawn_local(async {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(document) = window.document() else {
            return;
        };
        let Some(canvas) = document.get_element_by_id("pudduscroll") else {
            return;
        };
        let Ok(canvas) = canvas.dyn_into::<web_sys::HtmlCanvasElement>() else {
            return;
        };
        let _ = eframe::WebLogger::init(log::LevelFilter::Info);
        let _ = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(PudduScrollApp::new(cc)))),
            )
            .await;
    });
}
