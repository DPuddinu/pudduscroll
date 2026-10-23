use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use image::RgbaImage;
use image::imageops::{FilterType, overlay, resize};
use pudduscroll_core::{
    AspectRatio, CANVAS_GRID, CANVAS_PRESETS, CROP_PRESETS, Crop, CropPreset, ExportError,
    ExportSettings, Frame, FrameId, FrameImages, Handle, LayerMove, Layout, MAX_CANVASES,
    begin_crop_rect, canvas_file_name, cover_contains_frame, cover_fit, crop_lock_ratio,
    estimate_layout_bytes, export_layout_canvases, layout_export_width, move_crop_inside,
    pan_for_origin, quality_description, render_layout, resize_crop_inside, resize_frame,
    snap_frame_to_grid, snap_resized_frame_to_grid, write_canvases,
};

use crate::library::{Blob, Library, load_blob};

const BG: egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const PANEL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const CANVAS_BG: egui::Color32 = egui::Color32::from_rgb(244, 241, 236);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(232, 160, 96);
const TEXT: egui::Color32 = egui::Color32::from_rgb(240, 236, 230);
const MUTED: egui::Color32 = egui::Color32::from_rgb(164, 158, 148);
const SELECT_BG: egui::Color32 = egui::Color32::from_rgb(48, 38, 30);
const PREVIEW_MAX: u32 = 1800;
const MIN_EDITOR_ZOOM: f32 = 0.25;
const MAX_EDITOR_ZOOM: f32 = 4.0;
const EDITOR_ZOOM_SPEED: f32 = 1.0 / 200.0;
const UNDO_LIMIT: usize = 50;
const CROP_HANDLE_HIT: f32 = 12.0;
const TOOLBAR_H: f32 = 112.0;
const RAIL_W: f32 = 320.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Home,
    Template(u64),
    Carousel(u64),
}

enum HomeAction {
    OpenTemplate(u64),
    EditTemplate(u64),
    DeleteTemplate(u64),
    OpenCarousel(u64),
    DeleteCarousel(u64),
}

#[derive(Clone, Copy)]
enum Drag {
    MoveFrame {
        id: FrameId,
        grab_x: f32,
        grab_y: f32,
    },
    ResizeFrame {
        id: FrameId,
        handle: Handle,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    Pan {
        id: FrameId,
        grab_x: f32,
        grab_y: f32,
    },
}

impl Drag {
    fn id(self) -> FrameId {
        match self {
            Drag::MoveFrame { id, .. } | Drag::ResizeFrame { id, .. } | Drag::Pan { id, .. } => id,
        }
    }
}

struct CropSession {
    id: FrameId,
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

#[derive(Clone, Copy)]
enum DecodeTarget {
    Frame { carousel_id: u64, frame_id: FrameId },
    Cover,
}

struct DecodedPhoto {
    blob: Blob,
    preview: RgbaImage,
}

enum IncomingPhoto {
    Decoded {
        token: u64,
        target: DecodeTarget,
        result: Result<DecodedPhoto, String>,
    },
    Preview {
        token: u64,
        carousel_id: u64,
        frame_id: FrameId,
        preview: RgbaImage,
    },
    CoverPreview {
        token: u64,
        template_id: u64,
        preview: RgbaImage,
    },
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "editor flags stay independent"
)]
pub struct PudduScrollApp {
    library: Library,
    route: Route,
    assets: HashMap<FrameId, PhotoAsset>,
    cover_textures: HashMap<u64, egui::TextureHandle>,
    card_previews: HashMap<u64, (u64, egui::TextureHandle)>,
    selected: Option<FrameId>,
    edit_frame: bool,
    quality: u8,
    preview_index: usize,
    preview_texture: Option<egui::TextureHandle>,
    preview_key: Option<(u64, usize)>,
    geometry_revision: u64,
    drag: Option<Drag>,
    gesture: String,
    status: String,
    estimate_mb: Option<f64>,
    estimate_for: Option<(u64, u8)>,
    estimate_after: Option<Instant>,
    estimate_rx: Option<Receiver<EstimateMsg>>,
    export_rx: Option<Receiver<Result<String, String>>>,
    photo_tx: Sender<IncomingPhoto>,
    photo_rx: Receiver<IncomingPhoto>,
    loading: HashMap<(u64, FrameId), u64>,
    cover_load: Option<u64>,
    cover_pending: HashMap<u64, u64>,
    load_token: u64,
    painted: u64,
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
    undo: Vec<Layout>,
    crop_session: Option<CropSession>,
    crop_press: Option<egui::Pos2>,
    show_grid: bool,
    snap_to_grid: bool,
    create_open: bool,
    create_name: String,
    create_ratio: AspectRatio,
    create_cover: Option<Blob>,
    create_cover_texture: Option<egui::TextureHandle>,
    name_dirty: bool,
    widgets: Vec<(String, egui::Rect)>,
    plus_rects: Vec<(FrameId, egui::Rect)>,
}

impl PudduScrollApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        let (library, status) = match Library::load() {
            Ok(library) => (library, String::new()),
            Err(error) => (Library::empty(), error),
        };
        let (photo_tx, photo_rx) = mpsc::channel();
        Self {
            library,
            route: Route::Home,
            assets: HashMap::new(),
            cover_textures: HashMap::new(),
            card_previews: HashMap::new(),
            selected: None,
            edit_frame: false,
            quality: 90,
            preview_index: 0,
            preview_texture: None,
            preview_key: None,
            geometry_revision: 0,
            drag: None,
            gesture: "none".to_string(),
            status,
            estimate_mb: None,
            estimate_for: None,
            estimate_after: None,
            estimate_rx: None,
            export_rx: None,
            photo_tx,
            photo_rx,
            loading: HashMap::new(),
            cover_load: None,
            cover_pending: HashMap::new(),
            load_token: 0,
            painted: 0,
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
            show_grid: true,
            snap_to_grid: true,
            create_open: false,
            create_name: "Template".to_string(),
            create_ratio: AspectRatio::FOUR_FIVE,
            create_cover: None,
            create_cover_texture: None,
            name_dirty: false,
            widgets: Vec::new(),
            plus_rects: Vec::new(),
        }
    }

    fn layout(&self) -> Option<&Layout> {
        match self.route {
            Route::Home => None,
            Route::Template(id) => self
                .library
                .templates
                .iter()
                .find(|template| template.id == id)
                .map(|template| &template.layout),
            Route::Carousel(id) => self
                .library
                .carousels
                .iter()
                .find(|carousel| carousel.id == id)
                .map(|carousel| &carousel.layout),
        }
    }

    fn layout_mut(&mut self) -> Option<&mut Layout> {
        match self.route {
            Route::Home => None,
            Route::Template(id) => self
                .library
                .templates
                .iter_mut()
                .find(|template| template.id == id)
                .map(|template| &mut template.layout),
            Route::Carousel(id) => self
                .library
                .carousels
                .iter_mut()
                .find(|carousel| carousel.id == id)
                .map(|carousel| &mut carousel.layout),
        }
    }

    fn metrics(&self) -> Option<(f32, usize)> {
        self.layout()
            .map(|layout| (layout.ratio.value(), layout.canvas_count))
    }

    fn item_name(&self) -> String {
        match self.route {
            Route::Home => String::new(),
            Route::Template(id) => self
                .library
                .templates
                .iter()
                .find(|template| template.id == id)
                .map(|template| template.name.clone())
                .unwrap_or_default(),
            Route::Carousel(id) => self
                .library
                .carousels
                .iter()
                .find(|carousel| carousel.id == id)
                .map(|carousel| carousel.name.clone())
                .unwrap_or_default(),
        }
    }

    fn has_photo(&self) -> bool {
        self.layout()
            .is_some_and(|layout| layout.frames.iter().any(|frame| frame.photo.is_some()))
    }

    fn remember(&mut self, name: impl Into<String>, rect: egui::Rect) {
        self.widgets.push((name.into(), rect));
    }

    fn persist(&mut self) {
        if let Err(error) = self.library.store() {
            self.status = error;
        }
    }

    fn note_geometry(&mut self) {
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.library.visuals = self.library.visuals.wrapping_add(1);
        if self.export_open {
            self.schedule_estimate();
        }
        self.persist();
    }

    fn push_undo(&mut self, layout: Layout) {
        self.undo.push(layout);
        if self.undo.len() > UNDO_LIMIT {
            let extra = self.undo.len() - UNDO_LIMIT;
            self.undo.drain(0..extra);
        }
    }

    fn schedule_estimate(&mut self) {
        self.estimate_after = Some(Instant::now() + Duration::from_millis(280));
    }

    fn image_map(&self) -> FrameImages {
        let Route::Carousel(id) = self.route else {
            return FrameImages::new();
        };
        let Some(carousel) = self
            .library
            .carousels
            .iter()
            .find(|carousel| carousel.id == id)
        else {
            return FrameImages::new();
        };
        carousel
            .photos
            .iter()
            .map(|(frame_id, blob)| (*frame_id, Arc::clone(&blob.image)))
            .collect()
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        loop {
            let message = match self.photo_rx.try_recv() {
                Ok(message) => message,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            self.apply_incoming(ctx, message);
        }
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
        if self.photos_busy() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.estimate_rx.is_some() || self.export_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn photos_busy(&self) -> bool {
        !self.loading.is_empty() || self.cover_load.is_some() || !self.cover_pending.is_empty()
    }

    fn next_token(&mut self) -> u64 {
        self.load_token = self.load_token.wrapping_add(1);
        self.load_token
    }

    fn forget_load(&mut self, id: FrameId) {
        if let Route::Carousel(carousel_id) = self.route {
            self.loading.remove(&(carousel_id, id));
        }
    }

    fn apply_incoming(&mut self, ctx: &egui::Context, message: IncomingPhoto) {
        match message {
            IncomingPhoto::Decoded {
                token,
                target,
                result,
            } => self.apply_decoded(ctx, token, target, result),
            IncomingPhoto::Preview {
                token,
                carousel_id,
                frame_id,
                preview,
            } => self.apply_preview(ctx, token, carousel_id, frame_id, preview),
            IncomingPhoto::CoverPreview {
                token,
                template_id,
                preview,
            } => self.apply_cover_preview(ctx, token, template_id, preview),
        }
    }

    fn apply_decoded(
        &mut self,
        ctx: &egui::Context,
        token: u64,
        target: DecodeTarget,
        result: Result<DecodedPhoto, String>,
    ) {
        match target {
            DecodeTarget::Frame {
                carousel_id,
                frame_id,
            } => {
                if self.loading.get(&(carousel_id, frame_id)) != Some(&token) {
                    return;
                }
                self.loading.remove(&(carousel_id, frame_id));
                match result {
                    Ok(decoded) => self.finish_frame_photo(ctx, carousel_id, frame_id, decoded),
                    Err(error) => self.status = error,
                }
            }
            DecodeTarget::Cover => {
                if self.cover_load != Some(token) {
                    return;
                }
                self.cover_load = None;
                match result {
                    Ok(decoded) => {
                        let name = decoded.blob.name.clone();
                        self.create_cover_texture =
                            Some(texture_from_preview(ctx, "create-cover", &decoded.preview));
                        self.create_cover = Some(decoded.blob);
                        self.status = format!("{name} ready");
                    }
                    Err(error) => self.status = error,
                }
            }
        }
    }

    fn finish_frame_photo(
        &mut self,
        ctx: &egui::Context,
        carousel_id: u64,
        frame_id: FrameId,
        decoded: DecodedPhoto,
    ) {
        let exists = self.library.carousels.iter().any(|carousel| {
            carousel.id == carousel_id && carousel.layout.frame(frame_id).is_some()
        });
        if !exists {
            return;
        }
        let name = decoded.blob.name.clone();
        let image = Arc::clone(&decoded.blob.image);
        self.library.set_photo(carousel_id, frame_id, decoded.blob);
        if self.route == Route::Carousel(carousel_id) {
            self.upload_asset(ctx, frame_id, &name, image, decoded.preview);
            self.selected = Some(frame_id);
        }
        self.status = format!("{name} added");
        self.note_geometry();
    }

    fn apply_preview(
        &mut self,
        ctx: &egui::Context,
        token: u64,
        carousel_id: u64,
        frame_id: FrameId,
        preview: RgbaImage,
    ) {
        if self.loading.get(&(carousel_id, frame_id)) != Some(&token) {
            return;
        }
        self.loading.remove(&(carousel_id, frame_id));
        if self.route != Route::Carousel(carousel_id) {
            return;
        }
        let prepared = self
            .library
            .carousels
            .iter()
            .find(|carousel| carousel.id == carousel_id)
            .and_then(|carousel| carousel.photos.get(&frame_id))
            .map(|blob| (blob.name.clone(), Arc::clone(&blob.image)));
        let Some((name, image)) = prepared else {
            return;
        };
        self.upload_asset(ctx, frame_id, &name, image, preview);
    }

    fn apply_cover_preview(
        &mut self,
        ctx: &egui::Context,
        token: u64,
        template_id: u64,
        preview: RgbaImage,
    ) {
        if self.cover_pending.get(&template_id) != Some(&token) {
            return;
        }
        self.cover_pending.remove(&template_id);
        if self
            .library
            .templates
            .iter()
            .all(|template| template.id != template_id)
        {
            return;
        }
        self.cover_textures.insert(
            template_id,
            texture_from_preview(ctx, format!("cover-{template_id}"), &preview),
        );
    }

    fn spawn_estimate(&mut self) {
        self.estimate_after = None;
        let Some(layout) = self.layout().cloned() else {
            return;
        };
        let images = self.image_map();
        let quality = self.quality;
        let geometry = self.geometry_revision;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = estimate_layout_bytes(
                &layout,
                &images,
                ExportSettings {
                    width: layout_export_width(&layout),
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
            self.delete_selection();
        }
        let Some((_, count)) = self.metrics() else {
            return;
        };
        if ctx.input(|input| input.key_pressed(egui::Key::ArrowLeft)) && self.preview_index > 0 {
            self.preview_index -= 1;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::ArrowRight))
            && self.preview_index + 1 < count
        {
            self.preview_index += 1;
        }
    }

    fn delete_selection(&mut self) {
        let Some(id) = self.selected else {
            return;
        };
        let has_photo = self
            .layout()
            .and_then(|layout| layout.frame(id))
            .is_some_and(|frame| frame.photo.is_some());
        if has_photo {
            self.remove_photo(id);
        } else {
            self.remove_frame(id);
        }
    }

    fn upload_asset(
        &mut self,
        ctx: &egui::Context,
        id: FrameId,
        name: &str,
        image: Arc<RgbaImage>,
        preview: RgbaImage,
    ) {
        let texture = texture_from_preview(ctx, name, &preview);
        self.assets.insert(id, PhotoAsset { image, texture });
    }

    fn reload_assets(&mut self, ctx: &egui::Context) {
        self.assets.clear();
        let Route::Carousel(carousel_id) = self.route else {
            return;
        };
        let Some(carousel) = self
            .library
            .carousels
            .iter()
            .find(|carousel| carousel.id == carousel_id)
        else {
            return;
        };
        let loaded: Vec<(FrameId, String, Arc<RgbaImage>)> = carousel
            .photos
            .iter()
            .map(|(frame_id, blob)| (*frame_id, blob.name.clone(), Arc::clone(&blob.image)))
            .collect();
        for (frame_id, name, image) in loaded {
            if self.loading.contains_key(&(carousel_id, frame_id)) {
                continue;
            }
            if image.width().max(image.height()) > PREVIEW_MAX {
                self.spawn_preview(ctx, carousel_id, frame_id, image);
            } else {
                let preview = editor_preview(&image);
                self.upload_asset(ctx, frame_id, &name, image, preview);
            }
        }
    }

    fn fill_frame(&mut self, ctx: &egui::Context, id: FrameId) {
        let Route::Carousel(carousel_id) = self.route else {
            return;
        };
        if self.loading.contains_key(&(carousel_id, id)) {
            return;
        }
        let Some(path) = pick_image_path("Add photo") else {
            return;
        };
        self.spawn_decode(
            ctx,
            path,
            DecodeTarget::Frame {
                carousel_id,
                frame_id: id,
            },
        );
    }

    fn spawn_decode(&mut self, ctx: &egui::Context, path: PathBuf, target: DecodeTarget) {
        let token = self.next_token();
        match target {
            DecodeTarget::Frame {
                carousel_id,
                frame_id,
            } => {
                self.loading.insert((carousel_id, frame_id), token);
                self.status = "Loading photo…".to_string();
            }
            DecodeTarget::Cover => {
                self.cover_load = Some(token);
                self.status = "Loading cover…".to_string();
            }
        }
        let tx = self.photo_tx.clone();
        thread::spawn(move || {
            let result = load_blob(&path).map(|blob| {
                let preview = editor_preview(&blob.image);
                DecodedPhoto { blob, preview }
            });
            let _ = tx.send(IncomingPhoto::Decoded {
                token,
                target,
                result,
            });
        });
        ctx.request_repaint();
    }

    fn spawn_preview(
        &mut self,
        ctx: &egui::Context,
        carousel_id: u64,
        frame_id: FrameId,
        image: Arc<RgbaImage>,
    ) {
        let token = self.next_token();
        self.loading.insert((carousel_id, frame_id), token);
        let tx = self.photo_tx.clone();
        thread::spawn(move || {
            let preview = editor_preview(&image);
            let _ = tx.send(IncomingPhoto::Preview {
                token,
                carousel_id,
                frame_id,
                preview,
            });
        });
        ctx.request_repaint();
    }

    fn remove_photo(&mut self, id: FrameId) {
        self.forget_load(id);
        if let Route::Carousel(carousel_id) = self.route {
            self.library.clear_photo(carousel_id, id);
        }
        self.assets.remove(&id);
        if self
            .crop_session
            .as_ref()
            .is_some_and(|session| session.id == id)
        {
            self.crop_session = None;
        }
        self.drag = None;
        self.status = "Photo removed".to_string();
        self.note_geometry();
    }

    fn remove_frame(&mut self, id: FrameId) {
        self.forget_load(id);
        if let Route::Carousel(carousel_id) = self.route {
            self.library.clear_photo(carousel_id, id);
        }
        if let Some(layout) = self.layout_mut() {
            layout.remove_frame(id);
        }
        self.assets.remove(&id);
        if self.selected == Some(id) {
            self.selected = None;
        }
        if self
            .crop_session
            .as_ref()
            .is_some_and(|session| session.id == id)
        {
            self.crop_session = None;
        }
        self.drag = None;
        self.edit_frame = false;
        self.status = "Frame removed".to_string();
        self.note_geometry();
    }

    fn enter(&mut self, route: Route, ctx: &egui::Context) {
        self.route = route;
        self.selected = None;
        self.edit_frame = false;
        self.drag = None;
        self.gesture = "none".to_string();
        self.crop_session = None;
        self.crop_press = None;
        self.preview_index = 0;
        self.preview_texture = None;
        self.preview_key = None;
        self.editor_zoom = 1.0;
        self.strip_lead = egui::Vec2::ZERO;
        self.pending_scroll = Some(egui::Vec2::ZERO);
        self.strip_wheel_pan = false;
        self.center_strip = true;
        self.strip_rect = None;
        self.view_rect = None;
        self.undo.clear();
        self.export_open = false;
        self.create_open = false;
        self.show_grid = true;
        self.snap_to_grid = true;
        self.plus_rects.clear();
        if let Some(layout) = self.layout() {
            self.zoom_basis = (layout.ratio.width, layout.ratio.height);
        }
        self.reload_assets(ctx);
    }

    fn go_home(&mut self) {
        self.library.visuals = self.library.visuals.wrapping_add(1);
        self.persist();
        self.route = Route::Home;
        self.drag = None;
        self.crop_session = None;
        self.crop_press = None;
        self.selected = None;
        self.edit_frame = false;
        self.export_open = false;
        self.assets.clear();
        self.plus_rects.clear();
    }

    fn start_export(&mut self) {
        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let Some(layout) = self.layout().cloned() else {
            return;
        };
        let images = self.image_map();
        let quality = self.quality;
        let (tx, rx) = mpsc::channel();
        self.exporting = true;
        self.status = "Exporting…".to_string();
        thread::spawn(move || {
            let outcome = export_folder(&layout, &images, quality, &dir);
            let _ = tx.send(outcome);
        });
        self.export_rx = Some(rx);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let kind = match self.route {
            Route::Template(_) => "Template",
            Route::Carousel(_) => "Carousel",
            Route::Home => "",
        };
        let name = self.item_name();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let home = ui.button("Home");
                self.remember("home", home.rect);
                if home.clicked() {
                    self.go_home();
                }
                ui.label(egui::RichText::new(kind).color(MUTED));
                ui.label(egui::RichText::new(short_name(&name)).strong());
                ui.separator();
                for preset in CANVAS_PRESETS {
                    let selected = self.layout().is_some_and(|layout| layout.ratio == preset);
                    let response = ui.selectable_label(selected, preset.label());
                    self.remember(format!("canvas-{}", preset.label()), response.rect);
                    if response.clicked() {
                        if let Some(layout) = self.layout_mut() {
                            layout.set_ratio(preset);
                        }
                        self.note_geometry();
                    }
                }
                if let Some(layout) = self.layout() {
                    let (export_w, export_h) =
                        layout.ratio.export_size(layout_export_width(layout));
                    ui.label(
                        egui::RichText::new(format!("{export_w}×{export_h}"))
                            .color(MUTED)
                            .size(13.0),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Canvases");
                let can_remove = self.layout().is_some_and(|layout| layout.canvas_count > 1);
                let remove = ui.add_enabled(can_remove, egui::Button::new("Remove"));
                self.remember("remove-canvas", remove.rect);
                if remove.clicked()
                    && self
                        .layout_mut()
                        .is_some_and(|layout| layout.remove_canvas())
                {
                    self.note_geometry();
                }
                if let Some(layout) = self.layout() {
                    ui.monospace(format!("{:02} / {MAX_CANVASES:02}", layout.canvas_count));
                }
                let can_add = self
                    .layout()
                    .is_some_and(|layout| layout.canvas_count < MAX_CANVASES);
                let add = ui.add_enabled(can_add, egui::Button::new("Add"));
                self.remember("add-canvas", add.rect);
                if add.clicked() && self.layout_mut().is_some_and(|layout| layout.add_canvas()) {
                    self.note_geometry();
                }
                ui.separator();
                let grid = ui.checkbox(&mut self.show_grid, "Grid");
                self.remember("grid", grid.rect);
                let snap = ui.checkbox(&mut self.snap_to_grid, "Snap");
                self.remember("snap", snap.rect);
                ui.separator();
                let add_frame = ui.button("Add frame");
                self.remember("add-frame", add_frame.rect);
                if add_frame.clicked() {
                    let id = self.layout_mut().map(|layout| layout.add_frame());
                    if let Some(id) = id {
                        self.selected = Some(id);
                        self.edit_frame = true;
                        self.note_geometry();
                    }
                }
                let edit = ui.add_enabled(
                    self.selected.is_some(),
                    egui::Button::new("Edit frame").selected(self.edit_frame),
                );
                self.remember("edit-frame", edit.rect);
                if edit.clicked() {
                    self.edit_frame = !self.edit_frame;
                }
                if self.edit_frame && self.selected.is_some() {
                    for preset in CANVAS_PRESETS {
                        let response = ui.button(preset.label());
                        self.remember(format!("frame-aspect-{}", preset.label()), response.rect);
                        if response.clicked() {
                            let id = self.selected;
                            if let Some(id) = id
                                && let Some(layout) = self.layout_mut()
                            {
                                layout.set_frame_aspect(id, preset.value());
                            }
                            self.note_geometry();
                        }
                    }
                }
                let remove_frame =
                    ui.add_enabled(self.selected.is_some(), egui::Button::new("Remove frame"));
                self.remember("remove-frame", remove_frame.rect);
                if remove_frame.clicked()
                    && let Some(id) = self.selected
                {
                    self.remove_frame(id);
                }
                let photo_ready = self.selected.is_some_and(|id| {
                    self.layout()
                        .and_then(|layout| layout.frame(id))
                        .is_some_and(|frame| frame.photo.is_some())
                });
                let remove_photo = ui.add_enabled(
                    photo_ready && matches!(self.route, Route::Carousel(_)),
                    egui::Button::new("Remove photo"),
                );
                self.remember("remove-photo", remove_photo.rect);
                if remove_photo.clicked()
                    && let Some(id) = self.selected
                {
                    self.remove_photo(id);
                }
                let export = ui.add_enabled(
                    !self.exporting && self.has_photo(),
                    egui::Button::new("Export"),
                );
                self.remember("export", export.rect);
                if export.clicked() {
                    self.export_open = true;
                    self.schedule_estimate();
                }
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).color(MUTED));
                }
            });
        });
    }

    fn export_modal(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let modal_id = egui::Id::unique("export-options");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("Export");
                ui.add_space(4.0);
                if let Some(layout) = self.layout() {
                    let (width, height) = layout.ratio.export_size(layout_export_width(layout));
                    let sample = canvas_file_name(0, self.quality);
                    ui.label(format!(
                        "{} canvases · {width}×{height}",
                        layout.canvas_count
                    ));
                    ui.label(egui::RichText::new(format!("Files {sample} and on")).color(MUTED));
                }
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
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if response.should_close() {
            self.export_open = false;
        }
    }

    fn layer_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Layers");
        let rows: Vec<(FrameId, String, Option<Crop>)> = self
            .layout()
            .map(|layout| {
                layout
                    .frames_front_to_back()
                    .into_iter()
                    .map(|frame| {
                        let name = frame
                            .photo
                            .as_ref()
                            .map(|photo| photo.name.clone())
                            .unwrap_or_else(|| "Frame".to_string());
                        let crop = frame.photo.as_ref().map(|photo| photo.crop);
                        (frame.id, name, crop)
                    })
                    .collect()
            })
            .unwrap_or_default();
        ui.label(egui::RichText::new(counted(rows.len(), "frame", "frames")).color(MUTED));
        ui.add_space(4.0);
        if rows.is_empty() {
            ui.label("No frames yet.");
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
                    if let Some(crop) = crop
                        && let Some(asset) = self.assets.get(&id)
                    {
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
                    );
                }
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
                        if icon_button(ui, rect, ("layer-move", id.0, slot), enabled, glyph)
                            .clicked()
                            && enabled
                            && self
                                .layout_mut()
                                .is_some_and(|layout| layout.move_layer(id, movement))
                        {
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
        if matches!(self.route, Route::Template(_)) {
            ui.label("Frames stay empty in a template.");
            return;
        }
        let Some(id) = self.selected else {
            ui.label("Select a frame to crop its photo.");
            return;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            ui.label("Select a frame to crop its photo.");
            return;
        };
        let Some(photo) = frame.photo.clone() else {
            ui.label("This frame is empty.");
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
        let count = self.metrics().map_or(1, |(_, count)| count);
        ui.horizontal(|ui| {
            ui.heading("Preview");
            ui.label(
                egui::RichText::new(format!("{}/{}", self.preview_index + 1, count)).size(16.0),
            );
        });
        let area = ui.available_rect_before_wrap();
        if !area.is_positive() {
            return;
        }
        ui.allocate_rect(area, egui::Sense::hover());
        let ratio = self.metrics().map_or(0.8, |(ratio, _)| ratio);
        let slots = preview_slots(area, ratio);
        let can_prev = self.preview_index > 0;
        if icon_button(ui, slots.left, "preview-prev", can_prev, Glyph::Left).clicked()
            && self.preview_index > 0
        {
            self.preview_index -= 1;
        }
        self.draw_phone(ui, slots.phone);
        let can_next = self.preview_index + 1 < count;
        if icon_button(ui, slots.right, "preview-next", can_next, Glyph::Right).clicked()
            && self.preview_index + 1 < count
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
        let Some(layout) = self.layout().cloned() else {
            return;
        };
        if layout.canvas_count == 0 {
            return;
        }
        if self.preview_index >= layout.canvas_count {
            self.preview_index = layout.canvas_count - 1;
        }
        let key = (self.geometry_revision, self.preview_index);
        if self.preview_key == Some(key) {
            return;
        }
        if ctx.input(|input| input.pointer.any_down()) && self.preview_texture.is_some() {
            ctx.request_repaint();
            return;
        }
        let images = self.image_map();
        let rendered = render_layout(&layout, self.preview_index, &images, 640);
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
        let layer = egui::LayerId::debug();
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
        let Some(layout) = self.layout() else {
            return;
        };
        let basis = (layout.ratio.width, layout.ratio.height);
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
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
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
        let new_size = editor_strip_size(bounds, zoom, ratio, count);
        let desired = pointer - egui::vec2(fx * new_size.x, fy * new_size.y);
        let (lead, offset) = place_strip(desired, new_size, view);
        self.strip_lead = lead;
        self.pending_scroll = Some(offset);
        self.center_strip = false;
    }

    fn editor(&mut self, ui: &mut egui::Ui, bounds: egui::Rect) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let bounds = bounds.intersect(ui.clip_rect());
        ui.shrink_clip_rect(bounds);
        self.sync_zoom_basis();
        self.apply_wheel_zoom(ui, bounds);
        let size = editor_strip_size(bounds, self.editor_zoom, ratio, count);
        let mut scroll_source = egui::scroll_area::ScrollSource::SCROLL_BAR;
        if self.crop_session.is_none() && !self.claims_strip_pointer(ui.ctx(), bounds) {
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
        self.plus_rects.clear();
        let clip = bounds.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(clip);
        self.draw_strip(ui, &painter, rect, clip);
        self.paint_crop_handles(&painter, rect, clip);
        self.paint_frame_handles(&painter, rect, clip);
        self.strip_cursor(&response, rect);
    }

    fn handle_strip(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: egui::Rect) {
        if self.crop_session.is_some() {
            self.drag_crop(ctx, rect, editor);
            return;
        }
        self.drag_frame(ctx, rect, editor);
    }

    fn claims_strip_pointer(&self, ctx: &egui::Context, editor: egui::Rect) -> bool {
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
        down && editor.contains(pos) && self.hits_frame_or_handle(strip, pos)
    }

    fn hits_frame_or_handle(&self, rect: egui::Rect, pos: egui::Pos2) -> bool {
        if self.plus_hit(pos) || self.gesture_at(rect, pos).is_some() {
            return true;
        }
        let count = self.metrics().map_or(1, |(_, count)| count);
        let (nx, ny) = screen_to_norm(rect, count, pos);
        self.layout()
            .and_then(|layout| layout.frame_at(nx, ny))
            .is_some()
    }

    fn plus_hit(&self, pos: egui::Pos2) -> bool {
        self.plus_rects.iter().any(|(_, rect)| rect.contains(pos))
    }

    fn gesture_at(&self, rect: egui::Rect, pos: egui::Pos2) -> Option<Drag> {
        let layout = self.layout()?;
        let ratio = layout.ratio.value();
        let count = layout.canvas_count;
        if self.edit_frame
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
        {
            let screen = frame_screen(rect, count, frame);
            if let Some(handle) = handle_at(screen, pos) {
                return Some(Drag::ResizeFrame {
                    id,
                    handle,
                    x: frame.x,
                    y: frame.y,
                    w: frame.w,
                    h: frame.h,
                });
            }
        }
        let (nx, ny) = screen_to_norm(rect, count, pos);
        let id = layout.frame_at(nx, ny)?;
        let frame = layout.frame(id)?;
        if self.edit_frame {
            return Some(Drag::MoveFrame {
                id,
                grab_x: nx - frame.x,
                grab_y: ny - frame.y,
            });
        }
        let fit = cover_fit(frame, ratio)?;
        Some(Drag::Pan {
            id,
            grab_x: nx - fit.x,
            grab_y: ny - fit.y,
        })
    }

    fn drag_frame(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: egui::Rect) {
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
                if editor.contains(origin) && !self.plus_hit(origin) {
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
            Some(Drag::MoveFrame { .. } | Drag::Pan { .. }) => {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                if down {
                    ctx.set_dragged_id(egui::Id::unique("frame-gesture"));
                }
            }
            Some(Drag::ResizeFrame { handle, .. }) => {
                ctx.set_cursor_icon(cursor_for(handle));
                if down {
                    ctx.set_dragged_id(egui::Id::unique("frame-gesture"));
                }
            }
            None => {
                if let Some(pos) = latest.or(interact).or(press) {
                    if editor.contains(pos)
                        && let Some(icon) = self.cursor_at(rect, pos)
                    {
                        ctx.set_cursor_icon(icon);
                    }
                }
            }
        }
        if released {
            if let Some(drag) = self.drag.take() {
                match drag {
                    Drag::Pan { .. } => self.gesture = "pan".to_string(),
                    Drag::MoveFrame { id, .. } => {
                        if self.show_grid && self.snap_to_grid {
                            self.snap_released_frame(id);
                            self.gesture = "snap".to_string();
                        } else {
                            self.gesture = "frame".to_string();
                        }
                    }
                    Drag::ResizeFrame { id, handle, .. } => {
                        if self.show_grid && self.snap_to_grid {
                            self.snap_released_resize(id, handle);
                            self.gesture = "snap".to_string();
                        } else {
                            self.gesture = "frame".to_string();
                        }
                    }
                }
                self.note_geometry();
                ctx.stop_dragging();
            }
        }
    }

    fn snap_released_frame(&mut self, id: FrameId) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let (x, y) = snap_frame_to_grid(&frame);
        if let Some(layout) = self.layout_mut() {
            layout.set_frame_rect(id, x, y, frame.w, frame.h);
        }
    }

    fn snap_released_resize(&mut self, id: FrameId, handle: Handle) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let (x, y, w, h) = snap_resized_frame_to_grid(handle, frame.x, frame.y, frame.w, frame.h);
        if let Some(layout) = self.layout_mut() {
            layout.set_frame_rect(id, x, y, w, h);
        }
    }

    fn begin_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        if let Some(drag) = self.gesture_at(rect, pos) {
            self.selected = Some(drag.id());
            self.drag = Some(drag);
            return;
        }
        let count = self.metrics().map_or(1, |(_, count)| count);
        let (nx, ny) = screen_to_norm(rect, count, pos);
        if let Some(id) = self.layout().and_then(|layout| layout.frame_at(nx, ny)) {
            self.selected = Some(id);
        } else {
            self.selected = None;
            self.edit_frame = false;
            self.drag = None;
        }
    }

    fn update_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let (nx, ny) = screen_to_norm(rect, count, pos);
        match self.drag {
            Some(Drag::MoveFrame { id, grab_x, grab_y }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                if let Some(layout) = self.layout_mut() {
                    layout.set_frame_rect(id, nx - grab_x, ny - grab_y, frame.w, frame.h);
                }
            }
            Some(Drag::ResizeFrame {
                id,
                handle,
                x,
                y,
                w,
                h,
            }) => {
                let lock = matches!(handle, Handle::Ne | Handle::Nw | Handle::Se | Handle::Sw);
                let (x, y, w, h) = resize_frame(x, y, w, h, handle, nx, ny, lock);
                if let Some(layout) = self.layout_mut() {
                    layout.set_frame_rect(id, x, y, w, h);
                }
            }
            Some(Drag::Pan { id, grab_x, grab_y }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                let (pan_x, pan_y) = pan_for_origin(&frame, ratio, nx - grab_x, ny - grab_y);
                if let Some(layout) = self.layout_mut() {
                    layout.set_pan(id, pan_x, pan_y);
                }
            }
            None => {}
        }
    }

    fn draw_strip(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: egui::Rect,
    ) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let frames = self
            .layout()
            .map(|layout| layout.frames.clone())
            .unwrap_or_default();
        let canvas_w = rect.width() / count.max(1) as f32;
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
        let session_id = self.crop_session.as_ref().map(|session| session.id);
        let mut ordered = frames.clone();
        ordered.sort_by_key(|frame| frame.z);
        for frame in &ordered {
            if session_id == Some(frame.id) {
                continue;
            }
            let screen = frame_screen(rect, count, frame);
            if frame.photo.is_none() {
                let visible = screen.intersect(clip);
                if visible.is_positive() {
                    painter.rect_filled(visible, 0.0, egui::Color32::from_black_alpha(16));
                }
            }
            if let Some(photo) = &frame.photo
                && let Some(asset) = self.assets.get(&frame.id)
                && let Some(fit) = cover_fit(frame, ratio)
            {
                let cover = norm_rect(rect, count, fit.x, fit.y, fit.w, fit.h);
                let uv = egui::Rect::from_min_max(
                    egui::pos2(photo.crop.x, photo.crop.y),
                    egui::pos2(photo.crop.x + photo.crop.w, photo.crop.y + photo.crop.h),
                );
                paint_clipped_image(
                    painter,
                    cover,
                    clip.intersect(screen),
                    uv,
                    asset.texture.id(),
                );
            }
        }
        if self.show_grid {
            paint_alignment_grid(painter, rect, count, clip);
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
        for frame in &ordered {
            if session_id == Some(frame.id) {
                continue;
            }
            let screen = frame_screen(rect, count, frame);
            let shown = frame.photo.is_some() && self.assets.contains_key(&frame.id);
            if !shown {
                let color = if self.selected == Some(frame.id) {
                    ACCENT
                } else {
                    egui::Color32::from_rgb(42, 36, 30)
                };
                paint_frame(painter, screen, clip, 2.0, color);
            }
            let loading_here = matches!(
                self.route,
                Route::Carousel(carousel_id) if self.loading.contains_key(&(carousel_id, frame.id))
            );
            if loading_here {
                paint_loading(ui, painter, screen, clip);
            } else if frame.photo.is_none() && matches!(self.route, Route::Carousel(_)) {
                let center = screen.center();
                let radius = 16.0;
                let hit =
                    egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
                self.plus_rects.push((frame.id, hit));
                paint_plus(painter, center, radius);
                let response = ui.allocate_rect(hit, egui::Sense::click());
                self.remember(format!("plus-{}", frame.id.0), response.rect);
                if response.clicked() {
                    let id = frame.id;
                    self.fill_frame(ui.ctx(), id);
                }
            }
        }
        if self.crop_session.is_some() {
            self.paint_session_crop(painter, rect, clip);
        }
    }

    fn paint_frame_handles(&self, painter: &egui::Painter, strip: egui::Rect, clip: egui::Rect) {
        if self.crop_session.is_some() || !self.edit_frame {
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        let Some((_, count)) = self.metrics() else {
            return;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)) else {
            return;
        };
        let screen = frame_screen(strip, count, frame);
        if !screen.intersects(clip.expand(24.0)) {
            return;
        }
        let painter = painter.with_clip_rect(clip);
        for (_, pos) in handle_points(screen) {
            paint_resize_handle(&painter, pos, 9.0, clip);
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
            Some(Drag::MoveFrame { .. } | Drag::Pan { .. }) => Some(egui::CursorIcon::Grabbing),
            Some(Drag::ResizeFrame { handle, .. }) => Some(cursor_for(handle)),
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
        let layout = self.layout()?;
        let count = layout.canvas_count;
        if self.edit_frame
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
        {
            let screen = frame_screen(rect, count, frame);
            if let Some(handle) = handle_at(screen, pos) {
                return Some(cursor_for(handle));
            }
        }
        let (nx, ny) = screen_to_norm(rect, count, pos);
        let id = layout.frame_at(nx, ny)?;
        if self.edit_frame || layout.frame(id).is_some_and(|frame| frame.photo.is_some()) {
            Some(egui::CursorIcon::Grab)
        } else {
            Some(egui::CursorIcon::PointingHand)
        }
    }

    fn open_crop_session(&mut self, id: FrameId, preset: CropPreset) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let Some(photo) = frame.photo else {
            return;
        };
        let current = photo.crop;
        if let Some(session) = self.crop_session.as_mut() {
            if session.id == id {
                if session.preset != preset {
                    session.preset = preset;
                    session.crop =
                        begin_crop_rect(current, preset, photo.image_width, photo.image_height);
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
            crop: begin_crop_rect(current, preset, photo.image_width, photo.image_height),
            drag: None,
        });
    }

    fn commit_crop_session(&mut self) {
        let Some(session) = self.crop_session.take() else {
            return;
        };
        let Some(frame) = self
            .layout()
            .and_then(|layout| layout.frame(session.id))
            .cloned()
        else {
            return;
        };
        let Some(photo) = frame.photo else {
            return;
        };
        if photo.preset == session.preset && crop_unchanged(photo.crop, session.crop) {
            return;
        }
        if let Some(snapshot) = self.layout().cloned() {
            self.push_undo(snapshot);
        }
        if let Some(layout) = self.layout_mut() {
            layout.set_photo_crop(session.id, session.crop, session.preset);
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
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            self.crop_session = None;
            self.crop_press = None;
            return;
        };
        let Some(photo) = frame.photo.as_ref() else {
            self.crop_session = None;
            self.crop_press = None;
            return;
        };
        let count = self.metrics().map_or(1, |(_, count)| count);
        let image_rect = full_image_screen(frame_screen(rect, count, &frame), photo.crop);
        let preset = self
            .crop_session
            .as_ref()
            .map_or(CropPreset::Freehand, |session| session.preset);
        let lock = crop_lock_ratio(preset, photo.image_width, photo.image_height);
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
                if editor.contains(origin)
                    && let Some(handle) = crop_handle_at(crop_rect, origin)
                {
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
                if editor.contains(origin)
                    && let Some(handle) = crop_handle_at(crop_rect, origin)
                {
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
                        if editor.contains(pos)
                            && let Some(handle) = crop_handle_at(crop_rect, pos)
                        {
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
        let Some(frame) = self.layout().and_then(|layout| layout.frame(session.id)) else {
            return;
        };
        let Some(photo) = frame.photo.as_ref() else {
            return;
        };
        let Some(texture) = self.assets.get(&session.id).map(|asset| asset.texture.id()) else {
            return;
        };
        let count = self.metrics().map_or(1, |(_, count)| count);
        let image = full_image_screen(frame_screen(strip, count, frame), photo.crop);
        let visible = image.intersect(clip);
        if !visible.is_positive() {
            return;
        }
        let crop_rect = crop_to_screen(image, session.crop);
        let full_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        paint_clipped_image(painter, image, clip, full_uv, texture);
        paint_crop_veil(painter, visible, crop_rect);
        let inside_uv = egui::Rect::from_min_max(
            egui::pos2(session.crop.x, session.crop.y),
            egui::pos2(
                session.crop.x + session.crop.w,
                session.crop.y + session.crop.h,
            ),
        );
        paint_clipped_image(painter, crop_rect, clip, inside_uv, texture);
        paint_frame(
            painter,
            image,
            clip,
            1.0,
            egui::Color32::from_rgb(70, 70, 70),
        );
        paint_frame(painter, crop_rect, clip, 2.0, egui::Color32::WHITE);
    }

    fn paint_crop_handles(&self, painter: &egui::Painter, strip: egui::Rect, clip: egui::Rect) {
        let Some(crop_rect) = self.session_crop_screen(strip) else {
            return;
        };
        let painter = painter.with_clip_rect(clip);
        for (_, pos) in handle_points(crop_rect) {
            paint_resize_handle(&painter, pos, 11.0, clip);
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
        let count = self.layout()?.canvas_count;
        let frame = self.layout()?.frame(id)?;
        let photo = frame.photo.as_ref()?;
        let image = full_image_screen(frame_screen(strip, count, frame), photo.crop);
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
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let canvas_w = strip.width() / count.max(1) as f32;
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
        if let Some(layout) = self.layout() {
            for frame in &layout.frames {
                let screen = frame_screen(strip, count, frame);
                let _ = writeln!(
                    text,
                    "frame {} x={} y={} w={} h={} crop={:?} screen={:?} ratio={ratio}",
                    frame.id.0,
                    frame.x,
                    frame.y,
                    frame.w,
                    frame.h,
                    frame.photo.as_ref().map(|photo| photo.crop),
                    screen
                );
            }
        }
        if let Some(session) = &self.crop_session {
            let screen = self.session_crop_screen(strip);
            let _ = writeln!(
                text,
                "session {:?} crop={:?} screen={:?}",
                session.preset, session.crop, screen
            );
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

    fn home_screen(&mut self, ui: &mut egui::Ui, full: egui::Rect) -> Option<HomeAction> {
        ui.painter().rect_filled(full, 0.0, BG);
        let inner = full.shrink(16.0);
        let mut home = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("home")
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        home.horizontal(|ui| {
            ui.label(egui::RichText::new("PudduScroll").strong().size(22.0));
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(&self.status).color(MUTED));
            }
        });
        home.add_space(8.0);
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("home-scroll")
            .auto_shrink([false, false])
            .show(&mut home, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Templates");
                    let response = ui.button("New template");
                    self.remember("new-template", response.rect);
                    if response.clicked() {
                        self.create_name = "Template".to_string();
                        self.create_ratio = AspectRatio::FOUR_FIVE;
                        self.create_cover = None;
                        self.create_cover_texture = None;
                        self.cover_load = None;
                        self.create_open = true;
                    }
                });
                ui.add_space(8.0);
                if self.library.templates.is_empty() {
                    ui.label(egui::RichText::new("No templates yet.").color(MUTED));
                }
                let template_count = self.library.templates.len();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    for index in 0..template_count {
                        if let Some(next) = self.template_card(ui, index) {
                            action = Some(next);
                        }
                    }
                });
                ui.add_space(18.0);
                ui.heading("Carousels");
                ui.add_space(8.0);
                if self.library.carousels.is_empty() {
                    ui.label(egui::RichText::new("No carousels yet.").color(MUTED));
                }
                let carousel_count = self.library.carousels.len();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    for index in 0..carousel_count {
                        if let Some(next) = self.carousel_card(ui, index) {
                            action = Some(next);
                        }
                    }
                });
            });
        if self.name_dirty {
            self.name_dirty = false;
            self.persist();
        }
        action
    }

    fn ensure_cover_texture(&mut self, ctx: &egui::Context, index: usize) {
        let id = self.library.templates[index].id;
        if self.cover_textures.contains_key(&id) || self.cover_pending.contains_key(&id) {
            return;
        }
        let image = Arc::clone(&self.library.templates[index].cover.image);
        if image.width().max(image.height()) > PREVIEW_MAX {
            let token = self.next_token();
            self.cover_pending.insert(id, token);
            let tx = self.photo_tx.clone();
            thread::spawn(move || {
                let preview = editor_preview(&image);
                let _ = tx.send(IncomingPhoto::CoverPreview {
                    token,
                    template_id: id,
                    preview,
                });
            });
            ctx.request_repaint();
            return;
        }
        let preview = editor_preview(&image);
        self.cover_textures.insert(
            id,
            texture_from_preview(ctx, format!("cover-{id}"), &preview),
        );
    }

    fn template_card(&mut self, ui: &mut egui::Ui, index: usize) -> Option<HomeAction> {
        let id = self.library.templates[index].id;
        let name = self.library.templates[index].name.clone();
        let canvases = self.library.templates[index].layout.canvas_count;
        let frames = self.library.templates[index].layout.frames.len();
        self.ensure_cover_texture(ui.ctx(), index);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(236.0, 286.0), egui::Sense::hover());
        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("template-card", id))
                .max_rect(rect.shrink(8.0))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        card.set_clip_rect(rect);
        card.painter().rect_filled(rect, 8.0, PANEL);
        paint_frame(
            &card.painter(),
            rect,
            rect,
            1.0,
            egui::Color32::from_rgb(58, 58, 58),
        );
        let cover = egui::Rect::from_min_size(
            rect.min + egui::vec2(12.0, 12.0),
            egui::vec2(rect.width() - 24.0, 132.0),
        );
        card.painter()
            .rect_filled(cover, 4.0, egui::Color32::from_rgb(36, 36, 36));
        if let Some(texture) = self.cover_textures.get(&id) {
            let size = texture.size();
            let fitted = fitted_rect(cover, size[0] as f32, size[1] as f32);
            card.painter().image(
                texture.id(),
                fitted,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else if self.cover_pending.contains_key(&id) {
            card.painter().text(
                cover.center(),
                egui::Align2::CENTER_CENTER,
                "Loading",
                egui::FontId::proportional(14.0),
                TEXT,
            );
        }
        card.add_space(142.0);
        card.label(egui::RichText::new(short_name(&name)).strong().size(16.0));
        card.label(
            egui::RichText::new(format!(
                "{} · {}",
                counted(canvases, "canvas", "canvases"),
                counted(frames, "frame", "frames")
            ))
            .color(MUTED),
        );
        let mut action = None;
        card.horizontal(|ui| {
            let open = ui.button("Open");
            self.remember(format!("template-open-{id}"), open.rect);
            if open.clicked() {
                action = Some(HomeAction::OpenTemplate(id));
            }
            let edit = ui.button("Edit");
            self.remember(format!("template-edit-{id}"), edit.rect);
            if edit.clicked() {
                action = Some(HomeAction::EditTemplate(id));
            }
            let delete = ui.button("Delete");
            self.remember(format!("template-delete-{id}"), delete.rect);
            if delete.clicked() {
                action = Some(HomeAction::DeleteTemplate(id));
            }
        });
        self.remember(format!("template-card-{id}"), rect);
        action
    }

    fn carousel_card(&mut self, ui: &mut egui::Ui, index: usize) -> Option<HomeAction> {
        let id = self.library.carousels[index].id;
        self.ensure_carousel_preview(ui.ctx(), index);
        let canvases = self.library.carousels[index].layout.canvas_count;
        let frames = self.library.carousels[index].layout.frames.len();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(236.0, 300.0), egui::Sense::hover());
        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("carousel-card", id))
                .max_rect(rect.shrink(8.0))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        card.set_clip_rect(rect);
        card.painter().rect_filled(rect, 8.0, PANEL);
        paint_frame(
            &card.painter(),
            rect,
            rect,
            1.0,
            egui::Color32::from_rgb(58, 58, 58),
        );
        let preview = egui::Rect::from_min_size(
            rect.min + egui::vec2(12.0, 12.0),
            egui::vec2(rect.width() - 24.0, 132.0),
        );
        card.painter().rect_filled(preview, 4.0, CANVAS_BG);
        if let Some((_, texture)) = self.card_previews.get(&id) {
            let size = texture.size();
            let fitted = fitted_rect(preview, size[0] as f32, size[1] as f32);
            card.painter().image(
                texture.id(),
                fitted,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        card.add_space(142.0);
        let (changed, name_rect) = {
            let carousel = &mut self.library.carousels[index];
            let response = card.add(
                egui::TextEdit::singleline(&mut carousel.name)
                    .id_salt(("carousel-name", id))
                    .desired_width(rect.width() - 28.0),
            );
            (response.changed(), response.rect)
        };
        self.remember(format!("carousel-name-{id}"), name_rect);
        if changed {
            self.name_dirty = true;
        }
        card.label(
            egui::RichText::new(format!(
                "{} · {}",
                counted(canvases, "canvas", "canvases"),
                counted(frames, "frame", "frames")
            ))
            .color(MUTED),
        );
        let mut action = None;
        card.horizontal(|ui| {
            let open = ui.button("Open");
            self.remember(format!("carousel-open-{id}"), open.rect);
            if open.clicked() {
                action = Some(HomeAction::OpenCarousel(id));
            }
            let delete = ui.button("Delete");
            self.remember(format!("carousel-delete-{id}"), delete.rect);
            if delete.clicked() {
                action = Some(HomeAction::DeleteCarousel(id));
            }
        });
        self.remember(format!("carousel-card-{id}"), rect);
        action
    }

    fn ensure_carousel_preview(&mut self, ctx: &egui::Context, index: usize) {
        let id = self.library.carousels[index].id;
        let visuals = self.library.visuals;
        if self
            .card_previews
            .get(&id)
            .is_some_and(|(stamp, _)| *stamp == visuals)
        {
            return;
        }
        let carousel = &self.library.carousels[index];
        let images: FrameImages = carousel
            .photos
            .iter()
            .map(|(frame_id, blob)| (*frame_id, Arc::clone(&blob.image)))
            .collect();
        let rendered = carousel_preview_image(&carousel.layout, &images);
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [rendered.width() as usize, rendered.height() as usize],
            rendered.as_raw(),
        );
        let texture = ctx.load_texture(
            format!("carousel-{id}-{visuals}"),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.card_previews.insert(id, (visuals, texture));
    }

    fn apply_home(&mut self, action: HomeAction, ctx: &egui::Context) {
        match action {
            HomeAction::OpenTemplate(id) => {
                if let Some(carousel_id) = self.library.instantiate(id) {
                    self.note_geometry();
                    self.enter(Route::Carousel(carousel_id), ctx);
                }
            }
            HomeAction::EditTemplate(id) => self.enter(Route::Template(id), ctx),
            HomeAction::DeleteTemplate(id) => {
                self.library.delete_template(id);
                self.cover_textures.remove(&id);
                self.cover_pending.remove(&id);
                self.note_geometry();
            }
            HomeAction::OpenCarousel(id) => self.enter(Route::Carousel(id), ctx),
            HomeAction::DeleteCarousel(id) => {
                self.library.delete_carousel(id);
                self.card_previews.remove(&id);
                self.note_geometry();
            }
        }
    }

    fn create_template(&mut self, ctx: &egui::Context) {
        let name = self.create_name.trim().to_string();
        if name.is_empty() {
            self.status = "Name the template".to_string();
            return;
        }
        let Some(cover) = self.create_cover.take() else {
            self.status = "Choose a cover".to_string();
            return;
        };
        let ratio = self.create_ratio;
        let id = self.library.create_template(name, ratio, cover);
        self.create_cover_texture = None;
        self.create_open = false;
        self.note_geometry();
        self.enter(Route::Template(id), ctx);
        self.status = String::new();
    }

    fn create_modal(&mut self, ctx: &egui::Context) {
        if !self.create_open {
            return;
        }
        let modal_id = egui::Id::unique("create-template");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("New template");
                ui.add_space(6.0);
                ui.label("Name");
                let name = ui.add(
                    egui::TextEdit::singleline(&mut self.create_name)
                        .id_salt("template-name")
                        .desired_width(360.0),
                );
                self.remember("template-name", name.rect);
                ui.add_space(6.0);
                ui.label("Cover");
                ui.horizontal(|ui| {
                    let choose = ui.button("Choose cover");
                    self.remember("choose-cover", choose.rect);
                    if choose.clicked()
                        && let Some(path) = pick_image_path("Choose cover")
                    {
                        self.spawn_decode(ctx, path, DecodeTarget::Cover);
                    }
                    if let Some(cover) = &self.create_cover {
                        ui.label(egui::RichText::new(short_name(&cover.name)).color(MUTED));
                    }
                });
                if self.cover_load.is_some() {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0).color(ACCENT));
                        ui.label("Loading");
                    });
                } else if let Some(texture) = &self.create_cover_texture {
                    let size = texture.size();
                    let bounds =
                        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(120.0, 90.0));
                    let fitted = fitted_rect(bounds, size[0] as f32, size[1] as f32);
                    ui.allocate_exact_size(egui::vec2(120.0, 90.0), egui::Sense::hover());
                    ui.painter().image(
                        texture.id(),
                        fitted,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                ui.add_space(6.0);
                ui.label("Canvas aspect");
                ui.horizontal(|ui| {
                    for preset in CANVAS_PRESETS {
                        let response =
                            ui.selectable_label(self.create_ratio == preset, preset.label());
                        self.remember(format!("aspect-{}", preset.label()), response.rect);
                        if response.clicked() {
                            self.create_ratio = preset;
                        }
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let cancel = ui.button("Cancel");
                    self.remember("cancel-template", cancel.rect);
                    if cancel.clicked() {
                        self.create_open = false;
                        self.create_cover = None;
                        self.create_cover_texture = None;
                        self.cover_load = None;
                    }
                    let ready = self.cover_load.is_none()
                        && self.create_cover.is_some()
                        && !self.create_name.trim().is_empty();
                    let create = ui.add_enabled(
                        ready,
                        egui::Button::new(
                            egui::RichText::new("Create template").color(egui::Color32::BLACK),
                        )
                        .fill(ACCENT),
                    );
                    self.remember("create-template", create.rect);
                    if create.clicked() {
                        self.create_template(ctx);
                    }
                });
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if response.should_close() {
            self.create_open = false;
            self.cover_load = None;
        }
    }

    fn editor_screen(&mut self, ui: &mut egui::Ui, full: egui::Rect) {
        let chrome = egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::symmetric(12, 6));
        egui::Panel::top("toolbar")
            .exact_size(TOOLBAR_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(chrome)
            .show(ui, |ui| self.toolbar(ui));
        if self.route == Route::Home {
            return;
        }
        let rail_rect = egui::Rect::from_min_max(
            egui::pos2(full.right() - RAIL_W, full.top() + TOOLBAR_H),
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
        self.write_crop_probe(ui.ctx(), editor_rect);
        self.apply_crop_click(ui.ctx(), editor_rect);
    }

    fn write_probe(&self, ctx: &egui::Context) {
        let mut text = String::new();
        let view = ctx.viewport_rect();
        let _ = writeln!(text, "ppp {}", ctx.pixels_per_point());
        let _ = writeln!(
            text,
            "viewport {} {} {} {}",
            view.min.x,
            view.min.y,
            view.width(),
            view.height()
        );
        let _ = writeln!(
            text,
            "library {}",
            crate::library::library_directory().display()
        );
        match self.route {
            Route::Home => {
                let _ = writeln!(text, "screen home");
            }
            Route::Template(id) => {
                let _ = writeln!(text, "screen template {id}");
            }
            Route::Carousel(id) => {
                let _ = writeln!(text, "screen carousel {id}");
            }
        }
        let _ = writeln!(
            text,
            "grid {} snap {} edit_frame {}",
            u8::from(self.show_grid),
            u8::from(self.snap_to_grid),
            u8::from(self.edit_frame)
        );
        let drag = match self.drag {
            Some(Drag::MoveFrame { .. }) => "move",
            Some(Drag::ResizeFrame { .. }) => "resize",
            Some(Drag::Pan { .. }) => "pan",
            None => "none",
        };
        let _ = writeln!(text, "drag {drag}");
        let _ = writeln!(text, "gesture {}", self.gesture);
        let _ = writeln!(text, "painted {}", self.painted);
        let _ = writeln!(text, "status {}", self.status.replace(['\n', '\r'], " "));
        let _ = writeln!(
            text,
            "cover_loading {}",
            u8::from(self.cover_load.is_some())
        );
        for ((carousel_id, frame_id), _) in &self.loading {
            let _ = writeln!(text, "loading {carousel_id} {}", frame_id.0);
        }
        for template in &self.library.templates {
            let _ = writeln!(
                text,
                "template {} \"{}\" {} {} {}",
                template.id,
                template.name.replace('"', "'"),
                template.layout.canvas_count,
                template.layout.frames.len(),
                u8::from(true)
            );
        }
        for carousel in &self.library.carousels {
            let photos = carousel
                .layout
                .frames
                .iter()
                .filter(|frame| frame.photo.is_some())
                .count();
            let _ = writeln!(
                text,
                "carousel {} \"{}\" {} {} {}",
                carousel.id,
                carousel.name.replace('"', "'"),
                carousel.layout.canvas_count,
                carousel.layout.frames.len(),
                photos
            );
        }
        let strip = self.strip_rect;
        if let Some(layout) = self.layout() {
            let ratio = layout.ratio.value();
            for frame in &layout.frames {
                let covers = u8::from(frame.photo.is_some() && cover_contains_frame(frame, ratio));
                let photo = frame.photo.as_ref();
                let _ = writeln!(
                    text,
                    "frame {} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {} {}",
                    frame.id.0,
                    frame.x,
                    frame.y,
                    frame.w,
                    frame.h,
                    photo.map_or(0.5, |photo| photo.pan_x),
                    photo.map_or(0.5, |photo| photo.pan_y),
                    u8::from(photo.is_some()),
                    covers
                );
                if let Some(strip) = strip {
                    let screen = frame_screen(strip, layout.canvas_count, frame);
                    let _ = writeln!(
                        text,
                        "screen {} {} {} {} {}",
                        frame.id.0,
                        screen.min.x,
                        screen.min.y,
                        screen.width(),
                        screen.height()
                    );
                }
            }
        }
        if self.edit_frame
            && let Some(strip) = strip
            && let Some(id) = self.selected
        {
            let count = self.metrics().map_or(1, |(_, count)| count);
            if let Some(frame) = self.layout().and_then(|layout| layout.frame(id)) {
                let screen = frame_screen(strip, count, frame);
                for (handle, pos) in handle_points(screen) {
                    let _ = writeln!(text, "handle {} {} {}", handle_name(handle), pos.x, pos.y);
                }
            }
        }
        for (name, rect) in &self.widgets {
            let _ = writeln!(
                text,
                "widget {name} {} {} {} {}",
                rect.min.x,
                rect.min.y,
                rect.width(),
                rect.height()
            );
        }
        let _ = std::fs::write(
            r"C:\Users\dario\AppData\Local\Temp\pudduscroll-ui.txt",
            text,
        );
    }
}

impl eframe::App for PudduScrollApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.painted = self.painted.wrapping_add(1);
        let ctx = ui.ctx().clone();
        self.poll_jobs(&ctx);
        self.handle_keys(&ctx);
        self.widgets.clear();
        let full = ui.max_rect();
        if full.width() < 900.0 || full.height() < 640.0 {
            self.write_probe(&ctx);
            return;
        }
        let action = if self.route == Route::Home {
            self.home_screen(ui, full)
        } else {
            self.editor_screen(ui, full);
            None
        };
        if let Some(action) = action {
            self.apply_home(action, &ctx);
        }
        self.create_modal(&ctx);
        self.export_modal(&ctx);
        self.write_probe(&ctx);
    }

    fn on_exit(&mut self) {
        let _ = self.library.store();
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn handle_name(handle: Handle) -> &'static str {
    match handle {
        Handle::N => "N",
        Handle::S => "S",
        Handle::E => "E",
        Handle::W => "W",
        Handle::Ne => "Ne",
        Handle::Nw => "Nw",
        Handle::Se => "Se",
        Handle::Sw => "Sw",
    }
}

fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(16)
        .corner_radius(8)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 58, 58)))
}

fn fitted_rect(bounds: egui::Rect, width: f32, height: f32) -> egui::Rect {
    let aspect = width / height.max(1.0);
    let mut fitted_w = bounds.width();
    let mut fitted_h = fitted_w / aspect.max(0.05);
    if fitted_h > bounds.height() {
        fitted_h = bounds.height();
        fitted_w = fitted_h * aspect;
    }
    egui::Rect::from_center_size(bounds.center(), egui::vec2(fitted_w, fitted_h))
}

fn paint_plus(painter: &egui::Painter, center: egui::Pos2, radius: f32) {
    painter.circle_filled(center, radius, egui::Color32::from_rgb(28, 28, 28));
    painter.circle_stroke(center, radius, egui::Stroke::new(2.0, ACCENT));
    let arm = radius * 0.46;
    let stroke = egui::Stroke::new(2.0, ACCENT);
    painter.line_segment(
        [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
        stroke,
    );
    painter.line_segment(
        [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
        stroke,
    );
}

fn carousel_preview_image(layout: &Layout, images: &FrameImages) -> RgbaImage {
    let canvas_w = 140u32;
    let (width, height) = layout.ratio.export_size(canvas_w);
    let count = layout.canvas_count.max(1) as u32;
    let mut output =
        RgbaImage::from_pixel(width * count, height, image::Rgba([255, 255, 255, 255]));
    for index in 0..layout.canvas_count {
        let canvas = render_layout(layout, index, images, canvas_w);
        overlay(
            &mut output,
            &canvas,
            i64::from(index as u32) * i64::from(width),
            0,
        );
    }
    output
}

fn crop_unchanged(a: Crop, b: Crop) -> bool {
    (a.x - b.x).abs() < 1.0e-4
        && (a.y - b.y).abs() < 1.0e-4
        && (a.w - b.w).abs() < 1.0e-4
        && (a.h - b.h).abs() < 1.0e-4
}

fn full_image_screen(placed: egui::Rect, crop: Crop) -> egui::Rect {
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

fn paint_frame(
    painter: &egui::Painter,
    rect: egui::Rect,
    clip: egui::Rect,
    width: f32,
    color: egui::Color32,
) {
    let rect = rect.intersect(clip.shrink(1.0));
    if width <= 0.0 || rect.width() <= width * 2.0 || rect.height() <= width * 2.0 {
        return;
    }
    let bands = [
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.right(), rect.top() + width),
        ),
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - width),
            rect.right_bottom(),
        ),
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + width),
            egui::pos2(rect.left() + width, rect.bottom() - width),
        ),
        egui::Rect::from_min_max(
            egui::pos2(rect.right() - width, rect.top() + width),
            egui::pos2(rect.right(), rect.bottom() - width),
        ),
    ];
    for band in bands {
        if band.is_positive() {
            painter.rect_filled(band, 0.0, color);
        }
    }
}

fn paint_resize_handle(painter: &egui::Painter, center: egui::Pos2, size: f32, clip: egui::Rect) {
    let handle =
        egui::Rect::from_center_size(center, egui::vec2(size, size)).intersect(clip.shrink(1.0));
    if handle.width() < 2.0 || handle.height() < 2.0 {
        return;
    }
    painter.rect_filled(handle, 2.0, egui::Color32::WHITE);
    paint_frame(painter, handle, handle, 1.5, ACCENT);
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
    layout: &Layout,
    images: &FrameImages,
    quality: u8,
    dir: &Path,
) -> Result<String, String> {
    let encoded = export_layout_canvases(
        layout,
        images,
        ExportSettings {
            width: layout_export_width(layout),
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

fn pick_image_path(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
        .pick_file()
}

fn texture_from_preview(
    ctx: &egui::Context,
    name: impl Into<String>,
    preview: &RgbaImage,
) -> egui::TextureHandle {
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [preview.width() as usize, preview.height() as usize],
        preview.as_raw(),
    );
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

fn paint_loading(ui: &egui::Ui, painter: &egui::Painter, screen: egui::Rect, clip: egui::Rect) {
    let center = screen.center();
    if !clip.contains(center) {
        return;
    }
    let spin = egui::Rect::from_center_size(center, egui::vec2(28.0, 28.0));
    egui::Spinner::new()
        .size(28.0)
        .color(ACCENT)
        .paint_at(ui, spin);
    let label =
        egui::Rect::from_center_size(center + egui::vec2(0.0, 28.0), egui::vec2(88.0, 22.0));
    let visible = label.intersect(clip);
    if !visible.is_positive() {
        return;
    }
    painter.rect_filled(visible, 4.0, egui::Color32::from_rgb(28, 28, 28));
    painter.text(
        label.center(),
        egui::Align2::CENTER_CENTER,
        "Loading",
        egui::FontId::proportional(14.0),
        TEXT,
    );
}

fn editor_preview(src: &RgbaImage) -> RgbaImage {
    let long = src.width().max(src.height());
    if long <= PREVIEW_MAX {
        return src.clone();
    }
    let scale = PREVIEW_MAX as f32 / long as f32;
    let width = ((src.width() as f32 * scale).round() as u32).max(1);
    let height = ((src.height() as f32 * scale).round() as u32).max(1);
    resize(src, width, height, FilterType::Triangle)
}

fn next_editor_zoom(current: f32, scroll_y: f32) -> f32 {
    (current * (scroll_y * EDITOR_ZOOM_SPEED).exp()).clamp(MIN_EDITOR_ZOOM, MAX_EDITOR_ZOOM)
}

fn paint_alignment_grid(
    painter: &egui::Painter,
    strip: egui::Rect,
    count: usize,
    clip: egui::Rect,
) {
    let count = count.max(1);
    let canvas_w = strip.width() / count as f32;
    let cells = CANVAS_GRID as f32;
    let dark = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(150));
    let light = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(120));
    for index in 0..count {
        let canvas = egui::Rect::from_min_size(
            egui::pos2(strip.min.x + index as f32 * canvas_w, strip.min.y),
            egui::vec2(canvas_w, strip.height()),
        );
        let visible = canvas.intersect(clip);
        if !visible.is_positive() {
            continue;
        }
        let painter = painter.with_clip_rect(visible);
        let step_x = canvas.width() / cells;
        let step_y = canvas.height() / cells;
        for line in 0..=CANVAS_GRID {
            let x = canvas.min.x + line as f32 * step_x;
            let y = canvas.min.y + line as f32 * step_y;
            let vertical = [egui::pos2(x, canvas.top()), egui::pos2(x, canvas.bottom())];
            let horizontal = [egui::pos2(canvas.left(), y), egui::pos2(canvas.right(), y)];
            painter.line_segment(vertical, dark);
            painter.line_segment(vertical, light);
            painter.line_segment(horizontal, dark);
            painter.line_segment(horizontal, light);
        }
    }
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

fn norm_rect(strip: egui::Rect, count: usize, x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
    let canvas_w = strip.width() / count.max(1) as f32;
    egui::Rect::from_min_max(
        egui::pos2(strip.min.x + x * canvas_w, strip.min.y + y * strip.height()),
        egui::pos2(
            strip.min.x + (x + w) * canvas_w,
            strip.min.y + (y + h) * strip.height(),
        ),
    )
}

fn frame_screen(strip: egui::Rect, count: usize, frame: &Frame) -> egui::Rect {
    norm_rect(strip, count, frame.x, frame.y, frame.w, frame.h)
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
