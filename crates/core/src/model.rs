pub const MAX_CANVASES: usize = 20;
pub const DEFAULT_EXPORT_WIDTH: u32 = 1080;
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

    pub fn value(self) -> f32 {
        self.width as f32 / self.height as f32
    }

    pub fn label(self) -> String {
        format!("{}:{}", self.width, self.height)
    }

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
    pub fn full() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }

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

    pub fn target_aspect(self) -> f32 {
        match self {
            CropPreset::Square => 1.0,
            CropPreset::FourFive => 4.0 / 5.0,
            CropPreset::ThreeFour => 3.0 / 4.0,
            CropPreset::ThreeTwo => 3.0 / 2.0,
            CropPreset::SixteenNine => 16.0 / 9.0,
            CropPreset::NineSixteen => 9.0 / 16.0,
            CropPreset::Original | CropPreset::Freehand => 1.0,
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

#[derive(Clone, Debug)]
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
    pub fn height(&self, ratio: f32) -> f32 {
        let aspect = self
            .crop
            .pixel_aspect(self.image_width, self.image_height);
        self.w * ratio / aspect
    }

    pub fn placement(&self, ratio: f32) -> Placement {
        Placement {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.height(ratio),
        }
    }
}

pub fn placed_pixel_aspect(photo: &Photo, ratio: f32) -> f32 {
    let h = photo.height(ratio).max(1e-6);
    (photo.w / h) * ratio
}

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
            let new_w = ((from_x + from_y) * 0.5).max(MIN_WIDTH);
            let new_h = new_w * aspect;
            let nx = if anchor_left { ax } else { ax - new_w };
            let ny = if anchor_top { ay } else { ay - new_h };
            (nx, ny, new_w)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub ratio: AspectRatio,
    pub canvas_count: usize,
    pub photos: Vec<Photo>,
    next_id: u64,
    next_z: i32,
}

impl Document {
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
            origin.x,
            origin.y,
            origin.w,
            origin.h,
            handle,
            pointer_x,
            pointer_y,
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
            crop_rect_for_aspect(photo.image_width, photo.image_height, preset.target_aspect())
        };
        self.set_crop(id, crop);
        if let Some(photo) = self.photos.iter_mut().find(|photo| photo.id == id) {
            photo.preset = preset;
        }
    }

    pub fn move_layer(&mut self, id: PhotoId, movement: LayerMove) -> bool {
        let mut order: Vec<(i32, PhotoId)> = self.photos.iter().map(|photo| (photo.z, photo.id)).collect();
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

    pub fn photos_front_to_back(&self) -> Vec<&Photo> {
        let mut photos: Vec<&Photo> = self.photos.iter().collect();
        photos.sort_by(|a, b| b.z.cmp(&a.z).then(b.id.0.cmp(&a.id.0)));
        photos
    }

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
