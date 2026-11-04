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
    click_at(app, ctx, rect(app, name).center());
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

fn click_at(app: &mut PudduScrollApp, ctx: &egui::Context, pos: egui::Pos2) {
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

fn visible_canvas_hit(app: &PudduScrollApp) -> (usize, egui::Pos2) {
    let strip = app.strip_rect.expect("strip");
    let view = app.view_rect.expect("view");
    let count = app.layout().expect("layout").canvas_count;
    let canvas_w = strip.width() / count.max(1) as f32;
    let mut hit = None;
    for index in 0..count {
        let canvas = egui::Rect::from_min_size(
            egui::pos2(strip.min.x + index as f32 * canvas_w, strip.min.y),
            egui::vec2(canvas_w, strip.height()),
        );
        let visible = canvas.intersect(view);
        if visible.width() > 40.0 && visible.height() > 40.0 {
            let pos = egui::pos2(visible.min.x + 12.0, visible.min.y + 12.0);
            hit = Some((index, pos));
        }
    }
    hit.expect("visible canvas")
}

#[test]
fn canvas_starts_inactive_and_activates_on_click() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    settle(&mut app, &ctx);
    assert_eq!(app.active_canvas, None);
    assert!(!shown(&app, "add-canvas-left"));
    assert!(!shown(&app, "add-canvas"));
    assert!(!shown(&app, "remove-canvas"));
    let (index, pos) = visible_canvas_hit(&app);
    click_at(&mut app, &ctx, pos);
    settle(&mut app, &ctx);
    assert_eq!(app.active_canvas, Some(index));
    assert!(shown(&app, "add-canvas-left"));
    assert!(shown(&app, "add-canvas"));
    assert!(shown(&app, "remove-canvas"));
    let view = app.view_rect.expect("view");
    let strip = app.strip_rect.expect("strip");
    let outside = egui::pos2(view.min.x + 8.0, strip.min.y - 12.0);
    assert!(view.contains(outside));
    assert!(!strip.contains(outside));
    click_at(&mut app, &ctx, outside);
    settle(&mut app, &ctx);
    assert_eq!(app.active_canvas, None);
    assert!(!shown(&app, "add-canvas"));
}

#[test]
fn adding_beside_keeps_the_active_canvas() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    settle(&mut app, &ctx);
    let (_, pos) = visible_canvas_hit(&app);
    click_at(&mut app, &ctx, pos);
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "add-canvas");
    click(&mut app, &ctx, "add-canvas");
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").canvas_count, 3);
    app.active_canvas = Some(2);
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "add-canvas-left");
    assert_eq!(app.layout().expect("layout").canvas_count, 4);
    assert_eq!(app.active_canvas, Some(3));
    app.active_canvas = Some(2);
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "add-canvas");
    assert_eq!(app.layout().expect("layout").canvas_count, 5);
    assert_eq!(app.active_canvas, Some(2));
}

#[test]
fn delete_removes_the_active_canvas() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    settle(&mut app, &ctx);
    let (_, pos) = visible_canvas_hit(&app);
    click_at(&mut app, &ctx, pos);
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "add-canvas");
    settle(&mut app, &ctx);
    app.active_canvas = Some(1);
    settle(&mut app, &ctx);
    tap(&mut app, &ctx, egui::Key::Delete);
    assert!(matches!(
        &app.confirm,
        Some(super::Confirm::RemoveCanvases(indices)) if indices == &vec![1]
    ));
    app.confirm = None;
    app.active_canvas = None;
    tap(&mut app, &ctx, egui::Key::Delete);
    assert!(matches!(app.confirm, Some(super::Confirm::RemoveFrame(_))));
}

#[test]
fn home_offers_a_template_and_an_import() {
    let (ctx, mut app) = ready();
    settle(&mut app, &ctx);
    assert!(shown(&app, "nav-carousels"));
    assert!(shown(&app, "nav-templates"));
    assert!(shown(&app, "import-carousel"));
    assert!(shown(&app, "browse-templates"));
    assert!(!shown(&app, "new-template"));
    click(&mut app, &ctx, "nav-templates");
    settle(&mut app, &ctx);
    assert!(shown(&app, "new-template"));
    assert!(!shown(&app, "import-carousel"));
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
    for name in ["home", "export", "share"] {
        assert!(shown(&app, name), "{name} missing");
    }
    for name in ["canvas-1:1", "canvas-4:5", "canvas-3:4", "canvas-9:16"] {
        assert!(shown(&app, name), "{name} missing");
    }
    click(&mut app, &ctx, "rail-tool-frame");
    settle(&mut app, &ctx);
    for name in [
        "grid",
        "snap",
        "grid-squares",
        "add-frame",
        "edit-frame",
        "remove-frame",
        "frame-aspect-1:1",
        "frame-aspect-4:5",
        "frame-aspect-3:4",
        "frame-aspect-9:16",
    ] {
        assert!(shown(&app, name), "{name} missing");
    }
    click(&mut app, &ctx, "rail-tool-photo");
    settle(&mut app, &ctx);
    for name in ["from-link", "rotate-ccw", "rotate-cw", "rotate-free"] {
        assert!(shown(&app, name), "{name} missing");
    }
    assert!(shown(&app, "rail-tool-snapshots"));
}

#[test]
fn add_frame_lands_on_the_active_canvas_and_undo_removes_it() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    settle(&mut app, &ctx);
    let (_, pos) = visible_canvas_hit(&app);
    click_at(&mut app, &ctx, pos);
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "add-canvas");
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").canvas_count, 2);
    assert_eq!(app.active_canvas, Some(0));
    click(&mut app, &ctx, "rail-tool-frame");
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "add-frame");
    settle(&mut app, &ctx);
    let layout = app.layout().expect("layout");
    assert_eq!(layout.frames.len(), 2);
    let added = layout.frames.iter().max_by_key(|frame| frame.id.0).unwrap();
    assert!((added.x).abs() < 1e-4);
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
        .add_frame(0);
    app.enter(Route::Template(id), &ctx);
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").frames.len(), 1);
    assert_eq!(plus_names(&app), Vec::<String>::new());
    assert!(!shown(&app, "share"));
    click(&mut app, &ctx, "rail-tool-photo");
    settle(&mut app, &ctx);
    assert!(!shown(&app, "from-link"));
    assert!(!shown(&app, "rotate-ccw"));
    click(&mut app, &ctx, "rail-tool-frame");
    settle(&mut app, &ctx);
    assert!(shown(&app, "add-frame"));
}

#[test]
fn creating_a_template_does_not_save_a_carousel() {
    let (ctx, mut app) = ready();
    assert!(app.library.carousels.is_empty());
    app.create_name = "Blank".to_string();
    app.create_ratio = AspectRatio::FOUR_FIVE;
    app.create_template(&ctx);
    settle(&mut app, &ctx);
    assert!(matches!(app.route, Route::Template(_)));
    assert_eq!(app.library.templates.len(), 1);
    assert!(app.library.carousels.is_empty());
    app.go_home();
    settle(&mut app, &ctx);
    assert!(matches!(app.route, Route::Home));
    assert_eq!(app.library.templates.len(), 1);
    assert!(app.library.carousels.is_empty());
}

#[test]
fn settings_keep_export_format_and_bindings() {
    let (ctx, mut app) = ready();
    app.settings.format = pudduscroll_core::ExportFormat::Jpeg;
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
