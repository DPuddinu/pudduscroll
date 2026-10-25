use std::collections::HashMap;
use std::hash::BuildHasher;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbImage, RgbaImage};

use crate::{
    DEFAULT_EXPORT_WIDTH, Document, FrameId, FrameImages, ImageMap, Layout, PROXY_LONG_EDGE,
    render_canvas, render_layout, shrink_long_edge,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Jpeg,
    Png,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportSettings {
    pub width: u32,
    pub quality: u8,
    pub format: ExportFormat,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            width: DEFAULT_EXPORT_WIDTH,
            quality: 90,
            format: ExportFormat::Jpeg,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EncodedCanvas {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub proxy: RgbaImage,
}

#[derive(Debug)]
pub enum ExportError {
    Encode(String),
    Io(std::io::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Encode(message) => f.write_str(message),
            ExportError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<image::ImageError> for ExportError {
    fn from(value: image::ImageError) -> Self {
        ExportError::Encode(value.to_string())
    }
}

impl From<std::io::Error> for ExportError {
    fn from(value: std::io::Error) -> Self {
        ExportError::Io(value)
    }
}

#[must_use]
pub fn jpeg_quality_value(quality: u8) -> u8 {
    if quality >= 100 {
        return 100;
    }
    let t = quality as f32 / 99.0;
    (8.0 + t * 87.0).round().clamp(1.0, 100.0) as u8
}

#[must_use]
pub fn quality_description(quality: u8, format: ExportFormat) -> String {
    match format {
        ExportFormat::Png => "PNG (native quality)".to_string(),
        ExportFormat::Jpeg => format!("JPEG quality {}", jpeg_quality_value(quality)),
    }
}

#[must_use]
pub fn canvas_file_name(name: &str, index: usize, format: ExportFormat) -> String {
    let ext = match format {
        ExportFormat::Png => "png",
        ExportFormat::Jpeg => "jpg",
    };
    format!("{}-{:02}.{ext}", export_stem(name), index + 1)
}

fn breaks_file_name(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

fn export_stem(name: &str) -> String {
    let mut stem = String::new();
    let mut separator = false;
    for ch in name.chars() {
        if breaks_file_name(ch) {
            separator = !stem.is_empty();
            continue;
        }
        if separator {
            stem.push('_');
            separator = false;
        }
        stem.push(ch);
        if stem.len() >= 200 {
            break;
        }
    }
    while stem.ends_with('.') {
        stem.pop();
    }
    if stem.is_empty() {
        "pudduscroll".to_string()
    } else {
        stem
    }
}

#[must_use]
pub fn export_width(doc: &Document) -> u32 {
    let ratio = doc.ratio.value();
    let mut limit = f32::INFINITY;
    for photo in &doc.photos {
        let placed_h = photo.height(ratio);
        if photo.w <= 1e-6 || placed_h <= 1e-6 {
            continue;
        }
        let from_width = photo.image_width as f32 * photo.crop.w / photo.w;
        let from_height = ratio * (photo.image_height as f32 * photo.crop.h) / placed_h;
        limit = limit.min(from_width).min(from_height);
    }
    if !limit.is_finite() {
        return DEFAULT_EXPORT_WIDTH;
    }
    limit.floor().max(1.0) as u32
}

pub fn export_canvases(
    doc: &Document,
    images: &ImageMap,
    settings: ExportSettings,
) -> Result<Vec<EncodedCanvas>, ExportError> {
    let mut canvases = Vec::with_capacity(doc.canvas_count);
    for index in 0..doc.canvas_count {
        let rendered = render_canvas(doc, index, images, settings.width);
        canvases.push(EncodedCanvas {
            file_name: canvas_file_name("", index, settings.format),
            bytes: encode_rendered(&rendered, settings)?,
        });
    }
    Ok(canvases)
}

pub fn estimate_total_bytes(
    doc: &Document,
    images: &ImageMap,
    settings: ExportSettings,
) -> Result<u64, ExportError> {
    let canvases = export_canvases(doc, images, settings)?;
    Ok(canvases
        .iter()
        .map(|canvas| canvas.bytes.len() as u64)
        .sum())
}

pub fn image_from_bytes(bytes: &[u8]) -> Result<RgbaImage, ExportError> {
    let image = image::load_from_memory(bytes)?.into_rgba8();
    if image.width() == 0 || image.height() == 0 {
        return Err(ExportError::Encode("image has no pixels".to_string()));
    }
    Ok(image)
}

pub fn decode_frame_images<S: BuildHasher>(
    encoded: &HashMap<FrameId, Arc<[u8]>, S>,
    on_photo: &(impl Fn(usize, usize) + Sync),
) -> Result<FrameImages, ExportError> {
    let jobs: Vec<(FrameId, &[u8])> = encoded.iter().map(|(id, bytes)| (*id, &**bytes)).collect();
    let total = jobs.len();
    let next = &AtomicUsize::new(0);
    let finished = &AtomicUsize::new(0);
    let jobs = &jobs;
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..export_workers(total))
            .map(|_| {
                scope.spawn(move || -> Result<Vec<(FrameId, RgbaImage)>, ExportError> {
                    let mut part = Vec::new();
                    while let Some((id, bytes)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        part.push((*id, image_from_bytes(bytes)?));
                        note_slide(finished, total, on_photo);
                    }
                    Ok(part)
                })
            })
            .collect();
        let mut images = FrameImages::with_capacity(total);
        for worker in workers {
            let part = worker
                .join()
                .map_err(|_panic| ExportError::Encode("decode worker stopped".to_string()))??;
            for (id, image) in part {
                images.insert(id, Arc::new(image));
            }
        }
        Ok(images)
    })
}

pub fn raster_from_bytes(bytes: &[u8]) -> Result<Raster, ExportError> {
    let image = image_from_bytes(bytes)?;
    let width = image.width();
    let height = image.height();
    let proxy = shrink_long_edge(image, PROXY_LONG_EDGE);
    Ok(Raster {
        width,
        height,
        proxy,
    })
}

pub fn export_layout_canvases(
    layout: &Layout,
    images: &FrameImages,
    settings: ExportSettings,
) -> Result<Vec<EncodedCanvas>, ExportError> {
    export_layout_canvases_with(layout, images, settings, &|_, _| {})
}

pub fn export_layout_canvases_with(
    layout: &Layout,
    images: &FrameImages,
    settings: ExportSettings,
    on_slide: &(impl Fn(usize, usize) + Sync),
) -> Result<Vec<EncodedCanvas>, ExportError> {
    let count = layout.canvas_count;
    let finished = &AtomicUsize::new(0);
    let workers = export_workers(count);
    if workers == 1 {
        let mut canvases = Vec::with_capacity(count);
        for index in 0..count {
            canvases.push(encode_layout_canvas(layout, images, settings, index)?);
            note_slide(finished, count, on_slide);
        }
        return Ok(canvases);
    }
    let mut encoded = Vec::new();
    encoded.resize_with(count, || None);
    let chunk = count.div_ceil(workers);
    std::thread::scope(|scope| -> Result<(), ExportError> {
        let mut joins = Vec::new();
        for worker in 0..workers {
            let start = worker * chunk;
            if start >= count {
                break;
            }
            let end = (start + chunk).min(count);
            joins.push(scope.spawn(
                move || -> Result<Vec<(usize, EncodedCanvas)>, ExportError> {
                    let mut part = Vec::with_capacity(end - start);
                    for index in start..end {
                        part.push((
                            index,
                            encode_layout_canvas(layout, images, settings, index)?,
                        ));
                        note_slide(finished, count, on_slide);
                    }
                    Ok(part)
                },
            ));
        }
        for join in joins {
            let part = join
                .join()
                .map_err(|_panic| ExportError::Encode("export worker stopped".to_string()))??;
            for (index, canvas) in part {
                encoded[index] = Some(canvas);
            }
        }
        Ok(())
    })?;
    let mut canvases = Vec::with_capacity(count);
    for slot in encoded {
        let Some(canvas) = slot else {
            return Err(ExportError::Encode("missing canvas".to_string()));
        };
        canvases.push(canvas);
    }
    Ok(canvases)
}

fn note_slide(finished: &AtomicUsize, total: usize, on_slide: &(impl Fn(usize, usize) + Sync)) {
    let done = finished.fetch_add(1, Ordering::Relaxed) + 1;
    on_slide(done, total);
}

fn export_workers(canvas_count: usize) -> usize {
    if canvas_count <= 1 {
        return 1;
    }
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, canvas_count)
}

fn encode_layout_canvas(
    layout: &Layout,
    images: &FrameImages,
    settings: ExportSettings,
    index: usize,
) -> Result<EncodedCanvas, ExportError> {
    let rendered = render_layout(layout, index, images, settings.width);
    Ok(EncodedCanvas {
        file_name: canvas_file_name("", index, settings.format),
        bytes: encode_rendered(&rendered, settings)?,
    })
}

fn encode_rendered(img: &RgbaImage, settings: ExportSettings) -> Result<Vec<u8>, ExportError> {
    match settings.format {
        ExportFormat::Png => encode_png(img),
        ExportFormat::Jpeg => encode_jpeg(img, jpeg_quality_value(settings.quality)),
    }
}

pub fn estimate_layout_bytes(
    layout: &Layout,
    images: &FrameImages,
    settings: ExportSettings,
) -> Result<u64, ExportError> {
    estimate_layout_bytes_with(layout, images, settings, &|_, _| {})
}

pub fn estimate_layout_bytes_with(
    layout: &Layout,
    images: &FrameImages,
    settings: ExportSettings,
    on_slide: &(impl Fn(usize, usize) + Sync),
) -> Result<u64, ExportError> {
    let canvases = export_layout_canvases_with(layout, images, settings, on_slide)?;
    Ok(canvases
        .iter()
        .map(|canvas| canvas.bytes.len() as u64)
        .sum())
}

pub fn write_canvases(
    dir: &Path,
    name: &str,
    format: ExportFormat,
    canvases: &[EncodedCanvas],
) -> Result<(), ExportError> {
    for (index, canvas) in canvases.iter().enumerate() {
        std::fs::write(
            dir.join(canvas_file_name(name, index, format)),
            &canvas.bytes,
        )?;
    }
    Ok(())
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, ExportError> {
    if fully_opaque(img) {
        let rgb = strip_alpha(img);
        return write_png(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        );
    }
    write_png(
        img.as_raw(),
        img.width(),
        img.height(),
        ExtendedColorType::Rgba8,
    )
}

fn write_png(
    data: &[u8],
    width: u32,
    height: u32,
    color: ExtendedColorType,
) -> Result<Vec<u8>, ExportError> {
    let mut buf = Cursor::new(Vec::new());
    PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, FilterType::Sub)
        .write_image(data, width, height, color)?;
    Ok(buf.into_inner())
}

fn fully_opaque(img: &RgbaImage) -> bool {
    img.as_raw().chunks_exact(4).all(|pixel| pixel[3] == 255)
}

fn strip_alpha(img: &RgbaImage) -> RgbImage {
    let width = img.width();
    let height = img.height();
    let mut rgb = vec![0; width as usize * height as usize * 3];
    let mut dst = 0;
    for pixel in img.as_raw().chunks_exact(4) {
        rgb[dst] = pixel[0];
        rgb[dst + 1] = pixel[1];
        rgb[dst + 2] = pixel[2];
        dst += 3;
    }
    RgbImage::from_raw(width, height, rgb)
        .unwrap_or_else(|| RgbImage::from_pixel(width, height, image::Rgb([255, 255, 255])))
}

fn encode_jpeg(img: &RgbaImage, quality: u8) -> Result<Vec<u8>, ExportError> {
    let rgb = composite_on_white(img);
    let width = u16::try_from(rgb.width()).map_err(|_error| {
        ExportError::Encode(format!("width {} is above the JPEG limit", rgb.width()))
    })?;
    let height = u16::try_from(rgb.height()).map_err(|_error| {
        ExportError::Encode(format!("height {} is above the JPEG limit", rgb.height()))
    })?;
    let mut buf = Vec::new();
    jpeg_encoder::Encoder::new(&mut buf, quality.clamp(1, 100))
        .encode(rgb.as_raw(), width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|error| ExportError::Encode(error.to_string()))?;
    Ok(buf)
}

fn composite_on_white(img: &RgbaImage) -> RgbImage {
    let width = img.width();
    let height = img.height();
    let src = img.as_raw();
    let mut rgb = vec![0; width as usize * height as usize * 3];
    let mut src_index = 0;
    let mut dst_index = 0;
    while src_index + 3 < src.len() {
        let alpha = src[src_index + 3];
        if alpha == 255 {
            rgb[dst_index] = src[src_index];
            rgb[dst_index + 1] = src[src_index + 1];
            rgb[dst_index + 2] = src[src_index + 2];
        } else if alpha == 0 {
            rgb[dst_index] = 255;
            rgb[dst_index + 1] = 255;
            rgb[dst_index + 2] = 255;
        } else {
            let covered = f32::from(alpha) / 255.0;
            let open = 1.0 - covered;
            let channel = |value: u8| (f32::from(value) * covered + 255.0 * open).round() as u8;
            rgb[dst_index] = channel(src[src_index]);
            rgb[dst_index + 1] = channel(src[src_index + 1]);
            rgb[dst_index + 2] = channel(src[src_index + 2]);
        }
        src_index += 4;
        dst_index += 3;
    }
    RgbImage::from_raw(width, height, rgb)
        .unwrap_or_else(|| RgbImage::from_pixel(width, height, image::Rgb([255, 255, 255])))
}
