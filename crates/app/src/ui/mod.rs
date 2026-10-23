mod card;
mod crop;
mod draw;
mod home;
mod rail;
mod session;
mod shell;
mod strip;
mod toolbar;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use eframe::egui;
use image::RgbaImage;
use pudduscroll_core::{AspectRatio, Crop, CropPreset, FrameId, Handle, Layout};

use crate::library::{Blob, Library};

const BG: egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const PANEL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const CANVAS_BG: egui::Color32 = egui::Color32::from_rgb(244, 241, 236);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(232, 160, 96);
const TEXT: egui::Color32 = egui::Color32::from_rgb(240, 236, 230);
const MUTED: egui::Color32 = egui::Color32::from_rgb(164, 158, 148);
const SELECT_BG: egui::Color32 = egui::Color32::from_rgb(48, 38, 30);
const PREVIEW_MAX: u32 = 1800;
const DEFAULT_EDITOR_ZOOM: f32 = 0.7;
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

#[derive(Clone, Copy)]
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
    #[expect(dead_code, reason = "retains the decoded image for the texture")]
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
