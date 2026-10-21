use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use image::imageops::{resize, FilterType};
use image::RgbaImage;
use pudduscroll_core::{
    canvas_file_name, estimate_total_bytes, export_canvases, quality_description, render_canvas,
    write_canvases, AspectRatio, Crop, CropPreset, Document, ExportError, ExportSettings, Handle,
    ImageMap, LayerMove, Photo, PhotoId, Placement, CANVAS_PRESETS, CROP_PRESETS,
    DEFAULT_EXPORT_WIDTH, MAX_CANVASES,
};

const BG: egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const PANEL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const CANVAS_BG: egui::Color32 = egui::Color32::from_rgb(244, 241, 236);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(232, 160, 96);
const TEXT: egui::Color32 = egui::Color32::from_rgb(240, 236, 230);
const MUTED: egui::Color32 = egui::Color32::from_rgb(164, 158, 148);
const SELECT_BG: egui::Color32 = egui::Color32::from_rgb(48, 38, 30);

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

struct EstimateMsg {
    geometry: u64,
    quality: u8,
    result: Result<u64, String>,
}

struct PhotoAsset {
    image: Arc<RgbaImage>,
    texture: egui::TextureHandle,
}

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
        let estimate_msg = match self.estimate_rx.as_ref().map(|rx| rx.try_recv()) {
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
        let export_msg = match self.export_rx.as_ref().map(|rx| rx.try_recv()) {
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
                    width: DEFAULT_EXPORT_WIDTH,
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
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Delete)) {
            if let Some(id) = self.selected.take() {
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
        let id = self
            .doc
            .add_photo(&name, decoded.width(), decoded.height());
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
                    if ui
                        .selectable_label(selected, preset.label())
                        .clicked()
                    {
                        self.doc.set_ratio(preset);
                        self.note_geometry();
                    }
                }
                let (export_w, export_h) = self.doc.ratio.export_size(DEFAULT_EXPORT_WIDTH);
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
                ui.monospace(format!(
                    "{:02} / {MAX_CANVASES:02}",
                    self.doc.canvas_count
                ));
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
            let (width, height) = self.doc.ratio.export_size(DEFAULT_EXPORT_WIDTH);
            let sample = canvas_file_name(0, self.quality);
            ui.label(format!(
                "{} canvases · {width}×{height}",
                self.doc.canvas_count
            ));
            ui.label(
                egui::RichText::new(format!("Files {sample} and on")).color(MUTED),
            );
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
                    egui::Button::new(egui::RichText::new("Choose folder").color(egui::Color32::BLACK))
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
                ui.label(
                    egui::RichText::new(format!("{} photos", self.doc.photos.len())).color(MUTED),
                );
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
                    for (id, name, crop) in rows {
                        let selected = self.selected == Some(id);
                        let (row, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 48.0),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            self.selected = Some(id);
                        }
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
                            short_name(&name),
                            egui::FontId::proportional(14.0),
                            if selected { ACCENT } else { TEXT },
                        );
                    }
                });
                ui.separator();
                let enabled = self.selected.is_some();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(enabled, egui::Button::new("Up"))
                        .clicked()
                    {
                        if let Some(id) = self.selected {
                            self.doc.move_layer(id, LayerMove::Up);
                            self.note_geometry();
                        }
                    }
                    if ui
                        .add_enabled(enabled, egui::Button::new("Down"))
                        .clicked()
                    {
                        if let Some(id) = self.selected {
                            self.doc.move_layer(id, LayerMove::Down);
                            self.note_geometry();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(enabled, egui::Button::new("Top"))
                        .clicked()
                    {
                        if let Some(id) = self.selected {
                            self.doc.move_layer(id, LayerMove::Top);
                            self.note_geometry();
                        }
                    }
                    if ui
                        .add_enabled(enabled, egui::Button::new("Bottom"))
                        .clicked()
                    {
                        if let Some(id) = self.selected {
                            self.doc.move_layer(id, LayerMove::Bottom);
                            self.note_geometry();
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
                let ratio = photo.crop.pixel_aspect(photo.image_width, photo.image_height);
                ui.label(
                    egui::RichText::new(format!("Visible {ratio:.2}")).color(MUTED),
                );
                ui.add_space(4.0);
                {
                    let mut scroll = egui::style::ScrollStyle::solid();
                    scroll.bar_width = 8.0;
                    scroll.bar_inner_margin = 0.0;
                    scroll.bar_outer_margin = 0.0;
                    ui.spacing_mut().scroll = scroll;
                }
                egui::ScrollArea::vertical()
                    .id_salt("crop-list")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                    for preset in CROP_PRESETS {
                        if ui
                            .selectable_label(photo.preset == preset, preset.label())
                            .clicked()
                        {
                            self.doc.apply_crop_preset(id, preset);
                            self.note_geometry();
                        }
                    }
                    if self.doc.photo(id).is_some_and(|photo| photo.preset == CropPreset::Freehand)
                    {
                        ui.add_space(6.0);
                        let mut crop = self.doc.photo(id).unwrap().crop;
                        let mut changed = false;
                        changed |= ui
                            .add(egui::Slider::new(&mut crop.x, 0.0..=1.0).text("X"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut crop.y, 0.0..=1.0).text("Y"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut crop.w, 0.02..=1.0).text("W"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut crop.h, 0.02..=1.0).text("H"))
                            .changed();
                        if changed {
                            self.doc.set_crop(id, crop);
                            self.note_geometry();
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
                ui.horizontal(|ui| {
                    let can_prev = self.preview_index > 0;
                    if ui
                        .add_enabled(can_prev, egui::Button::new("Previous"))
                        .clicked()
                        && self.preview_index > 0
                    {
                        self.preview_index -= 1;
                    }
                    let can_next = self.preview_index + 1 < self.doc.canvas_count;
                    if ui
                        .add_enabled(can_next, egui::Button::new("Next"))
                        .clicked()
                        && self.preview_index + 1 < self.doc.canvas_count
                    {
                        self.preview_index += 1;
                    }
                });
                ui.vertical_centered(|ui| {
                    self.draw_phone(ui);
                });
    }

    fn draw_phone(&mut self, ui: &mut egui::Ui) {
        self.ensure_preview(ui.ctx());
        let ratio = self.doc.ratio.value();
        let screen_h = (ui.available_height() - 36.0).clamp(72.0, 168.0);
        let screen_w = (screen_h * ratio).max(56.0);
        let (bezel, _) = ui.allocate_exact_size(
            egui::vec2(screen_w + 28.0, screen_h + 40.0),
            egui::Sense::hover(),
        );
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

    fn editor(&mut self, ui: &mut egui::Ui, bounds: egui::Rect) {
            let bounds = bounds.intersect(ui.clip_rect());
            ui.shrink_clip_rect(bounds);
            if self.doc.photos.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Import photos to start a carousel").size(18.0),
                    );
                    if ui.button("Import photos").clicked() {
                        self.import_dialog(ui.ctx());
                    }
                    ui.add_space(8.0);
                });
            }
            let canvas_h = (bounds.height() - 36.0).max(120.0);
            let canvas_w = canvas_h * self.doc.ratio.value();
            let strip_w = canvas_w * self.doc.canvas_count as f32;
            let size = egui::vec2(strip_w, canvas_h);
            let avail_w = bounds.width() - 16.0;
            if strip_w <= avail_w {
                let pad = ((avail_w - strip_w) * 0.5).max(0.0);
                ui.horizontal(|ui| {
                    ui.shrink_clip_rect(bounds);
                    ui.add_space(pad);
                    self.paint_strip(ui, size, bounds);
                });
            } else {
                egui::ScrollArea::horizontal()
                    .scroll_source(
                        egui::scroll_area::ScrollSource::SCROLL_BAR
                            | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
                    )
                    .show(ui, |ui| {
                        ui.shrink_clip_rect(bounds);
                        self.paint_strip(ui, size, bounds);
                    });
            }
    }

    fn paint_strip(&mut self, ui: &mut egui::Ui, size: egui::Vec2, bounds: egui::Rect) {
        let (rect, response) =
            ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        self.handle_strip(&ctx, rect, &response);
        let clip = bounds.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(clip);
        self.draw_strip(&painter, rect, clip);
        self.strip_cursor(&response, rect);
    }

    fn handle_strip(&mut self, ctx: &egui::Context, rect: egui::Rect, response: &egui::Response) {
        if response.drag_started() {
            let origin = ctx
                .input(|input| input.pointer.press_origin())
                .or(response.interact_pointer_pos());
            if let Some(pos) = origin {
                self.begin_drag(rect, pos);
            }
        }
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.update_drag(rect, pos);
            }
        }
        if response.drag_stopped() && self.drag.take().is_some() {
            self.note_geometry();
        }
        if response.clicked() {
            if let Some(pos) = ctx.input(|input| input.pointer.latest_pos()) {
                self.select_at(rect, pos);
            }
        }
    }

    fn begin_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let ratio = self.doc.ratio.value();
        if let Some(id) = self.selected {
            if let Some(photo) = self.doc.photo(id) {
                let screen = photo_screen(rect, self.doc.canvas_count, photo, ratio);
                if let Some(handle) = handle_at(screen, pos) {
                    let origin = photo.placement(ratio);
                    self.drag = Some(Drag::Resize { id, handle, origin });
                    return;
                }
            }
        }
        let (nx, ny) = screen_to_norm(rect, self.doc.canvas_count, pos);
        if let Some(id) = self.doc.photo_at(nx, ny) {
            self.selected = Some(id);
            if let Some(photo) = self.doc.photo(id) {
                self.drag = Some(Drag::Move {
                    id,
                    grab_x: nx - photo.x,
                    grab_y: ny - photo.y,
                });
            }
        } else {
            self.selected = None;
            self.drag = None;
        }
    }

    fn update_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let (nx, ny) = screen_to_norm(rect, self.doc.canvas_count, pos);
        match self.drag {
            Some(Drag::Move { id, grab_x, grab_y }) => {
                let width = self.doc.photo(id).map(|photo| photo.w).unwrap_or(0.2);
                self.doc.set_box(id, nx - grab_x, ny - grab_y, width);
            }
            Some(Drag::Resize { id, handle, origin }) => {
                self.doc.resize_photo(id, handle, nx, ny, origin);
            }
            None => {}
        }
    }

    fn select_at(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let (nx, ny) = screen_to_norm(rect, self.doc.canvas_count, pos);
        self.selected = self.doc.photo_at(nx, ny);
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
        for photo in photos {
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
        if let Some(id) = self.selected {
            if let Some(photo) = self.doc.photo(id) {
                let screen = photo_screen(rect, count, photo, ratio).intersect(clip);
                if screen.is_positive() {
                    painter.rect_stroke(
                        screen,
                        0.0,
                        egui::Stroke::new(2.0, ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                let full = photo_screen(rect, count, photo, ratio);
                for (_, pos) in handle_points(full) {
                    if !clip.contains(pos) {
                        continue;
                    }
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
        }
    }

    fn strip_cursor(&self, response: &egui::Response, rect: egui::Rect) {
        let icon = match self.drag {
            Some(Drag::Move { .. }) => Some(egui::CursorIcon::Grabbing),
            Some(Drag::Resize { handle, .. }) => Some(cursor_for(handle)),
            None => self.hover_cursor(response, rect),
        };
        if let Some(icon) = icon {
            response.clone().on_hover_cursor(icon);
        }
    }

    fn hover_cursor(&self, response: &egui::Response, rect: egui::Rect) -> Option<egui::CursorIcon> {
        let pos = response.hover_pos()?;
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
        self.doc
            .photo_at(nx, ny)
            .map(|_| egui::CursorIcon::Grab)
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
        egui::Panel::top("toolbar")
            .exact_size(64.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(chrome)
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::right("rail")
            .exact_size(320.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(chrome)
            .show(ui, |ui| {
                let (preview_h, crop_h, layers_h) = section_heights(ui.available_height());
                rail_section(ui, preview_h, "preview", false, |ui| self.preview_panel(ui));
                rail_section(ui, crop_h, "crop", true, |ui| self.crop_panel(ui));
                rail_section(ui, layers_h, "layers", true, |ui| self.layer_panel(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ui, |ui| {
                let bounds = ui.max_rect().intersect(ui.clip_rect());
                ui.set_clip_rect(bounds);
                self.editor(ui, bounds);
            });
        self.export_modal(&ctx);
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
            width: DEFAULT_EXPORT_WIDTH,
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

fn handle_at(rect: egui::Rect, pos: egui::Pos2) -> Option<Handle> {
    handle_points(rect)
        .into_iter()
        .find(|(_, point)| point.distance(pos) <= 10.0)
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
