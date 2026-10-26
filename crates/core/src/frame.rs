use crate::{
    CANVAS_GRID_STEP, Crop, CropPreset, Handle, MAX_CANVASES, MIN_WIDTH, resize_uniform,
    snap_frame_resize, snap_move,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct FramePhoto {
    pub name: String,
    pub image_width: u32,
    pub image_height: u32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    pub crop: Crop,
    pub preset: CropPreset,
    pub rotation: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Clockwise,
    Counterclockwise,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: FrameId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub z: i32,
    pub border: f32,
    pub border_color: [u8; 4],
    pub photo: Option<FramePhoto>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoverFit {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub ratio: crate::AspectRatio,
    pub canvas_count: usize,
    pub frames: Vec<Frame>,
    next_id: u64,
    next_z: i32,
}

impl Layout {
    #[must_use]
    pub fn new(ratio: crate::AspectRatio) -> Self {
        Self {
            ratio,
            canvas_count: 1,
            frames: Vec::new(),
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

    pub fn add_canvas_left(&mut self) -> bool {
        if self.canvas_count >= MAX_CANVASES {
            return false;
        }
        self.canvas_count += 1;
        for frame in &mut self.frames {
            frame.x += 1.0;
        }
        true
    }

    pub fn remove_canvas(&mut self) -> Option<Vec<FrameId>> {
        if self.canvas_count <= 1 {
            return None;
        }
        let index = self.canvas_count - 1;
        self.canvas_count -= 1;
        let removed = self
            .frames
            .iter()
            .filter(|frame| frame_on_canvas(frame, index))
            .map(|frame| frame.id)
            .collect();
        self.frames.retain(|frame| !frame_on_canvas(frame, index));
        Some(removed)
    }

    #[must_use]
    pub fn adjacent_frame(&self, id: FrameId, to_the_right: bool) -> Option<FrameId> {
        let x = self.frame(id)?.x;
        let neighbor = if to_the_right {
            self.frames
                .iter()
                .filter(|other| other.id != id && other.x > x)
                .min_by(|a, b| {
                    a.x.total_cmp(&b.x)
                        .then(b.z.cmp(&a.z))
                        .then(a.id.0.cmp(&b.id.0))
                })
        } else {
            self.frames
                .iter()
                .filter(|other| other.id != id && other.x < x)
                .max_by(|a, b| {
                    a.x.total_cmp(&b.x)
                        .then(a.z.cmp(&b.z))
                        .then(b.id.0.cmp(&a.id.0))
                })
        }?;
        Some(neighbor.id)
    }

    pub fn move_frame(&mut self, id: FrameId, to_the_right: bool) -> bool {
        let Some(neighbor_id) = self.adjacent_frame(id, to_the_right) else {
            return false;
        };
        let anchor = if to_the_right {
            self.frame(id).map(|frame| frame.x)
        } else {
            self.frame(neighbor_id).map(|frame| frame.x)
        };
        let Some(anchor) = anchor else {
            return false;
        };
        let new_left = if to_the_right { neighbor_id } else { id };
        let new_right = if to_the_right { id } else { neighbor_id };
        let Some(width) = self.frame(new_left).map(|frame| frame.w) else {
            return false;
        };
        if let Some(frame) = self.frame_mut(new_left) {
            frame.x = anchor;
        }
        if let Some(frame) = self.frame_mut(new_right) {
            frame.x = anchor + width;
        }
        true
    }

    pub fn set_ratio(&mut self, ratio: crate::AspectRatio) {
        self.ratio = ratio;
    }

    pub fn restore_frame(&mut self, mut frame: Frame) {
        frame.w = frame.w.max(MIN_WIDTH);
        frame.h = frame.h.max(MIN_WIDTH);
        frame.border = frame.border.clamp(0.0, 1.0);
        self.next_id = self.next_id.max(frame.id.0.saturating_add(1));
        self.next_z = self.next_z.max(frame.z.saturating_add(1));
        self.frames.push(frame);
    }

    pub fn add_frame(&mut self) -> FrameId {
        let id = FrameId(self.next_id);
        self.next_id += 1;
        let slot = self.frames.len() % self.canvas_count.max(1);
        let z = self.next_z;
        self.next_z += 1;
        self.frames.push(Frame {
            id,
            x: slot as f32,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            z,
            border: 0.0,
            border_color: [255, 255, 255, 255],
            photo: None,
        });
        id
    }

    pub fn remove_frame(&mut self, id: FrameId) -> bool {
        let before = self.frames.len();
        self.frames.retain(|frame| frame.id != id);
        self.frames.len() != before
    }

    #[must_use]
    pub fn frame(&self, id: FrameId) -> Option<&Frame> {
        self.frames.iter().find(|frame| frame.id == id)
    }

    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        self.frames.iter_mut().find(|frame| frame.id == id)
    }

    pub fn set_border(&mut self, id: FrameId, border: f32) {
        if let Some(frame) = self.frame_mut(id) {
            frame.border = border.clamp(0.0, 1.0);
        }
    }

    pub fn set_border_color(&mut self, id: FrameId, color: [u8; 4]) {
        if let Some(frame) = self.frame_mut(id) {
            frame.border_color = color;
        }
    }

    pub fn set_frame_rect(&mut self, id: FrameId, x: f32, y: f32, w: f32, h: f32) {
        if let Some(frame) = self.frame_mut(id) {
            frame.x = x;
            frame.y = y;
            frame.w = w.max(MIN_WIDTH);
            frame.h = h.max(MIN_WIDTH);
        }
    }

    pub fn set_frame_aspect(&mut self, id: FrameId, pixel_aspect: f32) {
        let ratio = self.ratio.value().max(1.0e-4);
        let Some(frame) = self.frame_mut(id) else {
            return;
        };
        let pixel_aspect = pixel_aspect.max(0.05);
        let h = (frame.w * ratio / pixel_aspect).max(MIN_WIDTH);
        let cy = frame.y + frame.h * 0.5;
        frame.h = h;
        frame.y = cy - h * 0.5;
    }

    pub fn fill_frame(&mut self, id: FrameId, name: &str, image_width: u32, image_height: u32) {
        let Some(frame) = self.frame_mut(id) else {
            return;
        };
        frame.photo = Some(FramePhoto {
            name: name.to_string(),
            image_width: image_width.max(1),
            image_height: image_height.max(1),
            pan_x: 0.5,
            pan_y: 0.5,
            zoom: MIN_PHOTO_ZOOM,
            crop: Crop::full(),
            preset: CropPreset::Original,
            rotation: 0.0,
        });
    }

    pub fn clear_photo(&mut self, id: FrameId) -> bool {
        let Some(frame) = self.frame_mut(id) else {
            return false;
        };
        if frame.photo.is_none() {
            return false;
        }
        frame.photo = None;
        true
    }

    pub fn set_photo_crop(&mut self, id: FrameId, crop: Crop, preset: CropPreset) {
        if let Some(photo) = self.frame_mut(id).and_then(|frame| frame.photo.as_mut()) {
            photo.crop = crop.clamp();
            photo.preset = preset;
        }
    }

    pub fn set_pan(&mut self, id: FrameId, pan_x: f32, pan_y: f32) {
        if let Some(photo) = self.frame_mut(id).and_then(|frame| frame.photo.as_mut()) {
            photo.pan_x = pan_x.clamp(0.0, 1.0);
            photo.pan_y = pan_y.clamp(0.0, 1.0);
        }
    }

    pub fn zoom_photo(&mut self, id: FrameId, x: f32, y: f32, factor: f32) {
        let ratio = self.ratio.value();
        let Some(frame) = self.frame(id).cloned() else {
            return;
        };
        let (zoom, pan_x, pan_y) = zoom_about(&frame, ratio, x, y, factor);
        let Some(photo) = self.frame_mut(id).and_then(|frame| frame.photo.as_mut()) else {
            return;
        };
        photo.zoom = zoom;
        photo.pan_x = pan_x;
        photo.pan_y = pan_y;
    }

    pub fn turn_photo(&mut self, id: FrameId, turn: Turn) {
        let Some(photo) = self.frame_mut(id).and_then(|frame| frame.photo.as_mut()) else {
            return;
        };
        let delta = match turn {
            Turn::Clockwise => std::f32::consts::FRAC_PI_2,
            Turn::Counterclockwise => -std::f32::consts::FRAC_PI_2,
        };
        photo.rotation = normalize_rotation(photo.rotation + delta);
    }

    pub fn set_rotation(&mut self, id: FrameId, radians: f32) {
        if !radians.is_finite() {
            return;
        }
        if let Some(photo) = self.frame_mut(id).and_then(|frame| frame.photo.as_mut()) {
            photo.rotation = normalize_rotation(radians);
        }
    }

    pub fn move_layer(&mut self, id: FrameId, movement: crate::LayerMove) -> bool {
        let mut order: Vec<(i32, FrameId)> = self
            .frames
            .iter()
            .map(|frame| (frame.z, frame.id))
            .collect();
        order.sort_by_key(|(z, frame_id)| (*z, frame_id.0));
        let mut ids: Vec<FrameId> = order.into_iter().map(|(_, frame_id)| frame_id).collect();
        let Some(pos) = ids.iter().position(|frame_id| *frame_id == id) else {
            return false;
        };
        match movement {
            crate::LayerMove::Up => {
                if pos + 1 >= ids.len() {
                    return false;
                }
                ids.swap(pos, pos + 1);
            }
            crate::LayerMove::Down => {
                if pos == 0 {
                    return false;
                }
                ids.swap(pos, pos - 1);
            }
            crate::LayerMove::Top => {
                if pos + 1 == ids.len() {
                    return false;
                }
                let moved = ids.remove(pos);
                ids.push(moved);
            }
            crate::LayerMove::Bottom => {
                if pos == 0 {
                    return false;
                }
                let moved = ids.remove(pos);
                ids.insert(0, moved);
            }
        }
        for (index, frame_id) in ids.into_iter().enumerate() {
            if let Some(frame) = self.frames.iter_mut().find(|frame| frame.id == frame_id) {
                frame.z = index as i32;
            }
        }
        true
    }

    #[must_use]
    pub fn frames_front_to_back(&self) -> Vec<&Frame> {
        let mut frames: Vec<&Frame> = self.frames.iter().collect();
        frames.sort_by(|a, b| b.z.cmp(&a.z).then(b.id.0.cmp(&a.id.0)));
        frames
    }

    #[must_use]
    pub fn frame_at(&self, x: f32, y: f32) -> Option<FrameId> {
        self.frames
            .iter()
            .filter(|frame| {
                x >= frame.x && x <= frame.x + frame.w && y >= frame.y && y <= frame.y + frame.h
            })
            .max_by_key(|frame| frame.z)
            .map(|frame| frame.id)
    }
}

pub const MIN_PHOTO_ZOOM: f32 = 1.0;
pub const MAX_PHOTO_ZOOM: f32 = 8.0;

#[must_use]
pub fn clamp_photo_zoom(zoom: f32) -> f32 {
    if zoom.is_nan() {
        MIN_PHOTO_ZOOM
    } else {
        zoom.clamp(MIN_PHOTO_ZOOM, MAX_PHOTO_ZOOM)
    }
}

#[must_use]
pub fn border_thickness(border: f32, canvas_width: f32, canvas_height: f32) -> f32 {
    let span = canvas_width.max(0.0).min(canvas_height.max(0.0));
    border.clamp(0.0, 1.0) * 0.2 * span
}

#[must_use]
pub fn frame_pixel_aspect(w: f32, h: f32, canvas_ratio: f32) -> f32 {
    (w / h.max(1.0e-6)) * canvas_ratio
}

#[must_use]
pub fn normalize_rotation(radians: f32) -> f32 {
    if !radians.is_finite() {
        return 0.0;
    }
    let tau = std::f32::consts::TAU;
    let mut angle = radians.rem_euclid(tau);
    if angle > std::f32::consts::PI {
        angle -= tau;
    }
    if angle.abs() <= 1.0e-5 { 0.0 } else { angle }
}

#[must_use]
pub fn photo_local(frame: &Frame, canvas_ratio: f32, x: f32, y: f32) -> (f32, f32) {
    let angle = frame.photo.as_ref().map_or(0.0, |photo| photo.rotation);
    let cx = frame.x + frame.w * 0.5;
    let cy = frame.y + frame.h * 0.5;
    let ratio = canvas_ratio.max(1.0e-4);
    let dx = (x - cx) * ratio;
    let dy = y - cy;
    let (sin, cos) = angle.sin_cos();
    let local_x = dx * cos + dy * sin;
    let local_y = -dx * sin + dy * cos;
    (cx + local_x / ratio, cy + local_y)
}

#[must_use]
pub fn rotation_from_pointer(frame: &Frame, canvas_ratio: f32, x: f32, y: f32) -> f32 {
    let cx = frame.x + frame.w * 0.5;
    let cy = frame.y + frame.h * 0.5;
    let ratio = canvas_ratio.max(1.0e-4);
    let dx = (x - cx) * ratio;
    let dy = y - cy;
    if dx * dx + dy * dy < 1.0e-8 {
        return frame.photo.as_ref().map_or(0.0, |photo| photo.rotation);
    }
    normalize_rotation(dx.atan2(-dy))
}

struct CoverSize {
    image_w: f32,
    image_h: f32,
    bounds_w: f32,
    bounds_h: f32,
}

fn cover_size(frame: &Frame, canvas_ratio: f32) -> Option<CoverSize> {
    let photo = frame.photo.as_ref()?;
    let ratio = canvas_ratio.max(1.0e-4);
    let aspect = photo
        .crop
        .pixel_aspect(photo.image_width, photo.image_height)
        .max(1.0e-4);
    let (sin, cos) = photo.rotation.sin_cos();
    let frame_px_w = frame.w.max(1.0e-4) * ratio;
    let frame_px_h = frame.h.max(1.0e-4);
    let bounds_px_w = frame_px_w * cos.abs() + frame_px_h * sin.abs();
    let bounds_px_h = frame_px_w * sin.abs() + frame_px_h * cos.abs();
    let image_px_w = bounds_px_w.max(bounds_px_h * aspect) * clamp_photo_zoom(photo.zoom);
    Some(CoverSize {
        image_w: image_px_w / ratio,
        image_h: image_px_w / aspect,
        bounds_w: bounds_px_w / ratio,
        bounds_h: bounds_px_h,
    })
}

#[must_use]
pub fn displayed_cover(frame: &Frame, canvas_ratio: f32) -> Option<(f32, f32)> {
    let size = cover_size(frame, canvas_ratio)?;
    Some((size.image_w, size.image_h))
}

#[must_use]
pub fn cover_fit(frame: &Frame, canvas_ratio: f32) -> Option<CoverFit> {
    let photo = frame.photo.as_ref()?;
    let size = cover_size(frame, canvas_ratio)?;
    let cx = frame.x + frame.w * 0.5;
    let cy = frame.y + frame.h * 0.5;
    let slack_x = (size.image_w - size.bounds_w).max(0.0);
    let slack_y = (size.image_h - size.bounds_h).max(0.0);
    Some(CoverFit {
        x: cx - size.bounds_w * 0.5 - photo.pan_x.clamp(0.0, 1.0) * slack_x,
        y: cy - size.bounds_h * 0.5 - photo.pan_y.clamp(0.0, 1.0) * slack_y,
        w: size.image_w,
        h: size.image_h,
    })
}

fn frame_on_canvas(frame: &Frame, index: usize) -> bool {
    let x0 = index as f32;
    let x1 = x0 + 1.0;
    let ix0 = frame.x.max(x0);
    let iy0 = frame.y.max(0.0);
    let ix1 = (frame.x + frame.w).min(x1);
    let iy1 = (frame.y + frame.h).min(1.0);
    ix1 - ix0 > 1.0e-5 && iy1 - iy0 > 1.0e-5
}

#[must_use]
pub fn photo_on_canvases(frame: &Frame, canvas_count: usize) -> Option<FrameRect> {
    let right = canvas_count.max(1) as f32;
    let x0 = frame.x.max(0.0);
    let y0 = frame.y.max(0.0);
    let x1 = (frame.x + frame.w).min(right);
    let y1 = (frame.y + frame.h).min(1.0);
    let w = x1 - x0;
    let h = y1 - y0;
    if w <= 1.0e-5 || h <= 1.0e-5 {
        None
    } else {
        Some(FrameRect { x: x0, y: y0, w, h })
    }
}

#[must_use]
pub fn cover_contains_frame(frame: &Frame, canvas_ratio: f32) -> bool {
    let Some(fit) = cover_fit(frame, canvas_ratio) else {
        return true;
    };
    let corners = [
        (frame.x, frame.y),
        (frame.x + frame.w, frame.y),
        (frame.x, frame.y + frame.h),
        (frame.x + frame.w, frame.y + frame.h),
    ];
    corners.into_iter().all(|(x, y)| {
        let (local_x, local_y) = photo_local(frame, canvas_ratio, x, y);
        local_x >= fit.x - 1.0e-3
            && local_y >= fit.y - 1.0e-3
            && local_x <= fit.x + fit.w + 1.0e-3
            && local_y <= fit.y + fit.h + 1.0e-3
    })
}

fn zoom_about(frame: &Frame, canvas_ratio: f32, x: f32, y: f32, factor: f32) -> (f32, f32, f32) {
    let Some(photo) = frame.photo.as_ref() else {
        return (MIN_PHOTO_ZOOM, 0.5, 0.5);
    };
    let pan_x = photo.pan_x.clamp(0.0, 1.0);
    let pan_y = photo.pan_y.clamp(0.0, 1.0);
    let current = clamp_photo_zoom(photo.zoom);
    if !factor.is_finite() || factor <= 0.0 {
        return (current, pan_x, pan_y);
    }
    let next = clamp_photo_zoom(current * factor);
    if (next - current).abs() <= 1.0e-5 {
        return (current, pan_x, pan_y);
    }
    let Some(size) = cover_size(frame, canvas_ratio) else {
        return (next, pan_x, pan_y);
    };
    if size.image_w <= 1.0e-6 || size.image_h <= 1.0e-6 {
        return (next, pan_x, pan_y);
    }
    let (local_x, local_y) = photo_local(frame, canvas_ratio, x, y);
    let cx = frame.x + frame.w * 0.5;
    let cy = frame.y + frame.h * 0.5;
    let slack_x = (size.image_w - size.bounds_w).max(0.0);
    let slack_y = (size.image_h - size.bounds_h).max(0.0);
    let image_x = cx - size.bounds_w * 0.5 - pan_x * slack_x;
    let image_y = cy - size.bounds_h * 0.5 - pan_y * slack_y;
    let mut u = (local_x - image_x) / size.image_w;
    let mut v = (local_y - image_y) / size.image_h;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        u = 0.5;
        v = 0.5;
    }
    let scale = next / current;
    let new_w = size.image_w * scale;
    let new_h = size.image_h * scale;
    let new_x = image_x + u * (size.image_w - new_w);
    let new_y = image_y + v * (size.image_h - new_h);
    let new_slack_x = (new_w - size.bounds_w).max(0.0);
    let new_slack_y = (new_h - size.bounds_h).max(0.0);
    let pan_x = if new_slack_x <= 1.0e-5 {
        0.5
    } else {
        ((cx - size.bounds_w * 0.5 - new_x) / new_slack_x).clamp(0.0, 1.0)
    };
    let pan_y = if new_slack_y <= 1.0e-5 {
        0.5
    } else {
        ((cy - size.bounds_h * 0.5 - new_y) / new_slack_y).clamp(0.0, 1.0)
    };
    (next, pan_x, pan_y)
}

#[must_use]
pub fn pan_for_origin(frame: &Frame, canvas_ratio: f32, image_x: f32, image_y: f32) -> (f32, f32) {
    let Some(size) = cover_size(frame, canvas_ratio) else {
        return (0.5, 0.5);
    };
    let cx = frame.x + frame.w * 0.5;
    let cy = frame.y + frame.h * 0.5;
    let slack_x = (size.image_w - size.bounds_w).max(0.0);
    let slack_y = (size.image_h - size.bounds_h).max(0.0);
    let pan_x = if slack_x <= 1.0e-5 {
        0.5
    } else {
        ((cx - size.bounds_w * 0.5 - image_x) / slack_x).clamp(0.0, 1.0)
    };
    let pan_y = if slack_y <= 1.0e-5 {
        0.5
    } else {
        ((cy - size.bounds_h * 0.5 - image_y) / slack_y).clamp(0.0, 1.0)
    };
    (pan_x, pan_y)
}

#[must_use]
pub fn snap_frame(frame: &Frame, step: f32) -> (f32, f32) {
    snap_move(frame.x, frame.y, frame.w, frame.h, step)
}

#[must_use]
pub fn snap_frame_to_grid(frame: &Frame) -> (f32, f32) {
    snap_frame(frame, CANVAS_GRID_STEP)
}

#[must_use]
pub fn snap_resized_frame_to_grid(
    handle: Handle,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> (f32, f32, f32, f32) {
    snap_frame_resize(handle, x, y, w, h, CANVAS_GRID_STEP)
}

#[must_use]
pub fn resize_frame(
    rect: FrameRect,
    handle: Handle,
    pointer_x: f32,
    pointer_y: f32,
    lock_aspect: bool,
) -> FrameRect {
    let FrameRect { x, y, w, h } = rect;
    if lock_aspect {
        let (nx, ny, nw) = resize_uniform(x, y, w, h, handle, pointer_x, pointer_y);
        let aspect = if w.abs() < 1.0e-4 { 1.0 } else { h / w };
        let nh = (nw * aspect).max(MIN_WIDTH);
        return FrameRect {
            x: nx,
            y: ny,
            w: nw.max(MIN_WIDTH),
            h: nh,
        };
    }
    let (x, y, w, h) = resize_frame_free(x, y, w, h, handle, pointer_x, pointer_y);
    FrameRect { x, y, w, h }
}

fn resize_frame_free(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    handle: Handle,
    pointer_x: f32,
    pointer_y: f32,
) -> (f32, f32, f32, f32) {
    let left0 = x;
    let right0 = x + w;
    let top0 = y;
    let bottom0 = y + h;
    let (left, right) = match handle {
        Handle::N | Handle::S => (left0, right0),
        Handle::E | Handle::Ne | Handle::Se => (left0, pointer_x.max(left0 + MIN_WIDTH)),
        Handle::W | Handle::Nw | Handle::Sw => (pointer_x.min(right0 - MIN_WIDTH), right0),
    };
    let (top, bottom) = match handle {
        Handle::E | Handle::W => (top0, bottom0),
        Handle::S | Handle::Se | Handle::Sw => (top0, pointer_y.max(top0 + MIN_WIDTH)),
        Handle::N | Handle::Ne | Handle::Nw => (pointer_y.min(bottom0 - MIN_WIDTH), bottom0),
    };
    (
        left,
        top,
        (right - left).max(MIN_WIDTH),
        (bottom - top).max(MIN_WIDTH),
    )
}

#[must_use]
pub fn layout_export_width(layout: &Layout) -> u32 {
    let ratio = layout.ratio.value();
    let mut limit = f32::INFINITY;
    for frame in &layout.frames {
        let Some(photo) = &frame.photo else {
            continue;
        };
        let Some((width, height)) = displayed_cover(frame, ratio) else {
            continue;
        };
        if width <= 1.0e-6 || height <= 1.0e-6 {
            continue;
        }
        let from_width = photo.image_width as f32 * photo.crop.w / width;
        let from_height = ratio * (photo.image_height as f32 * photo.crop.h) / height;
        limit = limit.min(from_width).min(from_height);
    }
    if !limit.is_finite() {
        return crate::DEFAULT_EXPORT_WIDTH;
    }
    limit.floor().max(1.0) as u32
}
