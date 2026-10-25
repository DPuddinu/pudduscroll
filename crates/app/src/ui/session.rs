use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use pudduscroll_core::{
    AspectRatio, CANVAS_GRID, ExportFormat, ExportSettings, FrameId, FrameImages, Layout,
    decode_frame_images, export_layout_canvases_with, layout_export_width,
};

use crate::library::{Library, SaveRequest, library_directory, load_blob, write_snapshot};

use super::draw::{apply_style, export_folder, pick_image_path, save_folder, texture_from_preview};
use super::{
    DEFAULT_EDITOR_ZOOM, DEFAULT_RAIL_WEIGHTS, DecodeTarget, EstimateMsg, EstimateUpdate,
    ExportUpdate, IncomingPhoto, PendingUpload, PhotoAsset, PudduScrollApp, Route, UNDO_LIMIT,
};

impl PudduScrollApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        let (photo_tx, photo_rx) = mpsc::channel();
        let save_tx = spawn_saver(photo_tx.clone());
        let load_tx = photo_tx.clone();
        thread::spawn(move || {
            let _ = load_tx.send(IncomingPhoto::Library(Library::load()));
        });
        Self {
            library: Library::empty(),
            library_ready: false,
            can_store: false,
            route: Route::Home,
            assets: HashMap::new(),
            cover_textures: HashMap::new(),
            card_previews: HashMap::new(),
            selected: None,
            edit_frame: false,
            free_rotate: false,
            quality: 90,
            format: ExportFormat::Jpeg,
            preview_index: 0,
            preview_texture: None,
            preview_key: None,
            preview_job: None,
            card_jobs: HashMap::new(),
            geometry_revision: 0,
            drag: None,
            gesture: "none".to_string(),
            status: "Opening library…".to_string(),
            estimate_mb: None,
            estimate_for: None,
            estimate_canvases: None,
            estimate_after: None,
            estimate_label: String::new(),
            estimate_done: 0,
            estimate_rx: None,
            export_rx: None,
            export_done: 0,
            photo_tx,
            photo_rx,
            save_tx,
            pending_uploads: VecDeque::new(),
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
            grid_cells: CANVAS_GRID as i32,
            rail_weights: DEFAULT_RAIL_WEIGHTS,
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
        if !self.can_store {
            return;
        }
        let snapshot = self.library.snapshot();
        let _ = self.save_tx.send(SaveRequest {
            snapshot,
            ack: None,
        });
    }

    pub(super) fn flush_store(&mut self) {
        if !self.can_store {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let snapshot = self.library.snapshot();
        if self
            .save_tx
            .send(SaveRequest {
                snapshot,
                ack: Some(tx),
            })
            .is_ok()
            && let Ok(Err(error)) = rx.recv()
        {
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
        self.estimate_label.clear();
        self.estimate_done = 0;
    }

    pub(super) fn proxy_map(&self) -> FrameImages {
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
            .map(|(frame_id, blob)| (*frame_id, Arc::clone(&blob.proxy)))
            .collect()
    }

    fn original_bytes(&self) -> HashMap<FrameId, Arc<[u8]>> {
        let Route::Carousel(id) = self.route else {
            return HashMap::new();
        };
        let Some(carousel) = self
            .library
            .carousels
            .iter()
            .find(|carousel| carousel.id == id)
        else {
            return HashMap::new();
        };
        carousel
            .photos
            .iter()
            .map(|(frame_id, blob)| (*frame_id, Arc::clone(&blob.bytes)))
            .collect()
    }

    pub(super) fn poll_jobs(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.photo_rx.try_recv() {
            self.apply_incoming(message);
        }
        let estimate_msg = self.poll_estimate();
        if let Some(msg) = estimate_msg {
            if msg.geometry == self.geometry_revision
                && msg.quality == self.quality
                && msg.format == self.format
            {
                match msg.result {
                    Ok(canvases) => {
                        let bytes: usize = canvases.iter().map(|canvas| canvas.bytes.len()).sum();
                        self.estimate_mb = Some(bytes as f64 / 1_048_576.0);
                        self.estimate_for = Some((msg.geometry, msg.quality, msg.format));
                        self.estimate_canvases = Some(canvases);
                    }
                    Err(error) => {
                        self.status = error;
                        self.estimate_mb = None;
                        self.estimate_canvases = None;
                    }
                }
            }
        }
        let estimate_current =
            self.estimate_for == Some((self.geometry_revision, self.quality, self.format));
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
        self.poll_export();
        self.upload_one(ctx);
        if self.photos_busy() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.estimate_rx.is_some() || self.export_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn poll_estimate(&mut self) -> Option<EstimateMsg> {
        let (updates, closed) = self.drain_estimate();
        let mut ready = None;
        for update in updates {
            match update {
                EstimateUpdate::Decoding { done, total } => {
                    self.estimate_label = format!("Decoding {done} of {total}");
                }
                EstimateUpdate::Slide { done, total } => {
                    if done >= self.estimate_done {
                        self.estimate_done = done;
                        self.estimate_label = if done == 0 {
                            if total == 1 {
                                "Estimating 1 slide".to_string()
                            } else {
                                format!("Estimating {total} slides")
                            }
                        } else {
                            format!("Estimating {done} of {total}")
                        };
                    }
                }
                EstimateUpdate::Ready(msg) => ready = Some(msg),
            }
        }
        if closed || ready.is_some() {
            self.estimate_rx = None;
            if ready.is_some() {
                self.estimate_label.clear();
            }
        }
        ready
    }

    fn drain_estimate(&self) -> (Vec<EstimateUpdate>, bool) {
        let Some(rx) = &self.estimate_rx else {
            return (Vec::new(), false);
        };
        let mut updates = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(update) => updates.push(update),
                Err(mpsc::TryRecvError::Empty) => return (updates, false),
                Err(mpsc::TryRecvError::Disconnected) => return (updates, true),
            }
        }
    }

    fn poll_export(&mut self) {
        let (updates, closed) = self.drain_export();
        let mut finished = None;
        for update in updates {
            match update {
                ExportUpdate::Decoding { done, total } => {
                    self.status = format!("Decoding {done} of {total}");
                }
                ExportUpdate::Exporting { done, total } => {
                    if done >= self.export_done {
                        self.export_done = done;
                        self.status = if done == 0 {
                            if total == 1 {
                                "Exporting 1 slide".to_string()
                            } else {
                                format!("Exporting {total} slides")
                            }
                        } else {
                            format!("Exporting {done} of {total}")
                        };
                    }
                }
                ExportUpdate::Saving => {
                    self.status = "Saving…".to_string();
                }
                ExportUpdate::Finished(result) => finished = Some(result),
            }
        }
        if closed && finished.is_none() {
            finished = Some(Err("Export stopped before it finished.".to_string()));
        }
        if let Some(result) = finished {
            self.export_rx = None;
            self.exporting = false;
            self.status = match result {
                Ok(message) => message,
                Err(error) => error,
            };
        }
    }

    fn drain_export(&self) -> (Vec<ExportUpdate>, bool) {
        let Some(rx) = &self.export_rx else {
            return (Vec::new(), false);
        };
        let mut updates = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(update) => updates.push(update),
                Err(mpsc::TryRecvError::Empty) => return (updates, false),
                Err(mpsc::TryRecvError::Disconnected) => return (updates, true),
            }
        }
    }

    pub(super) fn photos_busy(&self) -> bool {
        !self.library_ready
            || !self.loading.is_empty()
            || self.cover_load.is_some()
            || !self.cover_pending.is_empty()
            || self.preview_job.is_some()
            || !self.card_jobs.is_empty()
            || !self.pending_uploads.is_empty()
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

    pub(super) fn apply_incoming(&mut self, message: IncomingPhoto) {
        match message {
            IncomingPhoto::Library(result) => self.apply_library(result),
            IncomingPhoto::Decoded {
                token,
                target,
                result,
            } => self.apply_decoded(token, target, result),
            IncomingPhoto::PhonePreview {
                revision,
                index,
                image,
            } => {
                if self.preview_job == Some((revision, index)) {
                    self.pending_uploads.push_back(PendingUpload::Phone {
                        revision,
                        index,
                        image,
                    });
                }
            }
            IncomingPhoto::CardPreview {
                carousel_id,
                visuals,
                image,
            } => {
                if self.card_jobs.get(&carousel_id) == Some(&visuals) {
                    self.pending_uploads.push_back(PendingUpload::Card {
                        carousel_id,
                        visuals,
                        image,
                    });
                }
            }
            IncomingPhoto::Saved { generation, result } => match result {
                Ok(()) => self.library.stored_generation(generation),
                Err(error) => self.status = error,
            },
        }
    }

    fn apply_library(&mut self, result: Result<Library, String>) {
        if self.library_ready {
            return;
        }
        match result {
            Ok(library) => {
                self.library = library;
                self.can_store = true;
                self.status.clear();
            }
            Err(error) => {
                self.can_store = false;
                self.status = error;
            }
        }
        self.library_ready = true;
    }

    pub(super) fn apply_decoded(
        &mut self,
        token: u64,
        target: DecodeTarget,
        result: Result<crate::library::Blob, String>,
    ) {
        match target {
            DecodeTarget::Frame {
                carousel_id,
                frame_id,
            } => {
                if self.loading.get(&(carousel_id, frame_id)) != Some(&token) {
                    return;
                }
                match result {
                    Ok(blob) => {
                        if !self.finish_frame_photo(carousel_id, frame_id, token, blob) {
                            self.loading.remove(&(carousel_id, frame_id));
                        }
                    }
                    Err(error) => {
                        self.loading.remove(&(carousel_id, frame_id));
                        self.status = error;
                    }
                }
            }
            DecodeTarget::Cover => {
                if self.cover_load != Some(token) {
                    return;
                }
                match result {
                    Ok(blob) => {
                        let name = blob.name.clone();
                        let proxy = Arc::clone(&blob.proxy);
                        self.create_cover = Some(blob);
                        self.pending_uploads
                            .push_back(PendingUpload::CreateCover { token, proxy });
                        self.status = format!("{name} ready");
                    }
                    Err(error) => {
                        self.cover_load = None;
                        self.status = error;
                    }
                }
            }
        }
    }

    pub(super) fn finish_frame_photo(
        &mut self,
        carousel_id: u64,
        frame_id: FrameId,
        token: u64,
        blob: crate::library::Blob,
    ) -> bool {
        let exists = self.library.carousels.iter().any(|carousel| {
            carousel.id == carousel_id && carousel.layout.frame(frame_id).is_some()
        });
        if !exists {
            return false;
        }
        let name = blob.name.clone();
        let proxy = Arc::clone(&blob.proxy);
        self.library.set_photo(carousel_id, frame_id, blob);
        let queued = self.route == Route::Carousel(carousel_id);
        if queued {
            self.pending_uploads.push_back(PendingUpload::Frame {
                carousel_id,
                frame_id,
                token,
                name: name.clone(),
                proxy,
            });
            self.selected = Some(frame_id);
        }
        self.status = format!("{name} added");
        self.note_geometry();
        queued
    }

    pub(super) fn queue_cover(&mut self, template_id: u64, proxy: Arc<image::RgbaImage>) {
        let token = self.next_token();
        self.cover_pending.insert(template_id, token);
        self.pending_uploads.push_back(PendingUpload::Cover {
            template_id,
            token,
            proxy,
        });
    }

    pub(super) fn spawn_estimate(&mut self) {
        self.estimate_after = None;
        self.estimate_done = 0;
        self.estimate_label.clear();
        let Some(layout) = self.layout().cloned() else {
            return;
        };
        let encoded = self.original_bytes();
        let quality = self.quality;
        let format = self.format;
        let geometry = self.geometry_revision;
        let (tx, rx) = mpsc::channel();
        let progress = tx.clone();
        thread::spawn(move || {
            let result = decode_frame_images(&encoded, &|done, total| {
                let _ = progress.send(EstimateUpdate::Decoding { done, total });
            })
            .and_then(|images| {
                let slides = progress.clone();
                let _ = slides.send(EstimateUpdate::Slide {
                    done: 0,
                    total: layout.canvas_count,
                });
                export_layout_canvases_with(
                    &layout,
                    &images,
                    ExportSettings {
                        width: layout_export_width(&layout),
                        quality,
                        format,
                    },
                    &|done, total| {
                        let _ = slides.send(EstimateUpdate::Slide { done, total });
                    },
                )
                .map(Arc::from)
            })
            .map_err(|error| error.to_string());
            let _ = tx.send(EstimateUpdate::Ready(EstimateMsg {
                geometry,
                quality,
                format,
                result,
            }));
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
        if !matches!(self.route, Route::Home) {
            ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            });
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Delete)) {
            self.delete_selection();
        }
        if !self.export_open {
            return;
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

    fn upload_one(&mut self, ctx: &egui::Context) {
        while let Some(pending) = self.pending_uploads.pop_front() {
            if self.apply_upload(ctx, pending) {
                return;
            }
        }
    }

    fn apply_upload(&mut self, ctx: &egui::Context, pending: PendingUpload) -> bool {
        match pending {
            PendingUpload::Frame {
                carousel_id,
                frame_id,
                token,
                name,
                proxy,
            } => {
                if self.loading.get(&(carousel_id, frame_id)) != Some(&token) {
                    return false;
                }
                if self.route != Route::Carousel(carousel_id) {
                    self.loading.remove(&(carousel_id, frame_id));
                    return false;
                }
                let texture = texture_from_preview(ctx, &name, &proxy);
                self.assets.insert(frame_id, PhotoAsset { texture });
                self.loading.remove(&(carousel_id, frame_id));
                true
            }
            PendingUpload::Cover {
                template_id,
                token,
                proxy,
            } => {
                if self.cover_pending.get(&template_id) != Some(&token) {
                    return false;
                }
                let present = self
                    .library
                    .templates
                    .iter()
                    .any(|template| template.id == template_id);
                if present {
                    self.cover_textures.insert(
                        template_id,
                        texture_from_preview(ctx, format!("cover-{template_id}"), &proxy),
                    );
                }
                self.cover_pending.remove(&template_id);
                present
            }
            PendingUpload::CreateCover { token, proxy } => {
                if self.cover_load != Some(token) {
                    return false;
                }
                self.create_cover_texture = Some(texture_from_preview(ctx, "create-cover", &proxy));
                self.cover_load = None;
                true
            }
            PendingUpload::Phone {
                revision,
                index,
                image,
            } => {
                let current = self.preview_job == Some((revision, index));
                if !self.export_open
                    || self.geometry_revision != revision
                    || self.preview_index != index
                {
                    if current {
                        self.preview_job = None;
                    }
                    return false;
                }
                self.preview_texture = Some(texture_from_preview(
                    ctx,
                    format!("preview-{revision}-{index}"),
                    &image,
                ));
                self.preview_key = Some((revision, index));
                self.preview_job = None;
                true
            }
            PendingUpload::Card {
                carousel_id,
                visuals,
                image,
            } => {
                if self.card_jobs.get(&carousel_id) == Some(&visuals) {
                    self.card_jobs.remove(&carousel_id);
                }
                if self.library.visuals != visuals
                    || self
                        .library
                        .carousels
                        .iter()
                        .all(|carousel| carousel.id != carousel_id)
                {
                    return false;
                }
                self.card_previews.insert(
                    carousel_id,
                    (
                        visuals,
                        texture_from_preview(
                            ctx,
                            format!("carousel-{carousel_id}-{visuals}"),
                            &image,
                        ),
                    ),
                );
                true
            }
        }
    }

    pub(super) fn reload_assets(&mut self, ctx: &egui::Context) {
        self.assets.clear();
        self.pending_uploads
            .retain(|pending| !matches!(pending, PendingUpload::Frame { .. }));
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
        let loaded: Vec<(FrameId, String, Arc<image::RgbaImage>)> = carousel
            .photos
            .iter()
            .map(|(frame_id, blob)| (*frame_id, blob.name.clone(), Arc::clone(&blob.proxy)))
            .collect();
        for (frame_id, name, proxy) in loaded {
            if self.loading.contains_key(&(carousel_id, frame_id)) {
                continue;
            }
            let token = self.next_token();
            self.loading.insert((carousel_id, frame_id), token);
            self.pending_uploads.push_back(PendingUpload::Frame {
                carousel_id,
                frame_id,
                token,
                name,
                proxy,
            });
        }
        ctx.request_repaint();
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
            let result = load_blob(&path);
            let _ = tx.send(IncomingPhoto::Decoded {
                token,
                target,
                result,
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
        self.free_rotate = false;
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
        self.free_rotate = false;
        self.drag = None;
        self.gesture = "none".to_string();
        self.crop_session = None;
        self.crop_press = None;
        self.preview_index = 0;
        self.preview_texture = None;
        self.preview_key = None;
        self.preview_job = None;
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
        self.free_rotate = false;
        self.export_open = false;
        self.assets.clear();
        self.preview_job = None;
        self.pending_uploads.retain(|pending| {
            !matches!(
                pending,
                PendingUpload::Frame { .. } | PendingUpload::Phone { .. }
            )
        });
        self.plus_rects.clear();
    }

    pub(super) fn start_export(&mut self) {
        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let Some(layout) = self.layout().cloned() else {
            return;
        };
        let quality = self.quality;
        let format = self.format;
        let (tx, rx) = mpsc::channel();
        self.exporting = true;
        self.export_done = 0;
        self.status = "Exporting…".to_string();
        self.export_rx = Some(rx);
        let estimated = self
            .estimate_canvases
            .clone()
            .filter(|_| self.estimate_for == Some((self.geometry_revision, quality, format)));
        if let Some(canvases) = estimated {
            thread::spawn(move || {
                let _ = tx.send(ExportUpdate::Saving);
                let _ = tx.send(ExportUpdate::Finished(save_folder(&dir, &canvases, format)));
            });
            return;
        }
        let encoded = self.original_bytes();
        let progress = tx.clone();
        thread::spawn(move || {
            let outcome = (|| {
                let images = decode_frame_images(&encoded, &|done, total| {
                    let _ = progress.send(ExportUpdate::Decoding { done, total });
                })
                .map_err(|error| error.to_string())?;
                let slides = progress.clone();
                let _ = slides.send(ExportUpdate::Exporting {
                    done: 0,
                    total: layout.canvas_count,
                });
                export_folder(
                    &layout,
                    &images,
                    quality,
                    format,
                    &dir,
                    &|done, total| {
                        let _ = slides.send(ExportUpdate::Exporting { done, total });
                    },
                    &|| {
                        let _ = progress.send(ExportUpdate::Saving);
                    },
                )
            })();
            let _ = tx.send(ExportUpdate::Finished(outcome));
        });
    }
}

fn spawn_saver(photo_tx: Sender<IncomingPhoto>) -> Sender<SaveRequest> {
    let (tx, rx) = mpsc::channel::<SaveRequest>();
    thread::spawn(move || {
        let root = library_directory();
        while let Ok(request) = rx.recv() {
            let generation = request.snapshot.generation;
            let had_images = request.snapshot.images.is_some();
            let result = write_snapshot(&root, &request.snapshot);
            if had_images {
                let _ = photo_tx.send(IncomingPhoto::Saved {
                    generation,
                    result: result.clone(),
                });
            }
            if let Some(ack) = request.ack {
                let _ = ack.send(result);
            }
        }
    });
    tx
}
