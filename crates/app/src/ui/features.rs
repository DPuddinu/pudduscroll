use eframe::App;
use eframe::egui;
use pudduscroll_core::AspectRatio;

use super::draw::frame_screen;
use super::{PudduScrollApp, Route};

const VIEW: egui::Vec2 = egui::vec2(1440.0, 900.0);

fn ready() -> (egui::Context, PudduScrollApp) {
    let ctx = egui::Context::default();
    let mut app = PudduScrollApp::assemble(&ctx);
    app.library_ready = true;
    app.status.clear();
    (ctx, app)
}

fn paint(app: &mut PudduScrollApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let mut host = eframe::Frame::_new_kittest();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, VIEW)),
        time: Some(0.0),
        predicted_dt: 1.0 / 60.0,
        focused: true,
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        app.ui(ui, &mut host);
    });
    output.textures_delta.clear();
}

fn settle(app: &mut PudduScrollApp, ctx: &egui::Context) {
    paint(app, ctx, Vec::new());
    paint(app, ctx, Vec::new());
}

fn shown(app: &PudduScrollApp, name: &str) -> bool {
    app.widgets.iter().any(|(widget, _)| widget == name)
}

fn rect(app: &PudduScrollApp, name: &str) -> egui::Rect {
    app.widgets
        .iter()
        .find(|(widget, _)| widget == name)
        .map_or(egui::Rect::NOTHING, |(_, rect)| *rect)
}

fn click(app: &mut PudduScrollApp, ctx: &egui::Context, name: &str) {
    let pos = rect(app, name).center();
    paint(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    paint(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

fn plus_names(app: &PudduScrollApp) -> Vec<String> {
    app.widgets
        .iter()
        .filter(|(name, _)| name.starts_with("plus-"))
        .map(|(name, _)| name.clone())
        .collect()
}

fn cover() -> crate::library::Blob {
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("png");
    crate::library::blob_from_bytes("cover.png", bytes).expect("cover")
}

fn empty_frames_hold_a_visible_plus(app: &PudduScrollApp) {
    let layout = app.layout().expect("layout");
    let strip = app.strip_rect.expect("strip");
    let view = app.view_rect.expect("view");
    let count = layout.canvas_count;
    for frame in &layout.frames {
        if frame.photo.is_some() {
            continue;
        }
        let screen = frame_screen(strip, count, frame);
        let hit = rect(app, &format!("plus-{}", frame.id.0));
        assert!(
            screen.contains(hit.center()),
            "plus {} sits outside its frame",
            frame.id.0
        );
        assert!(
            view.contains(hit.center()),
            "plus {} sits outside the editor",
            frame.id.0
        );
    }
}

fn tap(app: &mut PudduScrollApp, ctx: &egui::Context, key: egui::Key) {
    paint(
        app,
        ctx,
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

#[test]
fn delete_confirms_selected_canvases() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    app.add_canvas_command();
    app.selected_canvases = vec![1];
    tap(&mut app, &ctx, egui::Key::Delete);
    assert!(matches!(
        &app.confirm,
        Some(super::Confirm::RemoveCanvases(indices)) if indices == &vec![1]
    ));
    app.confirm = None;
    app.selected_canvases = vec![0, 1];
    tap(&mut app, &ctx, egui::Key::Delete);
    assert!(matches!(app.confirm, Some(super::Confirm::RemoveFrame(_))));
    app.confirm = None;
    app.selected_canvases.clear();
    tap(&mut app, &ctx, egui::Key::Delete);
    assert!(matches!(app.confirm, Some(super::Confirm::RemoveFrame(_))));
}

#[test]
fn home_offers_a_template_and_an_import() {
    let (ctx, mut app) = ready();
    settle(&mut app, &ctx);
    assert!(shown(&app, "new-template"));
    assert!(shown(&app, "import-carousel"));
    assert_eq!(plus_names(&app), Vec::<String>::new());
    assert!(!shown(&app, "share"));
}

#[test]
fn an_empty_carousel_frame_shows_a_plus() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    settle(&mut app, &ctx);
    let frames = app.layout().expect("layout").frames.len();
    assert_eq!(frames, 1);
    assert_eq!(plus_names(&app).len(), 1);
    empty_frames_hold_a_visible_plus(&app);
    for name in [
        "home",
        "export",
        "share",
        "add-canvas-left",
        "add-canvas",
        "add-frame",
        "edit-frame",
        "remove-frame",
        "grid",
        "snap",
        "grid-squares",
        "from-link",
        "rotate-ccw",
        "rotate-cw",
        "rotate-free",
        "canvas-1:1",
        "canvas-4:5",
        "canvas-3:4",
        "canvas-9:16",
        "frame-aspect-1:1",
        "frame-aspect-4:5",
        "frame-aspect-3:4",
        "frame-aspect-9:16",
    ] {
        assert!(shown(&app, name), "{name} missing");
    }
}

#[test]
fn add_frame_paints_a_plus_on_the_next_canvas_and_undo_removes_it() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "add-canvas");
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").canvas_count, 2);
    click(&mut app, &ctx, "add-frame");
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").frames.len(), 2);
    assert_eq!(plus_names(&app).len(), 2);
    empty_frames_hold_a_visible_plus(&app);
    app.undo_edit(&ctx);
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").frames.len(), 1);
    assert_eq!(plus_names(&app).len(), 1);
}

#[test]
fn a_template_frame_has_no_plus() {
    let (ctx, mut app) = ready();
    let id = app
        .library
        .create_template("Template".to_string(), AspectRatio::FOUR_FIVE, cover());
    app.library
        .templates
        .iter_mut()
        .find(|template| template.id == id)
        .expect("template")
        .layout
        .add_frame();
    app.enter(Route::Template(id), &ctx);
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").frames.len(), 1);
    assert_eq!(plus_names(&app), Vec::<String>::new());
    assert!(!shown(&app, "share"));
    assert!(!shown(&app, "from-link"));
    assert!(shown(&app, "add-frame"));
}

#[test]
fn settings_keep_export_format_and_bindings() {
    let (ctx, mut app) = ready();
    app.settings_open = true;
    settle(&mut app, &ctx);
    for name in [
        "settings-back",
        "settings-jpeg",
        "settings-png",
        "settings-quality",
        "bind-new-carousel",
        "bind-export",
        "bind-undo",
        "bind-add-frame",
    ] {
        assert!(shown(&app, name), "{name} missing");
    }
    #[cfg(not(target_arch = "wasm32"))]
    assert!(shown(&app, "settings-choose-folder"));
    #[cfg(target_arch = "wasm32")]
    assert!(!shown(&app, "settings-choose-folder"));
}

#[test]
fn export_modal_offers_the_save_action() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    app.export_open = true;
    settle(&mut app, &ctx);
    assert!(shown(&app, "export-go"));
}
