use std::collections::HashMap;
use std::sync::Arc;

use image::imageops::{FilterType, crop_imm, overlay, resize};
use image::{Rgba, RgbaImage};

use crate::{Document, Photo, PhotoId};

pub type ImageMap = HashMap<PhotoId, Arc<RgbaImage>>;

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
        blit(&mut output, image, &region);
    }
    output
}

fn blit(dest: &mut RgbaImage, src: &RgbaImage, region: &ClippedRegion) {
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
    let max_w = src.width() as f32;
    let max_h = src.height() as f32;
    let sx = region.src[0].clamp(0.0, max_w - 1.0);
    let sy = region.src[1].clamp(0.0, max_h - 1.0);
    let sx1 = (region.src[0] + region.src[2]).clamp(sx + 1.0, max_w);
    let sy1 = (region.src[1] + region.src[3]).clamp(sy + 1.0, max_h);
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
