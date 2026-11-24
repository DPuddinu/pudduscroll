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
    paint_view(app, ctx, events, VIEW);
}

fn paint_view(
    app: &mut PudduScrollApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    view: egui::Vec2,
) {
    let mut host = eframe::Frame::_new_kittest();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, view)),
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
    cover_image(8, 8, [10, 20, 30, 255])
}

fn cover_image(width: u32, height: u32, pixel: [u8; 4]) -> crate::library::Blob {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba(pixel));
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
    click_view(app, ctx, VIEW, pos);
}

fn click_view(app: &mut PudduScrollApp, ctx: &egui::Context, view: egui::Vec2, pos: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    paint_view(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), press(true)],
        view,
    );
    paint_view(app, ctx, vec![press(false)], view);
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
    let count = app.layout().expect("layout").canvas_count;
    assert!(shown(&app, "add-canvas-left"));
    click(&mut app, &ctx, "add-canvas-left");
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").canvas_count, count + 1);
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
    assert_eq!(app.layout().expect("layout").canvas_count, count + 1);
    click(&mut app, &ctx, "add-canvas");
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").canvas_count, count + 2);
}

#[test]
fn a_frame_above_the_strip_selects_the_canvas_under_it() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    settle(&mut app, &ctx);
    let id = app.layout().expect("layout").frames[0].id;
    app.layout_mut()
        .expect("layout")
        .set_frame_rect(id, 0.0, -0.45, 1.0, 0.7);
    settle(&mut app, &ctx);
    let strip = app.strip_rect.expect("strip");
    let view = app.view_rect.expect("view");
    let screen = frame_screen(strip, 1, app.layout().expect("layout").frame(id).unwrap());
    let pos = egui::pos2(screen.center().x, strip.top() - 8.0);
    assert!(screen.contains(pos));
    assert!(view.contains(pos));
    assert!(!strip.contains(pos));
    click_at(&mut app, &ctx, pos);
    settle(&mut app, &ctx);
    assert_eq!(app.selected, Some(id));
    assert_eq!(app.active_canvas, Some(0));
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
    app.selected = None;
    settle(&mut app, &ctx);
    tap(&mut app, &ctx, egui::Key::Delete);
    assert!(matches!(
        &app.confirm,
        Some(super::Confirm::RemoveCanvases(indices)) if indices == &vec![1]
    ));
    app.confirm = None;
    let id = app.layout().expect("layout").frames[0].id;
    app.selected = Some(id);
    settle(&mut app, &ctx);
    tap(&mut app, &ctx, egui::Key::Delete);
    assert!(matches!(
        app.confirm,
        Some(super::Confirm::RemoveFrame(frame)) if frame == id
    ));
}

#[test]
fn template_cards_share_a_row() {
    let (ctx, mut app) = ready();
    let mut ids = Vec::new();
    for name in ["One", "Two", "Three", "Four"] {
        ids.push(
            app.library
                .create_template(name.to_string(), AspectRatio::FOUR_FIVE, cover()),
        );
    }
    app.home_section = super::HomeSection::Templates;
    settle(&mut app, &ctx);
    let cards: Vec<egui::Rect> = ids
        .iter()
        .map(|id| rect(&app, &format!("template-card-{id}")))
        .collect();
    let top = cards[0].top();
    let height = cards[0].height();
    let stride = cards[1].left() - cards[0].left();
    for (index, card) in cards.iter().enumerate() {
        assert!((card.top() - top).abs() < 0.5, "card {index} top");
        assert!((card.height() - height).abs() < 0.5, "card {index} height");
        if index > 0 {
            let gap = card.left() - cards[index - 1].left();
            assert!((gap - stride).abs() < 0.5, "card {index} stride");
        }
    }
}

#[test]
fn home_offers_a_template_and_an_import() {
    let (ctx, mut app) = ready();
    settle(&mut app, &ctx);
    assert!(shown(&app, "nav-carousels"));
    assert!(shown(&app, "nav-templates"));
    assert!(shown(&app, "import-carousel"));
    assert!(shown(&app, "new-carousel"));
    let gap = rect(&app, "new-carousel").left() - rect(&app, "import-carousel").right();
    assert!(gap >= 8.0, "{gap}");
    assert!(shown(&app, "browse-templates"));
    assert!(!shown(&app, "new-template"));
    click(&mut app, &ctx, "nav-templates");
    settle(&mut app, &ctx);
    assert!(shown(&app, "new-template"));
    assert!(!shown(&app, "import-carousel"));
    assert!(!shown(&app, "new-carousel"));
    assert_eq!(plus_names(&app), Vec::<String>::new());
    assert!(!shown(&app, "share"));
}

#[test]
fn new_carousel_starts_from_one_canvas() {
    let (ctx, mut app) = ready();
    settle(&mut app, &ctx);
    click(&mut app, &ctx, "new-carousel");
    settle(&mut app, &ctx);
    assert!(matches!(app.route, Route::Carousel(_)));
    assert_eq!(app.library.carousels.len(), 1);
    assert_eq!(app.layout().expect("layout").canvas_count, 1);
    assert_eq!(app.layout().expect("layout").frames.len(), 1);
}

#[test]
fn shell_stays_visible_on_a_short_narrow_screen() {
    let (ctx, mut app) = ready();
    let view = egui::vec2(800.0, 520.0);
    paint_view(&mut app, &ctx, Vec::new(), view);
    assert!(shown(&app, "nav-carousels"));
    assert!(shown(&app, "new-carousel"));
    app.new_carousel(&ctx);
    paint_view(&mut app, &ctx, Vec::new(), view);
    assert!(shown(&app, "home"));
    assert!(shown(&app, "export"));
}

#[test]
fn narrow_editor_keeps_the_strip_beside_an_icon_rail() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    paint_view(&mut app, &ctx, Vec::new(), egui::vec2(900.0, 720.0));
    let editor = rect(&app, "editor");
    assert!(editor.width() > 800.0, "{}", editor.width());
    let canvas = rect(&app, "rail-tool-canvas");
    assert!(canvas.left() > editor.right() - 8.0);
    paint_view(&mut app, &ctx, Vec::new(), egui::vec2(1440.0, 900.0));
    let wide = rect(&app, "editor");
    assert!(wide.width() < 1200.0, "{}", wide.width());
    assert!(wide.width() > 1000.0, "{}", wide.width());
    paint_view(&mut app, &ctx, Vec::new(), egui::vec2(390.0, 844.0));
    let view = app.view_rect.expect("view");
    let strip = app.strip_rect.expect("strip");
    assert!(view.intersects(strip));
}

#[test]
fn home_nav_stacks_on_a_narrow_screen() {
    let (ctx, mut app) = ready();
    paint_view(&mut app, &ctx, Vec::new(), egui::vec2(700.0, 800.0));
    let carousels = rect(&app, "nav-carousels");
    let templates = rect(&app, "nav-templates");
    assert!((carousels.top() - templates.top()).abs() < 1.0);
    assert!(templates.left() > carousels.right());
}

fn open_canvas_panel(app: &mut PudduScrollApp, ctx: &egui::Context, view: egui::Vec2) {
    paint_view(app, ctx, Vec::new(), view);
    paint_view(app, ctx, Vec::new(), view);
    click_view(app, ctx, view, rect(app, "rail-tool-canvas").center());
    paint_view(app, ctx, Vec::new(), view);
}

#[test]
fn a_phone_tap_adds_a_canvas_on_either_side() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    let view = egui::vec2(390.0, 844.0);
    paint_view(&mut app, &ctx, Vec::new(), view);
    paint_view(&mut app, &ctx, Vec::new(), view);
    let (_, pos) = visible_canvas_hit(&app);
    click_view(&mut app, &ctx, view, pos);
    open_canvas_panel(&mut app, &ctx, view);
    let left = rect(&app, "add-canvas-left");
    let right = rect(&app, "add-canvas");
    assert!(left.height() >= 18.0, "{left:?}");
    assert!(right.height() >= 18.0, "{right:?}");
    click_view(&mut app, &ctx, view, right.center());
    assert_eq!(app.layout().expect("layout").canvas_count, 2);
    let left_again = rect(&app, "add-canvas-left");
    click_view(&mut app, &ctx, view, left_again.center());
    assert_eq!(app.layout().expect("layout").canvas_count, 3);
}

#[test]
fn a_rotated_phone_keeps_the_canvas_options_readable() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    let view = egui::vec2(844.0, 390.0);
    open_canvas_panel(&mut app, &ctx, view);
    app.active_canvas = Some(0);
    paint_view(&mut app, &ctx, Vec::new(), view);
    let left = rect(&app, "add-canvas-left");
    let right = rect(&app, "add-canvas");
    assert!(
        left.top() < 260.0 && left.bottom() < view.y - 36.0,
        "canvas actions are crushed: {left:?}"
    );
    assert!((left.center().y - right.center().y).abs() < 4.0 || right.top() > left.bottom());
    assert!(left.height() >= 18.0 && right.height() >= 18.0);
    let body = rect(&app, "rail-tool-body");
    assert!(body.height() >= 160.0, "tool pane {body:?}");
    click_view(&mut app, &ctx, view, right.center());
    assert_eq!(app.layout().expect("layout").canvas_count, 2);
}

#[test]
fn space_drag_pans_the_strip_on_a_narrow_screen() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    let view = egui::vec2(390.0, 844.0);
    paint_view(&mut app, &ctx, Vec::new(), view);
    paint_view(&mut app, &ctx, Vec::new(), view);
    let from = app.view_rect.expect("view").center();
    let to = from + egui::vec2(0.0, -80.0);
    let before = app.strip_pan;
    let space = egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    paint_view(
        &mut app,
        &ctx,
        vec![space.clone(), egui::Event::PointerMoved(from)],
        view,
    );
    paint_view(&mut app, &ctx, vec![space.clone(), press(from, true)], view);
    paint_view(
        &mut app,
        &ctx,
        vec![space, egui::Event::PointerMoved(to)],
        view,
    );
    assert!(
        (app.strip_pan.y - before.y).abs() > 40.0,
        "pan {:?} -> {:?}",
        before,
        app.strip_pan
    );
}

#[test]
fn dragging_an_empty_canvas_pans_the_strip() {
    let (ctx, mut app) = ready();
    app.new_carousel(&ctx);
    let view = egui::vec2(390.0, 844.0);
    paint_view(&mut app, &ctx, Vec::new(), view);
    paint_view(&mut app, &ctx, Vec::new(), view);
    app.edit_frame = false;
    let (_, from) = visible_canvas_hit(&app);
    let to = from + egui::vec2(0.0, -80.0);
    let before = app.strip_pan;
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    paint_view(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(from), press(from, true)],
        view,
    );
    paint_view(&mut app, &ctx, vec![egui::Event::PointerMoved(to)], view);
    assert!(
        (app.strip_pan.y - before.y).abs() > 40.0,
        "pan {:?} -> {:?} gesture {} from {:?} view {:?}",
        before,
        app.strip_pan,
        app.gesture,
        from,
        app.view_rect
    );
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
    for name in [
        "canvas-1:1",
        "canvas-4:5",
        "canvas-3:4",
        "canvas-16:9",
        "canvas-9:16",
    ] {
        assert!(shown(&app, name), "{name} missing");
    }
    assert!(shown(&app, "rail-tool-canvas"));
    assert!(!shown(&app, "rail-tool-frame"));
    assert!(shown(&app, "add-canvas-left"));
    assert!(shown(&app, "add-canvas"));
    assert!(shown(&app, "remove-canvas"));
    let canvases = app.layout().expect("layout").canvas_count;
    click(&mut app, &ctx, "add-canvas");
    settle(&mut app, &ctx);
    assert_eq!(app.layout().expect("layout").canvas_count, canvases + 1);
    app.active_canvas = Some(0);
    settle(&mut app, &ctx);
    let preset = rect(&app, "canvas-1:1");
    let count = rect(&app, "canvas-count");
    let left = rect(&app, "add-canvas-left");
    let right = rect(&app, "add-canvas");
    let remove = rect(&app, "remove-canvas");
    assert!((preset.center().y - count.center().y).abs() < 4.0);
    assert!(left.top() - preset.bottom() < 20.0);
    assert!(count.left() > rect(&app, "canvas-9:16").right());
    let row = count.right() - left.left();
    assert!(left.width() < row * 0.7);
    assert!(right.width() < row * 0.7);
    assert!(remove.width() < row * 0.7);
    assert!((left.center().y - right.center().y).abs() < 4.0);
    assert!(right.left() > left.right());
    let row_bottom = left.bottom().max(right.bottom()).max(remove.bottom());
    let after = rect(&app, "add-frame").top() - row_bottom;
    assert!(after < 20.0, "{after}");
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
        "frame-aspect-16:9",
        "frame-aspect-9:16",
        "rotate-frame-ccw",
        "rotate-frame-cw",
        "rotate-frame-free",
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
    click(&mut app, &ctx, "rail-tool-canvas");
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

fn render_home(
    app: &mut PudduScrollApp,
    ctx: &egui::Context,
    raster: &mut pudduscroll_ui_check::Raster,
    view: egui::Vec2,
) -> pudduscroll_ui_check::Image {
    let mut host = eframe::Frame::_new_kittest();
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(1.0),
            ..Default::default()
        },
    );
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, view)),
        viewports,
        time: Some(0.0),
        predicted_dt: 1.0 / 60.0,
        focused: true,
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        app.ui(ui, &mut host);
    });
    raster.render(
        ctx,
        &mut output,
        [view.x.round() as u32, view.y.round() as u32],
    )
}

fn expect_cards(name: &str, image: &pudduscroll_ui_check::Image) -> Result<(), String> {
    let report = pudduscroll_ui_check::standard().run(image);
    if report.ok() && report.items >= 4 {
        return Ok(());
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ui-check")
        .join(format!("{name}.png"));
    image.save_png(&path).expect("png");
    Err(format!("{name} ({} items): {report:#?}", report.items))
}

fn paint_home(
    app: &mut PudduScrollApp,
    ctx: &egui::Context,
    raster: &mut pudduscroll_ui_check::Raster,
    view: egui::Vec2,
) -> pudduscroll_ui_check::Image {
    for _ in 0..12 {
        let _ = render_home(app, ctx, raster, view);
    }
    render_home(app, ctx, raster, view)
}

#[test]
fn home_card_lists_pass_pixel_checks() {
    let (ctx, mut app) = ready();
    for (index, name) in [
        "North", "East", "South", "West", "Up", "Down", "Left", "Right",
    ]
    .into_iter()
    .enumerate()
    {
        let cover = if index % 2 == 0 {
            cover_image(64, 8, [20, 40, 90, 255])
        } else {
            cover_image(32, 32, [180, 50, 40, 255])
        };
        let id = app
            .library
            .create_template(name.to_string(), AspectRatio::FOUR_FIVE, cover);
        let template = app
            .library
            .templates
            .iter_mut()
            .find(|template| template.id == id)
            .expect("template");
        template.layout.set_canvas_count(index + 1);
    }
    for index in 0..8 {
        let id = app.library.create_carousel();
        let carousel = app
            .library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == id)
            .expect("carousel");
        carousel.name = format!("Carousel {index}");
        carousel.layout.set_canvas_count(index + 1);
        let cover = if index % 2 == 0 {
            cover_image(64, 8, [20, 40, 90, 255])
        } else {
            cover_image(32, 32, [180, 50, 40, 255])
        };
        app.library.set_cover(id, cover);
    }
    let mut raster = pudduscroll_ui_check::Raster::default();
    let mut failures = Vec::new();
    for (name, section, view) in [
        (
            "carousels",
            super::HomeSection::Carousels,
            egui::vec2(1440.0, 900.0),
        ),
        (
            "templates",
            super::HomeSection::Templates,
            egui::vec2(1440.0, 900.0),
        ),
        (
            "carousels-narrow",
            super::HomeSection::Carousels,
            egui::vec2(1100.0, 720.0),
        ),
    ] {
        app.home_section = section;
        let image = paint_home(&mut app, &ctx, &mut raster, view);
        if let Err(failure) = expect_cards(name, &image) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
