use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use std::sync::Mutex;

use image::RgbaImage;
use pudduscroll_core::{
    AspectRatio, CANVAS_GRID, CANVAS_PRESETS, Crop, CropPreset, Frame, FrameId, FramePhoto, Layout,
    clamp_photo_zoom, normalize_rotation, raster_from_bytes,
};

pub(crate) const MIN_GRID_CELLS: i32 = 2;
pub(crate) const MAX_GRID_CELLS: i32 = 16;
pub(crate) const DEFAULT_GRID_CELLS: i32 = CANVAS_GRID as i32;

pub(crate) fn clamp_grid_cells(cells: i32) -> i32 {
    cells.clamp(MIN_GRID_CELLS, MAX_GRID_CELLS)
}

#[derive(Clone)]
pub struct Blob {
    pub name: String,
    pub ext: String,
    pub bytes: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
    pub proxy: Arc<RgbaImage>,
    profile: Option<Arc<[u8]>>,
    screen_cache: Arc<Mutex<Option<Arc<RgbaImage>>>>,
}

impl Blob {
    pub(crate) fn screen(&self) -> Arc<RgbaImage> {
        let mut slot = self
            .screen_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(image) = slot.as_ref() {
            return Arc::clone(image);
        }
        let image = crate::color::for_display(&self.proxy, self.profile.as_deref());
        *slot = Some(Arc::clone(&image));
        image
    }
}

pub struct Template {
    pub id: u64,
    pub name: String,
    pub cover: Blob,
    pub layout: Layout,
}

pub struct Carousel {
    pub id: u64,
    pub name: String,
    pub layout: Layout,
    pub photos: HashMap<FrameId, Blob>,
    pub grid_cells: i32,
}

pub struct Library {
    pub templates: Vec<Template>,
    pub carousels: Vec<Carousel>,
    pub visuals: u64,
    next_id: u64,
    images_dirty: bool,
    image_generation: u64,
}

#[derive(Clone, Debug)]
pub struct LibrarySnapshot {
    pub generation: u64,
    pub text: String,
    pub images: Option<Vec<(String, Arc<[u8]>)>>,
}

pub struct SaveRequest {
    pub snapshot: LibrarySnapshot,
    pub ack: Option<Sender<Result<(), String>>>,
}

impl Library {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            templates: Vec::new(),
            carousels: Vec::new(),
            visuals: 1,
            next_id: 1,
            images_dirty: false,
            image_generation: 0,
        }
    }

    pub fn load() -> Result<Self, String> {
        let root = library_directory();
        let path = root.join("library.txt");
        if !path.exists() {
            return Ok(Self::empty());
        }
        Self::load_from(&root)
    }

    pub fn create_template(&mut self, name: String, ratio: AspectRatio, cover: Blob) -> u64 {
        let id = self.alloc();
        self.templates.push(Template {
            id,
            name,
            cover,
            layout: Layout::new(ratio),
        });
        self.touch_images();
        id
    }

    pub fn delete_template(&mut self, id: u64) -> bool {
        let before = self.templates.len();
        self.templates.retain(|template| template.id != id);
        let removed = self.templates.len() != before;
        if removed {
            self.touch_images();
        }
        removed
    }

    pub fn duplicate_carousel(&mut self, id: u64) -> Option<u64> {
        let (name, layout, photos, grid_cells) = {
            let source = self.carousels.iter().find(|carousel| carousel.id == id)?;
            (
                format!("{} copy", source.name),
                source.layout.clone(),
                source.photos.clone(),
                source.grid_cells,
            )
        };
        let index = self
            .carousels
            .iter()
            .position(|carousel| carousel.id == id)?;
        let new_id = self.alloc();
        self.carousels.insert(
            index + 1,
            Carousel {
                id: new_id,
                name,
                layout,
                photos,
                grid_cells,
            },
        );
        self.touch_images();
        Some(new_id)
    }

    pub fn delete_carousel(&mut self, id: u64) -> bool {
        let before = self.carousels.len();
        self.carousels.retain(|carousel| carousel.id != id);
        let removed = self.carousels.len() != before;
        if removed {
            self.touch_images();
        }
        removed
    }

    pub fn instantiate(&mut self, template_id: u64) -> Option<u64> {
        let template = self
            .templates
            .iter()
            .find(|template| template.id == template_id)?;
        let name = template.name.clone();
        let layout = template.layout.clone();
        let id = self.alloc();
        self.carousels.push(Carousel {
            id,
            name,
            layout,
            photos: HashMap::new(),
            grid_cells: DEFAULT_GRID_CELLS,
        });
        Some(id)
    }

    pub fn set_photo(&mut self, carousel_id: u64, frame_id: FrameId, blob: Blob) {
        let Some(carousel) = self
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
        else {
            return;
        };
        carousel
            .layout
            .fill_frame(frame_id, &blob.name, blob.width, blob.height);
        carousel.photos.insert(frame_id, blob);
        self.touch_images();
    }

    pub fn clear_photo(&mut self, carousel_id: u64, frame_id: FrameId) -> bool {
        let Some(carousel) = self
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
        else {
            return false;
        };
        let cleared = carousel.layout.clear_photo(frame_id);
        carousel.photos.remove(&frame_id);
        if cleared {
            self.touch_images();
        }
        cleared
    }

    #[must_use]
    pub fn snapshot(&self) -> LibrarySnapshot {
        LibrarySnapshot {
            generation: self.image_generation,
            text: serialize(self),
            images: self.images_dirty.then(|| image_files(self)),
        }
    }

    pub fn stored_generation(&mut self, generation: u64) {
        if self.image_generation == generation {
            self.images_dirty = false;
        }
    }

    pub(crate) fn touch_images(&mut self) {
        self.images_dirty = true;
        self.image_generation = self.image_generation.wrapping_add(1);
    }

    fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn load_from(root: &Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(root.join("library.txt")).map_err(|error| error.to_string())?;
        parse_library(&text, root)
    }

    #[cfg(test)]
    fn store_to(&mut self, root: &Path) -> Result<(), String> {
        let snapshot = self.snapshot();
        write_snapshot(root, &snapshot)?;
        self.stored_generation(snapshot.generation);
        Ok(())
    }
}

pub fn write_snapshot(root: &Path, snapshot: &LibrarySnapshot) -> Result<(), String> {
    std::fs::create_dir_all(root.join("images")).map_err(|error| error.to_string())?;
    if let Some(images) = &snapshot.images {
        write_named_images(root, images)?;
    }
    let destination = root.join("library.txt");
    let temporary = root.join("library.txt.tmp");
    std::fs::write(&temporary, &snapshot.text).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&destination);
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(())
}

#[must_use]
pub fn library_directory() -> PathBuf {
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(dir).join("pudduscroll");
    }
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(dir).join("pudduscroll");
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if cfg!(target_os = "macos") {
            return home
                .join("Library")
                .join("Application Support")
                .join("pudduscroll");
        }
        return home.join(".local").join("share").join("pudduscroll");
    }
    PathBuf::from("pudduscroll")
}

pub fn load_blob(path: &Path) -> Result<Blob, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    decode_blob(path, bytes)
}

fn decode_blob(path: &Path, bytes: Vec<u8>) -> Result<Blob, String> {
    let raster = raster_from_bytes(&bytes)
        .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("photo")
        .to_string();
    let profile = crate::color::embedded_profile(&bytes).map(Arc::from);
    Ok(Blob {
        name,
        ext: image_ext(path),
        width: raster.width.max(1),
        height: raster.height.max(1),
        bytes: Arc::from(bytes),
        proxy: Arc::new(raster.proxy),
        profile,
        screen_cache: Arc::new(Mutex::new(None)),
    })
}

fn image_ext(path: &Path) -> String {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("webp") => "webp".to_string(),
        Some("jpg" | "jpeg") => "jpg".to_string(),
        _ => "png".to_string(),
    }
}

fn cover_file(template: &Template) -> String {
    format!("t{}.{}", template.id, template.cover.ext)
}

fn photo_file(carousel_id: u64, frame_id: FrameId, ext: &str) -> String {
    format!("c{carousel_id}-f{}.{ext}", frame_id.0)
}

fn image_files(library: &Library) -> Vec<(String, Arc<[u8]>)> {
    let mut files = Vec::new();
    for template in &library.templates {
        files.push((cover_file(template), Arc::clone(&template.cover.bytes)));
    }
    for carousel in &library.carousels {
        for (frame_id, blob) in &carousel.photos {
            files.push((
                photo_file(carousel.id, *frame_id, &blob.ext),
                Arc::clone(&blob.bytes),
            ));
        }
    }
    files
}

fn write_named_images(root: &Path, files: &[(String, Arc<[u8]>)]) -> Result<(), String> {
    let dir = root.join("images");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes.as_ref()).map_err(|error| error.to_string())?;
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !files.iter().any(|(kept, _)| kept == &name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

fn serialize(library: &Library) -> String {
    let mut out = String::new();
    out.push_str("pudduscroll 1\n");
    let _ = writeln!(out, "next {}", library.next_id);
    for template in &library.templates {
        let _ = writeln!(out, "template {}", template.id);
        out.push_str("name ");
        push_quoted(&mut out, &template.name);
        out.push('\n');
        let _ = writeln!(
            out,
            "ratio {} {}",
            template.layout.ratio.width, template.layout.ratio.height
        );
        let _ = writeln!(out, "canvases {}", template.layout.canvas_count);
        let _ = writeln!(out, "cover {}", template.cover.ext);
        push_frames(&mut out, &template.layout, None);
    }
    for carousel in &library.carousels {
        let _ = writeln!(out, "carousel {}", carousel.id);
        out.push_str("name ");
        push_quoted(&mut out, &carousel.name);
        out.push('\n');
        let _ = writeln!(
            out,
            "ratio {} {}",
            carousel.layout.ratio.width, carousel.layout.ratio.height
        );
        let _ = writeln!(out, "canvases {}", carousel.layout.canvas_count);
        let _ = writeln!(out, "squares {}", clamp_grid_cells(carousel.grid_cells));
        push_frames(&mut out, &carousel.layout, Some(carousel));
    }
    out
}

fn push_frames(out: &mut String, layout: &Layout, carousel: Option<&Carousel>) {
    for frame in &layout.frames {
        let _ = writeln!(
            out,
            "frame {} {:.6} {:.6} {:.6} {:.6} {} {:.6} {} {} {}",
            frame.id.0,
            frame.x,
            frame.y,
            frame.w,
            frame.h,
            frame.z,
            frame.border,
            frame.border_color[0],
            frame.border_color[1],
            frame.border_color[2]
        );
        let Some(photo) = &frame.photo else {
            continue;
        };
        let Some(carousel) = carousel else {
            continue;
        };
        let Some(blob) = carousel.photos.get(&frame.id) else {
            continue;
        };
        out.push_str("photo ");
        push_quoted(out, &photo.name);
        let _ = writeln!(
            out,
            " {} {} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {} {} {:.6} {:.6}",
            photo.image_width,
            photo.image_height,
            photo.pan_x,
            photo.pan_y,
            photo.crop.x,
            photo.crop.y,
            photo.crop.w,
            photo.crop.h,
            preset_token(photo.preset),
            blob.ext,
            photo.rotation,
            photo.zoom
        );
    }
}

fn push_quoted(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            other => out.push(other),
        }
    }
    out.push('"');
}

fn parse_library(text: &str, root: &Path) -> Result<Library, String> {
    let mut library = Library::empty();
    let mut kind: Option<ItemKind> = None;
    let mut template: Option<TemplateBuild> = None;
    let mut carousel: Option<CarouselBuild> = None;
    let mut saw_header = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (word, rest) = split_word(line);
        let place = index + 1;
        match word {
            "pudduscroll" => {
                if rest != "1" {
                    return Err(format!("line {place}: unknown library version"));
                }
                saw_header = true;
            }
            "next" => {
                library.next_id = parse_u64(rest, place)?;
            }
            "template" => {
                finish(&mut library, &mut template, &mut carousel, root)?;
                kind = Some(ItemKind::Template);
                template = Some(TemplateBuild {
                    id: parse_u64(rest, place)?,
                    name: String::new(),
                    ratio: AspectRatio::FOUR_FIVE,
                    canvases: 1,
                    cover_ext: String::new(),
                    frames: Vec::new(),
                });
            }
            "carousel" => {
                finish(&mut library, &mut template, &mut carousel, root)?;
                kind = Some(ItemKind::Carousel);
                carousel = Some(CarouselBuild {
                    id: parse_u64(rest, place)?,
                    name: String::new(),
                    ratio: AspectRatio::FOUR_FIVE,
                    canvases: 1,
                    grid_cells: DEFAULT_GRID_CELLS,
                    frames: Vec::new(),
                });
            }
            "name" => match kind {
                Some(ItemKind::Template) => {
                    template_mut(&mut template, place)?.name = parse_quoted(rest, place)?;
                }
                Some(ItemKind::Carousel) => {
                    carousel_mut(&mut carousel, place)?.name = parse_quoted(rest, place)?;
                }
                None => return Err(format!("line {place}: name without an item")),
            },
            "ratio" => {
                let ratio = parse_ratio(rest, place)?;
                match kind {
                    Some(ItemKind::Template) => template_mut(&mut template, place)?.ratio = ratio,
                    Some(ItemKind::Carousel) => carousel_mut(&mut carousel, place)?.ratio = ratio,
                    None => return Err(format!("line {place}: ratio without an item")),
                }
            }
            "canvases" => {
                let count = parse_u64(rest, place)? as usize;
                match kind {
                    Some(ItemKind::Template) => {
                        template_mut(&mut template, place)?.canvases = count;
                    }
                    Some(ItemKind::Carousel) => {
                        carousel_mut(&mut carousel, place)?.canvases = count;
                    }
                    None => return Err(format!("line {place}: canvases without an item")),
                }
            }
            "cover" => {
                template_mut(&mut template, place)?.cover_ext = rest.to_string();
            }
            "squares" => {
                let cells = i32::try_from(parse_u64(rest, place)?).unwrap_or(MAX_GRID_CELLS);
                match kind {
                    Some(ItemKind::Carousel) => {
                        carousel_mut(&mut carousel, place)?.grid_cells = cells;
                    }
                    Some(ItemKind::Template) | None => {
                        return Err(format!("line {place}: squares without a carousel"));
                    }
                }
            }
            "frame" => {
                let frame = parse_frame(rest, place)?;
                match kind {
                    Some(ItemKind::Template) => {
                        template_mut(&mut template, place)?.frames.push(frame);
                    }
                    Some(ItemKind::Carousel) => {
                        carousel_mut(&mut carousel, place)?.frames.push(frame);
                    }
                    None => return Err(format!("line {place}: frame without an item")),
                }
            }
            "photo" => {
                let photo = parse_photo(rest, place)?;
                let build = carousel_mut(&mut carousel, place)?;
                let Some(frame) = build.frames.last_mut() else {
                    return Err(format!("line {place}: photo without a frame"));
                };
                frame.photo = Some(photo);
            }
            _ => return Err(format!("line {place}: unknown {word}")),
        }
    }
    if !saw_header {
        return Err("library file is missing its header".to_string());
    }
    finish(&mut library, &mut template, &mut carousel, root)?;
    let mut max_id = library.next_id.saturating_sub(1);
    for template in &library.templates {
        max_id = max_id.max(template.id);
    }
    for carousel in &library.carousels {
        max_id = max_id.max(carousel.id);
    }
    library.next_id = library.next_id.max(max_id.saturating_add(1)).max(1);
    Ok(library)
}

fn finish(
    library: &mut Library,
    template: &mut Option<TemplateBuild>,
    carousel: &mut Option<CarouselBuild>,
    root: &Path,
) -> Result<(), String> {
    if let Some(build) = template.take() {
        library.templates.push(build.finish(root)?);
    }
    if let Some(build) = carousel.take() {
        library.carousels.push(build.finish(root)?);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ItemKind {
    Template,
    Carousel,
}

struct FrameBuild {
    id: FrameId,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    z: i32,
    border: f32,
    border_color: [u8; 4],
    photo: Option<PhotoBuild>,
}

struct PhotoBuild {
    name: String,
    pan_x: f32,
    pan_y: f32,
    crop: Crop,
    preset: CropPreset,
    ext: String,
    rotation: f32,
    zoom: f32,
}

struct TemplateBuild {
    id: u64,
    name: String,
    ratio: AspectRatio,
    canvases: usize,
    cover_ext: String,
    frames: Vec<FrameBuild>,
}

struct CarouselBuild {
    id: u64,
    name: String,
    ratio: AspectRatio,
    canvases: usize,
    grid_cells: i32,
    frames: Vec<FrameBuild>,
}

impl TemplateBuild {
    fn finish(self, root: &Path) -> Result<Template, String> {
        if self.name.trim().is_empty() {
            return Err(format!("template {} has no name", self.id));
        }
        if self.cover_ext.is_empty() {
            return Err(format!("template {} has no cover", self.id));
        }
        let path = root
            .join("images")
            .join(format!("t{}.{}", self.id, self.cover_ext));
        let mut cover = load_blob(&path)?;
        cover.name.clone_from(&self.name);
        let mut layout = Layout::new(self.ratio);
        layout.set_canvas_count(self.canvases);
        push_loaded_frames(&mut layout, &self.frames, None)?;
        Ok(Template {
            id: self.id,
            name: self.name,
            cover,
            layout,
        })
    }
}

impl CarouselBuild {
    fn finish(self, root: &Path) -> Result<Carousel, String> {
        if self.name.trim().is_empty() {
            return Err(format!("carousel {} has no name", self.id));
        }
        let mut photos = HashMap::new();
        let mut layout = Layout::new(self.ratio);
        layout.set_canvas_count(self.canvases);
        push_loaded_frames(
            &mut layout,
            &self.frames,
            Some((self.id, root, &mut photos)),
        )?;
        Ok(Carousel {
            id: self.id,
            name: self.name,
            layout,
            photos,
            grid_cells: clamp_grid_cells(self.grid_cells),
        })
    }
}

fn push_loaded_frames(
    layout: &mut Layout,
    frames: &[FrameBuild],
    mut photos: Option<(u64, &Path, &mut HashMap<FrameId, Blob>)>,
) -> Result<(), String> {
    for source in frames {
        let mut frame = Frame {
            id: source.id,
            x: source.x,
            y: source.y,
            w: source.w,
            h: source.h,
            z: source.z,
            border: source.border,
            border_color: source.border_color,
            photo: None,
        };
        if let Some(photo) = &source.photo
            && let Some((carousel_id, root, blobs)) = photos.as_mut()
        {
            let path = root
                .join("images")
                .join(photo_file(*carousel_id, source.id, &photo.ext));
            let mut blob = load_blob(&path)?;
            blob.name.clone_from(&photo.name);
            frame.photo = Some(FramePhoto {
                name: photo.name.clone(),
                image_width: blob.width.max(1),
                image_height: blob.height.max(1),
                pan_x: photo.pan_x.clamp(0.0, 1.0),
                pan_y: photo.pan_y.clamp(0.0, 1.0),
                zoom: clamp_photo_zoom(photo.zoom),
                crop: photo.crop.clamp(),
                preset: photo.preset,
                rotation: normalize_rotation(photo.rotation),
            });
            blobs.insert(source.id, blob);
        }
        layout.restore_frame(frame);
    }
    Ok(())
}

fn template_mut(
    template: &mut Option<TemplateBuild>,
    line: usize,
) -> Result<&mut TemplateBuild, String> {
    template
        .as_mut()
        .ok_or_else(|| format!("line {line}: missing template"))
}

fn carousel_mut(
    carousel: &mut Option<CarouselBuild>,
    line: usize,
) -> Result<&mut CarouselBuild, String> {
    carousel
        .as_mut()
        .ok_or_else(|| format!("line {line}: missing carousel"))
}

fn split_word(line: &str) -> (&str, &str) {
    let line = line.trim();
    line.split_once(char::is_whitespace)
        .map_or((line, ""), |(word, rest)| (word, rest.trim()))
}

fn parse_u64(text: &str, line: usize) -> Result<u64, String> {
    let token = split_word(text).0;
    token
        .parse::<u64>()
        .map_err(|error| format!("line {line}: expected a number: {error}"))
}

fn parse_f32(text: &str) -> Result<f32, String> {
    let value = text
        .parse::<f32>()
        .map_err(|error| format!("expected a number, got {text}: {error}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("invalid number {text}"))
    }
}

fn parse_ratio(text: &str, line: usize) -> Result<AspectRatio, String> {
    let (width, height) = split_word(text);
    let width = parse_u64(width, line)? as u32;
    let height = parse_u64(height, line)? as u32;
    CANVAS_PRESETS
        .into_iter()
        .find(|preset| preset.width == width && preset.height == height)
        .ok_or_else(|| format!("line {line}: unknown canvas aspect"))
}

fn parse_quoted(text: &str, line: usize) -> Result<String, String> {
    let text = text.trim();
    if !text.starts_with('"') {
        return Err(format!("line {line}: expected a quoted name"));
    }
    let mut out = String::new();
    let mut chars = text[1..].chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let Some(next) = chars.next() else {
                    return Err(format!("line {line}: bad escape"));
                };
                out.push(next);
            }
            '"' => return Ok(out),
            other => out.push(other),
        }
    }
    Err(format!("line {line}: unterminated name"))
}

#[expect(
    clippy::many_single_char_names,
    reason = "frame records use x, y, w, h, and z"
)]
fn parse_frame(text: &str, line: usize) -> Result<FrameBuild, String> {
    let mut parts = text.split_whitespace();
    let id = parts
        .next()
        .ok_or_else(|| format!("line {line}: frame id"))?
        .parse::<u64>()
        .map_err(|error| format!("line {line}: frame id: {error}"))?;
    let mut number = || -> Result<f32, String> {
        parse_f32(
            parts
                .next()
                .ok_or_else(|| format!("line {line}: frame value"))?,
        )
        .map_err(|error| format!("line {line}: {error}"))
    };
    let x = number()?;
    let y = number()?;
    let w = number()?;
    let h = number()?;
    let z = parts
        .next()
        .ok_or_else(|| format!("line {line}: frame layer"))?
        .parse::<i32>()
        .map_err(|error| format!("line {line}: frame layer: {error}"))?;
    let border = match parts.next() {
        Some(token) => parse_f32(token)
            .map_err(|error| format!("line {line}: {error}"))?
            .clamp(0.0, 1.0),
        None => 0.0,
    };
    let border_color = if let Some(red) = parts.next() {
        let green = parts
            .next()
            .ok_or_else(|| format!("line {line}: border color"))?;
        let blue = parts
            .next()
            .ok_or_else(|| format!("line {line}: border color"))?;
        [
            parse_channel(red, line)?,
            parse_channel(green, line)?,
            parse_channel(blue, line)?,
            255,
        ]
    } else {
        [255, 255, 255, 255]
    };
    Ok(FrameBuild {
        id: FrameId(id),
        x,
        y,
        w,
        h,
        z,
        border,
        border_color,
        photo: None,
    })
}

fn parse_channel(text: &str, line: usize) -> Result<u8, String> {
    text.parse::<u8>()
        .map_err(|error| format!("line {line}: border color: {error}"))
}

fn parse_photo(text: &str, line: usize) -> Result<PhotoBuild, String> {
    let name = parse_quoted(text, line)?;
    let rest = after_quoted(text).ok_or_else(|| format!("line {line}: photo values"))?;
    let mut parts = rest.split_whitespace();
    let mut token = || {
        parts
            .next()
            .ok_or_else(|| format!("line {line}: photo value"))
    };
    token()?
        .parse::<u32>()
        .map_err(|error| format!("line {line}: photo size: {error}"))?;
    token()?
        .parse::<u32>()
        .map_err(|error| format!("line {line}: photo size: {error}"))?;
    let mut number = || -> Result<f32, String> {
        parse_f32(token()?).map_err(|error| format!("line {line}: {error}"))
    };
    let pan_x = number()?;
    let pan_y = number()?;
    let crop = Crop {
        x: number()?,
        y: number()?,
        w: number()?,
        h: number()?,
    };
    let preset = preset_from(token()?).ok_or_else(|| format!("line {line}: crop preset"))?;
    let ext = token()?.to_string();
    let rotation = match token() {
        Ok(value) => parse_f32(value).map_err(|error| format!("line {line}: {error}"))?,
        Err(_) => 0.0,
    };
    let zoom = match token() {
        Ok(value) => parse_f32(value).map_err(|error| format!("line {line}: {error}"))?,
        Err(_) => 1.0,
    };
    Ok(PhotoBuild {
        name,
        pan_x,
        pan_y,
        crop,
        preset,
        ext,
        rotation,
        zoom,
    })
}

fn after_quoted(text: &str) -> Option<&str> {
    let text = text.trim();
    if !text.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    for (index, ch) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some(text[index + ch.len_utf8()..].trim()),
            _ => {}
        }
    }
    None
}

fn preset_token(preset: CropPreset) -> &'static str {
    match preset {
        CropPreset::Original => "original",
        CropPreset::Square => "square",
        CropPreset::FourFive => "fourfive",
        CropPreset::ThreeFour => "threefour",
        CropPreset::ThreeTwo => "threetwo",
        CropPreset::SixteenNine => "sixteenine",
        CropPreset::NineSixteen => "ninesixteen",
        CropPreset::Freehand => "freehand",
    }
}

fn preset_from(token: &str) -> Option<CropPreset> {
    Some(match token {
        "original" => CropPreset::Original,
        "square" => CropPreset::Square,
        "fourfive" => CropPreset::FourFive,
        "threefour" => CropPreset::ThreeFour,
        "threetwo" => CropPreset::ThreeTwo,
        "sixteenine" => CropPreset::SixteenNine,
        "ninesixteen" => CropPreset::NineSixteen,
        "freehand" => CropPreset::Freehand,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_roundtrip_keeps_template_and_carousel_apart() {
        let root =
            std::env::temp_dir().join(format!("pudduscroll-library-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut library = Library::empty();
        let cover = sample_blob("dune.png");
        let id = library.create_template("Sea \"m\"".to_string(), AspectRatio::FOUR_FIVE, cover);
        let _frame = {
            let template = library
                .templates
                .iter_mut()
                .find(|template| template.id == id)
                .unwrap();
            template.layout.add_canvas();
            let frame = template.layout.add_frame();
            template.layout.set_frame_rect(frame, 0.0, 0.0, 2.0, 1.0);
            frame
        };
        let carousel_id = library.instantiate(id).unwrap();
        library.set_photo(carousel_id, FrameId(1), sample_blob("night.jpg"));
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .set_pan(FrameId(1), 0.2, 0.7);
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .set_rotation(FrameId(1), 0.4);
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .frame_mut(FrameId(1))
            .unwrap()
            .photo
            .as_mut()
            .unwrap()
            .zoom = 2.0;
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .set_border(FrameId(1), 0.35);
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .set_border_color(FrameId(1), [12, 34, 56, 255]);
        library.store_to(&root).unwrap();
        library
            .templates
            .iter_mut()
            .find(|template| template.id == id)
            .unwrap()
            .layout
            .set_frame_rect(FrameId(1), 0.25, 0.25, 0.5, 0.5);
        library.store_to(&root).unwrap();
        let loaded = Library::load_from(&root).unwrap();
        assert_eq!(loaded.templates.len(), 1);
        assert_eq!(loaded.carousels.len(), 1);
        assert_eq!(loaded.templates[0].name, "Sea \"m\"");
        assert_eq!(loaded.templates[0].layout.canvas_count, 2);
        assert_eq!(loaded.templates[0].layout.frames.len(), 1);
        let template_frame = &loaded.templates[0].layout.frames[0];
        assert!(template_frame.photo.is_none());
        assert!((template_frame.w - 0.5).abs() < 1e-4);
        let carousel = &loaded.carousels[0];
        assert_eq!(carousel.name, "Sea \"m\"");
        assert_eq!(carousel.photos.len(), 1);
        let frame = &carousel.layout.frames[0];
        assert!((frame.w - 2.0).abs() < 1e-4);
        let photo = frame.photo.as_ref().unwrap();
        assert_eq!(photo.name, "night.jpg");
        assert!((photo.pan_x - 0.2).abs() < 1e-4);
        assert!((photo.pan_y - 0.7).abs() < 1e-4);
        assert!((photo.rotation - 0.4).abs() < 1e-4);
        assert!((photo.zoom - 2.0).abs() < 1e-4);
        assert!((frame.border - 0.35).abs() < 1e-4);
        assert_eq!(frame.border_color, [12, 34, 56, 255]);
        assert!(carousel.photos.contains_key(&frame.id));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_carousel_copies_layout_and_photos() {
        let mut library = Library::empty();
        let template_id = library.create_template(
            "Sea".to_string(),
            AspectRatio::FOUR_FIVE,
            sample_blob("dune.png"),
        );
        let carousel_id = library.instantiate(template_id).unwrap();
        let frame = library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .add_frame();
        library.set_photo(carousel_id, frame, sample_blob("night.jpg"));
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .set_pan(frame, 0.2, 0.7);
        let copy_id = library.duplicate_carousel(carousel_id).unwrap();
        assert_ne!(copy_id, carousel_id);
        assert_eq!(library.carousels[1].id, copy_id);
        assert_eq!(library.carousels[1].name, "Sea copy");
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout
            .set_pan(frame, 0.9, 0.1);
        library.clear_photo(carousel_id, frame);
        let copy = library
            .carousels
            .iter()
            .find(|carousel| carousel.id == copy_id)
            .unwrap();
        let photo = copy.layout.frame(frame).unwrap().photo.as_ref().unwrap();
        assert!((photo.pan_x - 0.2).abs() < 1e-4);
        assert!((photo.pan_y - 0.7).abs() < 1e-4);
        assert!(copy.photos.contains_key(&frame));
        assert!(library.duplicate_carousel(99).is_none());
    }

    #[test]
    fn carousel_grid_squares_stay_on_that_carousel() {
        let root =
            std::env::temp_dir().join(format!("pudduscroll-grid-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut library = Library::empty();
        let template_id = library.create_template(
            "Sea".to_string(),
            AspectRatio::FOUR_FIVE,
            sample_blob("dune.png"),
        );
        let first = library.instantiate(template_id).unwrap();
        let second = library.instantiate(template_id).unwrap();
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == first)
            .unwrap()
            .grid_cells = 5;
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == second)
            .unwrap()
            .grid_cells = 12;
        let copy = library.duplicate_carousel(first).unwrap();
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == first)
            .unwrap()
            .grid_cells = 3;
        assert_eq!(
            library
                .carousels
                .iter()
                .find(|carousel| carousel.id == copy)
                .unwrap()
                .grid_cells,
            5
        );
        assert_eq!(
            library
                .carousels
                .iter()
                .find(|carousel| carousel.id == second)
                .unwrap()
                .grid_cells,
            12
        );
        library.store_to(&root).unwrap();
        let loaded = Library::load_from(&root).unwrap();
        assert_eq!(loaded.carousels[0].grid_cells, 3);
        assert_eq!(loaded.carousels[1].grid_cells, 5);
        assert_eq!(loaded.carousels[2].grid_cells, 12);
        let legacy = "\
pudduscroll 1
next 2
carousel 1
name \"Old\"
ratio 4 5
canvases 1
";
        let parsed = parse_library(legacy, Path::new(".")).unwrap();
        assert_eq!(parsed.carousels[0].grid_cells, DEFAULT_GRID_CELLS);
        let wide = "\
pudduscroll 1
next 2
carousel 1
name \"Wide\"
ratio 4 5
canvases 1
squares 99
";
        let parsed = parse_library(wide, Path::new(".")).unwrap();
        assert_eq!(parsed.carousels[0].grid_cells, MAX_GRID_CELLS);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sample_blob(name: &str) -> Blob {
        let image = RgbaImage::from_pixel(8, 6, image::Rgba([20, 30, 40, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image.clone())
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        Blob {
            name: name.to_string(),
            ext: "png".to_string(),
            bytes: Arc::from(bytes),
            width: image.width(),
            height: image.height(),
            proxy: Arc::new(image),
            profile: None,
            screen_cache: Arc::new(Mutex::new(None)),
        }
    }
}
