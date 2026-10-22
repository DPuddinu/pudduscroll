use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use image::RgbaImage;
use image::imageops::{FilterType, resize};
use pudduscroll_core::{
    AspectRatio, CANVAS_PRESETS, CROP_PRESETS, Crop, CropPreset, Document, ExportError,
    ExportSettings, Handle, ImageMap, LayerMove, MAX_CANVASES, Photo, PhotoId, Placement,
    begin_crop_rect, canvas_file_name, crop_lock_ratio, estimate_total_bytes, export_canvases,
    export_width, move_crop_inside, quality_description, render_canvas, resize_crop_inside,
    write_canvases,
};

const BG: egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const PANEL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const CANVAS_BG: egui::Color32 = egui::Color32::from_rgb(244, 241, 236);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(232, 160, 96);
const TEXT: egui::Color32 = egui::Color32::from_rgb(240, 236, 230);
const MUTED: egui::Color32 = egui::Color32::from_rgb(164, 158, 148);
const SELECT_BG: egui::Color32 = egui::Color32::from_rgb(48, 38, 30);
const MIN_EDITOR_ZOOM: f32 = 0.25;
const MAX_EDITOR_ZOOM: f32 = 4.0;
const EDITOR_ZOOM_SPEED: f32 = 1.0 / 200.0;
const UNDO_LIMIT: usize = 50;
const CROP_HANDLE_HIT: f32 = 12.0;

enum Drag {
    Move {
        id: PhotoId,
        grab_x: f32,
        grab_y: f32,
    },
    Resize {
        id: PhotoId,
        handle: Handle,
        origin: Placement,
    },
}

struct CropSession {
    id: PhotoId,
    preset: CropPreset,
    crop: Crop,
    drag: Option<CropGesture>,
}

#[derive(Clone, Copy)]
enum CropGesture {
    Move { grab_x: f32, grab_y: f32 },
    Resize { handle: Handle, origin: Crop },
}

struct EstimateMsg {
    geometry: u64,
    quality: u8,
    result: Result<u64, String>,
}

struct PhotoAsset {
    image: Arc<RgbaImage>,
    texture: egui::TextureHandle,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "editor flags stay independent"
)]
pub struct PudduScrollApp {
    doc: Document,
    assets: HashMap<PhotoId, PhotoAsset>,
    selected: Option<PhotoId>,
    quality: u8,
    preview_index: usize,
    preview_texture: Option<egui::TextureHandle>,
    preview_key: Option<(u64, usize)>,
    geometry_revision: u64,
    drag: Option<Drag>,
    status: String,
    estimate_mb: Option<f64>,
    estimate_for: Option<(u64, u8)>,
    estimate_after: Option<Instant>,
    estimate_rx: Option<Receiver<EstimateMsg>>,
    export_rx: Option<Receiver<Result<String, String>>>,
    exporting: bool,
    export_open: bool,
    editor_zoom: f32,
    zoom_basis: (u32, u32),
    strip_lead: egui::Vec2,
    pending_scroll: Option<egui::Vec2>,
    strip_wheel_pan: bool,
    center_strip: bool,
    strip_rect: Option<egui::Rect>,
    view_rect: Option<egui::Rect>,
    undo: Vec<Document>,
    crop_session: Option<CropSession>,
    crop_press: Option<egui::Pos2>,
}

impl PudduScrollApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        let mut app = Self {
            doc: Document::new(AspectRatio::FOUR_FIVE),
            assets: HashMap::new(),
            selected: None,
            quality: 90,
            preview_index: 0,
            preview_texture: None,
            preview_key: None,
            geometry_revision: 0,
            drag: None,
            status: String::new(),
            estimate_mb: None,
            estimate_for: None,
            estimate_after: None,
            estimate_rx: None,
            export_rx: None,
            exporting: false,
            export_open: false,
            editor_zoom: 1.0,
            zoom_basis: (4, 5),
            strip_lead: egui::Vec2::ZERO,
            pending_scroll: None,
            strip_wheel_pan: false,
            center_strip: true,
            strip_rect: None,
            view_rect: None,
            undo: Vec::new(),
            crop_session: None,
            crop_press: None,
        };
        for arg in std::env::args().skip(1) {
            let path = std::path::PathBuf::from(arg);
            if path.is_file() {
                app.import_path(&cc.egui_ctx, &path);
            }
        }
        app
    }

    fn schedule_estimate(&mut self) {
        self.estimate_after = Some(Instant::now() + Duration::from_millis(280));
    }

    fn note_geometry(&mut self) {
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        if self.export_open {
            self.schedule_estimate();
        }
    }

    fn image_map(&self) -> ImageMap {
        self.assets
            .iter()
            .map(|(id, asset)| (*id, Arc::clone(&asset.image)))
            .collect()
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        let estimate_msg = match self
            .estimate_rx
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv)
        {
            Some(Ok(msg)) => Some(msg),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.estimate_rx = None;
                None
            }
            _ => None,
        };
        if estimate_msg.is_some() {
            self.estimate_rx = None;
        }
        if let Some(msg) = estimate_msg {
            if msg.geometry == self.geometry_revision && msg.quality == self.quality {
                match msg.result {
                    Ok(bytes) => {
                        self.estimate_mb = Some(bytes as f64 / 1_048_576.0);
                        self.estimate_for = Some((msg.geometry, msg.quality));
                    }
                    Err(error) => {
                        self.status = error;
                        self.estimate_mb = None;
                    }
                }
            }
        }
        let estimate_current = self.estimate_for == Some((self.geometry_revision, self.quality));
        if self.export_open && self.estimate_rx.is_none() && !estimate_current {
            match self.estimate_after {
                Some(at) if Instant::now() >= at => self.spawn_estimate(),
                Some(at) => {
                    ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
                }
                None => {
                    self.schedule_estimate();
                    ctx.request_repaint_after(Duration::from_millis(280));
                }
            }
        }
        let export_msg = match self
            .export_rx
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv)
        {
            Some(Ok(result)) => Some(result),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.export_rx = None;
                self.exporting = false;
                Some(Err("Export stopped before it finished.".to_string()))
            }
            _ => None,
        };
        if let Some(result) = export_msg {
            self.export_rx = None;
            self.exporting = false;
            self.status = match result {
                Ok(message) => message,
                Err(error) => error,
            };
        }
        if self.estimate_rx.is_some() || self.export_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn spawn_estimate(&mut self) {
        self.estimate_after = None;
        let doc = self.doc.clone();
        let images = self.image_map();
        let quality = self.quality;
        let geometry = self.geometry_revision;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = estimate_total_bytes(
                &doc,
                &images,
                ExportSettings {
                    width: export_width(&doc),
                    quality,
                },
            )
            .map_err(|error| error.to_string());
            let _ = tx.send(EstimateMsg {
                geometry,
                quality,
                result,
            });
        });
        self.estimate_rx = Some(rx);
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.crop_session.is_some() && !self.export_open {
            let (enter, escape) = ctx.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                )
            });
            if enter {
                self.commit_crop_session();
                return;
            }
            if escape {
                self.crop_session = None;
                self.status = "Crop canceled".to_string();
                return;
            }
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Delete)) {
            if let Some(id) = self.selected.take() {
                if self
                    .crop_session
                    .as_ref()
                    .is_some_and(|session| session.id == id)
                {
                    self.crop_session = None;
                }
                self.doc.remove_photo(id);
                self.assets.remove(&id);
                self.drag = None;
                self.status = "Photo removed".to_string();
                self.note_geometry();
            }
        }
        if ctx.input(|input| input.key_pressed(egui::Key::ArrowLeft)) && self.preview_index > 0 {
            self.preview_index -= 1;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::ArrowRight))
            && self.preview_index + 1 < self.doc.canvas_count
        {
            self.preview_index += 1;
        }
    }

    fn import_dialog(&mut self, ctx: &egui::Context) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
            .pick_files()
        else {
            return;
        };
        let mut imported = 0usize;
        for path in paths {
            if self.import_path(ctx, &path) {
                imported += 1;
            }
        }
        if imported > 0 {
            self.status = format!("Imported {imported} photos");
        }
    }

    fn import_path(&mut self, ctx: &egui::Context, path: &Path) -> bool {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.status = format!("Could not read {}: {error}", path.display());
                return false;
            }
        };
        let decoded = match image::load_from_memory(&bytes) {
            Ok(image) => image.to_rgba8(),
            Err(error) => {
                self.status = format!("Could not decode {}: {error}", path.display());
                return false;
            }
        };
        if decoded.width() == 0 || decoded.height() == 0 {
            self.status = format!("{} has no pixels", path.display());
            return false;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("photo")
            .to_string();
        let id = self.doc.add_photo(&name, decoded.width(), decoded.height());
        let preview = editor_preview(&decoded);
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [preview.width() as usize, preview.height() as usize],
            preview.as_raw(),
        );
        let texture = ctx.load_texture(&name, color, egui::TextureOptions::LINEAR);
        self.assets.insert(
            id,
            PhotoAsset {
                image: Arc::new(decoded),
                texture,
            },
        );
        self.selected = Some(id);
        self.note_geometry();
        true
    }

    fn start_export(&mut self) {
        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let doc = self.doc.clone();
        let images = self.image_map();
        let quality = self.quality;
        let (tx, rx) = mpsc::channel();
        self.exporting = true;
        self.status = "Exporting…".to_string();
        thread::spawn(move || {
            let outcome = export_folder(&doc, &images, quality, &dir);
            let _ = tx.send(outcome);
        });
        self.export_rx = Some(rx);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("PudduScroll").strong().size(20.0));
            ui.separator();
            for preset in CANVAS_PRESETS {
                let selected = self.doc.ratio == preset;
                if ui.selectable_label(selected, preset.label()).clicked() {
                    self.doc.set_ratio(preset);
                    self.note_geometry();
                }
            }
            let (export_w, export_h) = self.doc.ratio.export_size(export_width(&self.doc));
            ui.label(
                egui::RichText::new(format!("{export_w}×{export_h}"))
                    .color(MUTED)
                    .size(13.0),
            );
            ui.separator();
            ui.label("Canvases");
            if ui
                .add_enabled(self.doc.canvas_count > 1, egui::Button::new("Remove"))
                .clicked()
            {
                self.doc.remove_canvas();
                self.note_geometry();
            }
            ui.monospace(format!("{:02} / {MAX_CANVASES:02}", self.doc.canvas_count));
            if ui
                .add_enabled(
                    self.doc.canvas_count < MAX_CANVASES,
                    egui::Button::new("Add"),
                )
                .clicked()
            {
                self.doc.add_canvas();
                self.note_geometry();
            }
            ui.separator();
            if ui.button("Import photos").clicked() {
                self.import_dialog(ui.ctx());
            }
            if ui
                .add_enabled(
                    !self.exporting && !self.doc.photos.is_empty(),
                    egui::Button::new("Export"),
                )
                .clicked()
            {
                self.export_open = true;
                self.schedule_estimate();
            }
            if !self.status.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(&self.status).color(MUTED));
            }
        });
        ui.add_space(4.0);
    }

    fn export_modal(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::unique("export-options"))
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(16)
                    .corner_radius(8)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 58, 58))),
            )
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("Export");
                ui.add_space(4.0);
                let (width, height) = self.doc.ratio.export_size(export_width(&self.doc));
                let sample = canvas_file_name(0, self.quality);
                ui.label(format!(
                    "{} canvases · {width}×{height}",
                    self.doc.canvas_count
                ));
                ui.label(egui::RichText::new(format!("Files {sample} and on")).color(MUTED));
                ui.add_space(8.0);
                ui.label("Quality");
                let slider = ui.add(
                    egui::Slider::new(&mut self.quality, 0..=100)
                        .show_value(false)
                        .trailing_fill(true),
                );
                if slider.changed() {
                    self.schedule_estimate();
                }
                ui.label(quality_description(self.quality));
                if self.quality >= 100 {
                    ui.label(
                        egui::RichText::new("No extra loss beyond the source. PNG.").color(MUTED),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("Stronger compression toward the left.").color(MUTED),
                    );
                }
                ui.add_space(4.0);
                if self.estimate_for == Some((self.geometry_revision, self.quality)) {
                    if let Some(mb) = self.estimate_mb {
                        ui.label(
                            egui::RichText::new(format!("About {mb:.2} MB for the carousel"))
                                .strong(),
                        );
                    }
                } else {
                    ui.label(egui::RichText::new("Estimating size…").color(MUTED));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.export_open = false;
                    }
                    let confirm = ui.add_enabled(
                        !self.exporting,
                        egui::Button::new(
                            egui::RichText::new("Choose folder").color(egui::Color32::BLACK),
                        )
                        .fill(ACCENT),
                    );
                    if confirm.clicked() {
                        self.export_open = false;
                        self.start_export();
                    }
                });
            });
        if response.should_close() {
            self.export_open = false;
        }
    }

    fn layer_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Layers");
        ui.label(egui::RichText::new(format!("{} photos", self.doc.photos.len())).color(MUTED));
        ui.add_space(4.0);
        let rows: Vec<(PhotoId, String, Crop)> = self
            .doc
            .photos_front_to_back()
            .into_iter()
            .map(|photo| (photo.id, photo.name.clone(), photo.crop))
            .collect();
        if rows.is_empty() {
            ui.label("No photos yet. Import to begin.");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            let count = rows.len();
            for (index, (id, name, crop)) in rows.iter().enumerate() {
                let id = *id;
                let selected = self.selected == Some(id);
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 48.0),
                    egui::Sense::click(),
                );
                if response.clicked() {
                    self.selected = Some(id);
                }
                {
                    let painter = ui.painter();
                    if selected {
                        painter.rect_filled(row, 6.0, SELECT_BG);
                    }
                    let thumb = egui::Rect::from_min_size(
                        row.min + egui::vec2(6.0, 4.0),
                        egui::vec2(40.0, 40.0),
                    );
                    painter.rect_filled(thumb, 2.0, egui::Color32::from_rgb(36, 36, 36));
                    if let Some(asset) = self.assets.get(&id) {
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(crop.x, crop.y),
                            egui::pos2(crop.x + crop.w, crop.y + crop.h),
                        );
                        painter.image(asset.texture.id(), thumb, uv, egui::Color32::WHITE);
                    }
                    painter.text(
                        row.min + egui::vec2(54.0, 15.0),
                        egui::Align2::LEFT_TOP,
                        short_name(name),
                        egui::FontId::proportional(14.0),
                        if selected { ACCENT } else { TEXT },
                    )
                };
                let pointer_on_row = ui.input(|input| {
                    input
                        .pointer
                        .latest_pos()
                        .is_some_and(|pos| row.contains(pos))
                });
                if ui.rect_contains_pointer(row) || pointer_on_row {
                    let button = 22.0;
                    let gap = 3.0;
                    let cluster_w = button * 4.0 + gap * 3.0;
                    let origin_x = row.right() - 4.0 - cluster_w;
                    let cover = egui::Rect::from_min_max(
                        egui::pos2(origin_x - 6.0, row.top()),
                        row.right_bottom(),
                    );
                    ui.painter().rect_filled(
                        cover,
                        egui::CornerRadius {
                            nw: 0,
                            ne: 6,
                            sw: 0,
                            se: 6,
                        },
                        if selected { SELECT_BG } else { PANEL },
                    );
                    let moves = [
                        (Glyph::Up, LayerMove::Up, index > 0),
                        (Glyph::Down, LayerMove::Down, index + 1 < count),
                        (Glyph::Top, LayerMove::Top, index > 0),
                        (Glyph::Bottom, LayerMove::Bottom, index + 1 < count),
                    ];
                    let y = row.center().y - button * 0.5;
                    for (slot, (glyph, movement, enabled)) in moves.into_iter().enumerate() {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(origin_x + (button + gap) * slot as f32, y),
                            egui::vec2(button, button),
                        );
                        if icon_button(ui, rect, ("layer-move", id, slot), enabled, glyph).clicked()
                            && enabled
                        {
                            self.doc.move_layer(id, movement);
                            self.note_geometry();
                        }
                    }
                }
            }
        });
        ui.add_space(8.0);
    }

    fn crop_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Crop");
        let Some(id) = self.selected else {
            ui.label("Select a photo to crop it.");
            return;
        };
        let Some(photo) = self.doc.photo(id).cloned() else {
            ui.label("Select a photo to crop it.");
            return;
        };
        ui.label(short_name(&photo.name));
        let shown = self
            .crop_session
            .as_ref()
            .filter(|session| session.id == id)
            .map_or(photo.crop, |session| session.crop);
        let ratio = shown.pixel_aspect(photo.image_width, photo.image_height);
        ui.label(egui::RichText::new(format!("Visible {ratio:.2}")).color(MUTED));
        ui.add_space(4.0);
        {
            let mut scroll = egui::style::ScrollStyle::solid();
            scroll.bar_width = 8.0;
            scroll.bar_inner_margin = 0.0;
            scroll.bar_outer_margin = 0.0;
            ui.spacing_mut().scroll = scroll;
        };
        egui::ScrollArea::vertical()
            .id_salt("crop-list")
            .auto_shrink([false, false])
            .content_margin(0.0)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                for preset in CROP_PRESETS {
                    let armed = self
                        .crop_session
                        .as_ref()
                        .filter(|session| session.id == id)
                        .map(|session| session.preset);
                    let selected = armed.unwrap_or(photo.preset) == preset;
                    let preset_response = ui.selectable_label(selected, preset.label());
                    if preset == CropPreset::Square {
                        let _ = std::fs::write(
                            r"C:\Users\dario\AppData\Local\Temp\pudduscroll-square.txt",
                            format!("{:?}", preset_response.rect),
                        );
                    }
                    if preset_response.clicked() {
                        self.open_crop_session(id, preset);
                        ui.ctx().request_repaint();
                    }
                }
            });
        ui.add_space(8.0);
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Preview");
            ui.label(
                egui::RichText::new(format!(
                    "{}/{}",
                    self.preview_index + 1,
                    self.doc.canvas_count
                ))
                .size(16.0),
            );
        });
        let area = ui.available_rect_before_wrap();
        if !area.is_positive() {
            return;
        }
        ui.allocate_rect(area, egui::Sense::hover());
        let slots = preview_slots(area, self.doc.ratio.value());
        let can_prev = self.preview_index > 0;
        if icon_button(ui, slots.left, "preview-prev", can_prev, Glyph::Left).clicked()
            && self.preview_index > 0
        {
            self.preview_index -= 1;
        }
        self.draw_phone(ui, slots.phone);
        let can_next = self.preview_index + 1 < self.doc.canvas_count;
        if icon_button(ui, slots.right, "preview-next", can_next, Glyph::Right).clicked()
            && self.preview_index + 1 < self.doc.canvas_count
        {
            self.preview_index += 1;
        }
    }

    fn draw_phone(&mut self, ui: &mut egui::Ui, bezel: egui::Rect) {
        self.ensure_preview(ui.ctx());
        let screen_w = (bezel.width() - 28.0).max(1.0);
        let screen_h = (bezel.height() - 40.0).max(1.0);
        let painter = ui.painter();
        painter.rect_filled(bezel, 24.0, egui::Color32::from_rgb(10, 10, 10));
        painter.circle_filled(
            egui::pos2(bezel.center().x, bezel.top() + 14.0),
            4.0,
            egui::Color32::from_rgb(48, 48, 48),
        );
        let screen = egui::Rect::from_center_size(
            bezel.center() + egui::vec2(0.0, 6.0),
            egui::vec2(screen_w, screen_h),
        );
        painter.rect_filled(screen, 3.0, egui::Color32::WHITE);
        if let Some(texture) = &self.preview_texture {
            painter.image(
                texture.id(),
                screen,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    fn ensure_preview(&mut self, ctx: &egui::Context) {
        if self.doc.canvas_count == 0 {
            return;
        }
        if self.preview_index >= self.doc.canvas_count {
            self.preview_index = self.doc.canvas_count - 1;
        }
        let key = (self.geometry_revision, self.preview_index);
        if self.preview_key == Some(key) {
            return;
        }
        if ctx.input(|input| input.pointer.any_down()) && self.preview_texture.is_some() {
            ctx.request_repaint();
            return;
        }
        let rendered = render_canvas(&self.doc, self.preview_index, &self.image_map(), 640);
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [rendered.width() as usize, rendered.height() as usize],
            rendered.as_raw(),
        );
        self.preview_texture = Some(ctx.load_texture(
            format!("preview-{}-{}", key.0, key.1),
            color,
            egui::TextureOptions::LINEAR,
        ));
        self.preview_key = Some(key);
    }

    fn paint_rail(&mut self, ui: &mut egui::Ui, rail_rect: egui::Rect) {
        if !rail_rect.is_positive() {
            return;
        }
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("rail"));
        ui.ctx().memory_mut(|mem| {
            mem.areas_mut().set_state(
                layer,
                egui::AreaState {
                    pivot_pos: Some(rail_rect.left_top()),
                    pivot: egui::Align2::LEFT_TOP,
                    size: Some(rail_rect.size()),
                    interactable: true,
                    last_became_visible_at: None,
                },
            );
        });
        let mut rail_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-ui")
                .max_rect(rail_rect)
                .layer_id(layer)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        rail_ui.set_clip_rect(rail_rect);
        rail_ui.painter().rect_filled(rail_rect, 0.0, PANEL);
        let inner = egui::Rect::from_min_max(
            egui::pos2(rail_rect.left() + 12.0, rail_rect.top() + 8.0),
            egui::pos2(rail_rect.right(), rail_rect.bottom() - 8.0),
        );
        let mut content = rail_ui.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-content")
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        content.set_clip_rect(inner);
        let (preview_h, crop_h, layers_h) = section_heights(content.available_height());
        rail_section(&mut content, preview_h, "preview", false, |ui| {
            self.preview_panel(ui);
        });
        rail_section(&mut content, crop_h, "crop", true, |ui| self.crop_panel(ui));
        rail_section(&mut content, layers_h, "layers", true, |ui| {
            self.layer_panel(ui);
        });
    }

    fn sync_zoom_basis(&mut self) {
        let basis = (self.doc.ratio.width, self.doc.ratio.height);
        if self.zoom_basis == basis {
            return;
        }
        self.zoom_basis = basis;
        self.editor_zoom = 1.0;
        self.strip_lead = egui::Vec2::ZERO;
        self.pending_scroll = Some(egui::Vec2::ZERO);
        self.strip_wheel_pan = false;
        self.center_strip = true;
    }

    fn apply_wheel_zoom(&mut self, ui: &egui::Ui, bounds: egui::Rect) {
        let (pointer, scroll, alt_wheel) = ui.input(|input| {
            let alt_wheel = input.modifiers.alt
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::MouseWheel { modifiers, .. } if modifiers.alt
                    )
                });
            (
                input.pointer.latest_pos(),
                input.smooth_scroll_delta,
                alt_wheel,
            )
        });
        if scroll == egui::Vec2::ZERO {
            self.strip_wheel_pan = false;
            return;
        }
        let Some(strip) = self.strip_rect else {
            return;
        };
        let Some(view) = self.view_rect else {
            return;
        };
        let Some(pointer) = pointer else {
            return;
        };
        if !strip.contains(pointer) {
            return;
        }
        if ui
            .layer_id_at(pointer)
            .is_some_and(|layer| layer != ui.layer_id())
        {
            return;
        }
        ui.input_mut(|input| {
            input.smooth_scroll_delta = egui::Vec2::ZERO;
        });
        if alt_wheel {
            self.strip_wheel_pan = true;
        }
        if self.strip_wheel_pan {
            let id = ui.make_persistent_id(egui::IdSalt::new("canvas-strip"));
            let mut offset = egui::scroll_area::State::load(ui.ctx(), id)
                .map_or(egui::Vec2::ZERO, |state| state.clamped_offset());
            offset.x = (offset.x - (scroll.x + scroll.y)).max(0.0);
            self.pending_scroll = Some(offset);
            self.center_strip = false;
            return;
        }
        let zoom = next_editor_zoom(self.editor_zoom, scroll.y);
        if (zoom - self.editor_zoom).abs() < 1.0e-4 {
            return;
        }
        let fx = ((pointer.x - strip.min.x) / strip.width().max(1.0)).clamp(0.0, 1.0);
        let fy = ((pointer.y - strip.min.y) / strip.height().max(1.0)).clamp(0.0, 1.0);
        self.editor_zoom = zoom;
        let new_size =
            editor_strip_size(bounds, zoom, self.doc.ratio.value(), self.doc.canvas_count);
        let desired = pointer - egui::vec2(fx * new_size.x, fy * new_size.y);
        let (lead, offset) = place_strip(desired, new_size, view);
        self.strip_lead = lead;
        self.pending_scroll = Some(offset);
        self.center_strip = false;
    }

    fn editor(&mut self, ui: &mut egui::Ui, bounds: egui::Rect) {
        let bounds = bounds.intersect(ui.clip_rect());
        ui.shrink_clip_rect(bounds);
        if self.doc.photos.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Import photos to start a carousel").size(18.0));
                if ui.button("Import photos").clicked() {
                    self.import_dialog(ui.ctx());
                }
                ui.add_space(8.0);
            });
        }
        self.sync_zoom_basis();
        self.apply_wheel_zoom(ui, bounds);
        let size = editor_strip_size(
            bounds,
            self.editor_zoom,
            self.doc.ratio.value(),
            self.doc.canvas_count,
        );
        let mut scroll_source = egui::scroll_area::ScrollSource::SCROLL_BAR;
        if self.crop_session.is_none() && !self.claims_photo_pointer(ui.ctx(), bounds) {
            scroll_source |= egui::scroll_area::ScrollSource::DRAG;
        }
        let mut area = egui::ScrollArea::both()
            .id_salt("canvas-strip")
            .auto_shrink([false, false])
            .scroll_source(scroll_source);
        if let Some(offset) = self.pending_scroll.take() {
            area = area.scroll_offset(offset);
        }
        let output = area.show(ui, |ui| {
            ui.shrink_clip_rect(bounds);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let view = ui.available_size();
            if self.center_strip {
                self.strip_lead.x = ((view.x - size.x) * 0.5).max(0.0);
                self.strip_lead.y = 0.0;
                self.center_strip = false;
            } else {
                self.strip_lead.x = self.strip_lead.x.clamp(0.0, (view.x - size.x).max(0.0));
                self.strip_lead.y = self.strip_lead.y.clamp(0.0, (view.y - size.y).max(0.0));
            }
            ui.add_space(self.strip_lead.y);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.add_space(self.strip_lead.x);
                self.paint_strip(ui, size, bounds);
            });
        });
        self.view_rect = Some(output.inner_rect);
    }

    fn paint_strip(&mut self, ui: &mut egui::Ui, size: egui::Vec2, bounds: egui::Rect) {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        self.strip_rect = Some(rect);
        let ctx = ui.ctx().clone();
        self.handle_strip(&ctx, rect, bounds);
        let clip = bounds.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(clip);
        self.draw_strip(&painter, rect, clip);
        self.paint_crop_handles(&ctx, rect);
        self.paint_photo_selection(&ctx, rect);
        self.strip_cursor(&response, rect);
    }

    fn handle_strip(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: egui::Rect) {
        if self.crop_session.is_some() {
            self.drag_crop(ctx, rect, editor);
            return;
        }
        self.drag_photo(ctx, rect, editor);
    }

    fn claims_photo_pointer(&self, ctx: &egui::Context, editor: egui::Rect) -> bool {
        if self.crop_session.is_some() || self.export_open {
            return false;
        }
        if self.drag.is_some() {
            return true;
        }
        let Some(strip) = self.strip_rect else {
            return false;
        };
        let (down, pos) = ctx.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.primary_down(),
                pointer
                    .press_origin()
                    .or(pointer.interact_pos())
                    .or(pointer.latest_pos()),
            )
        });
        let Some(pos) = pos else {
            return false;
        };
        down && editor.contains(pos) && self.hits_photo_or_handle(strip, pos)
    }

    fn hits_photo_or_handle(&self, rect: egui::Rect, pos: egui::Pos2) -> bool {
        self.gesture_at(rect, pos).is_some()
    }

    fn gesture_at(&self, rect: egui::Rect, pos: egui::Pos2) -> Option<Drag> {
        let ratio = self.doc.ratio.value();
        if let Some(id) = self.selected {
            if let Some(photo) = self.doc.photo(id) {
                let screen = photo_screen(rect, self.doc.canvas_count, photo, ratio);
                if let Some(handle) = handle_at(screen, pos) {
                    let origin = photo.placement(ratio);
                    return Some(Drag::Resize { id, handle, origin });
                }
            }
        }
        let (nx, ny) = screen_to_norm(rect, self.doc.canvas_count, pos);
        let id = self.doc.photo_at(nx, ny)?;
        let photo = self.doc.photo(id)?;
        Some(Drag::Move {
            id,
            grab_x: nx - photo.x,
            grab_y: ny - photo.y,
        })
    }

    fn drag_photo(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: egui::Rect) {
        if self.export_open {
            return;
        }
        let (pressed, down, released, press, interact, latest) = ctx.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.primary_pressed(),
                pointer.primary_down(),
                pointer.primary_released(),
                pointer.press_origin(),
                pointer.interact_pos(),
                pointer.latest_pos(),
            )
        });
        if self.drag.is_none() && pressed {
            if let Some(origin) = press.or(interact).or(latest) {
                if editor.contains(origin)
                    && (self.hits_photo_or_handle(rect, origin) || rect.contains(origin))
                {
                    self.begin_drag(rect, origin);
                }
            }
        }
        if self.drag.is_some() && (down || released) {
            if let Some(pos) = interact.or(latest).or(press) {
                self.update_drag(rect, pos);
            }
        }
        match self.drag {
            Some(Drag::Move { .. }) => {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                if down {
                    ctx.set_dragged_id(egui::Id::unique("photo-gesture"));
                }
            }
            Some(Drag::Resize { handle, .. }) => {
                ctx.set_cursor_icon(cursor_for(handle));
                if down {
                    ctx.set_dragged_id(egui::Id::unique("photo-gesture"));
                }
            }
            None => {
                if let Some(pos) = latest.or(interact).or(press) {
                    if editor.contains(pos) {
                        if let Some(icon) = self.cursor_at(rect, pos) {
                            ctx.set_cursor_icon(icon);
                        }
                    }
                }
            }
        }
        if released && self.drag.take().is_some() {
            self.note_geometry();
            ctx.stop_dragging();
        }
    }

    fn begin_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let drag = self.gesture_at(rect, pos);
        if let Some(Drag::Move { id, .. }) = &drag {
            self.selected = Some(*id);
        }
        if drag.is_some() {
            self.drag = drag;
        } else {
            self.selected = None;
            self.drag = None;
        }
    }

    fn update_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let (nx, ny) = screen_to_norm(rect, self.doc.canvas_count, pos);
        match self.drag {
            Some(Drag::Move { id, grab_x, grab_y }) => {
                let width = self.doc.photo(id).map_or(0.2, |photo| photo.w);
                self.doc.set_box(id, nx - grab_x, ny - grab_y, width);
            }
            Some(Drag::Resize { id, handle, origin }) => {
                self.doc.resize_photo(id, handle, nx, ny, origin);
            }
            None => {}
        }
    }

    fn draw_strip(&self, painter: &egui::Painter, rect: egui::Rect, clip: egui::Rect) {
        let count = self.doc.canvas_count.max(1);
        let canvas_w = rect.width() / count as f32;
        let ratio = self.doc.ratio.value();
        for index in 0..count {
            let canvas = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + index as f32 * canvas_w, rect.min.y),
                egui::vec2(canvas_w, rect.height()),
            );
            let visible = canvas.intersect(clip);
            if !visible.is_positive() {
                continue;
            }
            painter.rect_filled(visible, 0.0, CANVAS_BG);
            if visible.contains(canvas.left_top() + egui::vec2(8.0, 6.0)) {
                painter.text(
                    canvas.left_top() + egui::vec2(8.0, 6.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}", index + 1),
                    egui::FontId::proportional(13.0),
                    egui::Color32::from_black_alpha(110),
                );
            }
        }
        let mut photos: Vec<&Photo> = self.doc.photos.iter().collect();
        photos.sort_by_key(|photo| photo.z);
        let session_id = self.crop_session.as_ref().map(|session| session.id);
        for photo in photos {
            if session_id == Some(photo.id) {
                continue;
            }
            let Some(asset) = self.assets.get(&photo.id) else {
                continue;
            };
            let screen = photo_screen(rect, count, photo, ratio);
            let shadow = screen.translate(egui::vec2(4.0, 5.0)).intersect(clip);
            if shadow.is_positive() {
                painter.rect_filled(shadow, 2.0, egui::Color32::from_black_alpha(36));
            }
            let uv = egui::Rect::from_min_max(
                egui::pos2(photo.crop.x, photo.crop.y),
                egui::pos2(photo.crop.x + photo.crop.w, photo.crop.y + photo.crop.h),
            );
            paint_clipped_image(painter, screen, clip, uv, asset.texture.id());
        }
        for index in 1..count {
            let x = rect.min.x + index as f32 * canvas_w;
            if x < clip.left() || x > clip.right() {
                continue;
            }
            let y0 = rect.top().max(clip.top());
            let y1 = rect.bottom().min(clip.bottom());
            painter.line_segment(
                [egui::pos2(x, y0), egui::pos2(x, y1)],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(18, 18, 18)),
            );
        }
        if self.crop_session.is_some() {
            self.paint_session_crop(painter, rect, clip);
        }
    }

    fn paint_photo_selection(&self, ctx: &egui::Context, strip: egui::Rect) {
        if self.crop_session.is_some() {
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        let Some(photo) = self.doc.photo(id) else {
            return;
        };
        let screen = photo_screen(strip, self.doc.canvas_count, photo, self.doc.ratio.value());
        if !screen.is_positive() {
            return;
        }
        let layer =
            egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("photo-selection"));
        let mut painter = ctx.layer_painter(layer);
        painter.set_clip_rect(ctx.viewport_rect());
        painter.rect_stroke(
            screen,
            0.0,
            egui::Stroke::new(2.0, ACCENT),
            egui::StrokeKind::Inside,
        );
        for (_, pos) in handle_points(screen) {
            let handle = egui::Rect::from_center_size(pos, egui::vec2(9.0, 9.0));
            painter.rect_filled(handle, 2.0, egui::Color32::WHITE);
            painter.rect_stroke(
                handle,
                2.0,
                egui::Stroke::new(1.5, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn strip_cursor(&self, response: &egui::Response, rect: egui::Rect) {
        if self.crop_session.is_some() {
            if let Some(icon) = self.crop_cursor(response, rect) {
                response.clone().on_hover_cursor(icon);
            }
            return;
        }
        let icon = match self.drag {
            Some(Drag::Move { .. }) => Some(egui::CursorIcon::Grabbing),
            Some(Drag::Resize { handle, .. }) => Some(cursor_for(handle)),
            None => self.hover_cursor(response, rect),
        };
        if let Some(icon) = icon {
            response.clone().on_hover_cursor(icon);
        }
    }

    fn hover_cursor(
        &self,
        response: &egui::Response,
        rect: egui::Rect,
    ) -> Option<egui::CursorIcon> {
        let pos = response
            .hover_pos()
            .or_else(|| response.ctx.input(|input| input.pointer.hover_pos()))?;
        self.cursor_at(rect, pos)
    }

    fn cursor_at(&self, rect: egui::Rect, pos: egui::Pos2) -> Option<egui::CursorIcon> {
        let ratio = self.doc.ratio.value();
        if let Some(id) = self.selected {
            if let Some(photo) = self.doc.photo(id) {
                let screen = photo_screen(rect, self.doc.canvas_count, photo, ratio);
                if let Some(handle) = handle_at(screen, pos) {
                    return Some(cursor_for(handle));
                }
            }
        }
        let (nx, ny) = screen_to_norm(rect, self.doc.canvas_count, pos);
        self.doc.photo_at(nx, ny).map(|_| egui::CursorIcon::Grab)
    }

    fn open_crop_session(&mut self, id: PhotoId, preset: CropPreset) {
        let Some(photo) = self.doc.photo(id) else {
            return;
        };
        let image_w = photo.image_width;
        let image_h = photo.image_height;
        let current = photo.crop;
        if let Some(session) = self.crop_session.as_mut() {
            if session.id == id {
                if session.preset != preset {
                    session.preset = preset;
                    session.crop = begin_crop_rect(current, preset, image_w, image_h);
                    session.drag = None;
                }
                return;
            }
        }
        self.drag = None;
        self.status = "Cropping · Enter or click off the photo applies · Esc cancels".to_string();
        self.crop_session = Some(CropSession {
            id,
            preset,
            crop: begin_crop_rect(current, preset, image_w, image_h),
            drag: None,
        });
    }

    fn commit_crop_session(&mut self) {
        let Some(session) = self.crop_session.take() else {
            return;
        };
        let Some(photo) = self.doc.photo(session.id) else {
            return;
        };
        if photo.preset == session.preset && crop_unchanged(photo.crop, session.crop) {
            return;
        }
        self.undo.push(self.doc.clone());
        if self.undo.len() > UNDO_LIMIT {
            let extra = self.undo.len() - UNDO_LIMIT;
            self.undo.drain(0..extra);
        }
        let id = session.id;
        let preset = session.preset;
        self.doc.set_crop(id, session.crop);
        if let Some(photo) = self.doc.photos.iter_mut().find(|photo| photo.id == id) {
            photo.preset = preset;
        }
        self.note_geometry();
        self.status = "Crop updated".to_string();
    }

    fn drag_crop(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: egui::Rect) {
        if self.export_open {
            return;
        }
        let Some(id) = self.crop_session.as_ref().map(|session| session.id) else {
            return;
        };
        let Some(photo) = self.doc.photo(id) else {
            self.crop_session = None;
            self.crop_press = None;
            return;
        };
        let image_w = photo.image_width;
        let image_h = photo.image_height;
        let image_rect =
            full_image_screen(rect, self.doc.canvas_count, photo, self.doc.ratio.value());
        let preset = self
            .crop_session
            .as_ref()
            .map_or(CropPreset::Freehand, |session| session.preset);
        let lock = crop_lock_ratio(preset, image_w, image_h);
        if !image_rect.is_positive() {
            return;
        }
        let crop_now = self
            .crop_session
            .as_ref()
            .map_or_else(Crop::full, |session| session.crop);
        let crop_rect = crop_to_screen(image_rect, crop_now);
        let (pressed, down, released, press, interact, latest) = ctx.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.primary_pressed(),
                pointer.primary_down(),
                pointer.primary_released(),
                pointer.press_origin(),
                pointer.interact_pos(),
                pointer.latest_pos(),
            )
        });
        let mut gesture = self.crop_session.as_ref().and_then(|session| session.drag);
        if gesture.is_none() && pressed {
            let origin = press.or(interact).or(latest);
            if let Some(origin) = origin {
                if let Some(handle) = crop_handle_at(crop_rect, origin) {
                    gesture = Some(CropGesture::Resize {
                        handle,
                        origin: crop_now,
                    });
                } else if editor.contains(origin) && crop_rect.contains(origin) {
                    let (nx, ny) = image_to_norm(image_rect, origin);
                    gesture = Some(CropGesture::Move {
                        grab_x: nx - crop_now.x,
                        grab_y: ny - crop_now.y,
                    });
                }
            }
        }
        if gesture.is_none() && down {
            if let Some(origin) = press.or(interact).or(latest) {
                if let Some(handle) = crop_handle_at(crop_rect, origin) {
                    gesture = Some(CropGesture::Resize {
                        handle,
                        origin: crop_now,
                    });
                }
            }
        }
        if let Some(session) = self.crop_session.as_mut() {
            session.drag = gesture;
            let pos = interact.or(latest).or(press);
            match gesture {
                Some(CropGesture::Resize { handle, origin }) => {
                    if let Some(pos) = pos {
                        let (nx, ny) = image_to_norm(image_rect, pos);
                        session.crop = resize_crop_inside(origin, handle, nx, ny, lock);
                    }
                    ctx.set_cursor_icon(cursor_for(handle));
                    if down {
                        ctx.set_dragged_id(egui::Id::unique("crop-gesture"));
                    }
                }
                Some(CropGesture::Move { grab_x, grab_y }) => {
                    if let Some(pos) = pos {
                        let (nx, ny) = image_to_norm(image_rect, pos);
                        session.crop = move_crop_inside(session.crop, nx - grab_x, ny - grab_y);
                    }
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    if down {
                        ctx.set_dragged_id(egui::Id::unique("crop-gesture"));
                    }
                }
                None => {
                    if let Some(pos) = latest.or(interact) {
                        if let Some(handle) = crop_handle_at(crop_rect, pos) {
                            ctx.set_cursor_icon(cursor_for(handle));
                        }
                    }
                }
            }
            if released {
                if gesture.is_some() {
                    ctx.stop_dragging();
                }
                session.drag = None;
            }
        }
    }

    fn paint_session_crop(&self, painter: &egui::Painter, strip: egui::Rect, clip: egui::Rect) {
        let Some(session) = self.crop_session.as_ref() else {
            return;
        };
        let Some(photo) = self.doc.photo(session.id) else {
            return;
        };
        let Some(texture) = self.assets.get(&session.id).map(|asset| asset.texture.id()) else {
            return;
        };
        let image = full_image_screen(strip, self.doc.canvas_count, photo, self.doc.ratio.value());
        if !image.is_positive() {
            return;
        }
        let crop_rect = crop_to_screen(image, session.crop);
        let shadow = image.translate(egui::vec2(4.0, 5.0)).intersect(clip);
        if shadow.is_positive() {
            painter.rect_filled(shadow, 2.0, egui::Color32::from_black_alpha(36));
        }
        let full_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        paint_clipped_image(painter, image, clip, full_uv, texture);
        paint_crop_veil(painter, image, crop_rect);
        let inside_uv = egui::Rect::from_min_max(
            egui::pos2(session.crop.x, session.crop.y),
            egui::pos2(
                session.crop.x + session.crop.w,
                session.crop.y + session.crop.h,
            ),
        );
        paint_clipped_image(painter, crop_rect, clip, inside_uv, texture);
        painter.rect_stroke(
            image,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 70, 70)),
            egui::StrokeKind::Outside,
        );
        painter.rect_stroke(
            crop_rect,
            0.0,
            egui::Stroke::new(2.0, egui::Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }

    fn paint_crop_handles(&self, ctx: &egui::Context, strip: egui::Rect) {
        let Some(crop_rect) = self.session_crop_screen(strip) else {
            return;
        };
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("crop-handles"));
        let mut painter = ctx.layer_painter(layer);
        painter.set_clip_rect(ctx.viewport_rect());
        for (_, pos) in handle_points(crop_rect) {
            let handle = egui::Rect::from_center_size(pos, egui::vec2(11.0, 11.0));
            painter.rect_filled(handle, 2.0, egui::Color32::WHITE);
            painter.rect_stroke(
                handle,
                2.0,
                egui::Stroke::new(1.5, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn crop_cursor(&self, response: &egui::Response, rect: egui::Rect) -> Option<egui::CursorIcon> {
        let gesture = self.crop_session.as_ref().and_then(|session| session.drag);
        match gesture {
            Some(CropGesture::Move { .. }) => return Some(egui::CursorIcon::Grabbing),
            Some(CropGesture::Resize { handle, .. }) => return Some(cursor_for(handle)),
            None => {}
        }
        let pos = response
            .hover_pos()
            .or_else(|| response.ctx.input(|input| input.pointer.hover_pos()))?;
        let crop_rect = self.session_crop_screen(rect)?;
        if let Some(handle) = crop_handle_at(crop_rect, pos) {
            return Some(cursor_for(handle));
        }
        if crop_rect.contains(pos) {
            return Some(egui::CursorIcon::Grab);
        }
        None
    }

    fn session_image_screen(&self, strip: egui::Rect) -> Option<egui::Rect> {
        let id = self.crop_session.as_ref()?.id;
        let photo = self.doc.photo(id)?;
        let image = full_image_screen(strip, self.doc.canvas_count, photo, self.doc.ratio.value());
        image.is_positive().then_some(image)
    }

    fn session_crop_screen(&self, strip: egui::Rect) -> Option<egui::Rect> {
        let image = self.session_image_screen(strip)?;
        let crop = self.crop_session.as_ref()?.crop;
        Some(crop_to_screen(image, crop))
    }

    fn write_crop_probe(&self, ctx: &egui::Context, editor: egui::Rect) {
        let Some(strip) = self.strip_rect else {
            return;
        };
        let ratio = self.doc.ratio.value();
        let count = self.doc.canvas_count.max(1);
        let canvas_w = strip.width() / count as f32;
        let mut text = format!(
            "ppp={}\nviewport={:?}\neditor={:?}\nstrip={:?}\n",
            ctx.pixels_per_point(),
            ctx.viewport_rect(),
            editor,
            strip
        );
        for index in 0..count {
            let canvas = egui::Rect::from_min_size(
                egui::pos2(strip.min.x + index as f32 * canvas_w, strip.min.y),
                egui::vec2(canvas_w, strip.height()),
            );
            let _ = writeln!(text, "canvas {index} {canvas:?}");
        }
        for photo in &self.doc.photos {
            let screen = photo_screen(strip, count, photo, ratio);
            let _ = writeln!(
                text,
                "photo {} x={} y={} w={} crop={:?} screen={:?}",
                photo.id.0, photo.x, photo.y, photo.w, photo.crop, screen
            );
        }
        let (press, interact, latest, scroll) = ctx.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.press_origin(),
                pointer.interact_pos(),
                pointer.latest_pos(),
                input.smooth_scroll_delta,
            )
        });
        let _ = writeln!(
            text,
            "live zoom={} pointer press={press:?} interact={interact:?} latest={latest:?} scroll={scroll:?}",
            self.editor_zoom
        );
        if let Some(session) = &self.crop_session {
            let screen = self.session_crop_screen(strip);
            let _ = writeln!(
                text,
                "session {:?} crop={:?} screen={:?}",
                session.preset, session.crop, screen
            );
            if let Some(crop_rect) = screen {
                let canvases: Vec<egui::Rect> = (0..count)
                    .map(|index| {
                        egui::Rect::from_min_size(
                            egui::pos2(strip.min.x + index as f32 * canvas_w, strip.min.y),
                            egui::vec2(canvas_w, strip.height()),
                        )
                    })
                    .collect();
                for (handle, pos) in handle_points(crop_rect) {
                    let outside = canvases.iter().all(|canvas| !canvas.contains(pos));
                    let hit = crop_handle_target(pos);
                    let _ = writeln!(
                        text,
                        "handle {handle:?} center={pos:?} hit={hit:?} outside_canvas={outside}"
                    );
                }
                let (press, interact, latest) = ctx.input(|input| {
                    let pointer = &input.pointer;
                    (
                        pointer.press_origin(),
                        pointer.interact_pos(),
                        pointer.latest_pos(),
                    )
                });
                let sample = press.or(interact).or(latest);
                let hit = sample.and_then(|pos| crop_handle_at(crop_rect, pos));
                let drag = match session.drag {
                    Some(CropGesture::Move { .. }) => "move".to_string(),
                    Some(CropGesture::Resize { handle, .. }) => format!("resize {handle:?}"),
                    None => "none".to_string(),
                };
                let _ = writeln!(
                    text,
                    "pointer press={press:?} interact={interact:?} latest={latest:?} hit={hit:?} drag={drag} clip_consulted=false"
                );
            }
        }
        let _ = std::fs::write(
            r"C:\Users\dario\AppData\Local\Temp\pudduscroll-crop.txt",
            text,
        );
    }

    fn pointer_hits_crop_photo(&self, pos: egui::Pos2) -> bool {
        let Some(strip) = self.strip_rect else {
            return true;
        };
        let Some(image) = self.session_image_screen(strip) else {
            return true;
        };
        if image.contains(pos) {
            return true;
        }
        self.session_crop_screen(strip)
            .is_some_and(|crop_rect| crop_handle_at(crop_rect, pos).is_some())
    }

    fn apply_crop_click(&mut self, ctx: &egui::Context, editor_rect: egui::Rect) {
        if self.export_open || self.crop_session.is_none() {
            self.crop_press = None;
            return;
        }
        let (down, clicked, origin, latest) = ctx.input(|input| {
            let pointer = &input.pointer;
            let pressed_at = input.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    ..
                } if *button == egui::PointerButton::Primary => Some(*pos),
                _ => None,
            });
            (
                pointer.primary_down(),
                pointer.primary_clicked(),
                pointer.press_origin().or(pressed_at),
                pointer.latest_pos(),
            )
        });
        if let Some(origin) = origin {
            self.crop_press = Some(origin);
        }
        if !clicked {
            if !down {
                self.crop_press = None;
            }
            return;
        }
        let Some(release) = latest else {
            self.crop_press = None;
            return;
        };
        let press = self.crop_press.take().unwrap_or(release);
        if !editor_rect.contains(press) || !editor_rect.contains(release) {
            return;
        }
        if self.pointer_hits_crop_photo(press) || self.pointer_hits_crop_photo(release) {
            return;
        }
        self.commit_crop_session();
        ctx.request_repaint();
    }
}

impl eframe::App for PudduScrollApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_jobs(&ctx);
        self.handle_keys(&ctx);
        let full = ui.max_rect();
        if full.width() < 900.0 || full.height() < 640.0 {
            return;
        }
        let chrome = egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::symmetric(12, 8));
        let toolbar_h = 64.0;
        let rail_w = 320.0;
        egui::Panel::top("toolbar")
            .exact_size(toolbar_h)
            .resizable(false)
            .show_separator_line(false)
            .frame(chrome)
            .show(ui, |ui| self.toolbar(ui));
        let rail_rect = egui::Rect::from_min_max(
            egui::pos2(full.right() - rail_w, full.top() + toolbar_h),
            full.right_bottom(),
        );
        let editor_rect = egui::Rect::from_min_max(
            egui::pos2(full.left(), rail_rect.top()),
            egui::pos2(rail_rect.left(), full.bottom()),
        );
        let mut editor_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("editor")
                .max_rect(editor_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        editor_ui.set_clip_rect(editor_rect);
        editor_ui.painter().rect_filled(editor_rect, 0.0, BG);
        self.editor(&mut editor_ui, editor_rect);
        self.paint_rail(ui, rail_rect);
        self.write_crop_probe(&ctx, editor_rect);
        self.apply_crop_click(&ctx, editor_rect);
        self.export_modal(&ctx);
    }
}

fn crop_unchanged(a: Crop, b: Crop) -> bool {
    (a.x - b.x).abs() < 1.0e-4
        && (a.y - b.y).abs() < 1.0e-4
        && (a.w - b.w).abs() < 1.0e-4
        && (a.h - b.h).abs() < 1.0e-4
}

fn full_image_screen(strip: egui::Rect, count: usize, photo: &Photo, ratio: f32) -> egui::Rect {
    let placed = photo_screen(strip, count, photo, ratio);
    let crop = photo.crop;
    let w = placed.width() / crop.w.max(0.02);
    let h = placed.height() / crop.h.max(0.02);
    egui::Rect::from_min_size(
        egui::pos2(placed.min.x - crop.x * w, placed.min.y - crop.y * h),
        egui::vec2(w, h),
    )
}

fn image_to_norm(image: egui::Rect, pos: egui::Pos2) -> (f32, f32) {
    (
        (pos.x - image.min.x) / image.width().max(1.0),
        (pos.y - image.min.y) / image.height().max(1.0),
    )
}

fn crop_to_screen(image: egui::Rect, crop: Crop) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            image.min.x + crop.x * image.width(),
            image.min.y + crop.y * image.height(),
        ),
        egui::pos2(
            image.min.x + (crop.x + crop.w) * image.width(),
            image.min.y + (crop.y + crop.h) * image.height(),
        ),
    )
}

fn paint_crop_veil(painter: &egui::Painter, image: egui::Rect, crop: egui::Rect) {
    let veil = egui::Color32::from_black_alpha(140);
    let crop = crop.intersect(image);
    if !crop.is_positive() {
        painter.rect_filled(image, 0.0, veil);
        return;
    }
    let regions = [
        egui::Rect::from_min_max(image.left_top(), egui::pos2(image.right(), crop.top())),
        egui::Rect::from_min_max(
            egui::pos2(image.left(), crop.bottom()),
            image.right_bottom(),
        ),
        egui::Rect::from_min_max(egui::pos2(image.left(), crop.top()), crop.left_bottom()),
        egui::Rect::from_min_max(crop.right_top(), egui::pos2(image.right(), crop.bottom())),
    ];
    for region in regions {
        if region.is_positive() {
            painter.rect_filled(region, 0.0, veil);
        }
    }
}

fn section_heights(total: f32) -> (f32, f32, f32) {
    let mut preview_h = (total * 0.36).clamp(180.0, 300.0);
    let mut crop_h = (total * 0.32).clamp(160.0, 270.0);
    let min_layers = 120.0;
    if preview_h + crop_h + min_layers > total {
        let room = (total - min_layers).max(0.0);
        let sum = (preview_h + crop_h).max(1.0);
        preview_h = room * (preview_h / sum);
        crop_h = room - preview_h;
    }
    let layers_h = (total - preview_h - crop_h).max(0.0);
    (preview_h, crop_h, layers_h)
}

fn rail_section(
    ui: &mut egui::Ui,
    height: f32,
    id: &str,
    divider: bool,
    add: impl FnOnce(&mut egui::Ui),
) {
    let width = ui.available_width();
    let (_space_id, rect) = ui.allocate_space(egui::vec2(width, height.max(0.0)));
    let clip = rect.intersect(ui.clip_rect());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(clip);
    if divider && clip.height() > 1.0 {
        child.painter().hline(
            clip.x_range(),
            clip.top(),
            egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 48, 48)),
        );
    }
    add(&mut child);
}

fn paint_clipped_image(
    painter: &egui::Painter,
    screen: egui::Rect,
    clip: egui::Rect,
    uv: egui::Rect,
    texture: egui::TextureId,
) {
    let visible = screen.intersect(clip);
    if !visible.is_positive() || screen.width() <= 0.0 || screen.height() <= 0.0 {
        return;
    }
    let sx = uv.width() / screen.width();
    let sy = uv.height() / screen.height();
    let clipped_uv = egui::Rect::from_min_max(
        egui::pos2(
            uv.min.x + (visible.min.x - screen.min.x) * sx,
            uv.min.y + (visible.min.y - screen.min.y) * sy,
        ),
        egui::pos2(
            uv.min.x + (visible.max.x - screen.min.x) * sx,
            uv.min.y + (visible.max.y - screen.min.y) * sy,
        ),
    );
    painter.image(texture, visible, clipped_uv, egui::Color32::WHITE);
}

fn export_folder(
    doc: &Document,
    images: &ImageMap,
    quality: u8,
    dir: &Path,
) -> Result<String, String> {
    let encoded = export_canvases(
        doc,
        images,
        ExportSettings {
            width: export_width(doc),
            quality,
        },
    )
    .map_err(|error| error.to_string())?;
    let count = encoded.len();
    write_canvases(dir, &encoded).map_err(|error: ExportError| error.to_string())?;
    let sample = canvas_file_name(0, quality);
    Ok(format!(
        "Exported {count} images to {} ({sample})",
        dir.display()
    ))
}

fn editor_preview(src: &RgbaImage) -> RgbaImage {
    let max_side = 1800u32;
    let long = src.width().max(src.height());
    if long <= max_side {
        return src.clone();
    }
    let scale = max_side as f32 / long as f32;
    let width = ((src.width() as f32 * scale).round() as u32).max(1);
    let height = ((src.height() as f32 * scale).round() as u32).max(1);
    resize(src, width, height, FilterType::Triangle)
}

fn next_editor_zoom(current: f32, scroll_y: f32) -> f32 {
    (current * (scroll_y * EDITOR_ZOOM_SPEED).exp()).clamp(MIN_EDITOR_ZOOM, MAX_EDITOR_ZOOM)
}

fn editor_strip_size(bounds: egui::Rect, zoom: f32, ratio: f32, count: usize) -> egui::Vec2 {
    let fit_h = (bounds.height() - 36.0).max(120.0);
    let canvas_h = fit_h * zoom;
    let canvas_w = canvas_h * ratio;
    egui::vec2(canvas_w * count.max(1) as f32, canvas_h)
}

fn place_strip(
    desired: egui::Pos2,
    size: egui::Vec2,
    view: egui::Rect,
) -> (egui::Vec2, egui::Vec2) {
    let mut lead = egui::Vec2::ZERO;
    let mut scroll = egui::Vec2::ZERO;
    let want = [desired.x, desired.y];
    let len = [size.x, size.y];
    let origin = [view.min.x, view.min.y];
    let view_len = [view.width(), view.height()];
    for axis in 0..2 {
        if len[axis] <= view_len[axis] {
            let slack = (view_len[axis] - len[axis]).max(0.0);
            lead[axis] = (want[axis] - origin[axis]).clamp(0.0, slack);
        } else {
            scroll[axis] = (origin[axis] - want[axis]).clamp(0.0, len[axis] - view_len[axis]);
        }
    }
    (lead, scroll)
}

fn screen_to_norm(strip: egui::Rect, count: usize, pos: egui::Pos2) -> (f32, f32) {
    let canvas_w = strip.width() / count.max(1) as f32;
    let x = (pos.x - strip.min.x) / canvas_w.max(1.0);
    let y = (pos.y - strip.min.y) / strip.height().max(1.0);
    (x, y)
}

fn photo_screen(strip: egui::Rect, count: usize, photo: &Photo, ratio: f32) -> egui::Rect {
    let height = photo.height(ratio);
    let canvas_w = strip.width() / count.max(1) as f32;
    egui::Rect::from_min_max(
        egui::pos2(
            strip.min.x + photo.x * canvas_w,
            strip.min.y + photo.y * strip.height(),
        ),
        egui::pos2(
            strip.min.x + (photo.x + photo.w) * canvas_w,
            strip.min.y + (photo.y + height) * strip.height(),
        ),
    )
}

fn handle_points(rect: egui::Rect) -> [(Handle, egui::Pos2); 8] {
    let center = rect.center();
    [
        (Handle::Nw, rect.left_top()),
        (Handle::Ne, rect.right_top()),
        (Handle::Sw, rect.left_bottom()),
        (Handle::Se, rect.right_bottom()),
        (Handle::N, egui::pos2(center.x, rect.top())),
        (Handle::S, egui::pos2(center.x, rect.bottom())),
        (Handle::W, egui::pos2(rect.left(), center.y)),
        (Handle::E, egui::pos2(rect.right(), center.y)),
    ]
}

fn handle_target(point: egui::Pos2) -> egui::Rect {
    egui::Rect::from_center_size(point, egui::vec2(20.0, 20.0))
}

fn handle_at(rect: egui::Rect, pos: egui::Pos2) -> Option<Handle> {
    handle_points(rect)
        .into_iter()
        .find(|(_, point)| handle_target(*point).contains(pos))
        .map(|(handle, _)| handle)
}

fn crop_handle_target(point: egui::Pos2) -> egui::Rect {
    egui::Rect::from_center_size(
        point,
        egui::vec2(CROP_HANDLE_HIT * 2.0, CROP_HANDLE_HIT * 2.0),
    )
}

fn crop_handle_at(rect: egui::Rect, pos: egui::Pos2) -> Option<Handle> {
    handle_points(rect)
        .into_iter()
        .find(|(_, point)| crop_handle_target(*point).contains(pos))
        .map(|(handle, _)| handle)
}

fn cursor_for(handle: Handle) -> egui::CursorIcon {
    match handle {
        Handle::N | Handle::S => egui::CursorIcon::ResizeVertical,
        Handle::E | Handle::W => egui::CursorIcon::ResizeHorizontal,
        Handle::Ne | Handle::Sw => egui::CursorIcon::ResizeNeSw,
        Handle::Nw | Handle::Se => egui::CursorIcon::ResizeNwSe,
    }
}

fn short_name(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if index == 28 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

#[derive(Clone, Copy)]
enum Glyph {
    Left,
    Right,
    Up,
    Down,
    Top,
    Bottom,
}

struct PreviewSlots {
    left: egui::Rect,
    phone: egui::Rect,
    right: egui::Rect,
}

fn preview_slots(area: egui::Rect, ratio: f32) -> PreviewSlots {
    let button = 34.0;
    let gap = 10.0;
    let ratio = ratio.max(0.05);
    let max_bezel_h = area.height().max(48.0);
    let max_bezel_w = (area.width() - (button + gap) * 2.0).max(48.0);
    let mut screen_h = (max_bezel_h - 40.0).clamp(36.0, 168.0);
    let mut screen_w = screen_h * ratio;
    let max_screen_w = (max_bezel_w - 28.0).max(36.0);
    if screen_w > max_screen_w {
        screen_w = max_screen_w;
        screen_h = screen_w / ratio;
    }
    let phone =
        egui::Rect::from_center_size(area.center(), egui::vec2(screen_w + 28.0, screen_h + 40.0));
    let left = egui::Rect::from_center_size(
        egui::pos2(phone.left() - gap - button * 0.5, phone.center().y),
        egui::vec2(button, button),
    );
    let right = egui::Rect::from_center_size(
        egui::pos2(phone.right() + gap + button * 0.5, phone.center().y),
        egui::vec2(button, button),
    );
    PreviewSlots { left, phone, right }
}

fn icon_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
    enabled: bool,
    glyph: Glyph,
) -> egui::Response {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(salt).max_rect(rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
    ));
    child.set_clip_rect(rect.expand(1.0).intersect(child.clip_rect()));
    child.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
    let icon = egui::Atom::paint(rect.size() * 0.56, move |ui, args| {
        paint_glyph(
            ui.painter(),
            args.rect,
            glyph,
            icon_ink(args.fallback_text_color),
        );
    });
    child.add_enabled(
        enabled,
        egui::Button::new(icon)
            .min_size(rect.size())
            .corner_radius(egui::CornerRadius::same(6)),
    )
}

fn icon_ink(color: egui::Color32) -> egui::Color32 {
    let on_accent = color.r() > 200
        && color.g() > 120
        && color.g() < 210
        && color.b() < 140
        && color.r() > color.b();
    if on_accent {
        egui::Color32::BLACK
    } else {
        color
    }
}

fn paint_glyph(painter: &egui::Painter, rect: egui::Rect, glyph: Glyph, color: egui::Color32) {
    let width = (rect.width().min(rect.height()) * 0.14).clamp(1.4, 2.15);
    let stroke = egui::Stroke::new(width, color);
    match glyph {
        Glyph::Left => chevron(painter, rect, std::f32::consts::PI, stroke),
        Glyph::Right => chevron(painter, rect, 0.0, stroke),
        Glyph::Up => chevron(painter, rect, -std::f32::consts::FRAC_PI_2, stroke),
        Glyph::Down => chevron(painter, rect, std::f32::consts::FRAC_PI_2, stroke),
        Glyph::Top => capped(painter, rect, -std::f32::consts::FRAC_PI_2, stroke),
        Glyph::Bottom => capped(painter, rect, std::f32::consts::FRAC_PI_2, stroke),
    }
}

fn chevron(painter: &egui::Painter, rect: egui::Rect, angle: f32, stroke: egui::Stroke) {
    let reach = rect.width().min(rect.height()) * 0.34;
    chevron_at(painter, rect.center(), angle, reach, stroke);
}

fn chevron_at(
    painter: &egui::Painter,
    center: egui::Pos2,
    angle: f32,
    reach: f32,
    stroke: egui::Stroke,
) {
    let (sin, cos) = angle.sin_cos();
    let point = |x: f32, y: f32| center + egui::vec2(x * cos - y * sin, x * sin + y * cos) * reach;
    painter.line(
        vec![point(-0.65, -1.0), point(1.0, 0.0), point(-0.65, 1.0)],
        stroke,
    );
}

fn capped(painter: &egui::Painter, rect: egui::Rect, angle: f32, stroke: egui::Stroke) {
    let along = egui::vec2(angle.cos(), angle.sin());
    let side = egui::vec2(-along.y, along.x);
    let half = rect.width().min(rect.height()) * 0.42;
    let bar_center = rect.center() + along * half * 0.72;
    let bar_half = side * half * 0.85;
    painter.line(vec![bar_center - bar_half, bar_center + bar_half], stroke);
    let reach = rect.width().min(rect.height()) * 0.24;
    chevron_at(
        painter,
        rect.center() - along * reach * 0.35,
        angle,
        reach,
        stroke,
    );
}

fn apply_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = egui::Color32::from_rgb(12, 12, 12);
    visuals.faint_bg_color = egui::Color32::from_rgb(32, 32, 32);
    visuals.override_text_color = None;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(38, 38, 38);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 46, 42);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(64, 52, 42);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}
