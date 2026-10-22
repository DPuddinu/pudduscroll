pub const MAX_CANVASES: usize = 20;
pub const DEFAULT_EXPORT_WIDTH: u32 = 1080;
pub const CANVAS_GRID: usize = 8;
pub const CANVAS_GRID_STEP: f32 = 1.0 / 8.0;
const MIN_WIDTH: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

impl AspectRatio {
    pub const SQUARE: Self = Self {
        width: 1,
        height: 1,
    };
    pub const FOUR_FIVE: Self = Self {
        width: 4,
        height: 5,
    };
    pub const THREE_FOUR: Self = Self {
        width: 3,
        height: 4,
    };
    pub const NINE_SIXTEEN: Self = Self {
        width: 9,
        height: 16,
    };

    #[must_use]
    pub fn value(self) -> f32 {
        self.width as f32 / self.height as f32
    }

    #[must_use]
    pub fn label(self) -> String {
        format!("{}:{}", self.width, self.height)
    }

    #[must_use]
    pub fn export_size(self, width: u32) -> (u32, u32) {
        let width = width.max(1);
        let height = ((width as u64 * self.height as u64) / self.width as u64) as u32;
        (width, height.max(1))
    }
}

pub const CANVAS_PRESETS: [AspectRatio; 4] = [
    AspectRatio::SQUARE,
    AspectRatio::FOUR_FIVE,
    AspectRatio::THREE_FOUR,
    AspectRatio::NINE_SIXTEEN,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Crop {
    #[must_use]
    pub fn full() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }

    #[must_use]
    pub fn clamp(self) -> Self {
        let w = self.w.clamp(0.02, 1.0);
        let h = self.h.clamp(0.02, 1.0);
        Self {
            x: self.x.clamp(0.0, 1.0 - w),
            y: self.y.clamp(0.0, 1.0 - h),
            w,
            h,
        }
    }

    #[must_use]
    pub fn pixel_aspect(self, image_w: u32, image_h: u32) -> f32 {
        let pw = (self.w * image_w as f32).max(1.0);
        let ph = (self.h * image_h as f32).max(1.0);
        pw / ph
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropPreset {
    Original,
    Square,
    FourFive,
    ThreeFour,
    ThreeTwo,
    SixteenNine,
    NineSixteen,
    Freehand,
}

impl CropPreset {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            CropPreset::Original => "Original",
            CropPreset::Square => "1:1",
            CropPreset::FourFive => "4:5",
            CropPreset::ThreeFour => "3:4",
            CropPreset::ThreeTwo => "3:2",
            CropPreset::SixteenNine => "16:9",
            CropPreset::NineSixteen => "9:16",
            CropPreset::Freehand => "Freehand",
        }
    }

    #[must_use]
    pub fn target_aspect(self) -> f32 {
        match self {
            CropPreset::Square | CropPreset::Original | CropPreset::Freehand => 1.0,
            CropPreset::FourFive => 4.0 / 5.0,
            CropPreset::ThreeFour => 3.0 / 4.0,
            CropPreset::ThreeTwo => 3.0 / 2.0,
            CropPreset::SixteenNine => 16.0 / 9.0,
            CropPreset::NineSixteen => 9.0 / 16.0,
        }
    }
}

pub const CROP_PRESETS: [CropPreset; 8] = [
    CropPreset::Original,
    CropPreset::Square,
    CropPreset::FourFive,
    CropPreset::ThreeFour,
    CropPreset::ThreeTwo,
    CropPreset::SixteenNine,
    CropPreset::NineSixteen,
    CropPreset::Freehand,
];

#[must_use]
pub fn crop_rect_for_aspect(image_w: u32, image_h: u32, target_aspect: f32) -> Crop {
    let image_aspect = image_w.max(1) as f32 / image_h.max(1) as f32;
    if image_aspect > target_aspect {
        let w = (target_aspect / image_aspect).clamp(0.02, 1.0);
        Crop {
            x: (1.0 - w) * 0.5,
            y: 0.0,
            w,
            h: 1.0,
        }
    } else {
        let h = (image_aspect / target_aspect).clamp(0.02, 1.0);
        Crop {
            x: 0.0,
            y: (1.0 - h) * 0.5,
            w: 1.0,
            h,
        }
    }
}

const MIN_CROP: f32 = 0.02;

#[must_use]
pub fn crop_lock_ratio(preset: CropPreset, image_w: u32, image_h: u32) -> Option<f32> {
    match preset {
        CropPreset::Freehand => None,
        CropPreset::Original => Some(1.0),
        preset => {
            let iw = image_w.max(1) as f32;
            let ih = image_h.max(1) as f32;
            Some((preset.target_aspect() * ih / iw).max(1.0e-4))
        }
    }
}

#[must_use]
pub fn begin_crop_rect(current: Crop, preset: CropPreset, image_w: u32, image_h: u32) -> Crop {
    let current = current.clamp();
    if preset == CropPreset::Freehand || crop_matches_preset(current, preset, image_w, image_h) {
        return current;
    }
    if preset == CropPreset::Original {
        return Crop::full();
    }
    crop_rect_for_aspect(image_w, image_h, preset.target_aspect())
}

fn crop_matches_preset(crop: Crop, preset: CropPreset, image_w: u32, image_h: u32) -> bool {
    let target = match preset {
        CropPreset::Freehand => return true,
        CropPreset::Original => image_w.max(1) as f32 / image_h.max(1) as f32,
        preset => preset.target_aspect(),
    };
    let actual = crop.pixel_aspect(image_w, image_h);
    (actual - target).abs() <= 0.02 * target.max(1.0)
}

#[must_use]
pub fn move_crop_inside(crop: Crop, x: f32, y: f32) -> Crop {
    let crop = crop.clamp();
    Crop {
        x: x.clamp(0.0, (1.0 - crop.w).max(0.0)),
        y: y.clamp(0.0, (1.0 - crop.h).max(0.0)),
        w: crop.w,
        h: crop.h,
    }
}

#[must_use]
pub fn resize_crop_inside(
    origin: Crop,
    handle: Handle,
    pointer_x: f32,
    pointer_y: f32,
    lock_ratio: Option<f32>,
) -> Crop {
    let origin = origin.clamp();
    match lock_ratio {
        Some(ratio) => resize_locked(origin, handle, pointer_x, pointer_y, ratio.max(1.0e-4)),
        None => resize_free(origin, handle, pointer_x, pointer_y),
    }
}

fn resize_free(origin: Crop, handle: Handle, pointer_x: f32, pointer_y: f32) -> Crop {
    let left0 = origin.x;
    let right0 = origin.x + origin.w;
    let top0 = origin.y;
    let bottom0 = origin.y + origin.h;
    let (x, w) = if matches!(handle, Handle::N | Handle::S) {
        (left0, origin.w)
    } else if matches!(handle, Handle::E | Handle::Ne | Handle::Se) {
        let right = pointer_x.clamp(left0 + MIN_CROP, 1.0);
        (left0, right - left0)
    } else {
        let left = pointer_x.clamp(0.0, right0 - MIN_CROP);
        (left, right0 - left)
    };
    let (y, h) = if matches!(handle, Handle::E | Handle::W) {
        (top0, origin.h)
    } else if matches!(handle, Handle::S | Handle::Se | Handle::Sw) {
        let bottom = pointer_y.clamp(top0 + MIN_CROP, 1.0);
        (top0, bottom - top0)
    } else {
        let top = pointer_y.clamp(0.0, bottom0 - MIN_CROP);
        (top, bottom0 - top)
    };
    Crop { x, y, w, h }
}

fn resize_locked(origin: Crop, handle: Handle, pointer_x: f32, pointer_y: f32, ratio: f32) -> Crop {
    match handle {
        Handle::E | Handle::W | Handle::N | Handle::S => {
            resize_locked_edge(origin, handle, pointer_x, pointer_y, ratio)
        }
        Handle::Ne | Handle::Nw | Handle::Se | Handle::Sw => {
            resize_locked_corner(origin, handle, pointer_x, pointer_y, ratio)
        }
    }
}

fn axis_room(center: f32) -> f32 {
    (center * 2.0).min((1.0 - center) * 2.0).max(0.0)
}

fn locked_min_w(ratio: f32, max_w: f32, max_h: f32) -> f32 {
    let want = MIN_CROP.max(MIN_CROP * ratio);
    let fit = max_w.min(max_h * ratio).max(0.0);
    want.min(fit)
}

fn clamp_locked_size(w: f32, ratio: f32, max_w: f32, max_h: f32) -> (f32, f32) {
    let max_w = max_w.clamp(0.0, 1.0);
    let max_h = max_h.clamp(0.0, 1.0);
    let mut width = w.max(0.0).min(max_w);
    let mut height = width / ratio;
    if height > max_h {
        height = max_h;
        width = height * ratio;
    }
    let min_w = locked_min_w(ratio, max_w, max_h);
    if width < min_w {
        width = min_w;
        height = width / ratio;
        if height > max_h {
            height = max_h;
            width = (height * ratio).min(max_w);
            height = width / ratio;
        }
    }
    (width, height)
}

fn place_crop(x: f32, y: f32, w: f32, h: f32) -> Crop {
    let w = w.clamp(0.0, 1.0);
    let h = h.clamp(0.0, 1.0);
    Crop {
        x: x.clamp(0.0, (1.0 - w).max(0.0)),
        y: y.clamp(0.0, (1.0 - h).max(0.0)),
        w,
        h,
    }
}

fn resize_locked_edge(
    origin: Crop,
    handle: Handle,
    pointer_x: f32,
    pointer_y: f32,
    ratio: f32,
) -> Crop {
    match handle {
        Handle::E | Handle::W => {
            let move_right = handle == Handle::E;
            let anchor = if move_right {
                origin.x
            } else {
                origin.x + origin.w
            };
            let max_w = if move_right { 1.0 - anchor } else { anchor };
            let raw = if move_right {
                pointer_x - anchor
            } else {
                anchor - pointer_x
            };
            let cy = origin.y + origin.h * 0.5;
            let (w, h) = clamp_locked_size(raw, ratio, max_w, axis_room(cy));
            place_crop(
                if move_right { anchor } else { anchor - w },
                cy - h * 0.5,
                w,
                h,
            )
        }
        Handle::S | Handle::N => {
            let move_down = handle == Handle::S;
            let anchor = if move_down {
                origin.y
            } else {
                origin.y + origin.h
            };
            let max_h = if move_down { 1.0 - anchor } else { anchor };
            let raw = if move_down {
                pointer_y - anchor
            } else {
                anchor - pointer_y
            };
            let cx = origin.x + origin.w * 0.5;
            let (w, h) = clamp_locked_size(raw * ratio, ratio, axis_room(cx), max_h);
            place_crop(
                cx - w * 0.5,
                if move_down { anchor } else { anchor - h },
                w,
                h,
            )
        }
        Handle::Ne | Handle::Nw | Handle::Se | Handle::Sw => origin,
    }
}

#[expect(
    clippy::many_single_char_names,
    reason = "crop coordinates use x, y, w, and h"
)]
fn resize_locked_corner(
    origin: Crop,
    handle: Handle,
    pointer_x: f32,
    pointer_y: f32,
    ratio: f32,
) -> Crop {
    let anchor_left = matches!(handle, Handle::Se | Handle::Ne);
    let anchor_top = matches!(handle, Handle::Se | Handle::Sw);
    let ax = if anchor_left {
        origin.x
    } else {
        origin.x + origin.w
    };
    let ay = if anchor_top {
        origin.y
    } else {
        origin.y + origin.h
    };
    let sign_x = if anchor_left { 1.0 } else { -1.0 };
    let sign_y = if anchor_top { 1.0 } else { -1.0 };
    let along_x = (pointer_x - ax) * sign_x;
    let along_y = (pointer_y - ay) * sign_y;
    let denom = ratio * ratio + 1.0;
    let t = (along_x * ratio + along_y) / denom;
    let max_w = if anchor_left { 1.0 - ax } else { ax };
    let max_h = if anchor_top { 1.0 - ay } else { ay };
    let (w, h) = clamp_locked_size(t * ratio, ratio, max_w, max_h);
    let x = if anchor_left { ax } else { ax - w };
    let y = if anchor_top { ay } else { ay - h };
    place_crop(x, y, w, h)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PhotoId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerMove {
    Up,
    Down,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Photo {
    pub id: PhotoId,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub crop: Crop,
    pub preset: CropPreset,
    pub z: i32,
    pub image_width: u32,
    pub image_height: u32,
}

impl Photo {
    #[must_use]
    pub fn height(&self, ratio: f32) -> f32 {
        let aspect = self.crop.pixel_aspect(self.image_width, self.image_height);
        self.w * ratio / aspect
    }

    #[must_use]
    pub fn placement(&self, ratio: f32) -> Placement {
        Placement {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.height(ratio),
        }
    }
}

#[must_use]
pub fn placed_pixel_aspect(photo: &Photo, ratio: f32) -> f32 {
    let h = photo.height(ratio).max(1e-6);
    (photo.w / h) * ratio
}

#[must_use]
pub fn resize_uniform(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    handle: Handle,
    pointer_x: f32,
    pointer_y: f32,
) -> (f32, f32, f32) {
    let aspect = if w.abs() < 1e-4 { 1.0 } else { h / w };
    let aspect = if aspect.abs() < 1e-6 { 1.0 } else { aspect };
    match handle {
        Handle::E => {
            let new_w = (pointer_x - x).max(MIN_WIDTH);
            let new_h = new_w * aspect;
            let cy = y + h * 0.5;
            (x, cy - new_h * 0.5, new_w)
        }
        Handle::W => {
            let right = x + w;
            let new_w = (right - pointer_x).max(MIN_WIDTH);
            let new_h = new_w * aspect;
            let cy = y + h * 0.5;
            (right - new_w, cy - new_h * 0.5, new_w)
        }
        Handle::S => {
            let new_h = (pointer_y - y).max(MIN_WIDTH * aspect);
            let new_w = new_h / aspect;
            let cx = x + w * 0.5;
            (cx - new_w * 0.5, y, new_w)
        }
        Handle::N => {
            let bottom = y + h;
            let new_h = (bottom - pointer_y).max(MIN_WIDTH * aspect);
            let new_w = new_h / aspect;
            let cx = x + w * 0.5;
            (cx - new_w * 0.5, bottom - new_h, new_w)
        }
        Handle::Se | Handle::Sw | Handle::Ne | Handle::Nw => {
            let anchor_left = matches!(handle, Handle::Se | Handle::Ne);
            let anchor_top = matches!(handle, Handle::Sw | Handle::Se);
            let ax = if anchor_left { x } else { x + w };
            let ay = if anchor_top { y } else { y + h };
            let sign_x = if anchor_left { 1.0 } else { -1.0 };
            let sign_y = if anchor_top { 1.0 } else { -1.0 };
            let from_x = (pointer_x - ax) * sign_x;
            let from_y = (pointer_y - ay) * sign_y / aspect;
            let new_w = f32::midpoint(from_x, from_y).max(MIN_WIDTH);
            let new_h = new_w * aspect;
            let nx = if anchor_left { ax } else { ax - new_w };
            let ny = if anchor_top { ay } else { ay - new_h };
            (nx, ny, new_w)
        }
    }
}

fn snap_scalar(value: f32, step: f32) -> f32 {
    if step <= 1e-8 {
        return value;
    }
    (value / step).round() * step
}

fn snap_edge_pair(origin: f32, size: f32, step: f32) -> f32 {
    let start_delta = snap_scalar(origin, step) - origin;
    let end = origin + size;
    let end_delta = snap_scalar(end, step) - end;
    if start_delta.abs() <= end_delta.abs() {
        origin + start_delta
    } else {
        origin + end_delta
    }
}

#[must_use]
pub fn snap_move(x: f32, y: f32, w: f32, h: f32, step: f32) -> (f32, f32) {
    (snap_edge_pair(x, w, step), snap_edge_pair(y, h, step))
}

#[must_use]
pub fn snap_resize(handle: Handle, x: f32, y: f32, w: f32, h: f32, step: f32) -> (f32, f32, f32) {
    let aspect = if w.abs() < 1e-4 { 1.0 } else { h / w };
    let aspect = if aspect.abs() < 1e-6 { 1.0 } else { aspect };
    match handle {
        Handle::E => {
            let left = x;
            let cy = y + h * 0.5;
            let right = snap_scalar(x + w, step);
            let new_w = (right - left).max(MIN_WIDTH);
            let new_h = new_w * aspect;
            (left, cy - new_h * 0.5, new_w)
        }
        Handle::W => {
            let right = x + w;
            let cy = y + h * 0.5;
            let left = snap_scalar(x, step);
            let new_w = (right - left).max(MIN_WIDTH);
            let new_h = new_w * aspect;
            (right - new_w, cy - new_h * 0.5, new_w)
        }
        Handle::S => {
            let top = y;
            let cx = x + w * 0.5;
            let bottom = snap_scalar(y + h, step);
            let new_h = (bottom - top).max(MIN_WIDTH * aspect);
            let new_w = new_h / aspect;
            (cx - new_w * 0.5, top, new_w)
        }
        Handle::N => {
            let bottom = y + h;
            let cx = x + w * 0.5;
            let top = snap_scalar(y, step);
            let new_h = (bottom - top).max(MIN_WIDTH * aspect);
            let new_w = new_h / aspect;
            (cx - new_w * 0.5, bottom - new_h, new_w)
        }
        Handle::Se | Handle::Sw | Handle::Ne | Handle::Nw => {
            snap_resize_corner(handle, x, y, w, h, aspect, step)
        }
    }
}

fn snap_resize_corner(
    handle: Handle,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    aspect: f32,
    step: f32,
) -> (f32, f32, f32) {
    let anchor_left = matches!(handle, Handle::Se | Handle::Ne);
    let anchor_top = matches!(handle, Handle::Sw | Handle::Se);
    let ax = if anchor_left { x } else { x + w };
    let ay = if anchor_top { y } else { y + h };
    let moving_x = if anchor_left { x + w } else { x };
    let moving_y = if anchor_top { y + h } else { y };
    let snapped_x = snap_scalar(moving_x, step);
    let snapped_y = snap_scalar(moving_y, step);
    let width_from_x = if anchor_left {
        (snapped_x - ax).max(MIN_WIDTH)
    } else {
        (ax - snapped_x).max(MIN_WIDTH)
    };
    let height_from_x = width_from_x * aspect;
    let delta_x = (width_from_x - w, height_from_x - h);
    let height_from_y = if anchor_top {
        (snapped_y - ay).max(MIN_WIDTH * aspect)
    } else {
        (ay - snapped_y).max(MIN_WIDTH * aspect)
    };
    let width_from_y = height_from_y / aspect;
    let delta_y = (width_from_y - w, height_from_y - h);
    let x_dist = delta_x.0 * delta_x.0 + delta_x.1 * delta_x.1;
    let y_dist = delta_y.0 * delta_y.0 + delta_y.1 * delta_y.1;
    if x_dist <= y_dist {
        let nx = if anchor_left { ax } else { ax - width_from_x };
        let ny = if anchor_top { ay } else { ay - height_from_x };
        (nx, ny, width_from_x)
    } else {
        let nx = if anchor_left { ax } else { ax - width_from_y };
        let ny = if anchor_top { ay } else { ay - height_from_y };
        (nx, ny, width_from_y)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub ratio: AspectRatio,
    pub canvas_count: usize,
    pub photos: Vec<Photo>,
    next_id: u64,
    next_z: i32,
}

impl Document {
    #[must_use]
    pub fn new(ratio: AspectRatio) -> Self {
        Self {
            ratio,
            canvas_count: 3,
            photos: Vec::new(),
            next_id: 1,
            next_z: 0,
        }
    }

    pub fn set_canvas_count(&mut self, count: usize) {
        self.canvas_count = count.clamp(1, MAX_CANVASES);
    }

    pub fn add_canvas(&mut self) -> bool {
        if self.canvas_count >= MAX_CANVASES {
            return false;
        }
        self.canvas_count += 1;
        true
    }

    pub fn remove_canvas(&mut self) -> bool {
        if self.canvas_count <= 1 {
            return false;
        }
        self.canvas_count -= 1;
        true
    }

    pub fn set_ratio(&mut self, ratio: AspectRatio) {
        if self.ratio == ratio {
            return;
        }
        let old_ratio = self.ratio.value();
        let new_ratio = ratio.value();
        for photo in &mut self.photos {
            let aspect = photo
                .crop
                .pixel_aspect(photo.image_width, photo.image_height);
            let old_h = photo.w * old_ratio / aspect;
            let new_h = photo.w * new_ratio / aspect;
            let center_y = photo.y + old_h * 0.5;
            photo.y = center_y - new_h * 0.5;
        }
        self.ratio = ratio;
    }

    pub fn add_photo(&mut self, name: &str, image_width: u32, image_height: u32) -> PhotoId {
        let id = PhotoId(self.next_id);
        self.next_id += 1;
        let crop = Crop::full();
        let ratio = self.ratio.value();
        let aspect = crop.pixel_aspect(image_width.max(1), image_height.max(1));
        let height_if_full_width = ratio / aspect;
        let (w, y) = if height_if_full_width <= 1.0 {
            (1.0, (1.0 - height_if_full_width) * 0.5)
        } else {
            (aspect / ratio, 0.0)
        };
        let slot = self.photos.len();
        let base = (slot % self.canvas_count.max(1)) as f32;
        let mut x = base + (1.0 - w) * 0.5;
        if slot >= self.canvas_count {
            x += 0.12 * (slot / self.canvas_count) as f32;
        }
        let z = self.next_z;
        self.next_z += 1;
        self.photos.push(Photo {
            id,
            name: name.to_string(),
            x,
            y,
            w,
            crop,
            preset: CropPreset::Original,
            z,
            image_width: image_width.max(1),
            image_height: image_height.max(1),
        });
        id
    }

    pub fn remove_photo(&mut self, id: PhotoId) -> bool {
        let before = self.photos.len();
        self.photos.retain(|photo| photo.id != id);
        self.photos.len() != before
    }

    #[must_use]
    pub fn photo(&self, id: PhotoId) -> Option<&Photo> {
        self.photos.iter().find(|photo| photo.id == id)
    }

    pub fn set_box(&mut self, id: PhotoId, x: f32, y: f32, w: f32) {
        if let Some(photo) = self.photos.iter_mut().find(|photo| photo.id == id) {
            photo.x = x;
            photo.y = y;
            photo.w = w.max(MIN_WIDTH);
        }
    }

    pub fn resize_photo(
        &mut self,
        id: PhotoId,
        handle: Handle,
        pointer_x: f32,
        pointer_y: f32,
        origin: Placement,
    ) {
        let (x, y, w) = resize_uniform(
            origin.x, origin.y, origin.w, origin.h, handle, pointer_x, pointer_y,
        );
        self.set_box(id, x, y, w);
    }

    pub fn set_crop(&mut self, id: PhotoId, crop: Crop) {
        let ratio = self.ratio.value();
        let Some(photo) = self.photos.iter_mut().find(|photo| photo.id == id) else {
            return;
        };
        let old_h = photo.height(ratio);
        let center_y = photo.y + old_h * 0.5;
        photo.crop = crop.clamp();
        photo.preset = CropPreset::Freehand;
        let new_h = photo.height(ratio);
        photo.y = center_y - new_h * 0.5;
    }

    pub fn apply_crop_preset(&mut self, id: PhotoId, preset: CropPreset) {
        if preset == CropPreset::Freehand {
            if let Some(photo) = self.photos.iter_mut().find(|photo| photo.id == id) {
                photo.preset = CropPreset::Freehand;
            }
            return;
        }
        let Some(photo) = self.photo(id) else {
            return;
        };
        let crop = if preset == CropPreset::Original {
            Crop::full()
        } else {
            crop_rect_for_aspect(
                photo.image_width,
                photo.image_height,
                preset.target_aspect(),
            )
        };
        self.set_crop(id, crop);
        if let Some(photo) = self.photos.iter_mut().find(|photo| photo.id == id) {
            photo.preset = preset;
        }
    }

    pub fn move_layer(&mut self, id: PhotoId, movement: LayerMove) -> bool {
        let mut order: Vec<(i32, PhotoId)> = self
            .photos
            .iter()
            .map(|photo| (photo.z, photo.id))
            .collect();
        order.sort_by_key(|(z, photo_id)| (*z, photo_id.0));
        let mut ids: Vec<PhotoId> = order.into_iter().map(|(_, photo_id)| photo_id).collect();
        let Some(pos) = ids.iter().position(|photo_id| *photo_id == id) else {
            return false;
        };
        match movement {
            LayerMove::Up => {
                if pos + 1 >= ids.len() {
                    return false;
                }
                ids.swap(pos, pos + 1);
            }
            LayerMove::Down => {
                if pos == 0 {
                    return false;
                }
                ids.swap(pos, pos - 1);
            }
            LayerMove::Top => {
                if pos + 1 == ids.len() {
                    return false;
                }
                let moved = ids.remove(pos);
                ids.push(moved);
            }
            LayerMove::Bottom => {
                if pos == 0 {
                    return false;
                }
                let moved = ids.remove(pos);
                ids.insert(0, moved);
            }
        }
        for (index, photo_id) in ids.into_iter().enumerate() {
            if let Some(photo) = self.photos.iter_mut().find(|photo| photo.id == photo_id) {
                photo.z = index as i32;
            }
        }
        true
    }

    #[must_use]
    pub fn photos_front_to_back(&self) -> Vec<&Photo> {
        let mut photos: Vec<&Photo> = self.photos.iter().collect();
        photos.sort_by(|a, b| b.z.cmp(&a.z).then(b.id.0.cmp(&a.id.0)));
        photos
    }

    #[must_use]
    pub fn photo_at(&self, x: f32, y: f32) -> Option<PhotoId> {
        let ratio = self.ratio.value();
        self.photos
            .iter()
            .filter(|photo| {
                let h = photo.height(ratio);
                x >= photo.x && x <= photo.x + photo.w && y >= photo.y && y <= photo.y + h
            })
            .max_by_key(|photo| photo.z)
            .map(|photo| photo.id)
    }
}
