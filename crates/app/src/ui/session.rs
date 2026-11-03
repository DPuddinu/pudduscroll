use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use pudduscroll_core::existing_export_files;
use pudduscroll_core::{
    AspectRatio, ExportSettings, FrameId, FrameImages, Layout, decode_frame_images,
    export_layout_canvases_with, layout_export_width,
};

use crate::host::PickedFile;
#[cfg(target_arch = "wasm32")]
use crate::library::pasted_image;
use crate::library::{
    Carousel, DEFAULT_GRID_CELLS, Library, SaveRequest, blob_from_bytes, carousel_package,
    clamp_grid_cells, clipboard_has_image, clipboard_image, clipboard_image_stamp,
    direct_image_url, library_directory, linked_image, load_blob, read_carousel_package,
    write_snapshot,
};

use super::draw::{
    apply_style, carousel_preview_image, export_folder, save_folder, texture_from_preview,
    zip_file_name,
};
use super::history::{Edit, EditHistory, HISTORY_WINDOW, HistoryKind, photo_stamp};
use super::{
    Confirm, DEFAULT_EDITOR_ZOOM, DEFAULT_RAIL_WEIGHTS, DecodeTarget, EstimateMsg, EstimateUpdate,
    ExportUpdate, IncomingPhoto, PendingUpload, PhotoAsset, PudduScrollApp, Route, UNDO_LIMIT,
};

impl PudduScrollApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let app = Self::assemble(&cc.egui_ctx);
        let load_tx = app.photo_tx.clone();
        let ctx = cc.egui_ctx.clone();
        crate::host::boot(move || {
            let _ = load_tx.send(IncomingPhoto::Library(Library::load()));
            ctx.request_repaint();
        });
        app
    }

    pub(super) fn assemble(ctx: &egui::Context) -> Self {
        apply_style(ctx);
        let (photo_tx, photo_rx) = mpsc::channel();
        let save_tx = spawn_saver(photo_tx.clone());
        Self {
            library: Library::empty(),
            library_ready: false,
            can_store: false,
            route: Route::Home,
            assets: HashMap::new(),
            cover_textures: HashMap::new(),
            selected: None,
            active_canvas: None,
            edit_frame: false,
            free_rotate: false,
            settings: crate::settings::AppSettings::load(),
            settings_open: false,
            rebind: None,
            preview_index: 0,
            preview_texture: None,
            preview_key: None,
            preview_job: None,
            geometry_revision: 0,
            drag: None,
            gesture: "none".to_string(),
            status: String::new(),
            toaster: super::toast::Toaster::default(),
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
            update_rx: crate::update::start(),
            save_tx,
            pending_uploads: VecDeque::new(),
            loading: HashMap::new(),
            cover_load: None,
            cover_pending: HashMap::new(),
            load_token: 0,
            painted: 0,
            exporting: false,
            export_open: false,
            overwrite_dir: None,
            overwrite_names: Vec::new(),
            editor_zoom: DEFAULT_EDITOR_ZOOM,
            zoom_basis: (4, 5),
            strip_pan: egui::Vec2::ZERO,
            reveal_canvas: None,
            strip_wheel_pan: false,
            center_strip: true,
            strip_rect: None,
            view_rect: None,
            history: EditHistory::new(UNDO_LIMIT),
            drag_origin: None,
            crop_session: None,
            crop_press: None,
            show_grid: true,
            snap_to_grid: true,
            grid_cells: DEFAULT_GRID_CELLS,
            rail_weights: DEFAULT_RAIL_WEIGHTS,
            create_open: false,
            confirm: None,
            link_open: false,
            link_url: String::new(),
            sharing: false,
            importing: false,
            paste_stamp: None,
            paste_mark: None,
            paste_job: None,
            paste_focused: false,
            paste_look: 0,
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

    pub(super) fn grid_cells(&self) -> i32 {
        let cells = match self.route {
            Route::Carousel(id) => self
                .library
                .carousels
                .iter()
                .find(|carousel| carousel.id == id)
                .map_or(self.grid_cells, |carousel| carousel.grid_cells),
            Route::Home | Route::Template(_) => self.grid_cells,
        };
        clamp_grid_cells(cells)
    }

    pub(super) fn set_grid_cells(&mut self, cells: i32) {
        let cells = clamp_grid_cells(cells);
        if let Route::Carousel(id) = self.route
            && let Some(carousel) = self
                .library
                .carousels
                .iter_mut()
                .find(|carousel| carousel.id == id)
        {
            carousel.grid_cells = cells;
            return;
        }
        self.grid_cells = cells;
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
        let snapshot = self.library.snapshot();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = self.save_tx.send(SaveRequest {
                snapshot,
                ack: None,
            });
            drive_saver();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = mpsc::channel();
            if self
                .save_tx
                .send(SaveRequest {
                    snapshot,
                    ack: Some(tx),
                })
                .is_ok()
                && let Ok(Err(error)) = rx.recv()
            {
                self.toast(error);
            }
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

    pub(super) fn checkpoint(&mut self) {
        self.seal_drag();
        let Some(edit) = self.capture_edit() else {
            return;
        };
        self.history.checkpoint(edit);
        self.dismiss_undo_toasts();
    }

    pub(super) fn edit_coalesced(&mut self, kind: HistoryKind) {
        let now = Instant::now();
        if self.history.absorb(kind, now, HISTORY_WINDOW) {
            return;
        }
        self.checkpoint();
        self.history.arm(kind, now);
    }

    pub(super) fn edit_grid(&mut self, previous: i32, updated: i32) {
        let now = Instant::now();
        if self.history.absorb(HistoryKind::Grid, now, HISTORY_WINDOW) {
            self.set_grid_cells(updated);
            return;
        }
        self.set_grid_cells(previous);
        self.checkpoint();
        self.history.arm(HistoryKind::Grid, now);
        self.set_grid_cells(updated);
    }

    pub(super) fn begin_history_drag(&mut self) {
        self.seal_drag();
        self.drag_origin = self.capture_edit();
    }

    pub(super) fn seal_drag(&mut self) {
        let Some(before) = self.drag_origin.take() else {
            return;
        };
        let Some(current) = self.capture_edit() else {
            return;
        };
        if before.differs(&current) {
            self.history.checkpoint(before);
        }
    }

    pub(super) fn zoom_origin(&mut self) -> Option<Edit> {
        let now = Instant::now();
        if self.history.absorb(HistoryKind::Zoom, now, HISTORY_WINDOW) {
            None
        } else {
            self.seal_drag();
            self.capture_edit()
        }
    }

    pub(super) fn remember_zoom(&mut self, before: Option<Edit>) {
        let Some(before) = before else {
            return;
        };
        self.history.checkpoint(before);
        self.history.arm(HistoryKind::Zoom, Instant::now());
    }

    fn capture_edit(&self) -> Option<Edit> {
        let layout = self.layout()?.clone();
        let photos = match self.route {
            Route::Carousel(id) => self
                .library
                .carousels
                .iter()
                .find(|carousel| carousel.id == id)
                .map(|carousel| carousel.photos.clone())
                .unwrap_or_default(),
            Route::Home | Route::Template(_) => HashMap::new(),
        };
        Some(Edit {
            layout,
            photos,
            grid_cells: self.grid_cells(),
        })
    }

    fn restore_edit(&mut self, edit: Edit, ctx: &egui::Context) {
        let before = self.current_photo_stamp();
        match self.route {
            Route::Template(id) => {
                if let Some(template) = self
                    .library
                    .templates
                    .iter_mut()
                    .find(|template| template.id == id)
                {
                    template.layout = edit.layout;
                }
            }
            Route::Carousel(id) => {
                if let Some(carousel) = self
                    .library
                    .carousels
                    .iter_mut()
                    .find(|carousel| carousel.id == id)
                {
                    carousel.layout = edit.layout;
                    carousel.photos = edit.photos;
                }
            }
            Route::Home => return,
        }
        self.set_grid_cells(edit.grid_cells);
        let selected_ok = self.selected.is_some_and(|id| {
            self.layout()
                .is_some_and(|layout| layout.frame(id).is_some())
        });
        if !selected_ok {
            self.selected = None;
            self.edit_frame = false;
        }
        self.clamp_active_canvas();
        self.drag = None;
        self.drag_origin = None;
        self.crop_session = None;
        self.crop_press = None;
        if self.current_photo_stamp() != before {
            self.library.touch_images();
            self.reload_assets(ctx);
        }
        self.note_geometry();
    }

    fn current_photo_stamp(&self) -> Vec<(u64, usize)> {
        let Route::Carousel(id) = self.route else {
            return Vec::new();
        };
        self.library
            .carousels
            .iter()
            .find(|carousel| carousel.id == id)
            .map(|carousel| photo_stamp(&carousel.photos))
            .unwrap_or_default()
    }

    pub(super) fn undo_edit(&mut self, ctx: &egui::Context) {
        self.seal_drag();
        let Some(current) = self.capture_edit() else {
            return;
        };
        let Some(previous) = self.history.take_undo(current) else {
            return;
        };
        self.dismiss_undo_toasts();
        self.restore_edit(previous, ctx);
        self.status = "Undone".to_string();
    }

    pub(super) fn redo_edit(&mut self, ctx: &egui::Context) {
        self.seal_drag();
        let Some(current) = self.capture_edit() else {
            return;
        };
        let Some(next) = self.history.take_redo(current) else {
            return;
        };
        self.dismiss_undo_toasts();
        self.restore_edit(next, ctx);
        self.status = "Redone".to_string();
    }

    pub(super) fn schedule_estimate(&mut self) {
        self.estimate_after = Some(Instant::now() + Duration::from_millis(280));
        self.estimate_label.clear();
        self.estimate_done = 0;
    }

    pub(super) fn open_export(&mut self) {
        self.preview_index = 0;
        self.preview_texture = None;
        self.preview_key = None;
        self.preview_job = None;
        self.export_open = true;
        self.schedule_estimate();
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
            .map(|(frame_id, blob)| (*frame_id, blob.screen()))
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
        #[cfg(target_arch = "wasm32")]
        drive_saver();
        if let Some(error) = crate::disk::take_error() {
            self.toast(error);
        }
        while let Ok(message) = self.photo_rx.try_recv() {
            self.apply_incoming(message);
        }
        let estimate_msg = self.poll_estimate();
        if let Some(msg) = estimate_msg {
            if msg.geometry == self.geometry_revision
                && msg.quality == self.settings.quality
                && msg.format == self.settings.format
            {
                match msg.result {
                    Ok(canvases) => {
                        let bytes: usize = canvases.iter().map(|canvas| canvas.bytes.len()).sum();
                        self.estimate_mb = Some(bytes as f64 / 1_048_576.0);
                        self.estimate_for = Some((msg.geometry, msg.quality, msg.format));
                        self.estimate_canvases = Some(canvases);
                    }
                    Err(error) => {
                        self.toast(error);
                        self.estimate_mb = None;
                        self.estimate_canvases = None;
                    }
                }
            }
        }
        let estimate_current = self.estimate_for
            == Some((
                self.geometry_revision,
                self.settings.quality,
                self.settings.format,
            ));
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
        self.poll_update(ctx);
        self.upload_one(ctx);
        if !self.library_ready || self.photos_busy() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.estimate_rx.is_some() || self.export_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn poll_update(&mut self, ctx: &egui::Context) {
        let (events, waiting) = self.drain_update();
        let mut close = false;
        for event in events {
            match event {
                crate::update::Event::Installing(version) => {
                    self.status = format!("Updating to {version}…");
                    self.toast(format!("Updating to {version}…"));
                }
                crate::update::Event::Restart => close = true,
                crate::update::Event::Failed(message) => {
                    if self.status.starts_with("Updating to ") {
                        self.status.clear();
                    }
                    self.toast(message);
                    self.update_rx = None;
                }
            }
        }
        if close {
            self.update_rx = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if waiting && self.update_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn drain_update(&mut self) -> (Vec<crate::update::Event>, bool) {
        let mut events = Vec::new();
        let mut waiting = false;
        let mut closed = false;
        if let Some(rx) = &self.update_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(TryRecvError::Empty) => {
                        waiting = true;
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        closed = true;
                        break;
                    }
                }
            }
        }
        if closed {
            self.update_rx = None;
        }
        (events, waiting)
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
            match result {
                Ok(message) => self.status = message,
                Err(error) => {
                    self.status.clear();
                    self.toast(error);
                }
            }
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
            IncomingPhoto::Chosen { target, picked } => self.apply_chosen(target, picked),
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
            IncomingPhoto::Saved { generation, result } => match result {
                Ok(()) => self.library.stored_generation(generation),
                Err(error) => self.toast(error),
            },
            IncomingPhoto::Shared(result) => {
                self.sharing = false;
                match result {
                    Ok(message) => self.status = message,
                    Err(error) => {
                        self.status.clear();
                        self.toast(error);
                    }
                }
            }
            IncomingPhoto::Imported(result) => {
                self.importing = false;
                let Some(result) = result else {
                    self.status.clear();
                    return;
                };
                match result {
                    Ok(carousel) => {
                        let name = carousel.name.clone();
                        self.library.import_carousel(carousel);
                        self.note_geometry();
                        self.status = format!("{name} imported");
                    }
                    Err(error) => {
                        self.status.clear();
                        self.toast(error);
                    }
                }
            }
        }
    }

    fn apply_library(&mut self, result: Result<Library, String>) {
        if self.library_ready {
            return;
        }
        match result {
            Ok(library) => {
                #[cfg(target_arch = "wasm32")]
                {
                    self.settings = crate::settings::AppSettings::load();
                }
                self.library = library;
                self.can_store = true;
            }
            Err(error) => {
                self.can_store = false;
                self.toast_persist(error);
            }
        }
        self.library_ready = true;
    }

    fn apply_chosen(&mut self, target: DecodeTarget, picked: PickedFile) {
        let token = self.next_token();
        match target {
            DecodeTarget::Frame {
                carousel_id,
                frame_id,
            }
            | DecodeTarget::DiskFrame {
                carousel_id,
                frame_id,
            } => {
                if self.loading.contains_key(&(carousel_id, frame_id)) {
                    return;
                }
                self.loading.insert((carousel_id, frame_id), token);
                self.status = "Loading photo…".to_string();
            }
            DecodeTarget::Cover => {
                self.cover_load = Some(token);
                self.status = "Loading cover…".to_string();
            }
        }
        let tx = self.photo_tx.clone();
        crate::host::spawn(move || {
            let result = match picked {
                PickedFile::Path(path) => load_blob(&path),
                PickedFile::Bytes { name, bytes } => blob_from_bytes(&name, bytes),
            };
            let _ = tx.send(IncomingPhoto::Decoded {
                token,
                target,
                result,
            });
        });
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
                        let placed = self.finish_frame_photo(carousel_id, frame_id, token, blob);
                        if !placed {
                            self.loading.remove(&(carousel_id, frame_id));
                        }
                        self.finish_paste_job(token, frame_id, placed);
                    }
                    Err(error) => {
                        self.loading.remove(&(carousel_id, frame_id));
                        self.status.clear();
                        self.toast(error);
                        self.finish_paste_job(token, frame_id, false);
                    }
                }
            }
            DecodeTarget::DiskFrame {
                carousel_id,
                frame_id,
            } => {
                if self.loading.get(&(carousel_id, frame_id)) != Some(&token) {
                    return;
                }
                match result {
                    Ok(blob) => {
                        if !self.finish_disk_photo(carousel_id, frame_id, token, blob) {
                            self.loading.remove(&(carousel_id, frame_id));
                        }
                    }
                    Err(error) => {
                        self.loading.remove(&(carousel_id, frame_id));
                        self.status.clear();
                        self.toast(error);
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
                        let proxy = blob.screen();
                        self.create_cover = Some(blob);
                        self.pending_uploads
                            .push_back(PendingUpload::CreateCover { token, proxy });
                        self.status = format!("{name} ready");
                    }
                    Err(error) => {
                        self.cover_load = None;
                        self.status.clear();
                        self.toast(error);
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
        let proxy = blob.screen();
        self.checkpoint();
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

    fn finish_disk_photo(
        &mut self,
        carousel_id: u64,
        frame_id: FrameId,
        token: u64,
        blob: crate::library::Blob,
    ) -> bool {
        let Some(carousel) = self
            .library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
        else {
            return false;
        };
        if carousel.layout.frame(frame_id).is_none() {
            return false;
        }
        let name = blob.name.clone();
        let proxy = blob.screen();
        carousel.photos.insert(frame_id, blob);
        if self.route != Route::Carousel(carousel_id) {
            return false;
        }
        self.pending_uploads.push_back(PendingUpload::Frame {
            carousel_id,
            frame_id,
            token,
            name,
            proxy,
        });
        true
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
        let quality = self.settings.quality;
        let format = self.settings.format;
        let geometry = self.geometry_revision;
        let (tx, rx) = mpsc::channel();
        let progress = tx.clone();
        crate::host::spawn(move || {
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
        if self.capture_rebind(ctx) {
            return;
        }
        if self.overwrite_dir.is_some() || self.confirm.is_some() {
            return;
        }
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
        let paste = matches!(self.route, Route::Carousel(_))
            && self.selected.is_some()
            && self.crop_session.is_none()
            && !self.export_open
            && !self.link_open
            && !self.create_open
            && !self.settings_open
            && !ctx.text_edit_focused();
        let (paste_hit, pasted) =
            ctx.input_mut(|input| take_paste(input, self.paste_stamp.is_some()));
        if paste && paste_hit {
            self.paste_hit(ctx, pasted);
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let ran = self.dispatch_shortcuts(ctx);
        if !matches!(self.route, Route::Home) && !self.settings_open {
            ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            });
        }
        if ran || !self.export_open {
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

    pub(super) fn removable_canvases(&self) -> Option<Vec<usize>> {
        let layout = self.layout()?;
        let active = self.active_canvas?;
        (layout.canvas_count > 1 && active < layout.canvas_count).then_some(vec![active])
    }

    pub(super) fn delete_selection(&mut self) {
        if let Some(chosen) = self.removable_canvases() {
            self.confirm = Some(Confirm::RemoveCanvases(chosen));
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        let has_photo = self
            .layout()
            .and_then(|layout| layout.frame(id))
            .is_some_and(|frame| frame.photo.is_some());
        self.confirm = Some(if has_photo {
            Confirm::RemovePhoto(id)
        } else {
            Confirm::RemoveFrame(id)
        });
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
                    .any(|template| template.id == template_id)
                    || self
                        .library
                        .carousels
                        .iter()
                        .any(|carousel| carousel.id == template_id);
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
            .map(|(frame_id, blob)| (*frame_id, blob.name.clone(), blob.screen()))
            .collect();
        let missing: Vec<(FrameId, crate::library::PhotoDisk, String)> = carousel
            .disk
            .iter()
            .filter(|(frame_id, _)| !carousel.photos.contains_key(frame_id))
            .map(|(frame_id, meta)| {
                let name = carousel
                    .layout
                    .frame(*frame_id)
                    .and_then(|frame| frame.photo.as_ref())
                    .map_or_else(
                        || format!("photo-{}", frame_id.0),
                        |photo| photo.name.clone(),
                    );
                (*frame_id, meta.clone(), name)
            })
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
        for (frame_id, meta, name) in missing {
            if self.loading.contains_key(&(carousel_id, frame_id)) {
                continue;
            }
            let token = self.next_token();
            self.loading.insert((carousel_id, frame_id), token);
            let tx = self.photo_tx.clone();
            crate::host::spawn(move || {
                let result = crate::library::load_frame_photo(carousel_id, frame_id, &meta, &name);
                let _ = tx.send(IncomingPhoto::Decoded {
                    token,
                    target: DecodeTarget::DiskFrame {
                        carousel_id,
                        frame_id,
                    },
                    result,
                });
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
        self.pick_image(
            ctx,
            "Add photo",
            DecodeTarget::Frame {
                carousel_id,
                frame_id: id,
            },
        );
    }

    pub(super) fn pick_image(
        &mut self,
        ctx: &egui::Context,
        title: &'static str,
        target: DecodeTarget,
    ) {
        let tx = self.photo_tx.clone();
        let ctx = ctx.clone();
        crate::host::pick_file(
            title,
            "Images",
            &["png", "jpg", "jpeg", "webp"],
            move |picked| {
                if let Some(picked) = picked {
                    let _ = tx.send(IncomingPhoto::Chosen { target, picked });
                }
                ctx.request_repaint();
            },
        );
    }

    pub(super) fn spawn_link(&mut self, ctx: &egui::Context, frame_id: FrameId, url: String) {
        let Route::Carousel(carousel_id) = self.route else {
            return;
        };
        if self.loading.contains_key(&(carousel_id, frame_id)) {
            return;
        }
        if let Err(error) = direct_image_url(&url) {
            self.toast(error);
            return;
        }
        let token = self.next_token();
        self.loading.insert((carousel_id, frame_id), token);
        self.status = "Loading photo…".to_string();
        self.link_open = false;
        let tx = self.photo_tx.clone();
        let ctx = ctx.clone();
        crate::host::fetch(url.clone(), move |bytes| {
            let result = bytes.and_then(|bytes| linked_image(&url, bytes));
            let _ = tx.send(IncomingPhoto::Decoded {
                token,
                target: DecodeTarget::Frame {
                    carousel_id,
                    frame_id,
                },
                result,
            });
            ctx.request_repaint();
        });
    }

    pub(super) fn share_carousel(&mut self) {
        let Route::Carousel(id) = self.route else {
            return;
        };
        if self.sharing {
            return;
        }
        let Some(carousel) = self
            .library
            .carousels
            .iter()
            .find(|carousel| carousel.id == id)
        else {
            return;
        };
        let packaged = Carousel {
            id: carousel.id,
            name: carousel.name.clone(),
            layout: carousel.layout.clone(),
            photos: carousel.photos.clone(),
            disk: carousel.disk.clone(),
            cover: carousel.cover.clone(),
            grid_cells: carousel.grid_cells,
        };
        let file_name = zip_file_name(&packaged.name);
        #[cfg(target_arch = "wasm32")]
        {
            self.sharing = true;
            self.status = "Sharing…".to_string();
            let tx = self.photo_tx.clone();
            crate::host::spawn(move || {
                let result = carousel_package(&packaged).and_then(|bytes| {
                    crate::host::download(&file_name, &bytes)?;
                    Ok(format!("Downloaded {file_name}"))
                });
                let _ = tx.send(IncomingPhoto::Shared(result));
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(path) =
                crate::host::save_path("Share carousel", "Carousel", &["zip"], &file_name)
            else {
                return;
            };
            self.sharing = true;
            self.status = "Sharing…".to_string();
            let tx = self.photo_tx.clone();
            crate::host::spawn(move || {
                let result = carousel_package(&packaged)
                    .and_then(|bytes| {
                        std::fs::write(&path, bytes).map_err(|error| error.to_string())
                    })
                    .map(|()| format!("Shared {}", path.display()));
                let _ = tx.send(IncomingPhoto::Shared(result));
            });
        }
    }

    pub(super) fn import_package(&mut self) {
        if self.importing {
            return;
        }
        self.importing = true;
        self.status = "Importing…".to_string();
        let tx = self.photo_tx.clone();
        crate::host::pick_file("Import carousel", "Carousel", &["zip"], move |picked| {
            let Some(picked) = picked else {
                let _ = tx.send(IncomingPhoto::Imported(None));
                return;
            };
            let tx = tx.clone();
            crate::host::spawn(move || {
                let result = match picked {
                    PickedFile::Path(path) => crate::disk::read(&path)
                        .map_err(|error| format!("Could not read {}: {error}", path.display()))
                        .and_then(|bytes| read_carousel_package(&bytes)),
                    PickedFile::Bytes { bytes, .. } => read_carousel_package(&bytes),
                };
                let _ = tx.send(IncomingPhoto::Imported(Some(result)));
            });
        });
    }

    pub(super) fn refresh_paste_offer(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(false));
        if cfg!(windows) {
            self.paste_stamp = clipboard_image_stamp();
        } else if focused && !self.paste_focused {
            self.paste_look = self.paste_look.wrapping_add(1);
            self.paste_stamp = clipboard_has_image().then_some(self.paste_look);
        }
        self.paste_focused = focused;
    }

    pub(super) fn paste_ready(&self) -> bool {
        let Route::Carousel(carousel_id) = self.route else {
            return false;
        };
        let Some(frame_id) = self.selected else {
            return false;
        };
        if self.loading.contains_key(&(carousel_id, frame_id)) {
            return false;
        }
        let Some(stamp) = self.paste_stamp else {
            return false;
        };
        self.paste_mark != Some((stamp, frame_id))
    }

    #[cfg(target_arch = "wasm32")]
    fn paste_hit(&mut self, ctx: &egui::Context, pasted: Option<Arc<egui::ColorImage>>) {
        if let Some(image) = pasted {
            self.paste_pixels(ctx, &image);
        } else {
            self.toast("Copy an image first");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn paste_hit(&mut self, ctx: &egui::Context, _pasted: Option<Arc<egui::ColorImage>>) {
        self.paste_clipboard(ctx);
    }

    #[cfg(target_arch = "wasm32")]
    fn paste_pixels(&mut self, ctx: &egui::Context, image: &egui::ColorImage) {
        let Route::Carousel(carousel_id) = self.route else {
            return;
        };
        let Some(frame_id) = self.selected else {
            self.toast("Select a frame");
            return;
        };
        if self.loading.contains_key(&(carousel_id, frame_id)) {
            return;
        }
        let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
        for pixel in &image.pixels {
            rgba.extend_from_slice(&pixel.to_array());
        }
        let width = image.size[0];
        let height = image.size[1];
        let token = self.next_token();
        self.loading.insert((carousel_id, frame_id), token);
        self.paste_job = Some((token, self.paste_stamp.unwrap_or(0), frame_id));
        self.status = "Pasting photo…".to_string();
        let tx = self.photo_tx.clone();
        crate::host::spawn(move || {
            let result = pasted_image(width, height, &rgba);
            let _ = tx.send(IncomingPhoto::Decoded {
                token,
                target: DecodeTarget::Frame {
                    carousel_id,
                    frame_id,
                },
                result,
            });
        });
        ctx.request_repaint();
    }

    pub(super) fn paste_clipboard(&mut self, ctx: &egui::Context) {
        let Route::Carousel(carousel_id) = self.route else {
            return;
        };
        let Some(frame_id) = self.selected else {
            self.toast("Select a frame");
            return;
        };
        if self.loading.contains_key(&(carousel_id, frame_id)) {
            return;
        }
        let token = self.next_token();
        self.loading.insert((carousel_id, frame_id), token);
        self.paste_job = Some((token, self.paste_stamp.unwrap_or(0), frame_id));
        self.status = "Pasting photo…".to_string();
        let tx = self.photo_tx.clone();
        crate::host::spawn(move || {
            let result = clipboard_image();
            let _ = tx.send(IncomingPhoto::Decoded {
                token,
                target: DecodeTarget::Frame {
                    carousel_id,
                    frame_id,
                },
                result,
            });
        });
        ctx.request_repaint();
    }

    fn finish_paste_job(&mut self, token: u64, frame_id: FrameId, placed: bool) {
        let Some((job_token, _, job_frame)) = self.paste_job else {
            return;
        };
        if job_token != token || job_frame != frame_id {
            return;
        }
        self.paste_job = None;
        if placed {
            let stamp = self.paste_stamp.unwrap_or(0);
            self.paste_mark = Some((stamp, frame_id));
        }
    }

    pub(super) fn remove_photo(&mut self, id: FrameId) {
        self.checkpoint();
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
        self.toast_undo("Photo deleted");
        self.note_geometry();
    }

    pub(super) fn release_frame(&mut self, id: FrameId) {
        self.forget_load(id);
        if let Route::Carousel(carousel_id) = self.route {
            self.library.clear_photo(carousel_id, id);
        }
        self.assets.remove(&id);
        if self.selected == Some(id) {
            self.selected = None;
            self.edit_frame = false;
        }
        if self
            .crop_session
            .as_ref()
            .is_some_and(|session| session.id == id)
        {
            self.crop_session = None;
        }
        if self.drag.is_some_and(|drag| drag.id() == id) {
            self.drag = None;
        }
    }

    pub(super) fn remove_canvases(&mut self, indices: &[usize]) {
        self.checkpoint();
        let shift = indices
            .iter()
            .filter(|index| **index < self.preview_index)
            .count();
        let Some(removed) = self
            .layout_mut()
            .and_then(|layout| layout.remove_canvases(indices))
        else {
            return;
        };
        for id in removed {
            self.release_frame(id);
        }
        self.active_canvas = None;
        self.preview_index = self.preview_index.saturating_sub(shift);
        self.toast_undo(if indices.len() == 1 {
            "Canvas deleted"
        } else {
            "Canvases deleted"
        });
        self.note_geometry();
    }

    fn clamp_active_canvas(&mut self) {
        let Some(index) = self.active_canvas else {
            return;
        };
        let count = self.layout().map_or(0, |layout| layout.canvas_count);
        if index >= count {
            self.active_canvas = None;
        }
    }

    pub(super) fn remove_frame(&mut self, id: FrameId) {
        self.checkpoint();
        self.release_frame(id);
        if let Some(layout) = self.layout_mut() {
            layout.remove_frame(id);
        }
        self.drag = None;
        self.edit_frame = false;
        self.status = "Frame removed".to_string();
        self.toast_undo("Frame deleted");
        self.note_geometry();
    }

    pub(super) fn enter(&mut self, route: Route, ctx: &egui::Context) {
        self.route = route;
        self.selected = None;
        self.active_canvas = None;
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
        self.strip_pan = egui::Vec2::ZERO;
        self.reveal_canvas = None;
        self.strip_wheel_pan = false;
        self.center_strip = true;
        self.strip_rect = None;
        self.view_rect = None;
        self.history.clear();
        self.dismiss_undo_toasts();
        self.drag_origin = None;
        self.export_open = false;
        self.cancel_overwrite();
        self.create_open = false;
        self.confirm = None;
        self.link_open = false;
        self.show_grid = true;
        self.snap_to_grid = true;
        self.plus_rects.clear();
        if let Some(layout) = self.layout() {
            self.zoom_basis = (layout.ratio.width, layout.ratio.height);
        }
        self.reload_assets(ctx);
    }

    pub(super) fn refresh_open_carousel_cover(&mut self) {
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
        if carousel.disk.is_empty() && carousel.photos.is_empty() {
            return;
        }
        let layout = carousel.layout.clone();
        let images: FrameImages = carousel
            .photos
            .iter()
            .map(|(frame_id, blob)| (*frame_id, blob.screen()))
            .collect();
        let image = carousel_preview_image(&layout, &images);
        let Ok(cover) = crate::library::blob_from_cover_image(&image) else {
            return;
        };
        self.library.set_cover(carousel_id, cover);
        self.cover_textures.remove(&carousel_id);
        self.cover_pending.remove(&carousel_id);
    }

    pub(super) fn go_home(&mut self) {
        let open = match self.route {
            Route::Carousel(id) => Some(id),
            _ => None,
        };
        self.refresh_open_carousel_cover();
        if let Some(id) = open {
            self.library.clear_photos(id);
        }
        self.library.visuals = self.library.visuals.wrapping_add(1);
        self.persist();
        self.dismiss_undo_toasts();
        self.status.clear();
        self.route = Route::Home;
        self.drag = None;
        self.crop_session = None;
        self.crop_press = None;
        self.selected = None;
        self.active_canvas = None;
        self.edit_frame = false;
        self.free_rotate = false;
        self.export_open = false;
        self.cancel_overwrite();
        self.confirm = None;
        self.link_open = false;
        self.assets.clear();
        self.loading.clear();
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
        #[cfg(target_arch = "wasm32")]
        {
            self.launch_download();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(dir) = self.pick_export_folder() else {
                return;
            };
            self.use_export_folder(dir);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn export_saved_folder(&mut self) {
        match self.settings.folder.clone() {
            Some(dir) if dir.is_dir() => self.export_into(dir),
            _ => {
                self.settings.folder = None;
                self.save_settings();
                self.toast("Choose a folder");
                self.start_export();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pick_export_folder(&self) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = &self.settings.folder
            && dir.is_dir()
        {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_folder()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn use_export_folder(&mut self, dir: PathBuf) {
        if self.settings.folder.as_ref() != Some(&dir) {
            self.settings.folder = Some(dir.clone());
            self.save_settings();
        }
        self.export_into(dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn export_into(&mut self, dir: PathBuf) {
        let name = self.item_name();
        let Some(count) = self.layout().map(|layout| layout.canvas_count) else {
            return;
        };
        let existing = existing_export_files(&dir, &name, self.settings.format, count);
        if !existing.is_empty() {
            self.overwrite_names = existing;
            self.overwrite_dir = Some(dir);
            return;
        }
        self.launch_export(dir);
    }

    pub(super) fn confirm_overwrite(&mut self) {
        let Some(dir) = self.overwrite_dir.take() else {
            return;
        };
        self.overwrite_names.clear();
        self.launch_export(dir);
    }

    pub(super) fn cancel_overwrite(&mut self) {
        self.overwrite_dir = None;
        self.overwrite_names.clear();
    }

    fn launch_export(&mut self, dir: PathBuf) {
        let name = self.item_name();
        self.launch_job(move |encoded, format| save_folder(&dir, &name, encoded, format));
    }

    #[cfg(target_arch = "wasm32")]
    fn launch_download(&mut self) {
        let name = self.item_name();
        let file_name = zip_file_name(&name);
        self.launch_job(move |encoded, format| {
            let bytes = super::draw::zip_slides(&name, encoded, format)?;
            crate::host::download(&file_name, &bytes)?;
            Ok(format!("Downloaded {file_name}"))
        });
    }

    fn launch_job(
        &mut self,
        finish: impl FnOnce(
            &[pudduscroll_core::EncodedCanvas],
            pudduscroll_core::ExportFormat,
        ) -> Result<String, String>
        + Send
        + 'static,
    ) {
        let Some(layout) = self.layout().cloned() else {
            return;
        };
        let quality = self.settings.quality;
        let format = self.settings.format;
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
            crate::host::spawn(move || {
                let _ = tx.send(ExportUpdate::Saving);
                let _ = tx.send(ExportUpdate::Finished(finish(&canvases, format)));
            });
            return;
        }
        let encoded = self.original_bytes();
        let progress = tx.clone();
        crate::host::spawn(move || {
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
                let encoded = export_folder(&layout, &images, quality, format, &|done, total| {
                    let _ = slides.send(ExportUpdate::Exporting { done, total });
                })?;
                let _ = progress.send(ExportUpdate::Saving);
                finish(&encoded, format)
            })();
            let _ = tx.send(ExportUpdate::Finished(outcome));
        });
    }
}

#[cfg(test)]
fn take_frame_paste(input: &mut egui::InputState, has_image: bool) -> bool {
    take_paste(input, has_image).0
}

fn take_paste(
    input: &mut egui::InputState,
    has_image: bool,
) -> (bool, Option<Arc<egui::ColorImage>>) {
    let mut found = false;
    let mut image = None;
    input.events.retain(|event| {
        let hit = match event {
            egui::Event::PasteImage(pasted) => {
                image = Some(Arc::clone(pasted));
                true
            }
            egui::Event::Paste(_) => has_image,
            egui::Event::Key {
                key: egui::Key::V,
                modifiers,
                pressed: true,
                ..
            } => modifiers.matches_logically(egui::Modifiers::COMMAND),
            _ => false,
        };
        found |= hit;
        !hit
    });
    (found, image)
}

#[cfg(target_arch = "wasm32")]
struct SaveInbox {
    rx: mpsc::Receiver<SaveRequest>,
    photo_tx: Sender<IncomingPhoto>,
}

#[cfg(target_arch = "wasm32")]
static SAVE_INBOX: std::sync::Mutex<Option<SaveInbox>> = std::sync::Mutex::new(None);

fn spawn_saver(photo_tx: Sender<IncomingPhoto>) -> Sender<SaveRequest> {
    let (tx, rx) = mpsc::channel::<SaveRequest>();
    start_saver(rx, photo_tx);
    tx
}

#[cfg(not(target_arch = "wasm32"))]
fn start_saver(rx: mpsc::Receiver<SaveRequest>, photo_tx: Sender<IncomingPhoto>) {
    std::thread::spawn(move || {
        while let Ok(request) = rx.recv() {
            deliver_save(request, &photo_tx);
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn start_saver(rx: mpsc::Receiver<SaveRequest>, photo_tx: Sender<IncomingPhoto>) {
    *SAVE_INBOX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(SaveInbox { rx, photo_tx });
}

fn deliver_save(request: SaveRequest, photo_tx: &Sender<IncomingPhoto>) {
    let generation = request.snapshot.generation;
    let had_images = request.snapshot.images.is_some();
    let result = write_snapshot(&library_directory(), &request.snapshot);
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

#[cfg(target_arch = "wasm32")]
fn drive_saver() {
    let (pending, photo_tx) = {
        let mut inbox = SAVE_INBOX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(saver) = inbox.as_mut() else {
            return;
        };
        let mut pending = Vec::new();
        while let Ok(request) = saver.rx.try_recv() {
            pending.push(request);
        }
        (pending, saver.photo_tx.clone())
    };
    for request in pending {
        deliver_save(request, &photo_tx);
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    use super::take_frame_paste;

    fn image_paste() -> egui::Event {
        egui::Event::PasteImage(std::sync::Arc::new(egui::ColorImage::filled(
            [1, 1],
            egui::Color32::WHITE,
        )))
    }

    #[test]
    fn copied_image_paste_is_taken_without_a_v_key() {
        let mut input = egui::InputState::default();
        input.events.push(image_paste());
        assert!(take_frame_paste(&mut input, false));
        assert_eq!(input.events.len(), 0);
    }

    #[test]
    fn copied_image_with_a_url_is_taken() {
        let mut input = egui::InputState::default();
        input
            .events
            .push(egui::Event::Paste("https://example.com/a.jpg".into()));
        assert!(take_frame_paste(&mut input, true));
        assert_eq!(input.events.len(), 0);
    }

    #[test]
    fn text_paste_without_an_image_is_left_alone() {
        let mut input = egui::InputState::default();
        input.events.push(egui::Event::Paste("hello".into()));
        assert!(!take_frame_paste(&mut input, false));
        assert_eq!(input.events.len(), 1);
    }
}
