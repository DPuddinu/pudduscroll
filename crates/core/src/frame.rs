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
    pub crop: Crop,
    pub preset: CropPreset,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: FrameId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub z: i32,
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

    pub fn remove_canvas(&mut self) -> bool {
        if self.canvas_count <= 1 {
            return false;
        }
        self.canvas_count -= 1;
        true
    }

    pub fn set_ratio(&mut self, ratio: crate::AspectRatio) {
        self.ratio = ratio;
    }

    pub fn restore_frame(&mut self, mut frame: Frame) {
        frame.w = frame.w.max(MIN_WIDTH);
        frame.h = frame.h.max(MIN_WIDTH);
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
            crop: Crop::full(),
            preset: CropPreset::Original,
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

#[must_use]
pub fn frame_pixel_aspect(w: f32, h: f32, canvas_ratio: f32) -> f32 {
    (w / h.max(1.0e-6)) * canvas_ratio
}

#[must_use]
pub fn displayed_cover(frame: &Frame, canvas_ratio: f32) -> Option<(f32, f32)> {
    let photo = frame.photo.as_ref()?;
    let image_aspect = photo
        .crop
        .pixel_aspect(photo.image_width, photo.image_height)
        .max(1.0e-4);
    let frame_aspect = frame_pixel_aspect(frame.w, frame.h, canvas_ratio).max(1.0e-4);
    if image_aspect >= frame_aspect {
        let h = frame.h.max(MIN_WIDTH);
        let w = h * image_aspect / canvas_ratio.max(1.0e-4);
        Some((w, h))
    } else {
        let w = frame.w.max(MIN_WIDTH);
        let h = w * canvas_ratio.max(1.0e-4) / image_aspect;
        Some((w, h))
    }
}

#[must_use]
pub fn cover_fit(frame: &Frame, canvas_ratio: f32) -> Option<CoverFit> {
    let photo = frame.photo.as_ref()?;
    let (w, h) = displayed_cover(frame, canvas_ratio)?;
    let slack_x = (w - frame.w).max(0.0);
    let slack_y = (h - frame.h).max(0.0);
    Some(CoverFit {
        x: frame.x - photo.pan_x.clamp(0.0, 1.0) * slack_x,
        y: frame.y - photo.pan_y.clamp(0.0, 1.0) * slack_y,
        w,
        h,
    })
}

#[must_use]
pub fn cover_contains_frame(frame: &Frame, canvas_ratio: f32) -> bool {
    let Some(fit) = cover_fit(frame, canvas_ratio) else {
        return true;
    };
    fit.x <= frame.x + 1.0e-3
        && fit.y <= frame.y + 1.0e-3
        && fit.x + fit.w + 1.0e-3 >= frame.x + frame.w
        && fit.y + fit.h + 1.0e-3 >= frame.y + frame.h
}

#[must_use]
pub fn pan_for_origin(frame: &Frame, canvas_ratio: f32, image_x: f32, image_y: f32) -> (f32, f32) {
    let Some((w, h)) = displayed_cover(frame, canvas_ratio) else {
        return (0.5, 0.5);
    };
    let slack_x = (w - frame.w).max(0.0);
    let slack_y = (h - frame.h).max(0.0);
    let pan_x = if slack_x <= 1.0e-5 {
        0.5
    } else {
        ((frame.x - image_x) / slack_x).clamp(0.0, 1.0)
    };
    let pan_y = if slack_y <= 1.0e-5 {
        0.5
    } else {
        ((frame.y - image_y) / slack_y).clamp(0.0, 1.0)
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
