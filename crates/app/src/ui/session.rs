use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use image::RgbaImage;
use pudduscroll_core::{
    AspectRatio, ExportSettings, FrameId, FrameImages, Layout, estimate_layout_bytes,
    layout_export_width,
};

use crate::library::{Library, load_blob};

use super::draw::{
    apply_style, editor_preview, export_folder, pick_image_path, texture_from_preview,
};
use super::{
    DEFAULT_EDITOR_ZOOM, DecodeTarget, DecodedPhoto, EstimateMsg, IncomingPhoto, PREVIEW_MAX,
    PhotoAsset, PudduScrollApp, Route, UNDO_LIMIT,
};

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
            editor_zoom: DEFAULT_EDITOR_ZOOM,
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

    pub(super) fn layout(&self) -> Option<&Layout> {
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

    pub(super) fn layout_mut(&mut self) -> Option<&mut Layout> {
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

    pub(super) fn metrics(&self) -> Option<(f32, usize)> {
        self.layout()
            .map(|layout| (layout.ratio.value(), layout.canvas_count))
    }

    pub(super) fn item_name(&self) -> String {
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

    pub(super) fn has_photo(&self) -> bool {
        self.layout()
            .is_some_and(|layout| layout.frames.iter().any(|frame| frame.photo.is_some()))
    }

    pub(super) fn remember(&mut self, name: impl Into<String>, rect: egui::Rect) {
        self.widgets.push((name.into(), rect));
    }

    pub(super) fn persist(&mut self) {
        if let Err(error) = self.library.store() {
            self.status = error;
        }
    }

    pub(super) fn note_geometry(&mut self) {
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.library.visuals = self.library.visuals.wrapping_add(1);
        if self.export_open {
            self.schedule_estimate();
        }
        self.persist();
    }

    pub(super) fn push_undo(&mut self, layout: Layout) {
        self.undo.push(layout);
        if self.undo.len() > UNDO_LIMIT {
            let extra = self.undo.len() - UNDO_LIMIT;
            self.undo.drain(0..extra);
        }
    }

    pub(super) fn schedule_estimate(&mut self) {
        self.estimate_after = Some(Instant::now() + Duration::from_millis(280));
    }

    pub(super) fn image_map(&self) -> FrameImages {
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

    pub(super) fn poll_jobs(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.photo_rx.try_recv() {
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

    pub(super) fn photos_busy(&self) -> bool {
        !self.loading.is_empty() || self.cover_load.is_some() || !self.cover_pending.is_empty()
    }

    pub(super) fn next_token(&mut self) -> u64 {
        self.load_token = self.load_token.wrapping_add(1);
        self.load_token
    }

    pub(super) fn forget_load(&mut self, id: FrameId) {
        if let Route::Carousel(carousel_id) = self.route {
            self.loading.remove(&(carousel_id, id));
        }
    }

    pub(super) fn apply_incoming(&mut self, ctx: &egui::Context, message: IncomingPhoto) {
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
            } => self.apply_preview(ctx, token, carousel_id, frame_id, &preview),
            IncomingPhoto::CoverPreview {
                token,
                template_id,
                preview,
            } => self.apply_cover_preview(ctx, token, template_id, &preview),
        }
    }

    pub(super) fn apply_decoded(
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

    pub(super) fn finish_frame_photo(
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
            self.upload_asset(ctx, frame_id, &name, image, &decoded.preview);
            self.selected = Some(frame_id);
        }
        self.status = format!("{name} added");
        self.note_geometry();
    }

    pub(super) fn apply_preview(
        &mut self,
        ctx: &egui::Context,
        token: u64,
        carousel_id: u64,
        frame_id: FrameId,
        preview: &RgbaImage,
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

    pub(super) fn apply_cover_preview(
        &mut self,
        ctx: &egui::Context,
        token: u64,
        template_id: u64,
        preview: &RgbaImage,
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
            texture_from_preview(ctx, format!("cover-{template_id}"), preview),
        );
    }

    pub(super) fn spawn_estimate(&mut self) {
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

    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) {
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

    pub(super) fn delete_selection(&mut self) {
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

    pub(super) fn upload_asset(
        &mut self,
        ctx: &egui::Context,
        id: FrameId,
        name: &str,
        image: Arc<RgbaImage>,
        preview: &RgbaImage,
    ) {
        let texture = texture_from_preview(ctx, name, preview);
        self.assets.insert(id, PhotoAsset { image, texture });
    }

    pub(super) fn reload_assets(&mut self, ctx: &egui::Context) {
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
                self.upload_asset(ctx, frame_id, &name, image, &preview);
            }
        }
    }

    pub(super) fn fill_frame(&mut self, ctx: &egui::Context, id: FrameId) {
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

    pub(super) fn spawn_decode(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        target: DecodeTarget,
    ) {
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

    pub(super) fn spawn_preview(
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

    pub(super) fn remove_photo(&mut self, id: FrameId) {
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

    pub(super) fn remove_frame(&mut self, id: FrameId) {
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

    pub(super) fn enter(&mut self, route: Route, ctx: &egui::Context) {
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
        self.editor_zoom = DEFAULT_EDITOR_ZOOM;
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

    pub(super) fn go_home(&mut self) {
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

    pub(super) fn start_export(&mut self) {
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
}
