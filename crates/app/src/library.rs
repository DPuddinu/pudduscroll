use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::io::{Cursor, Read, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use std::sync::Mutex;

use image::RgbaImage;
use pudduscroll_core::{
    AspectRatio, CANVAS_GRID, CANVAS_PRESETS, Crop, CropPreset, Frame, FrameId, FramePhoto, Layout,
    clamp_photo_zoom, normalize_rotation, raster_from_bytes,
};

pub(crate) const MIN_GRID_CELLS: i32 = 2;
pub(crate) const MAX_GRID_CELLS: i32 = 16;
pub(crate) const DEFAULT_GRID_CELLS: i32 = CANVAS_GRID as i32;
pub(crate) const MAX_LINK_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PASTE_BYTES: usize = 128 * 1024 * 1024;
const MAX_PACKAGE_TEXT: u64 = 2 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const LINK_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn clamp_grid_cells(cells: i32) -> i32 {
    cells.clamp(MIN_GRID_CELLS, MAX_GRID_CELLS)
}

fn fresh_carousel_name(carousels: &[Carousel]) -> String {
    let taken = |name: &str| carousels.iter().any(|carousel| carousel.name == name);
    if !taken("Carousel") {
        return "Carousel".to_string();
    }
    for index in 2..10_000 {
        let name = format!("Carousel {index}");
        if !taken(&name) {
            return name;
        }
    }
    format!("Carousel {}", carousels.len() + 1)
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
    pub source: Option<String>,
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

#[derive(Clone)]
pub struct PhotoDisk {
    pub ext: String,
    pub source: Option<String>,
}

pub struct Carousel {
    pub id: u64,
    pub name: String,
    pub layout: Layout,
    pub photos: HashMap<FrameId, Blob>,
    pub disk: HashMap<FrameId, PhotoDisk>,
    pub cover: Option<Blob>,
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
    pub keep: HashSet<String>,
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
        if !crate::disk::exists(&path) {
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
        let (name, layout, photos, disk, cover, grid_cells) = {
            let source = self.carousels.iter().find(|carousel| carousel.id == id)?;
            (
                format!("{} copy", source.name),
                source.layout.clone(),
                source.photos.clone(),
                source.disk.clone(),
                source.cover.clone(),
                source.grid_cells,
            )
        };
        let index = self
            .carousels
            .iter()
            .position(|carousel| carousel.id == id)?;
        let new_id = self.alloc();
        copy_carousel_files(id, new_id, &disk, cover.as_ref());
        self.carousels.insert(
            index + 1,
            Carousel {
                id: new_id,
                name,
                layout,
                photos,
                disk,
                cover,
                grid_cells,
            },
        );
        self.touch_images();
        Some(new_id)
    }

    pub fn import_carousel(&mut self, mut carousel: Carousel) -> u64 {
        let id = self.alloc();
        carousel.id = id;
        self.carousels.push(carousel);
        self.touch_images();
        id
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

    pub fn create_carousel(&mut self) -> u64 {
        let name = fresh_carousel_name(&self.carousels);
        let id = self.alloc();
        let mut layout = Layout::new(AspectRatio::FOUR_FIVE);
        layout.add_frame(0);
        self.carousels.push(Carousel {
            id,
            name,
            layout,
            photos: HashMap::new(),
            disk: HashMap::new(),
            cover: None,
            grid_cells: DEFAULT_GRID_CELLS,
        });
        id
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
            disk: HashMap::new(),
            cover: None,
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
        carousel.disk.insert(
            frame_id,
            PhotoDisk {
                ext: blob.ext.clone(),
                source: blob.source.clone(),
            },
        );
        carousel.photos.insert(frame_id, blob);
        self.touch_images();
    }

    pub fn set_cover(&mut self, carousel_id: u64, cover: Blob) {
        let Some(carousel) = self
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
        else {
            return;
        };
        carousel.cover = Some(cover);
        self.touch_images();
    }

    pub fn clear_photos(&mut self, carousel_id: u64) {
        let Some(carousel) = self
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
        else {
            return;
        };
        carousel.photos.clear();
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
        carousel.disk.remove(&frame_id);
        if cleared {
            self.touch_images();
        }
        cleared
    }

    #[must_use]
    pub fn snapshot(&self) -> LibrarySnapshot {
        let keep = referenced_image_names(self);
        LibrarySnapshot {
            generation: self.image_generation,
            text: serialize(self),
            images: self.images_dirty.then(|| image_files(self)),
            keep,
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
        let text = crate::disk::read_to_string(root.join("library.txt"))
            .map_err(|error| error.to_string())?;
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
    crate::disk::create_dir_all(root.join("images")).map_err(|error| error.to_string())?;
    if let Some(images) = &snapshot.images {
        write_named_images(root, images, &snapshot.keep)?;
    }
    let destination = root.join("library.txt");
    let temporary = root.join("library.txt.tmp");
    crate::disk::write(&temporary, &snapshot.text).map_err(|error| error.to_string())?;
    let _ = crate::disk::remove_file(&destination);
    crate::disk::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    crate::disk::sync();
    Ok(())
}

#[must_use]
pub fn library_directory() -> PathBuf {
    #[cfg(target_arch = "wasm32")]
    {
        return PathBuf::from("pudduscroll");
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
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
}

pub fn load_blob(path: &Path) -> Result<Blob, String> {
    let bytes = crate::disk::read(path)
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
        source: None,
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

fn template_cover_file(template_id: u64, ext: &str) -> String {
    format!("t{template_id}.{ext}")
}

fn cover_file(template: &Template) -> String {
    template_cover_file(template.id, &template.cover.ext)
}

fn carousel_cover_file(carousel_id: u64, ext: &str) -> String {
    format!("c{carousel_id}-cover.{ext}")
}

fn photo_file(carousel_id: u64, frame_id: FrameId, ext: &str) -> String {
    format!("c{carousel_id}-f{}.{ext}", frame_id.0)
}

fn referenced_image_names(library: &Library) -> HashSet<String> {
    let mut names = HashSet::new();
    for template in &library.templates {
        names.insert(cover_file(template));
    }
    for carousel in &library.carousels {
        if let Some(cover) = &carousel.cover {
            names.insert(carousel_cover_file(carousel.id, &cover.ext));
        }
        for (frame_id, meta) in &carousel.disk {
            names.insert(photo_file(carousel.id, *frame_id, &meta.ext));
        }
    }
    names
}

fn image_files(library: &Library) -> Vec<(String, Arc<[u8]>)> {
    let mut files = Vec::new();
    for template in &library.templates {
        files.push((cover_file(template), Arc::clone(&template.cover.bytes)));
    }
    for carousel in &library.carousels {
        if let Some(cover) = &carousel.cover {
            files.push((
                carousel_cover_file(carousel.id, &cover.ext),
                Arc::clone(&cover.bytes),
            ));
        }
        for (frame_id, blob) in &carousel.photos {
            let ext = carousel
                .disk
                .get(frame_id)
                .map_or(blob.ext.as_str(), |meta| meta.ext.as_str());
            files.push((
                photo_file(carousel.id, *frame_id, ext),
                Arc::clone(&blob.bytes),
            ));
        }
    }
    files
}

fn write_named_images(
    root: &Path,
    files: &[(String, Arc<[u8]>)],
    keep: &HashSet<String>,
) -> Result<(), String> {
    let dir = root.join("images");
    crate::disk::create_dir_all(&dir).map_err(|error| error.to_string())?;
    for (name, bytes) in files {
        crate::disk::write(dir.join(name), bytes.as_ref()).map_err(|error| error.to_string())?;
    }
    if let Ok(entries) = crate::disk::list(&dir) {
        for path in entries {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !keep.contains(&name) {
                let _ = crate::disk::remove_file(&path);
            }
        }
    }
    Ok(())
}

fn copy_carousel_files(
    from_id: u64,
    to_id: u64,
    disk: &HashMap<FrameId, PhotoDisk>,
    cover: Option<&Blob>,
) {
    let dir = library_directory().join("images");
    for (frame_id, meta) in disk {
        let from = dir.join(photo_file(from_id, *frame_id, &meta.ext));
        let to = dir.join(photo_file(to_id, *frame_id, &meta.ext));
        if crate::disk::exists(&from)
            && let Ok(bytes) = crate::disk::read(&from)
        {
            let _ = crate::disk::write(&to, bytes);
        }
    }
    if let Some(cover) = cover {
        let from = dir.join(carousel_cover_file(from_id, &cover.ext));
        let to = dir.join(carousel_cover_file(to_id, &cover.ext));
        if crate::disk::exists(&from)
            && let Ok(bytes) = crate::disk::read(&from)
        {
            let _ = crate::disk::write(&to, bytes);
        }
    }
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
        push_carousel(&mut out, carousel);
    }
    out
}

fn package_text(carousel: &Carousel) -> String {
    let mut out = String::new();
    out.push_str("pudduscroll 1\n");
    let _ = writeln!(out, "next {}", carousel.id.saturating_add(1).max(1));
    push_carousel(&mut out, carousel);
    out
}

fn push_carousel(out: &mut String, carousel: &Carousel) {
    let _ = writeln!(out, "carousel {}", carousel.id);
    out.push_str("name ");
    push_quoted(out, &carousel.name);
    out.push('\n');
    let _ = writeln!(
        out,
        "ratio {} {}",
        carousel.layout.ratio.width, carousel.layout.ratio.height
    );
    let _ = writeln!(out, "canvases {}", carousel.layout.canvas_count);
    let _ = writeln!(out, "squares {}", clamp_grid_cells(carousel.grid_cells));
    if let Some(cover) = &carousel.cover {
        let _ = writeln!(out, "cover {}", cover.ext);
    }
    push_frames(out, &carousel.layout, Some(carousel));
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
        let Some(meta) = carousel.disk.get(&frame.id) else {
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
            meta.ext,
            photo.rotation,
            photo.zoom
        );
        if let Some(source) = meta.source.as_deref().filter(|source| !source.is_empty()) {
            out.push_str("source ");
            push_quoted(out, source);
            out.push('\n');
        }
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
    let mut covers = Vec::new();
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
                finish(
                    &mut library,
                    &mut template,
                    &mut carousel,
                    root,
                    &mut covers,
                )?;
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
                finish(
                    &mut library,
                    &mut template,
                    &mut carousel,
                    root,
                    &mut covers,
                )?;
                kind = Some(ItemKind::Carousel);
                carousel = Some(CarouselBuild {
                    id: parse_u64(rest, place)?,
                    name: String::new(),
                    ratio: AspectRatio::FOUR_FIVE,
                    canvases: 1,
                    grid_cells: DEFAULT_GRID_CELLS,
                    cover_ext: String::new(),
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
            "cover" => match kind {
                Some(ItemKind::Template) => {
                    template_mut(&mut template, place)?.cover_ext = rest.to_string();
                }
                Some(ItemKind::Carousel) => {
                    carousel_mut(&mut carousel, place)?.cover_ext = rest.to_string();
                }
                None => return Err(format!("line {place}: cover without an item")),
            },
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
            "source" => {
                let source = parse_quoted(rest, place)?;
                let build = carousel_mut(&mut carousel, place)?;
                let Some(frame) = build.frames.last_mut() else {
                    return Err(format!("line {place}: source without a frame"));
                };
                let Some(photo) = frame.photo.as_mut() else {
                    return Err(format!("line {place}: source without a photo"));
                };
                photo.source = Some(source);
            }
            _ => return Err(format!("line {place}: unknown {word}")),
        }
    }
    if !saw_header {
        return Err("library file is missing its header".to_string());
    }
    finish(
        &mut library,
        &mut template,
        &mut carousel,
        root,
        &mut covers,
    )?;
    fill_covers(&mut library, covers)?;
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

enum CoverJob {
    Template {
        index: usize,
        path: PathBuf,
        name: String,
    },
    Carousel {
        index: usize,
        path: PathBuf,
    },
}

fn finish(
    library: &mut Library,
    template: &mut Option<TemplateBuild>,
    carousel: &mut Option<CarouselBuild>,
    root: &Path,
    covers: &mut Vec<CoverJob>,
) -> Result<(), String> {
    if let Some(build) = template.take() {
        let (built, path) = build.finish(root)?;
        let name = built.name.clone();
        let index = library.templates.len();
        library.templates.push(built);
        covers.push(CoverJob::Template { index, path, name });
    }
    if let Some(build) = carousel.take() {
        let (built, path) = build.finish(root)?;
        let index = library.carousels.len();
        library.carousels.push(built);
        if let Some(path) = path {
            covers.push(CoverJob::Carousel { index, path });
        }
    }
    Ok(())
}

fn fill_covers(library: &mut Library, jobs: Vec<CoverJob>) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let handles: Vec<_> = jobs
            .into_iter()
            .map(|job| {
                std::thread::spawn(move || match job {
                    CoverJob::Template { index, path, name } => {
                        let mut blob = load_blob(&path)?;
                        blob.name = name;
                        Ok::<_, String>((CoverKind::Template(index), blob))
                    }
                    CoverJob::Carousel { index, path } => {
                        let blob = load_blob(&path)?;
                        Ok((CoverKind::Carousel(index), blob))
                    }
                })
            })
            .collect();
        for handle in handles {
            let (kind, blob) = handle
                .join()
                .map_err(|_join| "Cover decode stopped".to_string())??;
            match kind {
                CoverKind::Template(index) => {
                    library.templates[index].cover = blob;
                }
                CoverKind::Carousel(index) => {
                    library.carousels[index].cover = Some(blob);
                }
            }
        }
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        for job in jobs {
            match job {
                CoverJob::Template { index, path, name } => {
                    let mut blob = load_blob(&path)?;
                    blob.name = name;
                    library.templates[index].cover = blob;
                }
                CoverJob::Carousel { index, path } => {
                    library.carousels[index].cover = Some(load_blob(&path)?);
                }
            }
        }
        Ok(())
    }
}

enum CoverKind {
    Template(usize),
    Carousel(usize),
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
    image_width: u32,
    image_height: u32,
    pan_x: f32,
    pan_y: f32,
    crop: Crop,
    preset: CropPreset,
    ext: String,
    rotation: f32,
    zoom: f32,
    source: Option<String>,
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
    cover_ext: String,
    frames: Vec<FrameBuild>,
}

impl TemplateBuild {
    fn finish(self, root: &Path) -> Result<(Template, PathBuf), String> {
        if self.name.trim().is_empty() {
            return Err(format!("template {} has no name", self.id));
        }
        if self.cover_ext.is_empty() {
            return Err(format!("template {} has no cover", self.id));
        }
        let path = root
            .join("images")
            .join(template_cover_file(self.id, &self.cover_ext));
        let mut layout = Layout::new(self.ratio);
        layout.set_canvas_count(self.canvases);
        push_meta_frames(&mut layout, &self.frames, None);
        Ok((
            Template {
                id: self.id,
                name: self.name,
                cover: placeholder_blob(&self.cover_ext),
                layout,
            },
            path,
        ))
    }
}

impl CarouselBuild {
    fn finish(self, root: &Path) -> Result<(Carousel, Option<PathBuf>), String> {
        if self.name.trim().is_empty() {
            return Err(format!("carousel {} has no name", self.id));
        }
        let mut disk = HashMap::new();
        let mut layout = Layout::new(self.ratio);
        layout.set_canvas_count(self.canvases);
        push_meta_frames(&mut layout, &self.frames, Some(&mut disk));
        let cover_path = (!self.cover_ext.is_empty()).then(|| {
            root.join("images")
                .join(carousel_cover_file(self.id, &self.cover_ext))
        });
        Ok((
            Carousel {
                id: self.id,
                name: self.name,
                layout,
                photos: HashMap::new(),
                disk,
                cover: None,
                grid_cells: clamp_grid_cells(self.grid_cells),
            },
            cover_path,
        ))
    }
}

fn placeholder_blob(ext: &str) -> Blob {
    Blob {
        name: String::new(),
        ext: ext.to_string(),
        width: 1,
        height: 1,
        bytes: Arc::from([]),
        proxy: Arc::new(RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]))),
        profile: None,
        screen_cache: Arc::new(Mutex::new(None)),
        source: None,
    }
}

fn push_meta_frames(
    layout: &mut Layout,
    frames: &[FrameBuild],
    mut disk: Option<&mut HashMap<FrameId, PhotoDisk>>,
) {
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
        if let Some(photo) = &source.photo {
            frame.photo = Some(FramePhoto {
                name: photo.name.clone(),
                image_width: photo.image_width.max(1),
                image_height: photo.image_height.max(1),
                pan_x: photo.pan_x.clamp(0.0, 1.0),
                pan_y: photo.pan_y.clamp(0.0, 1.0),
                zoom: clamp_photo_zoom(photo.zoom),
                crop: photo.crop.clamp(),
                preset: photo.preset,
                rotation: normalize_rotation(photo.rotation),
            });
            if let Some(disk) = disk.as_mut() {
                disk.insert(
                    source.id,
                    PhotoDisk {
                        ext: photo.ext.clone(),
                        source: photo.source.clone(),
                    },
                );
            }
        }
        layout.restore_frame(frame);
    }
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
    let image_width = token()?
        .parse::<u32>()
        .map_err(|error| format!("line {line}: photo size: {error}"))?;
    let image_height = token()?
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
        image_width,
        image_height,
        pan_x,
        pan_y,
        crop,
        preset,
        ext,
        rotation,
        zoom,
        source: None,
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

pub(crate) fn direct_image_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err("Enter an http or https image link".to_string());
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err("Enter an http or https image link".to_string());
    }
    if rest.is_empty()
        || rest
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '\\')
    {
        return Err("Enter an http or https image link".to_string());
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host.is_empty() || host.starts_with(':') {
        return Err("Enter an http or https image link".to_string());
    }
    Ok(())
}

pub(crate) fn linked_image(url: &str, bytes: Vec<u8>) -> Result<Blob, String> {
    direct_image_url(url)?;
    let mut blob = blob_from_bytes(&link_name(url), bytes)?;
    blob.source = Some(url.trim().to_string());
    Ok(blob)
}

pub(crate) fn carousel_package(carousel: &Carousel) -> Result<Vec<u8>, String> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("library.txt", options)
        .map_err(|error| format!("Could not share the carousel: {error}"))?;
    writer
        .write_all(package_text(carousel).as_bytes())
        .map_err(|error| error.to_string())?;
    let images = library_directory().join("images");
    for (frame_id, meta) in &carousel.disk {
        let name = photo_file(carousel.id, *frame_id, &meta.ext);
        let bytes = if let Some(blob) = carousel.photos.get(frame_id) {
            Arc::clone(&blob.bytes)
        } else {
            let path = images.join(&name);
            Arc::from(
                crate::disk::read(&path)
                    .map_err(|error| format!("Could not share the carousel: {error}"))?,
            )
        };
        writer
            .start_file(format!("images/{name}"), options)
            .map_err(|error| format!("Could not share the carousel: {error}"))?;
        writer
            .write_all(bytes.as_ref())
            .map_err(|error| error.to_string())?;
    }
    if let Some(cover) = &carousel.cover {
        let name = carousel_cover_file(carousel.id, &cover.ext);
        writer
            .start_file(format!("images/{name}"), options)
            .map_err(|error| format!("Could not share the carousel: {error}"))?;
        writer
            .write_all(cover.bytes.as_ref())
            .map_err(|error| error.to_string())?;
    }
    writer
        .finish()
        .map_err(|error| format!("Could not share the carousel: {error}"))
        .map(Cursor::into_inner)
}

pub(crate) fn read_carousel_package(bytes: &[u8]) -> Result<Carousel, String> {
    let package = read_package(bytes)?;
    let root = std::env::temp_dir().join(format!(
        "pudduscroll-package-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    let outcome = (|| {
        crate::disk::create_dir_all(root.join("images")).map_err(|error| error.to_string())?;
        crate::disk::write(root.join("library.txt"), &package.text)
            .map_err(|error| error.to_string())?;
        for (name, bytes) in &package.images {
            crate::disk::write(root.join("images").join(name), bytes)
                .map_err(|error| error.to_string())?;
        }
        let library = parse_library(&package.text, &root)?;
        if !library.templates.is_empty() || library.carousels.len() != 1 {
            return Err("A share file holds one carousel".to_string());
        }
        let mut carousel = library
            .carousels
            .into_iter()
            .next()
            .ok_or_else(|| "A share file holds one carousel".to_string())?;
        load_carousel_photos(&mut carousel, &root)?;
        let expected = photo_names(&carousel);
        let stored = package
            .images
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| !name.contains("-cover."))
            .collect::<HashSet<_>>();
        if expected != stored {
            return Err("Share file photos do not match the carousel".to_string());
        }
        Ok(carousel)
    })();
    let _ = crate::disk::remove_dir_all(&root);
    outcome
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn download_image(url: &str) -> Result<Vec<u8>, String> {
    direct_image_url(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(LINK_TIMEOUT)
        .timeout(LINK_TIMEOUT)
        .build();
    let response = agent
        .get(url.trim())
        .call()
        .map_err(|error| format!("Could not download the photo: {error}"))?;
    if direct_image_url(response.get_url()).is_err() {
        return Err("Could not download the photo".to_string());
    }
    if let Some(length) = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        && length > MAX_LINK_BYTES
    {
        return Err("Photo is larger than 32 MB".to_string());
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_LINK_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not download the photo: {error}"))?;
    if bytes.len() as u64 > MAX_LINK_BYTES {
        return Err("Photo is larger than 32 MB".to_string());
    }
    Ok(bytes)
}

pub(crate) fn blob_from_bytes(name: &str, bytes: Vec<u8>) -> Result<Blob, String> {
    let format = image::guess_format(&bytes)
        .map_err(|_error| "Photo must be a png, jpeg, or webp".to_string())?;
    let ext = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        _ => return Err("Photo must be a png, jpeg, or webp".to_string()),
    };
    let raster = raster_from_bytes(&bytes)
        .map_err(|error| format!("Could not decode the photo: {error}"))?;
    let profile = crate::color::embedded_profile(&bytes).map(Arc::from);
    Ok(Blob {
        name: name.to_string(),
        ext: ext.to_string(),
        width: raster.width.max(1),
        height: raster.height.max(1),
        bytes: Arc::from(bytes),
        proxy: Arc::new(raster.proxy),
        profile,
        screen_cache: Arc::new(Mutex::new(None)),
        source: None,
    })
}

pub(crate) fn pasted_image(width: usize, height: usize, rgba: &[u8]) -> Result<Blob, String> {
    let width = u32::try_from(width).map_err(|_error| "Clipboard has no image".to_string())?;
    let height = u32::try_from(height).map_err(|_error| "Clipboard has no image".to_string())?;
    if width == 0 || height == 0 {
        return Err("Clipboard has no image".to_string());
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Photo is too large".to_string())?;
    if expected > MAX_PASTE_BYTES {
        return Err("Photo is too large".to_string());
    }
    if rgba.len() != expected {
        return Err("Clipboard has no image".to_string());
    }
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "Clipboard has no image".to_string())?;
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|error| format!("Could not read the clipboard: {error}"))?;
    blob_from_bytes("Pasted.png", bytes)
}

pub(crate) fn clipboard_image() -> Result<Blob, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|error| format!("Could not read the clipboard: {error}"))?;
        let image = clipboard
            .get_image()
            .map_err(|_error| "Clipboard has no image".to_string())?;
        pasted_image(image.width, image.height, image.bytes.as_ref())
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("Copy an image first".to_string())
    }
}

pub(crate) fn clipboard_image_stamp() -> Option<u64> {
    #[cfg(windows)]
    {
        windows_clipboard_stamp()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub(crate) fn clipboard_has_image() -> bool {
    #[cfg(windows)]
    {
        clipboard_image_stamp().is_some()
    }
    #[cfg(all(not(windows), not(target_arch = "wasm32")))]
    {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut clipboard| clipboard.get_image().ok())
            .is_some()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

#[cfg(windows)]
fn windows_clipboard_stamp() -> Option<u64> {
    let seq = clipboard_win::raw::seq_num()?;
    let png = clipboard_win::raw::register_format("PNG")
        .is_some_and(|format| clipboard_win::raw::is_format_avail(format.get()));
    let bitmap = clipboard_win::raw::is_format_avail(clipboard_win::formats::CF_DIB)
        || clipboard_win::raw::is_format_avail(clipboard_win::formats::CF_DIBV5)
        || clipboard_win::raw::is_format_avail(clipboard_win::formats::CF_BITMAP);
    (png || bitmap).then_some(u64::from(seq.get()))
}

fn link_name(url: &str) -> String {
    let path = url.trim().split(['?', '#']).next().unwrap_or("");
    let segment = path.rsplit('/').next().unwrap_or("").trim();
    let name: String = segment
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\\' && *ch != '"')
        .take(80)
        .collect();
    if name.is_empty() {
        "photo".to_string()
    } else {
        name
    }
}

struct PackageFiles {
    text: String,
    images: Vec<(String, Vec<u8>)>,
}

fn read_package(bytes: &[u8]) -> Result<PackageFiles, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("Could not read the share file: {error}"))?;
    let mut text = None;
    let mut images = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("Could not read the share file: {error}"))?;
        let raw = file.name().replace('\\', "/");
        if raw.split('/').any(|part| part == "..") || raw.starts_with('/') || raw.contains(':') {
            return Err("Share file has a path outside the carousel".to_string());
        }
        if file.is_dir() {
            if raw.trim_end_matches('/') == "images" {
                continue;
            }
            return Err("Share file has an unexpected file".to_string());
        }
        match package_entry(&raw)? {
            PackageKind::Text => {
                if text.is_some() {
                    return Err("A share file holds one carousel".to_string());
                }
                let bytes = read_capped(&mut file, MAX_PACKAGE_TEXT)?;
                text = Some(
                    String::from_utf8(bytes)
                        .map_err(|_error| "Share file text is not readable".to_string())?,
                );
            }
            PackageKind::Image(name) => {
                if images.iter().any(|(have, _)| have == &name) {
                    return Err("Share file has an unexpected file".to_string());
                }
                images.push((name, read_capped(&mut file, MAX_LINK_BYTES)?));
            }
        }
    }
    let text = text.ok_or_else(|| "A share file holds one carousel".to_string())?;
    package_shape(&text)?;
    Ok(PackageFiles { text, images })
}

enum PackageKind {
    Text,
    Image(String),
}

fn package_entry(name: &str) -> Result<PackageKind, String> {
    let parts: Vec<&str> = name.split('/').filter(|part| !part.is_empty()).collect();
    match parts.as_slice() {
        ["library.txt"] => Ok(PackageKind::Text),
        ["images", file] if image_file_name(file) => Ok(PackageKind::Image((*file).to_string())),
        _ => Err("Share file has an unexpected file".to_string()),
    }
}

fn image_file_name(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    if !matches!(ext, "png" | "jpg" | "webp") {
        return false;
    }
    let Some((carousel, frame)) = stem.split_once("-f") else {
        return false;
    };
    let Some(carousel) = carousel.strip_prefix('c') else {
        return false;
    };
    !carousel.is_empty()
        && carousel.chars().all(|ch| ch.is_ascii_digit())
        && !frame.is_empty()
        && frame.chars().all(|ch| ch.is_ascii_digit())
}

fn package_shape(text: &str) -> Result<(), String> {
    let mut header = false;
    let mut templates = 0;
    let mut carousels = 0;
    for line in text.lines() {
        let (word, rest) = split_word(line.trim());
        match word {
            "pudduscroll" if rest == "1" => header = true,
            "template" => templates += 1,
            "carousel" => carousels += 1,
            _ => {}
        }
    }
    if header && templates == 0 && carousels == 1 {
        Ok(())
    } else {
        Err("A share file holds one carousel".to_string())
    }
}

fn photo_names(carousel: &Carousel) -> HashSet<String> {
    carousel
        .disk
        .iter()
        .map(|(frame_id, meta)| photo_file(carousel.id, *frame_id, &meta.ext))
        .collect()
}

pub(crate) fn load_carousel_photos(carousel: &mut Carousel, root: &Path) -> Result<(), String> {
    let pending: Vec<(FrameId, PhotoDisk, String)> = carousel
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
    for (frame_id, meta, name) in pending {
        let path = root
            .join("images")
            .join(photo_file(carousel.id, frame_id, &meta.ext));
        let mut blob = load_blob(&path)?;
        blob.name = name;
        blob.source.clone_from(&meta.source);
        carousel.photos.insert(frame_id, blob);
    }
    Ok(())
}

pub(crate) fn load_frame_photo(
    carousel_id: u64,
    frame_id: FrameId,
    meta: &PhotoDisk,
    name: &str,
) -> Result<Blob, String> {
    let path =
        library_directory()
            .join("images")
            .join(photo_file(carousel_id, frame_id, &meta.ext));
    let mut blob = load_blob(&path)?;
    blob.name = name.to_string();
    blob.source.clone_from(&meta.source);
    Ok(blob)
}

pub(crate) fn blob_from_cover_image(image: &RgbaImage) -> Result<Blob, String> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|error| format!("Could not save the cover: {error}"))?;
    blob_from_bytes("cover.png", bytes)
}

fn read_capped(reader: &mut impl Read, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > max {
        Err("Share file is too large".to_string())
    } else {
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_roundtrip_keeps_template_and_carousel_apart() {
        let root =
            std::env::temp_dir().join(format!("pudduscroll-library-test-{}", std::process::id()));
        let _ = crate::disk::remove_dir_all(&root);
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
            let frame = template.layout.add_frame(0);
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
        let mut carousel = loaded.carousels.into_iter().next().unwrap();
        assert_eq!(carousel.name, "Sea \"m\"");
        assert!(carousel.photos.is_empty());
        assert_eq!(carousel.disk.len(), 1);
        let frame_id = carousel.layout.frames[0].id;
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
        load_carousel_photos(&mut carousel, &root).unwrap();
        assert!(carousel.photos.contains_key(&frame_id));
        assert!(carousel.photos[&frame_id].source.is_none());
        let _ = crate::disk::remove_dir_all(&root);
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
            .add_frame(0);
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
    fn carousel_cover_roundtrip_keeps_photos_lazy() {
        let root =
            std::env::temp_dir().join(format!("pudduscroll-cover-test-{}", std::process::id()));
        let _ = crate::disk::remove_dir_all(&root);
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
            .add_frame(0);
        library.set_photo(carousel_id, frame, sample_blob("night.jpg"));
        let cover = sample_blob("cover.png");
        let cover_bytes = Arc::clone(&cover.bytes);
        library.set_cover(carousel_id, cover);
        library.store_to(&root).unwrap();
        library.clear_photos(carousel_id);
        library.store_to(&root).unwrap();
        let loaded = Library::load_from(&root).unwrap();
        let carousel = &loaded.carousels[0];
        assert!(carousel.photos.is_empty());
        assert_eq!(carousel.disk.len(), 1);
        assert!(carousel.cover.is_some());
        assert_eq!(carousel.cover.as_ref().unwrap().bytes, cover_bytes);
        let mut carousel = loaded.carousels.into_iter().next().unwrap();
        load_carousel_photos(&mut carousel, &root).unwrap();
        assert!(carousel.photos.contains_key(&frame));
        let _ = crate::disk::remove_dir_all(&root);
    }

    #[test]
    fn carousel_grid_squares_stay_on_that_carousel() {
        let root =
            std::env::temp_dir().join(format!("pudduscroll-grid-test-{}", std::process::id()));
        let _ = crate::disk::remove_dir_all(&root);
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
        let _ = crate::disk::remove_dir_all(&root);
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
            source: None,
        }
    }

    #[test]
    fn share_zip_roundtrip_keeps_frames_photos_and_source() {
        let mut library = Library::empty();
        let template_id = library.create_template(
            "Sea".to_string(),
            AspectRatio::FOUR_FIVE,
            sample_blob("dune.png"),
        );
        let carousel_id = library.instantiate(template_id).unwrap();
        let layout = &mut library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .layout;
        layout.set_ratio(AspectRatio::SQUARE);
        layout.add_canvas();
        let linked_frame = layout.add_frame(0);
        layout.set_frame_rect(linked_frame, 0.15, 0.2, 1.4, 0.7);
        let empty_frame = layout.add_frame(0);
        layout.set_frame_rect(empty_frame, 1.1, 0.1, 0.6, 0.5);
        let disk_frame = layout.add_frame(0);
        layout.set_frame_rect(disk_frame, 0.4, 0.35, 0.8, 0.45);
        library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap()
            .grid_cells = 9;
        let mut linked = sample_blob("night.png");
        linked.source = Some("https://cdn.example.com/night.png?v=1".to_string());
        library.set_photo(carousel_id, linked_frame, linked);
        library.set_photo(carousel_id, disk_frame, sample_blob("day.png"));
        let carousel = library
            .carousels
            .iter_mut()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap();
        carousel.name = "Harbor \"east\"".to_string();
        carousel.layout.set_border(linked_frame, 0.2);
        carousel
            .layout
            .set_border_color(linked_frame, [9, 8, 7, 255]);
        carousel.layout.set_pan(linked_frame, 0.25, 0.8);
        carousel.layout.set_rotation(disk_frame, 0.6);
        carousel
            .layout
            .frame_mut(disk_frame)
            .unwrap()
            .photo
            .as_mut()
            .unwrap()
            .zoom = 1.5;
        let original_bytes = Arc::clone(&carousel.photos[&linked_frame].bytes);
        let disk_bytes = Arc::clone(&carousel.photos[&disk_frame].bytes);
        let packaged = carousel_package(carousel).unwrap();

        let mut home = Library::empty();
        let home_template = home.create_template(
            "Kept".to_string(),
            AspectRatio::FOUR_FIVE,
            sample_blob("dune.png"),
        );
        let kept = home.instantiate(home_template).unwrap();
        assert_eq!(kept, carousel_id);
        let imported = read_carousel_package(&packaged).unwrap();
        let new_id = home.import_carousel(imported);
        assert_ne!(new_id, kept);
        assert_eq!(home.carousels.len(), 2);
        let carousel = home
            .carousels
            .iter()
            .find(|carousel| carousel.id == new_id)
            .unwrap();
        assert_eq!(carousel.name, "Harbor \"east\"");
        assert_eq!(carousel.layout.ratio, AspectRatio::SQUARE);
        assert_eq!(carousel.layout.canvas_count, 2);
        assert_eq!(carousel.grid_cells, 9);
        assert_eq!(carousel.layout.frames.len(), 3);
        assert!(carousel.layout.frame(empty_frame).unwrap().photo.is_none());
        assert!(!carousel.photos.contains_key(&empty_frame));
        let linked_photo = carousel
            .layout
            .frame(linked_frame)
            .unwrap()
            .photo
            .as_ref()
            .unwrap();
        assert_eq!(linked_photo.name, "night.png");
        assert!((linked_photo.pan_x - 0.25).abs() < 1e-4);
        assert!((linked_photo.pan_y - 0.8).abs() < 1e-4);
        let linked_frame_rect = carousel.layout.frame(linked_frame).unwrap();
        assert!((linked_frame_rect.x - 0.15).abs() < 1e-4);
        assert!((linked_frame_rect.w - 1.4).abs() < 1e-4);
        assert!((linked_frame_rect.border - 0.2).abs() < 1e-4);
        assert_eq!(linked_frame_rect.border_color, [9, 8, 7, 255]);
        assert_eq!(
            carousel.photos[&linked_frame].source.as_deref(),
            Some("https://cdn.example.com/night.png?v=1")
        );
        assert_eq!(carousel.photos[&linked_frame].bytes, original_bytes);
        let disk_photo = carousel
            .layout
            .frame(disk_frame)
            .unwrap()
            .photo
            .as_ref()
            .unwrap();
        assert!((disk_photo.rotation - 0.6).abs() < 1e-4);
        assert!((disk_photo.zoom - 1.5).abs() < 1e-4);
        assert!(carousel.photos[&disk_frame].source.is_none());
        assert_eq!(carousel.photos[&disk_frame].bytes, disk_bytes);
        assert_eq!(
            home.carousels
                .iter()
                .find(|carousel| carousel.id == kept)
                .unwrap()
                .photos
                .len(),
            0
        );
    }

    #[test]
    fn share_zip_rejects_a_library_a_missing_photo_and_a_path_escape() {
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
            .add_frame(0);
        library.set_photo(carousel_id, frame, sample_blob("night.png"));
        let carousel = library
            .carousels
            .iter()
            .find(|carousel| carousel.id == carousel_id)
            .unwrap();
        let packed = carousel_package(carousel).unwrap();
        let text = package_text(carousel);
        assert!(read_carousel_package(&zip_bytes(&[("library.txt", text.as_bytes())])).is_err());
        assert!(
            read_carousel_package(&zip_bytes(&[(
                "library.txt",
                b"pudduscroll 1\nnext 2\ntemplate 1\ncarousel 2\n"
            )]))
            .is_err()
        );
        assert!(
            read_carousel_package(&zip_bytes(&[(
                "library.txt",
                b"pudduscroll 1\nnext 3\ncarousel 1\ncarousel 2\n"
            )]))
            .is_err()
        );
        assert!(
            read_carousel_package(&zip_bytes(&[
                ("library.txt", text.as_bytes()),
                ("../secret.txt", b"no"),
            ]))
            .is_err()
        );
        assert!(
            read_carousel_package(&zip_bytes(&[
                ("library.txt", text.as_bytes()),
                ("notes.txt", b"no"),
            ]))
            .is_err()
        );
        let mut extra = Vec::new();
        let mut archive = zip::ZipArchive::new(Cursor::new(packed)).unwrap();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().to_string();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            extra.push((name, bytes));
        }
        extra.push((
            "images/c9-f9.png".to_string(),
            sample_blob("day.png").bytes.to_vec(),
        ));
        let extra = zip_owned(&extra);
        assert!(read_carousel_package(&extra).is_err());
    }

    #[test]
    fn linked_image_rejects_a_page_and_a_bad_url() {
        let photo = sample_blob("photo.png");
        let linked = linked_image(
            "https://example.com/photo.png",
            photo.bytes.as_ref().to_vec(),
        )
        .unwrap();
        assert_eq!(
            linked.source.as_deref(),
            Some("https://example.com/photo.png")
        );
        assert_eq!(linked.ext, "png");
        assert_eq!(linked.bytes, photo.bytes);
        assert!(linked_image("ftp://example.com/a.png", photo.bytes.as_ref().to_vec()).is_err());
        assert!(direct_image_url("javascript:alert(1)").is_err());
        assert!(direct_image_url("https:///missing-host/a.png").is_err());
        assert!(linked_image("https://example.com/a.png", b"<html></html>".to_vec()).is_err());
    }

    #[test]
    fn create_carousel_starts_blank_at_four_five() {
        let mut library = Library::empty();
        let id = library.create_carousel();
        let carousel = library
            .carousels
            .iter()
            .find(|carousel| carousel.id == id)
            .unwrap();
        assert_eq!(carousel.name, "Carousel");
        assert!(carousel.photos.is_empty());
        assert_eq!(carousel.layout.ratio, AspectRatio::FOUR_FIVE);
        assert_eq!(carousel.layout.canvas_count, 1);
        assert_eq!(carousel.layout.frames.len(), 1);
        assert!(carousel.layout.frames[0].photo.is_none());
        let second = library.create_carousel();
        let name = library
            .carousels
            .iter()
            .find(|carousel| carousel.id == second)
            .unwrap()
            .name
            .clone();
        assert_eq!(name, "Carousel 2");
    }

    #[test]
    fn pasted_image_keeps_pixels_without_a_source() {
        let rgba = [10, 20, 30, 255, 40, 50, 60, 255];
        let blob = pasted_image(2, 1, &rgba).unwrap();
        assert!(blob.source.is_none());
        assert_eq!(blob.name, "Pasted.png");
        assert_eq!(blob.ext, "png");
        assert_eq!(blob.width, 2);
        assert_eq!(blob.height, 1);
        let decoded = image::load_from_memory(&blob.bytes).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), &rgba);
        assert!(pasted_image(2, 1, &[0, 0, 0, 255]).is_err());
        assert!(pasted_image(0, 1, &[]).is_err());
    }

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let owned: Vec<(String, Vec<u8>)> = files
            .iter()
            .map(|(name, bytes)| ((*name).to_string(), bytes.to_vec()))
            .collect();
        zip_owned(&owned)
    }

    fn zip_owned(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in files {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
}
