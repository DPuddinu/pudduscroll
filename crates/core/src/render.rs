use std::collections::HashMap;
use std::sync::Arc;

use image::imageops::{FilterType, crop_imm, overlay, resize, thumbnail};
use image::{Rgba, RgbaImage};

use crate::{
    Document, Frame, FrameId, Layout, Photo, PhotoId, border_thickness, cover_fit, photo_local,
};

pub type ImageMap = HashMap<PhotoId, Arc<RgbaImage>>;
pub type FrameImages = HashMap<FrameId, Arc<RgbaImage>>;

pub const PROXY_LONG_EDGE: u32 = 2048;

#[must_use]
pub fn shrink_long_edge(image: RgbaImage, max_edge: u32) -> RgbaImage {
    let width = image.width();
    let height = image.height();
    let long = width.max(height);
    let max_edge = max_edge.max(1);
    if long <= max_edge || width == 0 || height == 0 {
        return image;
    }
    let scale = max_edge as f32 / long as f32;
    let target_w = ((width as f32 * scale).round() as u32).max(1);
    let target_h = ((height as f32 * scale).round() as u32).max(1);
    thumbnail(&image, target_w, target_h)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClippedRegion {
    pub dest: [f32; 4],
    pub src: [f32; 4],
}

#[must_use]
pub fn clip_photo_to_canvas(photo: &Photo, canvas: usize, ratio: f32) -> Option<ClippedRegion> {
    let height = photo.height(ratio);
    if photo.w <= 1e-6 || height <= 1e-6 {
        return None;
    }
    let photo_right = photo.x + photo.w;
    let photo_bottom = photo.y + height;
    let canvas_x0 = canvas as f32;
    let canvas_x1 = canvas_x0 + 1.0;
    let ix0 = photo.x.max(canvas_x0);
    let iy0 = photo.y.max(0.0);
    let ix1 = photo_right.min(canvas_x1);
    let iy1 = photo_bottom.min(1.0);
    if ix1 - ix0 <= 1e-5 || iy1 - iy0 <= 1e-5 {
        return None;
    }
    let u0 = (ix0 - photo.x) / photo.w;
    let u1 = (ix1 - photo.x) / photo.w;
    let v0 = (iy0 - photo.y) / height;
    let v1 = (iy1 - photo.y) / height;
    let crop = photo.crop;
    let image_w = photo.image_width as f32;
    let image_h = photo.image_height as f32;
    Some(ClippedRegion {
        dest: [ix0 - canvas_x0, iy0, ix1 - ix0, iy1 - iy0],
        src: [
            (crop.x + u0 * crop.w) * image_w,
            (crop.y + v0 * crop.h) * image_h,
            (u1 - u0) * crop.w * image_w,
            (v1 - v0) * crop.h * image_h,
        ],
    })
}

#[must_use]
pub fn render_canvas(
    doc: &Document,
    canvas: usize,
    images: &ImageMap,
    dest_width: u32,
) -> RgbaImage {
    let (width, height) = doc.ratio.export_size(dest_width);
    let mut output = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
    if canvas >= doc.canvas_count {
        return output;
    }
    let ratio = doc.ratio.value();
    let mut photos: Vec<&Photo> = doc.photos.iter().collect();
    photos.sort_by_key(|photo| photo.z);
    for photo in photos {
        let Some(image) = images.get(&photo.id) else {
            continue;
        };
        let Some(region) = clip_photo_to_canvas(photo, canvas, ratio) else {
            continue;
        };
        blit(
            &mut output,
            image,
            &region,
            photo.image_width,
            photo.image_height,
        );
    }
    output
}

#[must_use]
pub fn clip_frame_to_canvas(frame: &Frame, canvas: usize, ratio: f32) -> Option<ClippedRegion> {
    let photo = frame.photo.as_ref()?;
    let fit = cover_fit(frame, ratio)?;
    if fit.w <= 1.0e-6 || fit.h <= 1.0e-6 || frame.w <= 1.0e-6 || frame.h <= 1.0e-6 {
        return None;
    }
    let canvas_x0 = canvas as f32;
    let canvas_x1 = canvas_x0 + 1.0;
    let ix0 = fit.x.max(frame.x).max(canvas_x0);
    let iy0 = fit.y.max(frame.y).max(0.0);
    let ix1 = (fit.x + fit.w).min(frame.x + frame.w).min(canvas_x1);
    let iy1 = (fit.y + fit.h).min(frame.y + frame.h).min(1.0);
    if ix1 - ix0 <= 1.0e-5 || iy1 - iy0 <= 1.0e-5 {
        return None;
    }
    let u0 = (ix0 - fit.x) / fit.w;
    let u1 = (ix1 - fit.x) / fit.w;
    let v0 = (iy0 - fit.y) / fit.h;
    let v1 = (iy1 - fit.y) / fit.h;
    let crop = photo.crop;
    let image_w = photo.image_width as f32;
    let image_h = photo.image_height as f32;
    Some(ClippedRegion {
        dest: [ix0 - canvas_x0, iy0, ix1 - ix0, iy1 - iy0],
        src: [
            (crop.x + u0 * crop.w) * image_w,
            (crop.y + v0 * crop.h) * image_h,
            (u1 - u0) * crop.w * image_w,
            (v1 - v0) * crop.h * image_h,
        ],
    })
}

#[must_use]
pub fn render_layout(
    layout: &Layout,
    canvas: usize,
    images: &FrameImages,
    dest_width: u32,
) -> RgbaImage {
    let (width, height) = layout.ratio.export_size(dest_width);
    let mut output = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
    if canvas >= layout.canvas_count {
        return output;
    }
    let ratio = layout.ratio.value();
    let mut frames: Vec<&Frame> = layout.frames.iter().collect();
    frames.sort_by_key(|frame| frame.z);
    for frame in frames {
        let Some(image) = images.get(&frame.id) else {
            continue;
        };
        let Some(photo) = frame.photo.as_ref() else {
            continue;
        };
        if photo.rotation.abs() > 1.0e-4 {
            paint_rotated(&mut output, image, frame, canvas, ratio);
            paint_frame_border(&mut output, frame, canvas);
            continue;
        }
        let Some(region) = clip_frame_to_canvas(frame, canvas, ratio) else {
            continue;
        };
        blit(
            &mut output,
            image,
            &region,
            photo.image_width,
            photo.image_height,
        );
        paint_frame_border(&mut output, frame, canvas);
    }
    output
}

fn paint_frame_border(dest: &mut RgbaImage, frame: &Frame, canvas: usize) {
    if frame.border <= 1.0e-4 || frame.photo.is_none() {
        return;
    }
    let dest_w = dest.width() as f32;
    let dest_h = dest.height() as f32;
    let left = (frame.x - canvas as f32) * dest_w;
    let top = frame.y * dest_h;
    let right = left + frame.w * dest_w;
    let bottom = top + frame.h * dest_h;
    let thickness = border_thickness(frame.border, dest_w, dest_h);
    if thickness <= 0.5 {
        return;
    }
    let color = frame.border_color;
    let x0 = left.floor().max(0.0) as u32;
    let y0 = top.floor().max(0.0) as u32;
    let x1 = right.ceil().min(dest_w) as u32;
    let y1 = bottom.ceil().min(dest_h) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            if px < left || py < top || px > right || py > bottom {
                continue;
            }
            let inset = (px - left).min(right - px).min(py - top).min(bottom - py);
            let coverage = (thickness + 0.5 - inset).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            if coverage >= 1.0 {
                dest.put_pixel(x, y, Rgba(color));
                continue;
            }
            let image = dest.get_pixel(x, y).0;
            dest.put_pixel(x, y, mix_border(image, color, coverage));
        }
    }
}

fn mix_border(image: [u8; 4], color: [u8; 4], coverage: f32) -> Rgba<u8> {
    let mix = |image: u8, color: u8| -> u8 {
        (f32::from(color) * coverage + f32::from(image) * (1.0 - coverage)).round() as u8
    };
    Rgba([
        mix(image[0], color[0]),
        mix(image[1], color[1]),
        mix(image[2], color[2]),
        mix(image[3], color[3]),
    ])
}

fn paint_rotated(dest: &mut RgbaImage, src: &RgbaImage, frame: &Frame, canvas: usize, ratio: f32) {
    let Some(photo) = frame.photo.as_ref() else {
        return;
    };
    let Some(fit) = cover_fit(frame, ratio) else {
        return;
    };
    if fit.w <= 1.0e-6 || fit.h <= 1.0e-6 || src.width() == 0 || src.height() == 0 {
        return;
    }
    let dest_w = dest.width();
    let dest_h = dest.height();
    let canvas_x = canvas as f32;
    let x0 = frame.x.max(canvas_x);
    let y0 = frame.y.max(0.0);
    let x1 = (frame.x + frame.w).min(canvas_x + 1.0);
    let y1 = (frame.y + frame.h).min(1.0);
    if x1 - x0 <= 1.0e-5 || y1 - y0 <= 1.0e-5 {
        return;
    }
    let px0 = (((x0 - canvas_x) * dest_w as f32).floor().max(0.0) as u32).min(dest_w);
    let py0 = ((y0 * dest_h as f32).floor().max(0.0) as u32).min(dest_h);
    let px1 = (((x1 - canvas_x) * dest_w as f32).ceil() as u32).min(dest_w);
    let py1 = ((y1 * dest_h as f32).ceil() as u32).min(dest_h);
    let image_w = src.width() as f32;
    let image_h = src.height() as f32;
    for py in py0..py1 {
        for px in px0..px1 {
            let lx = canvas_x + (px as f32 + 0.5) / dest_w as f32;
            let ly = (py as f32 + 0.5) / dest_h as f32;
            if lx < frame.x || ly < frame.y || lx > frame.x + frame.w || ly > frame.y + frame.h {
                continue;
            }
            let (local_x, local_y) = photo_local(frame, ratio, lx, ly);
            let u = (local_x - fit.x) / fit.w;
            let v = (local_y - fit.y) / fit.h;
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let sx = (photo.crop.x + u * photo.crop.w) * image_w;
            let sy = (photo.crop.y + v * photo.crop.h) * image_h;
            let sampled = sample_bilinear(src, sx, sy);
            let blended = over(dest.get_pixel(px, py).0, sampled.0);
            dest.put_pixel(px, py, Rgba(blended));
        }
    }
}

fn sample_bilinear(src: &RgbaImage, x: f32, y: f32) -> Rgba<u8> {
    let max_x = src.width().saturating_sub(1) as f32;
    let max_y = src.height().saturating_sub(1) as f32;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(src.width() - 1);
    let y1 = (y0 + 1).min(src.height() - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = src.get_pixel(x0, y0).0;
    let p10 = src.get_pixel(x1, y0).0;
    let p01 = src.get_pixel(x0, y1).0;
    let p11 = src.get_pixel(x1, y1).0;
    let mut out = [0; 4];
    for (channel, slot) in out.iter_mut().enumerate() {
        let top = p00[channel] as f32 * (1.0 - tx) + p10[channel] as f32 * tx;
        let bottom = p01[channel] as f32 * (1.0 - tx) + p11[channel] as f32 * tx;
        *slot = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
    }
    Rgba(out)
}

fn over(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let src_a = src[3] as f32 / 255.0;
    if src_a >= 1.0 {
        return src;
    }
    if src_a <= 0.0 {
        return dst;
    }
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    let channel = |src_channel: u8, dst_channel: u8| {
        ((src_channel as f32 * src_a + dst_channel as f32 * dst_a * (1.0 - src_a))
            / out_a.max(1.0e-4))
        .round()
        .clamp(0.0, 255.0) as u8
    };
    [
        channel(src[0], dst[0]),
        channel(src[1], dst[1]),
        channel(src[2], dst[2]),
        (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

fn blit(dest: &mut RgbaImage, src: &RgbaImage, region: &ClippedRegion, space_w: u32, space_h: u32) {
    let dest_w = dest.width() as f32;
    let dest_h = dest.height() as f32;
    let x0 = (region.dest[0] * dest_w).round() as i32;
    let y0 = (region.dest[1] * dest_h).round() as i32;
    let x1 = ((region.dest[0] + region.dest[2]) * dest_w).round() as i32;
    let y1 = ((region.dest[1] + region.dest[3]) * dest_h).round() as i32;
    let x0 = x0.clamp(0, dest.width() as i32);
    let y0 = y0.clamp(0, dest.height() as i32);
    let x1 = x1.clamp(x0, dest.width() as i32);
    let y1 = y1.clamp(y0, dest.height() as i32);
    let rw = (x1 - x0) as u32;
    let rh = (y1 - y0) as u32;
    if rw == 0 || rh == 0 || src.width() == 0 || src.height() == 0 {
        return;
    }
    let scale_x = src.width() as f32 / space_w.max(1) as f32;
    let scale_y = src.height() as f32 / space_h.max(1) as f32;
    let origin_x = region.src[0] * scale_x;
    let origin_y = region.src[1] * scale_y;
    let max_w = src.width() as f32;
    let max_h = src.height() as f32;
    let sx = origin_x.clamp(0.0, max_w - 1.0);
    let sy = origin_y.clamp(0.0, max_h - 1.0);
    let sx1 = (origin_x + region.src[2] * scale_x).clamp(sx + 1.0, max_w);
    let sy1 = (origin_y + region.src[3] * scale_y).clamp(sy + 1.0, max_h);
    let src_x = sx.floor() as u32;
    let src_y = sy.floor() as u32;
    let src_w = ((sx1.ceil() as u32).saturating_sub(src_x))
        .max(1)
        .min(src.width() - src_x);
    let src_h = ((sy1.ceil() as u32).saturating_sub(src_y))
        .max(1)
        .min(src.height() - src_y);
    let view = crop_imm(src, src_x, src_y, src_w, src_h);
    let resized = resize(&view.to_image(), rw, rh, FilterType::Triangle);
    overlay(dest, &resized, x0 as i64, y0 as i64);
}
