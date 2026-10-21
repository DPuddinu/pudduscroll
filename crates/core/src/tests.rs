use std::sync::Arc;

use image::{Rgba, RgbaImage};

use super::*;

fn solid(width: u32, height: u32, pixel: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba(pixel))
}

fn noisy(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x.wrapping_mul(17).wrapping_add(y.wrapping_mul(29)) % 256) as u8,
            (x.wrapping_mul(43).wrapping_add(y.wrapping_mul(11)) % 256) as u8,
            (x.wrapping_mul(7).wrapping_add(y.wrapping_mul(53)) % 256) as u8,
            255,
        ])
    })
}

fn insert(images: &mut ImageMap, id: PhotoId, image: RgbaImage) {
    images.insert(id, Arc::new(image));
}

fn pixel(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel(x, y).0
}

#[test]
fn instagram_export_sizes() {
    assert_eq!(AspectRatio::SQUARE.export_size(1080), (1080, 1080));
    assert_eq!(AspectRatio::FOUR_FIVE.export_size(1080), (1080, 1350));
    assert_eq!(AspectRatio::THREE_FOUR.export_size(1080), (1080, 1440));
    assert_eq!(AspectRatio::NINE_SIXTEEN.export_size(1080), (1080, 1920));
}

#[test]
fn canvas_count_stops_at_instagram_limit() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    for _ in 0..40 {
        doc.add_canvas();
    }
    assert_eq!(doc.canvas_count, MAX_CANVASES);
    assert!(!doc.add_canvas());
    while doc.remove_canvas() {}
    assert_eq!(doc.canvas_count, 1);
    assert!(!doc.remove_canvas());
}

#[test]
fn photo_span_matches_carousel_example() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    doc.set_canvas_count(2);
    let a = doc.add_photo("A", 100, 100);
    let b = doc.add_photo("B", 100, 100);
    doc.set_box(a, 0.0, 0.0, 0.75);
    doc.set_box(b, 0.75, 0.0, 1.25);
    let ratio = doc.ratio.value();

    let a_on_first = clip_photo_to_canvas(doc.photo(a).unwrap(), 0, ratio).unwrap();
    assert!((a_on_first.dest[0] - 0.0).abs() < 1e-4);
    assert!((a_on_first.dest[2] - 0.75).abs() < 1e-4);
    assert!(clip_photo_to_canvas(doc.photo(a).unwrap(), 1, ratio).is_none());

    let b_on_first = clip_photo_to_canvas(doc.photo(b).unwrap(), 0, ratio).unwrap();
    assert!((b_on_first.dest[0] - 0.75).abs() < 1e-4);
    assert!((b_on_first.dest[2] - 0.25).abs() < 1e-4);

    let b_on_second = clip_photo_to_canvas(doc.photo(b).unwrap(), 1, ratio).unwrap();
    assert!((b_on_second.dest[0] - 0.0).abs() < 1e-4);
    assert!((b_on_second.dest[2] - 1.0).abs() < 1e-4);
    assert!((b_on_second.src[0] - 20.0).abs() < 1e-2);
}

#[test]
fn spanning_photo_is_clipped_per_canvas() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    doc.set_canvas_count(2);
    let id = doc.add_photo("split", 40, 20);
    doc.set_box(id, 0.0, 0.0, 2.0);
    let mut image = RgbaImage::new(40, 20);
    for (x, _, pixel) in image.enumerate_pixels_mut() {
        *pixel = if x < 20 {
            Rgba([220, 20, 20, 255])
        } else {
            Rgba([20, 40, 220, 255])
        };
    }
    let mut images = ImageMap::new();
    insert(&mut images, id, image);
    let first = render_canvas(&doc, 0, &images, 32);
    let second = render_canvas(&doc, 1, &images, 32);
    assert_eq!(pixel(&first, 8, 16), [220, 20, 20, 255]);
    assert_eq!(pixel(&second, 8, 16), [20, 40, 220, 255]);
    assert_eq!(pixel(&second, 24, 16), [20, 40, 220, 255]);
}

#[test]
fn uniform_scale_locks_cropped_aspect() {
    let mut doc = Document::new(AspectRatio::FOUR_FIVE);
    let id = doc.add_photo("wide", 300, 200);
    let ratio = doc.ratio.value();
    let origin = doc.photo(id).unwrap().placement(ratio);
    let before = placed_pixel_aspect(doc.photo(id).unwrap(), ratio);
    doc.resize_photo(
        id,
        Handle::Se,
        origin.x + origin.w * 1.7,
        origin.y + origin.h * 2.4,
        origin,
    );
    let photo = doc.photo(id).unwrap();
    let after = placed_pixel_aspect(photo, ratio);
    assert!((before - after).abs() < 1e-3);
    assert!(photo.w > origin.w + 0.2);
    let h = photo.height(ratio);
    assert!((h / photo.w - origin.h / origin.w).abs() < 1e-3);
    assert!((after - photo.crop.pixel_aspect(300, 200)).abs() < 1e-3);
}

#[test]
fn edge_resize_keeps_the_opposite_edge() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    let id = doc.add_photo("box", 200, 200);
    let ratio = doc.ratio.value();
    let origin = doc.photo(id).unwrap().placement(ratio);
    let right = origin.x + origin.w;
    doc.resize_photo(id, Handle::W, origin.x + origin.w * 0.25, origin.y, origin);
    let photo = doc.photo(id).unwrap();
    assert!((photo.x + photo.w - right).abs() < 1e-3);
    assert!(photo.w < origin.w);
    let h = photo.height(ratio);
    assert!((h / photo.w - origin.h / origin.w).abs() < 1e-3);
}

#[test]
fn corner_resize_from_top_left_stays_uniform() {
    let (x, y, w) = resize_uniform(1.0, 2.0, 2.0, 1.0, Handle::Nw, -1.0, 1.0);
    assert!((w - 4.0).abs() < 1e-3);
    assert!((x + w - 3.0).abs() < 1e-3);
    assert!((y + w * 0.5 - 3.0).abs() < 1e-3);
}

#[test]
fn crop_presets_lock_visible_aspect_without_stretching() {
    let mut doc = Document::new(AspectRatio::THREE_FOUR);
    let id = doc.add_photo("landscape", 800, 400);
    let before_w = doc.photo(id).unwrap().w;
    for preset in [
        CropPreset::Square,
        CropPreset::FourFive,
        CropPreset::ThreeFour,
        CropPreset::ThreeTwo,
        CropPreset::SixteenNine,
        CropPreset::NineSixteen,
    ] {
        doc.apply_crop_preset(id, preset);
        let photo = doc.photo(id).unwrap();
        assert!((photo.w - before_w).abs() < 1e-5);
        assert_eq!(photo.preset, preset);
        let crop_aspect = photo.crop.pixel_aspect(800, 400);
        let placed = placed_pixel_aspect(photo, doc.ratio.value());
        assert!(
            (crop_aspect - preset.target_aspect()).abs() < 1e-3,
            "{preset:?} crop {crop_aspect}"
        );
        assert!((placed - crop_aspect).abs() < 1e-3);
    }
    doc.apply_crop_preset(id, CropPreset::Original);
    let photo = doc.photo(id).unwrap();
    assert!((photo.crop.w - 1.0).abs() < 1e-5);
    assert!((photo.crop.h - 1.0).abs() < 1e-5);
    assert!((photo.w - before_w).abs() < 1e-5);
}

#[test]
fn freehand_crop_changes_only_the_visible_region() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    let id = doc.add_photo("full", 400, 200);
    let before_w = doc.photo(id).unwrap().w;
    doc.set_crop(
        id,
        Crop {
            x: 0.1,
            y: 0.0,
            w: 0.5,
            h: 1.0,
        },
    );
    let photo = doc.photo(id).unwrap();
    assert_eq!(photo.preset, CropPreset::Freehand);
    assert!((photo.w - before_w).abs() < 1e-5);
    assert!((photo.crop.w - 0.5).abs() < 1e-5);
    let crop_aspect = photo.crop.pixel_aspect(400, 200);
    assert!((placed_pixel_aspect(photo, doc.ratio.value()) - crop_aspect).abs() < 1e-3);
    assert!((crop_aspect - 1.0).abs() < 1e-3);
}

#[test]
fn ratio_change_keeps_strip_placement() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    let id = doc.add_photo("placed", 400, 200);
    doc.set_box(id, 0.25, 0.1, 0.5);
    let old_h = doc.photo(id).unwrap().height(doc.ratio.value());
    let old_center = 0.1 + old_h * 0.5;
    doc.set_ratio(AspectRatio::NINE_SIXTEEN);
    let photo = doc.photo(id).unwrap();
    assert!((photo.x - 0.25).abs() < 1e-6);
    assert!((photo.w - 0.5).abs() < 1e-6);
    let new_center = photo.y + photo.height(doc.ratio.value()) * 0.5;
    assert!((old_center - new_center).abs() < 1e-4);
    assert!(
        (placed_pixel_aspect(photo, doc.ratio.value())
            - photo.crop.pixel_aspect(400, 200))
        .abs()
            < 1e-3
    );
}

#[test]
fn z_order_hit_test_and_pixels() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    doc.set_canvas_count(1);
    let red = doc.add_photo("red", 16, 16);
    let blue = doc.add_photo("blue", 16, 16);
    doc.set_box(red, 0.0, 0.0, 1.0);
    doc.set_box(blue, 0.0, 0.0, 1.0);
    assert_eq!(doc.photo_at(0.5, 0.5), Some(blue));
    assert_eq!(doc.photos_front_to_back()[0].id, blue);

    let mut images = ImageMap::new();
    insert(&mut images, red, solid(16, 16, [200, 0, 0, 255]));
    insert(&mut images, blue, solid(16, 16, [0, 0, 210, 255]));
    let covered = render_canvas(&doc, 0, &images, 24);
    assert_eq!(pixel(&covered, 12, 12), [0, 0, 210, 255]);

    assert!(doc.move_layer(blue, LayerMove::Bottom));
    assert_eq!(doc.photo_at(0.5, 0.5), Some(red));
    assert_eq!(doc.photos_front_to_back().last().unwrap().id, blue);
    let uncovered = render_canvas(&doc, 0, &images, 24);
    assert_eq!(pixel(&uncovered, 12, 12), [200, 0, 0, 255]);

    assert!(doc.move_layer(blue, LayerMove::Top));
    assert_eq!(doc.photos_front_to_back()[0].id, blue);
    assert!(doc.move_layer(blue, LayerMove::Down));
    assert_eq!(doc.photo_at(0.5, 0.5), Some(red));
}

#[test]
fn vertical_overflow_is_clipped_to_the_canvas() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    doc.set_canvas_count(1);
    let id = doc.add_photo("tall", 10, 10);
    doc.set_box(id, 0.0, -0.25, 1.0);
    let region = clip_photo_to_canvas(doc.photo(id).unwrap(), 0, doc.ratio.value()).unwrap();
    assert!((region.dest[1] - 0.0).abs() < 1e-4);
    assert!((region.dest[3] - 0.75).abs() < 1e-4);
    assert!(region.src[1] > 1.0);
}

#[test]
fn byte_estimate_matches_encoded_carousel() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    doc.set_canvas_count(2);
    let id = doc.add_photo("noise", 96, 96);
    doc.set_box(id, 0.25, 0.0, 1.5);
    let mut images = ImageMap::new();
    insert(&mut images, id, noisy(96, 96));

    let low = ExportSettings {
        width: 64,
        quality: 0,
    };
    let mid = ExportSettings {
        width: 64,
        quality: 70,
    };
    let native = ExportSettings {
        width: 64,
        quality: 100,
    };
    let low_files = export_canvases(&doc, &images, low).unwrap();
    let mid_files = export_canvases(&doc, &images, mid).unwrap();
    let native_files = export_canvases(&doc, &images, native).unwrap();

    assert_eq!(low_files.len(), 2);
    assert_eq!(low_files[0].file_name, "pudduscroll-01.jpg");
    assert_eq!(low_files[1].file_name, "pudduscroll-02.jpg");
    assert_eq!(native_files[0].file_name, "pudduscroll-01.png");
    assert_eq!(low_files[0].bytes[0], 0xff);
    assert_eq!(low_files[0].bytes[1], 0xd8);
    assert_eq!(&native_files[0].bytes[..8], b"\x89PNG\r\n\x1a\n");

    let low_bytes = estimate_total_bytes(&doc, &images, low).unwrap();
    let mid_bytes = estimate_total_bytes(&doc, &images, mid).unwrap();
    let native_bytes = estimate_total_bytes(&doc, &images, native).unwrap();
    let low_sum: u64 = low_files.iter().map(|file| file.bytes.len() as u64).sum();
    let native_sum: u64 = native_files.iter().map(|file| file.bytes.len() as u64).sum();
    assert_eq!(low_bytes, low_sum);
    assert_eq!(native_bytes, native_sum);
    assert!(low_bytes < mid_bytes);
    assert!(mid_files.iter().map(|file| file.bytes.len()).sum::<usize>() as u64 == mid_bytes);

    assert_eq!(quality_description(0), format!("JPEG quality {}", jpeg_quality_value(0)));
    assert_eq!(quality_description(100), "PNG (native quality)");
    assert!(jpeg_quality_value(0) < jpeg_quality_value(70));

    let dir = std::env::temp_dir().join(format!("pudduscroll-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    write_canvases(&dir, &low_files).unwrap();
    assert!(dir.join("pudduscroll-01.jpg").is_file());
    assert!(dir.join("pudduscroll-02.jpg").is_file());
    std::fs::remove_dir_all(&dir).ok();
}
