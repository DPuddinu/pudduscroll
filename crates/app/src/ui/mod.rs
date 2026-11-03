mod card;
mod crop;
mod draw;
#[cfg(test)]
mod features;
mod history;
mod home;
mod rail;
mod session;
mod settings;
mod shell;
mod skeleton;
mod strip;
mod toast;
mod toolbar;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use eframe::egui;
use image::RgbaImage;
use pudduscroll_core::{
    AspectRatio, Crop, CropPreset, EncodedCanvas, ExportFormat, FrameId, Handle,
};

use crate::host::PickedFile;
use crate::library::{Blob, Carousel, Library, SaveRequest};
use crate::settings::{AppSettings, Command};

use history::{Edit, EditHistory};

pub(super) use crate::library::{MAX_GRID_CELLS, MIN_GRID_CELLS};

const BG: egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const PANEL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const CANVAS_BG: egui::Color32 = egui::Color32::from_rgb(244, 241, 236);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(232, 160, 96);
const TEXT: egui::Color32 = egui::Color32::from_rgb(240, 236, 230);
const MUTED: egui::Color32 = egui::Color32::from_rgb(164, 158, 148);
const SELECT_BG: egui::Color32 = egui::Color32::from_rgb(48, 38, 30);
const OUTSIDE_FRAME_ALPHA: u8 = 102;
const DEFAULT_EDITOR_ZOOM: f32 = 0.7;
const MIN_EDITOR_ZOOM: f32 = 0.25;
const MAX_EDITOR_ZOOM: f32 = 4.0;
const EDITOR_ZOOM_SPEED: f32 = 1.0 / 200.0;
const UNDO_LIMIT: usize = 50;
const CROP_HANDLE_HIT: f32 = 12.0;
const MENU_H: f32 = 28.0;
const TOOLBAR_H: f32 = 48.0;
const RAIL_W: f32 = 320.0;
const RAIL_SECTIONS: usize = 5;
const RAIL_SECTION_MIN: f32 = 72.0;
const RAIL_SPLIT: f32 = 8.0;
const DEFAULT_RAIL_WEIGHTS: [f32; RAIL_SECTIONS] = [0.16, 0.18, 0.16, 0.22, 0.28];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Home,
    Template(u64),
    Carousel(u64),
}

#[derive(Clone, Copy)]
enum HomeAction {
    OpenTemplate(u64),
    EditTemplate(u64),
    DeleteTemplate(u64),
    OpenCarousel(u64),
    DuplicateCarousel(u64),
    DeleteCarousel(u64),
}

enum Confirm {
    DeleteCarousel(u64),
    DeleteTemplate(u64),
    RemoveCanvases(Vec<usize>),
    RemoveFrame(FrameId),
    RemovePhoto(FrameId),
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
    Rotate {
        id: FrameId,
    },
}

impl Drag {
    fn id(self) -> FrameId {
        match self {
            Drag::MoveFrame { id, .. }
            | Drag::ResizeFrame { id, .. }
            | Drag::Pan { id, .. }
            | Drag::Rotate { id } => id,
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
    format: ExportFormat,
    result: Result<Arc<[EncodedCanvas]>, String>,
}

enum EstimateUpdate {
    Decoding { done: usize, total: usize },
    Slide { done: usize, total: usize },
    Ready(EstimateMsg),
}

enum ExportUpdate {
    Decoding { done: usize, total: usize },
    Exporting { done: usize, total: usize },
    Saving,
    Finished(Result<String, String>),
}

struct PhotoAsset {
    texture: egui::TextureHandle,
}

#[derive(Clone, Copy)]
enum DecodeTarget {
    Frame { carousel_id: u64, frame_id: FrameId },
    DiskFrame { carousel_id: u64, frame_id: FrameId },
    Cover,
}

enum IncomingPhoto {
    Library(Result<Library, String>),
    Chosen {
        target: DecodeTarget,
        picked: PickedFile,
    },
    Decoded {
        token: u64,
        target: DecodeTarget,
        result: Result<Blob, String>,
    },
    PhonePreview {
        revision: u64,
        index: usize,
        image: RgbaImage,
    },
    Saved {
        generation: u64,
        result: Result<(), String>,
    },
    Shared(Result<String, String>),
    Imported(Option<Result<Carousel, String>>),
}

enum PendingUpload {
    Frame {
        carousel_id: u64,
        frame_id: FrameId,
        token: u64,
        name: String,
        proxy: Arc<RgbaImage>,
    },
    Cover {
        template_id: u64,
        token: u64,
        proxy: Arc<RgbaImage>,
    },
    CreateCover {
        token: u64,
        proxy: Arc<RgbaImage>,
    },
    Phone {
        revision: u64,
        index: usize,
        image: RgbaImage,
    },
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "editor flags stay independent"
)]
pub struct PudduScrollApp {
    library: Library,
    library_ready: bool,
    can_store: bool,
    route: Route,
    assets: HashMap<FrameId, PhotoAsset>,
    cover_textures: HashMap<u64, egui::TextureHandle>,
    selected: Option<FrameId>,
    active_canvas: Option<usize>,
    edit_frame: bool,
    free_rotate: bool,
    settings: AppSettings,
    settings_open: bool,
    rebind: Option<Command>,
    preview_index: usize,
    preview_texture: Option<egui::TextureHandle>,
    preview_key: Option<(u64, usize)>,
    preview_job: Option<(u64, usize)>,
    geometry_revision: u64,
    drag: Option<Drag>,
    gesture: String,
    status: String,
    toaster: toast::Toaster,
    estimate_mb: Option<f64>,
    estimate_for: Option<(u64, u8, ExportFormat)>,
    estimate_canvases: Option<Arc<[EncodedCanvas]>>,
    estimate_after: Option<Instant>,
    estimate_label: String,
    estimate_done: usize,
    estimate_rx: Option<Receiver<EstimateUpdate>>,
    export_rx: Option<Receiver<ExportUpdate>>,
    export_done: usize,
    photo_tx: Sender<IncomingPhoto>,
    photo_rx: Receiver<IncomingPhoto>,
    update_rx: Option<Receiver<crate::update::Event>>,
    update_offer: Option<crate::update::Offer>,
    save_tx: Sender<SaveRequest>,
    pending_uploads: VecDeque<PendingUpload>,
    loading: HashMap<(u64, FrameId), u64>,
    cover_load: Option<u64>,
    cover_pending: HashMap<u64, u64>,
    load_token: u64,
    painted: u64,
    exporting: bool,
    export_open: bool,
    overwrite_dir: Option<PathBuf>,
    overwrite_names: Vec<String>,
    editor_zoom: f32,
    zoom_basis: (u32, u32),
    strip_pan: egui::Vec2,
    reveal_canvas: Option<usize>,
    strip_wheel_pan: bool,
    center_strip: bool,
    strip_rect: Option<egui::Rect>,
    view_rect: Option<egui::Rect>,
    history: EditHistory<Edit>,
    drag_origin: Option<Edit>,
    crop_session: Option<CropSession>,
    crop_press: Option<egui::Pos2>,
    show_grid: bool,
    snap_to_grid: bool,
    grid_cells: i32,
    rail_weights: [f32; RAIL_SECTIONS],
    create_open: bool,
    about_open: bool,
    confirm: Option<Confirm>,
    link_open: bool,
    link_url: String,
    sharing: bool,
    importing: bool,
    paste_stamp: Option<u64>,
    paste_mark: Option<(u64, FrameId)>,
    paste_job: Option<(u64, u64, FrameId)>,
    paste_focused: bool,
    paste_look: u64,
    create_name: String,
    create_ratio: AspectRatio,
    create_cover: Option<Blob>,
    create_cover_texture: Option<egui::TextureHandle>,
    name_dirty: bool,
    widgets: Vec<(String, egui::Rect)>,
    plus_rects: Vec<(FrameId, egui::Rect)>,
}
