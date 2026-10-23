use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbImage, RgbaImage};

use crate::{
    DEFAULT_EXPORT_WIDTH, Document, FrameImages, ImageMap, Layout, render_canvas, render_layout,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportSettings {
    pub width: u32,
    pub quality: u8,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            width: DEFAULT_EXPORT_WIDTH,
            quality: 90,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EncodedCanvas {
    pub file_name: String,
    pub bytes: Vec<u8>,
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
pub fn is_native(quality: u8) -> bool {
    quality >= 100
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
pub fn quality_description(quality: u8) -> String {
    if is_native(quality) {
        "PNG (native quality)".to_string()
    } else {
        format!("JPEG quality {}", jpeg_quality_value(quality))
    }
}

#[must_use]
pub fn canvas_file_name(index: usize, quality: u8) -> String {
    let ext = if is_native(quality) { "png" } else { "jpg" };
    format!("pudduscroll-{:02}.{ext}", index + 1)
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
        let bytes = if is_native(settings.quality) {
            encode_png(&rendered)?
        } else {
            encode_jpeg(&rendered, jpeg_quality_value(settings.quality))?
        };
        canvases.push(EncodedCanvas {
            file_name: canvas_file_name(index, settings.quality),
            bytes,
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

pub fn export_layout_canvases(
    layout: &Layout,
    images: &FrameImages,
    settings: ExportSettings,
) -> Result<Vec<EncodedCanvas>, ExportError> {
    let mut canvases = Vec::with_capacity(layout.canvas_count);
    for index in 0..layout.canvas_count {
        let rendered = render_layout(layout, index, images, settings.width);
        let bytes = if is_native(settings.quality) {
            encode_png(&rendered)?
        } else {
            encode_jpeg(&rendered, jpeg_quality_value(settings.quality))?
        };
        canvases.push(EncodedCanvas {
            file_name: canvas_file_name(index, settings.quality),
            bytes,
        });
    }
    Ok(canvases)
}

pub fn estimate_layout_bytes(
    layout: &Layout,
    images: &FrameImages,
    settings: ExportSettings,
) -> Result<u64, ExportError> {
    let canvases = export_layout_canvases(layout, images, settings)?;
    Ok(canvases
        .iter()
        .map(|canvas| canvas.bytes.len() as u64)
        .sum())
}

pub fn write_canvases(dir: &Path, canvases: &[EncodedCanvas]) -> Result<(), ExportError> {
    for canvas in canvases {
        std::fs::write(dir.join(&canvas.file_name), &canvas.bytes)?;
    }
    Ok(())
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, ExportError> {
    let mut buf = Cursor::new(Vec::new());
    let encoder = PngEncoder::new(&mut buf);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        ExtendedColorType::Rgba8,
    )?;
    Ok(buf.into_inner())
}

fn encode_jpeg(img: &RgbaImage, quality: u8) -> Result<Vec<u8>, ExportError> {
    let rgb = composite_on_white(img);
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    encoder.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        ExtendedColorType::Rgb8,
    )?;
    Ok(buf.into_inner())
}

fn composite_on_white(img: &RgbaImage) -> RgbImage {
    RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let pixel = img.get_pixel(x, y);
        let alpha = pixel[3] as f32 / 255.0;
        let channel = |value: u8| (value as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        image::Rgb([channel(pixel[0]), channel(pixel[1]), channel(pixel[2])])
    })
}
