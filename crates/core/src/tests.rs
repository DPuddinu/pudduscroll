use std::sync::Arc;

use std::io::Cursor;

use image::{ImageFormat, Rgba, RgbaImage};

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
    let empty = Document::new(AspectRatio::FOUR_FIVE);
    assert_eq!(export_width(&empty), DEFAULT_EXPORT_WIDTH);
    assert_eq!(empty.ratio.export_size(export_width(&empty)), (1080, 1350));

    let mut wide = Document::new(AspectRatio::SQUARE);
    wide.set_canvas_count(2);
    let id = wide.add_photo("detail", 600, 600);
    wide.set_box(id, 0.0, 0.0, 0.5);
    let width = export_width(&wide);
    assert!(width > 1080);
    assert_eq!(width, 1200);
    let mut images = ImageMap::new();
    insert(&mut images, id, solid(600, 600, [20, 40, 60, 255]));
    let files = export_canvases(
        &wide,
        &images,
        ExportSettings {
            width,
            quality: 90,
            format: ExportFormat::Jpeg,
        },
    )
    .unwrap();
    assert_eq!(files.len(), 2);
    for file in &files {
        let encoded = image::load_from_memory(&file.bytes).unwrap().to_rgba8();
        assert_eq!(encoded.dimensions(), wide.ratio.export_size(width));
    }

    let mut low = Document::new(AspectRatio::SQUARE);
    let id = low.add_photo("low", 480, 480);
    assert!((low.photo(id).unwrap().w - 1.0).abs() < 1e-4);
    assert_eq!(export_width(&low), 480);
    assert!(export_width(&low) < 1080);
    let mut images = ImageMap::new();
    insert(&mut images, id, solid(480, 480, [8, 8, 8, 255]));
    let files = export_canvases(
        &low,
        &images,
        ExportSettings {
            width: export_width(&low),
            quality: 100,
            format: ExportFormat::Png,
        },
    )
    .unwrap();
    let encoded = image::load_from_memory(&files[0].bytes).unwrap().to_rgba8();
    assert_eq!(encoded.dimensions(), (480, 480));

    let mut pair = Document::new(AspectRatio::SQUARE);
    let half = pair.add_photo("half", 1000, 1000);
    let full = pair.add_photo("full", 1500, 1500);
    pair.set_box(half, 0.0, 0.0, 0.5);
    pair.set_box(full, 0.0, 0.0, 1.0);
    assert_eq!(export_width(&pair), 1500);

    let mut frame = Document::new(AspectRatio::FOUR_FIVE);
    let id = frame.add_photo("frame", 1200, 1200);
    assert!((frame.photo(id).unwrap().w - 1.0).abs() < 1e-4);
    let width = export_width(&frame);
    assert_eq!(width, 1200);
    let (export_w, export_h) = frame.ratio.export_size(width);
    assert_eq!(export_h, export_w * 5 / 4);
    let mut images = ImageMap::new();
    insert(&mut images, id, solid(1200, 1200, [30, 20, 10, 255]));
    let files = export_canvases(
        &frame,
        &images,
        ExportSettings {
            width,
            quality: 90,
            format: ExportFormat::Jpeg,
        },
    )
    .unwrap();
    let encoded = image::load_from_memory(&files[0].bytes).unwrap().to_rgba8();
    assert_eq!(encoded.dimensions(), (export_w, export_h));

    let mut cropped = Document::new(AspectRatio::SQUARE);
    let id = cropped.add_photo("crop", 2000, 2000);
    cropped.set_crop(
        id,
        Crop {
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        },
    );
    assert_eq!(export_width(&cropped), 1000);

    let mut samples = Document::new(AspectRatio::FOUR_FIVE);
    samples.add_photo("night.jpg", 900, 900);
    samples.add_photo("dune.png", 800, 1200);
    samples.add_photo("harbor.png", 1200, 700);
    assert_eq!(
        samples.ratio.export_size(export_width(&samples)),
        (900, 1125)
    );
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
        (placed_pixel_aspect(photo, doc.ratio.value()) - photo.crop.pixel_aspect(400, 200)).abs()
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
fn placement_snaps_to_eighth_grid_without_changing_aspect() {
    assert_eq!(CANVAS_GRID, 8);
    assert!((CANVAS_GRID_STEP * CANVAS_GRID as f32 - 1.0).abs() < 1e-6);

    let mut doc = Document::new(AspectRatio::FOUR_FIVE);
    let id = doc.add_photo("wide", 300, 200);
    let ratio = doc.ratio.value();
    doc.set_box(id, 1.1, 0.07, 0.41);
    let photo = doc.photo(id).unwrap();
    let before = placed_pixel_aspect(photo, ratio);
    let before_hw = photo.height(ratio) / photo.w;
    let h = photo.height(ratio);
    let (x, y) = snap_move(photo.x, photo.y, photo.w, h, CANVAS_GRID_STEP);
    doc.set_box(id, x, y, photo.w);
    let photo = doc.photo(id).unwrap();
    let moved_h = photo.height(ratio);
    assert!((photo.w - 0.41).abs() < 1e-6);
    assert!((placed_pixel_aspect(photo, ratio) - before).abs() < 1e-4);
    assert!((moved_h / photo.w - before_hw).abs() < 1e-4);
    assert!(on_eighth(photo.x) || on_eighth(photo.x + photo.w));
    assert!(on_eighth(photo.y) || on_eighth(photo.y + moved_h));

    let origin = photo.placement(ratio);
    let aspect = origin.h / origin.w;
    let (x, y, w) = resize_uniform(
        origin.x,
        origin.y,
        origin.w,
        origin.h,
        Handle::E,
        origin.x + origin.w + 0.17,
        origin.y,
    );
    let (x, y, w) = snap_resize(Handle::E, x, y, w, w * aspect, CANVAS_GRID_STEP);
    let snapped_h = w * aspect;
    doc.set_box(id, x, y, w);
    let photo = doc.photo(id).unwrap();
    assert!((x - origin.x).abs() < 1e-3);
    assert!((y + snapped_h * 0.5 - (origin.y + origin.h * 0.5)).abs() < 1e-3);
    assert!((snapped_h / w - aspect).abs() < 1e-4);
    assert!(on_eighth(x + w));
    assert!((placed_pixel_aspect(photo, ratio) - before).abs() < 1e-3);
    assert!((photo.height(ratio) / photo.w - before_hw).abs() < 1e-3);
}

fn on_eighth(value: f32) -> bool {
    let nearest = (value / CANVAS_GRID_STEP).round() * CANVAS_GRID_STEP;
    (nearest - value).abs() < 1e-3
}

#[test]
fn byte_estimate_matches_encoded_carousel() {
    let mut doc = Document::new(AspectRatio::SQUARE);
    doc.set_canvas_count(2);
    let id = doc.add_photo("noise", 96, 96);
    doc.set_box(id, 0.25, 0.0, 1.5);
    let mut images = ImageMap::new();
    insert(&mut images, id, noisy(96, 96));

    let width = export_width(&doc);
    assert_eq!(width, 64);
    assert_eq!(doc.ratio.export_size(width), (64, 64));
    let low = ExportSettings {
        width,
        quality: 0,
        format: ExportFormat::Jpeg,
    };
    let mid = ExportSettings {
        width,
        quality: 70,
        format: ExportFormat::Jpeg,
    };
    let full_jpeg = ExportSettings {
        width,
        quality: 100,
        format: ExportFormat::Jpeg,
    };
    let native = ExportSettings {
        width,
        quality: 100,
        format: ExportFormat::Png,
    };
    let low_files = export_canvases(&doc, &images, low).unwrap();
    let mid_files = export_canvases(&doc, &images, mid).unwrap();
    let full_jpeg_files = export_canvases(&doc, &images, full_jpeg).unwrap();
    let native_files = export_canvases(&doc, &images, native).unwrap();

    assert_eq!(low_files.len(), 2);
    assert_eq!(low_files[0].file_name, "pudduscroll-01.jpg");
    assert_eq!(low_files[1].file_name, "pudduscroll-02.jpg");
    assert_eq!(full_jpeg_files[0].file_name, "pudduscroll-01.jpg");
    assert_eq!(native_files[0].file_name, "pudduscroll-01.png");
    assert_eq!(low_files[0].bytes[0], 0xff);
    assert_eq!(low_files[0].bytes[1], 0xd8);
    assert_eq!(full_jpeg_files[0].bytes[0], 0xff);
    assert_eq!(full_jpeg_files[0].bytes[1], 0xd8);
    assert_eq!(&native_files[0].bytes[..8], b"\x89PNG\r\n\x1a\n");
    for file in low_files
        .iter()
        .chain(mid_files.iter())
        .chain(full_jpeg_files.iter())
        .chain(native_files.iter())
    {
        let encoded = image::load_from_memory(&file.bytes).unwrap().to_rgba8();
        assert_eq!(encoded.dimensions(), doc.ratio.export_size(width));
    }

    let low_bytes = estimate_total_bytes(&doc, &images, low).unwrap();
    let mid_bytes = estimate_total_bytes(&doc, &images, mid).unwrap();
    let full_jpeg_bytes = estimate_total_bytes(&doc, &images, full_jpeg).unwrap();
    let native_bytes = estimate_total_bytes(&doc, &images, native).unwrap();
    let low_sum: u64 = low_files.iter().map(|file| file.bytes.len() as u64).sum();
    let full_jpeg_sum: u64 = full_jpeg_files
        .iter()
        .map(|file| file.bytes.len() as u64)
        .sum();
    let native_sum: u64 = native_files
        .iter()
        .map(|file| file.bytes.len() as u64)
        .sum();
    assert_eq!(low_bytes, low_sum);
    assert_eq!(full_jpeg_bytes, full_jpeg_sum);
    assert_eq!(native_bytes, native_sum);
    assert!(low_bytes < mid_bytes);
    assert!(mid_bytes < full_jpeg_bytes);
    assert_eq!(
        mid_files.iter().map(|file| file.bytes.len()).sum::<usize>() as u64,
        mid_bytes
    );

    assert_eq!(jpeg_quality_value(100), 100);
    assert_eq!(
        quality_description(0, ExportFormat::Jpeg),
        format!("JPEG quality {}", jpeg_quality_value(0))
    );
    assert_eq!(
        quality_description(100, ExportFormat::Jpeg),
        "JPEG quality 100"
    );
    assert_eq!(
        quality_description(100, ExportFormat::Png),
        "PNG (native quality)"
    );
    assert!(jpeg_quality_value(0) < jpeg_quality_value(70));
    assert!(jpeg_quality_value(70) < jpeg_quality_value(100));

    let dir = std::env::temp_dir().join(format!("pudduscroll-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    write_canvases(&dir, "pudduscroll", ExportFormat::Jpeg, &low_files).unwrap();
    assert!(dir.join("pudduscroll-01.jpg").is_file());
    assert!(dir.join("pudduscroll-02.jpg").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_file_names_use_the_carousel_name() {
    assert_eq!(
        canvas_file_name("gemma luce", 0, ExportFormat::Jpeg),
        "gemma_luce-01.jpg"
    );
    assert_eq!(
        canvas_file_name("gemma luce", 1, ExportFormat::Jpeg),
        "gemma_luce-02.jpg"
    );
    assert_eq!(
        canvas_file_name("  Gemma   Luce  ", 0, ExportFormat::Png),
        "Gemma_Luce-01.png"
    );
    assert_eq!(
        canvas_file_name("Sea \"m\"", 0, ExportFormat::Jpeg),
        "Sea_m-01.jpg"
    );
    assert_eq!(
        canvas_file_name("...", 0, ExportFormat::Jpeg),
        "pudduscroll-01.jpg"
    );

    let dir = std::env::temp_dir().join(format!("pudduscroll-name-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_canvases(
        &dir,
        "gemma luce",
        ExportFormat::Jpeg,
        &[EncodedCanvas {
            file_name: "pudduscroll-01.jpg".to_string(),
            bytes: vec![1, 2, 3],
        }],
    )
    .unwrap();
    assert!(dir.join("gemma_luce-01.jpg").is_file());
    assert!(!dir.join("pudduscroll-01.jpg").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

fn assert_crop_inside(crop: Crop) {
    assert!(crop.x >= -1e-3, "{crop:?}");
    assert!(crop.y >= -1e-3, "{crop:?}");
    assert!(crop.x + crop.w <= 1.0 + 1e-3, "{crop:?}");
    assert!(crop.y + crop.h <= 1.0 + 1e-3, "{crop:?}");
    assert!(crop.w > 0.0 && crop.h > 0.0, "{crop:?}");
}

#[test]
fn crop_session_starts_centered_unless_the_aspect_already_matches() {
    assert!(crop_lock_ratio(CropPreset::Freehand, 800, 400).is_none());
    assert!((crop_lock_ratio(CropPreset::Original, 800, 400).unwrap() - 1.0).abs() < 1e-6);
    assert!((crop_lock_ratio(CropPreset::Square, 800, 400).unwrap() - 0.5).abs() < 1e-4);

    for preset in [
        CropPreset::Square,
        CropPreset::FourFive,
        CropPreset::ThreeFour,
        CropPreset::ThreeTwo,
        CropPreset::SixteenNine,
        CropPreset::NineSixteen,
    ] {
        let crop = begin_crop_rect(Crop::full(), preset, 800, 400);
        assert_crop_inside(crop);
        assert!(
            (crop.pixel_aspect(800, 400) - preset.target_aspect()).abs() < 1e-3,
            "{preset:?} {crop:?}"
        );
        assert!((crop.x + crop.w * 0.5 - 0.5).abs() < 1e-3);
        assert!((crop.y + crop.h * 0.5 - 0.5).abs() < 1e-3);
        assert!(crop.w > 0.99 || crop.h > 0.99);
    }

    let original = begin_crop_rect(Crop::full(), CropPreset::Original, 800, 400);
    assert!((original.w - 1.0).abs() < 1e-5);
    assert!((original.h - 1.0).abs() < 1e-5);

    let square = Crop {
        x: 0.05,
        y: 0.1,
        w: 0.2,
        h: 0.4,
    };
    let kept = begin_crop_rect(square, CropPreset::Square, 800, 400);
    assert!((kept.x - 0.05).abs() < 1e-4);
    assert!((kept.h - 0.4).abs() < 1e-4);
    let reset = begin_crop_rect(square, CropPreset::SixteenNine, 800, 400);
    assert!((reset.pixel_aspect(800, 400) - CropPreset::SixteenNine.target_aspect()).abs() < 1e-3);
    assert!(reset.w > 0.99 || reset.h > 0.99);

    let free = begin_crop_rect(square, CropPreset::Freehand, 800, 400);
    assert!((free.w - 0.2).abs() < 1e-4);
    assert!((free.y - 0.1).abs() < 1e-4);

    let different = begin_crop_rect(square, CropPreset::Original, 800, 400);
    assert!((different.w - 1.0).abs() < 1e-5);
    let same_aspect = Crop {
        x: 0.1,
        y: 0.25,
        w: 0.5,
        h: 0.5,
    };
    let kept_original = begin_crop_rect(same_aspect, CropPreset::Original, 800, 400);
    assert!((kept_original.x - 0.1).abs() < 1e-4);
    assert!((kept_original.w - 0.5).abs() < 1e-4);
}

#[test]
fn crop_move_and_resize_stay_inside_and_honor_aspect_lock() {
    let crop = Crop {
        x: 0.1,
        y: 0.2,
        w: 0.3,
        h: 0.4,
    };
    let moved = move_crop_inside(crop, -0.5, 0.9);
    assert!((moved.x).abs() < 1e-5);
    assert!((moved.y - 0.6).abs() < 1e-4);
    assert!((moved.w - 0.3).abs() < 1e-5);
    assert!((moved.h - 0.4).abs() < 1e-5);
    assert_crop_inside(moved);

    let origin = Crop {
        x: 0.2,
        y: 0.3,
        w: 0.4,
        h: 0.4,
    };
    let east = resize_crop_inside(origin, Handle::E, 0.9, 0.1, None);
    assert!((east.x - 0.2).abs() < 1e-4);
    assert!((east.y - 0.3).abs() < 1e-4);
    assert!((east.h - 0.4).abs() < 1e-4);
    assert!((east.w - 0.7).abs() < 1e-4);
    let corner = resize_crop_inside(origin, Handle::Se, 0.8, 0.9, None);
    assert!((corner.x - 0.2).abs() < 1e-4);
    assert!((corner.y - 0.3).abs() < 1e-4);
    assert!((corner.w - 0.6).abs() < 1e-4);
    assert!((corner.h - 0.6).abs() < 1e-4);
    let clamped = resize_crop_inside(origin, Handle::E, 2.0, 0.5, None);
    assert!((clamped.x + clamped.w - 1.0).abs() < 1e-4);
    assert!((clamped.h - origin.h).abs() < 1e-4);
    let minimum = resize_crop_inside(origin, Handle::W, 5.0, 0.5, None);
    assert!((minimum.w - 0.02).abs() < 1e-3);
    assert!((minimum.x + minimum.w - (origin.x + origin.w)).abs() < 1e-3);
    assert!((minimum.h - origin.h).abs() < 1e-4);

    let locked = Crop {
        x: 0.25,
        y: 0.25,
        w: 0.4,
        h: 0.4,
    };
    let grown = resize_crop_inside(locked, Handle::E, 0.95, 0.2, Some(1.0));
    assert!((grown.x - 0.25).abs() < 1e-3);
    assert!((grown.w - grown.h).abs() < 1e-3);
    assert!(grown.w > locked.w);
    assert_crop_inside(grown);
    let blocked = resize_crop_inside(locked, Handle::E, 5.0, 0.2, Some(1.0));
    assert!((blocked.x - locked.x).abs() < 1e-3);
    assert!(blocked.x + blocked.w <= 1.0 + 1e-3);
    assert!((blocked.w - blocked.h).abs() < 1e-3);
    let shrunk = resize_crop_inside(locked, Handle::E, 0.0, 0.2, Some(1.0));
    assert!(shrunk.w < 0.03 && shrunk.h < 0.03);
    assert!((shrunk.x - locked.x).abs() < 1e-3);

    let anchored = resize_crop_inside(
        Crop {
            x: 0.2,
            y: 0.2,
            w: 0.5,
            h: 0.5,
        },
        Handle::Nw,
        0.05,
        0.08,
        Some(1.0),
    );
    assert!((anchored.x + anchored.w - 0.7).abs() < 1e-3);
    assert!((anchored.y + anchored.h - 0.7).abs() < 1e-3);
    assert!((anchored.w - anchored.h).abs() < 1e-3);

    let handles = [
        Handle::N,
        Handle::S,
        Handle::E,
        Handle::W,
        Handle::Ne,
        Handle::Nw,
        Handle::Se,
        Handle::Sw,
    ];
    let box_crop = Crop {
        x: 0.3,
        y: 0.3,
        w: 0.3,
        h: 0.3,
    };
    for handle in handles {
        for pointer in [(-2.0, -2.0), (3.0, 3.0), (0.5, -1.0), (1.5, 0.4)] {
            let next = resize_crop_inside(box_crop, handle, pointer.0, pointer.1, Some(1.0));
            assert_crop_inside(next);
            assert!(
                (next.w - next.h).abs() < 1e-3,
                "{handle:?} {pointer:?} {next:?}"
            );
            assert!(next.w + 1e-4 >= 0.02, "{handle:?} {next:?}");
        }
        let free = resize_crop_inside(box_crop, handle, 3.0, -2.0, None);
        assert_crop_inside(free);
        assert!(free.w + 1e-4 >= 0.02 && free.h + 1e-4 >= 0.02);
    }

    let ratio = crop_lock_ratio(CropPreset::Square, 800, 400).unwrap();
    let square = begin_crop_rect(Crop::full(), CropPreset::Square, 800, 400);
    for handle in handles {
        let next = resize_crop_inside(square, handle, 0.1, 0.2, Some(ratio));
        assert_crop_inside(next);
        let pixel = next.pixel_aspect(800, 400);
        assert!((pixel - 1.0).abs() < 1e-2, "{handle:?} {next:?} {pixel}");
    }

    let original_ratio = crop_lock_ratio(CropPreset::Original, 800, 400).unwrap();
    let original = resize_crop_inside(
        Crop {
            x: 0.2,
            y: 0.2,
            w: 0.4,
            h: 0.4,
        },
        Handle::E,
        0.9,
        0.2,
        Some(original_ratio),
    );
    assert_crop_inside(original);
    assert!((original.w / original.h - 1.0).abs() < 1e-3);
    assert!((original.pixel_aspect(800, 400) - 2.0).abs() < 1e-2);
}

#[test]
fn frame_cover_stays_inside_while_panning_and_after_a_resize() {
    let mut layout = Layout::new(AspectRatio::FOUR_FIVE);
    assert!(layout.add_canvas());
    let id = layout.add_frame();
    layout.set_frame_rect(id, 0.4, 0.15, 1.35, 0.62);
    layout.fill_frame(id, "night.jpg", 900, 900);
    layout.set_pan(id, -4.0, 9.0);
    let ratio = layout.ratio.value();
    let frame = layout.frame(id).unwrap();
    let photo = frame.photo.as_ref().unwrap();
    assert!((0.0..=1.0).contains(&photo.pan_x));
    assert!((0.0..=1.0).contains(&photo.pan_y));
    assert!(cover_contains_frame(frame, ratio));
    let before = frame.clone();
    let (pan_x, pan_y) = pan_for_origin(frame, ratio, frame.x - 3.0, frame.y + 3.0);
    layout.set_pan(id, pan_x, pan_y);
    let frame = layout.frame(id).unwrap();
    assert!((frame.x - before.x).abs() < 1e-6);
    assert!((frame.y - before.y).abs() < 1e-6);
    assert!((frame.w - before.w).abs() < 1e-6);
    assert!((frame.h - before.h).abs() < 1e-6);
    assert!(cover_contains_frame(frame, ratio));

    layout.set_frame_rect(id, 0.2, 0.05, 1.7, 0.84);
    let frame = layout.frame(id).unwrap();
    assert!(cover_contains_frame(frame, ratio));
    layout.set_frame_aspect(id, 1.0);
    let frame = layout.frame(id).unwrap();
    assert!(cover_contains_frame(frame, ratio));
    assert!((frame_pixel_aspect(frame.w, frame.h, ratio) - 1.0).abs() < 1e-3);
}

#[test]
fn snap_lines_the_frame_up_without_moving_the_pan() {
    let mut layout = Layout::new(AspectRatio::FOUR_FIVE);
    let id = layout.add_frame();
    layout.set_frame_rect(id, 0.11, 0.07, 1.2, 0.63);
    layout.fill_frame(id, "dune.png", 800, 1200);
    layout.set_pan(id, 0.25, 0.8);
    let frame = layout.frame(id).unwrap().clone();
    let (x, y) = snap_frame_to_grid(&frame);
    layout.set_frame_rect(id, x, y, frame.w, frame.h);
    let moved = layout.frame(id).unwrap();
    assert!((moved.w - frame.w).abs() < 1e-6);
    assert!((moved.h - frame.h).abs() < 1e-6);
    assert!((moved.photo.as_ref().unwrap().pan_x - 0.25).abs() < 1e-6);
    assert!((moved.photo.as_ref().unwrap().pan_y - 0.8).abs() < 1e-6);
    assert!(on_eighth(moved.x) || on_eighth(moved.x + moved.w));
    assert!(on_eighth(moved.y) || on_eighth(moved.y + moved.h));
    assert!(cover_contains_frame(moved, layout.ratio.value()));
}

#[test]
fn resize_snaps_only_the_edges_that_moved() {
    let (x, y, w, h) = snap_resized_frame_to_grid(Handle::E, 0.125, 0.11, 0.4, 0.5);
    assert!((x - 0.125).abs() < 1e-6);
    assert!((y - 0.11).abs() < 1e-6);
    assert!((h - 0.5).abs() < 1e-6);
    assert!((x + w - 0.5).abs() < 1e-4);
    assert!(on_eighth(x + w));

    let (x, y, w, h) = snap_resized_frame_to_grid(Handle::E, 0.0, 0.11, 0.5, 0.4);
    assert!((x - 0.0).abs() < 1e-6);
    assert!((y - 0.11).abs() < 1e-6);
    assert!((w - 0.5).abs() < 1e-6);
    assert!((h - 0.4).abs() < 1e-6);

    let (x, y, w, h) = snap_resized_frame_to_grid(Handle::W, 0.18, 0.25, 0.5, 0.5);
    assert!((x + w - 0.68).abs() < 1e-4);
    assert!((y - 0.25).abs() < 1e-6);
    assert!((h - 0.5).abs() < 1e-6);
    assert!(on_eighth(x));

    let (x, y, w, h) = snap_resized_frame_to_grid(Handle::N, 0.25, 0.18, 0.5, 0.5);
    assert!((y + h - 0.68).abs() < 1e-4);
    assert!((x - 0.25).abs() < 1e-6);
    assert!((w - 0.5).abs() < 1e-6);
    assert!(on_eighth(y));

    let (x, y, w, h) = snap_resized_frame_to_grid(Handle::S, 0.25, 0.25, 0.5, 0.42);
    assert!((y - 0.25).abs() < 1e-6);
    assert!((x - 0.25).abs() < 1e-6);
    assert!((w - 0.5).abs() < 1e-6);
    assert!(on_eighth(y + h));

    let (x, y, w, h) = snap_resized_frame_to_grid(Handle::Se, 0.125, 0.25, 0.4, 0.42);
    assert!((x - 0.125).abs() < 1e-6);
    assert!((y - 0.25).abs() < 1e-6);
    assert!(on_eighth(x + w));
    assert!(on_eighth(y + h));
    assert!((x + w - 0.5).abs() < 1e-4);
    assert!((y + h - 0.625).abs() < 1e-4);

    let mut layout = Layout::new(AspectRatio::FOUR_FIVE);
    let id = layout.add_frame();
    layout.set_frame_rect(id, 0.125, 0.11, 0.4, 0.5);
    layout.fill_frame(id, "night.jpg", 900, 900);
    layout.set_pan(id, 0.2, 0.7);
    let (x, y, w, h) = snap_resized_frame_to_grid(Handle::E, 0.125, 0.11, 0.4, 0.5);
    layout.set_frame_rect(id, x, y, w, h);
    let frame = layout.frame(id).unwrap();
    let photo = frame.photo.as_ref().unwrap();
    assert!((photo.pan_x - 0.2).abs() < 1e-6);
    assert!((photo.pan_y - 0.7).abs() < 1e-6);
    assert!(cover_contains_frame(frame, layout.ratio.value()));
}

#[test]
fn frame_resize_keeps_aspect_on_corners_and_changes_it_on_edges() {
    let origin = FrameRect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 0.8,
    };
    let FrameRect { x, y, w, h } = resize_frame(origin, Handle::Se, 1.6, 2.2, true);
    assert!((h / w - 0.8).abs() < 1e-3);
    assert!(w > 1.2);
    let FrameRect {
        x: x2,
        y: y2,
        w: w2,
        h: h2,
    } = resize_frame(origin, Handle::E, 1.55, 0.2, false);
    assert!((y2 - 0.0).abs() < 1e-6);
    assert!((h2 - 0.8).abs() < 1e-6);
    assert!((w2 - 1.55).abs() < 1e-3);
    assert!((h2 / w2 - 0.8).abs() > 0.05);
    let _ = (x, y, x2);
}

#[test]
fn photo_stops_at_the_last_canvas_while_the_frame_continues() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    layout.set_canvas_count(2);
    let id = layout.add_frame();
    layout.set_frame_rect(id, 1.25, -0.2, 1.4, 1.3);
    layout.fill_frame(id, "edge", 16, 16);
    let frame = layout.frame(id).unwrap();
    assert!((frame.x + frame.w - 2.65).abs() < 1e-4);
    assert!(frame.y < 0.0);
    let visible = photo_on_canvases(frame, layout.canvas_count).unwrap();
    assert!((visible.x - 1.25).abs() < 1e-4);
    assert!(visible.y.abs() < 1e-4);
    assert!((visible.x + visible.w - 2.0).abs() < 1e-4);
    assert!((visible.y + visible.h - 1.0).abs() < 1e-4);
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(solid(16, 16, [200, 30, 30, 255])));
    let canvas = render_layout(&layout, 1, &images, 40);
    assert_eq!(pixel(&canvas, 12, 20), [200, 30, 30, 255]);
    assert_eq!(pixel(&canvas, 4, 20), [255, 255, 255, 255]);
    assert_eq!(canvas.dimensions(), (40, 40));
}

#[test]
fn spanning_frame_clips_the_photo_to_each_canvas() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    layout.set_canvas_count(2);
    let id = layout.add_frame();
    layout.set_frame_rect(id, 0.0, 0.25, 2.0, 0.5);
    layout.fill_frame(id, "split", 40, 10);
    let mut image = RgbaImage::new(40, 10);
    for (x, _, pixel) in image.enumerate_pixels_mut() {
        *pixel = if x < 20 {
            Rgba([220, 20, 20, 255])
        } else {
            Rgba([20, 40, 220, 255])
        };
    }
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(image));
    let ratio = layout.ratio.value();
    let frame = layout.frame(id).unwrap();
    assert!(cover_contains_frame(frame, ratio));
    let left = clip_frame_to_canvas(frame, 0, ratio).unwrap();
    let right = clip_frame_to_canvas(frame, 1, ratio).unwrap();
    assert!((left.dest[2] - 1.0).abs() < 1e-3);
    assert!((right.dest[2] - 1.0).abs() < 1e-3);
    assert!(clip_frame_to_canvas(frame, 0, ratio).is_some());
    let first = render_layout(&layout, 0, &images, 40);
    let second = render_layout(&layout, 1, &images, 40);
    assert_eq!(pixel(&first, 4, 2), [255, 255, 255, 255]);
    assert_eq!(pixel(&first, 8, 20), [220, 20, 20, 255]);
    assert_eq!(pixel(&first, 30, 20), [220, 20, 20, 255]);
    assert_eq!(pixel(&second, 8, 20), [20, 40, 220, 255]);
    assert_eq!(pixel(&first, 30, 36), [255, 255, 255, 255]);
}

#[test]
fn same_border_has_the_same_thickness_on_every_frame() {
    let small = bordered_frame(0.25, 0.25, 12);
    let wide = bordered_frame(1.0, 0.25, 12);
    let large = bordered_frame(1.0, 1.0, 50);
    assert_eq!(small, 10);
    assert_eq!(wide, small);
    assert_eq!(large, small);
}

fn bordered_frame(w: f32, h: f32, y: u32) -> u32 {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.set_frame_rect(id, 0.0, 0.0, w, h);
    layout.fill_frame(id, "solid", 16, 16);
    layout.set_border(id, 0.5);
    layout.set_border_color(id, [9, 18, 27, 255]);
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(solid(16, 16, [200, 30, 30, 255])));
    let image = render_layout(&layout, 0, &images, 100);
    let mut depth = 0;
    for x in 0..image.width() {
        if pixel(&image, x, y) == [9, 18, 27, 255] {
            depth += 1;
        } else {
            break;
        }
    }
    depth
}

#[test]
fn frame_border_paints_the_edge_and_keeps_the_photo() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "solid", 16, 16);
    layout.set_border_color(id, [9, 18, 27, 255]);
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(solid(16, 16, [200, 30, 30, 255])));
    let plain = render_layout(&layout, 0, &images, 32);
    assert_eq!(pixel(&plain, 0, 0), [200, 30, 30, 255]);
    assert_eq!(pixel(&plain, 16, 16), [200, 30, 30, 255]);
    layout.set_border(id, 4.0);
    assert!((layout.frame(id).unwrap().border - 1.0).abs() < 1e-6);
    let framed = render_layout(&layout, 0, &images, 32);
    assert_eq!(pixel(&framed, 0, 0), [9, 18, 27, 255]);
    assert_eq!(pixel(&framed, 2, 16), [9, 18, 27, 255]);
    assert_eq!(pixel(&framed, 16, 16), [200, 30, 30, 255]);
}

#[test]
fn clearing_a_photo_leaves_the_frame() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "night.jpg", 32, 32);
    assert!(layout.clear_photo(id));
    let frame = layout.frame(id).unwrap();
    assert!(frame.photo.is_none());
    assert!((frame.w - 1.0).abs() < 1e-6);
    assert_eq!(layout_export_width(&layout), DEFAULT_EXPORT_WIDTH);
}

#[test]
fn carousel_copy_does_not_share_frame_photos() {
    let mut template = Layout::new(AspectRatio::FOUR_FIVE);
    template.set_canvas_count(2);
    let id = template.add_frame();
    template.set_frame_rect(id, 0.0, 0.0, 2.0, 1.0);
    let mut carousel = template.clone();
    carousel.fill_frame(id, "night.jpg", 900, 900);
    assert!(template.frame(id).unwrap().photo.is_none());
    assert!(carousel.frame(id).unwrap().photo.is_some());
    assert!((carousel.frame(id).unwrap().w - 2.0).abs() < 1e-6);
    let ratio = carousel.ratio.value();
    assert!(cover_contains_frame(carousel.frame(id).unwrap(), ratio));
    assert_eq!(layout_export_width(&carousel), 450);
}

#[test]
fn moved_square_crop_keeps_placement_on_the_cropped_aspect() {
    let mut doc = Document::new(AspectRatio::FOUR_FIVE);
    let id = doc.add_photo("wide", 800, 400);
    let before_w = doc.photo(id).unwrap().w;
    let crop = begin_crop_rect(Crop::full(), CropPreset::Square, 800, 400);
    let moved = move_crop_inside(crop, 0.4, 0.1);
    assert!((moved.h - 1.0).abs() < 1e-4);
    assert!(moved.y.abs() < 1e-4);
    assert!((moved.x - 0.4).abs() < 1e-4);
    doc.set_crop(id, moved);
    if let Some(photo) = doc.photos.iter_mut().find(|photo| photo.id == id) {
        photo.preset = CropPreset::Square;
    }
    let photo = doc.photo(id).unwrap();
    assert_eq!(photo.preset, CropPreset::Square);
    assert!((photo.w - before_w).abs() < 1e-5);
    assert!((placed_pixel_aspect(photo, doc.ratio.value()) - 1.0).abs() < 1e-3);
}

#[test]
fn quarter_turns_spin_the_photo_and_keep_the_frame_covered() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "night.jpg", 80, 40);
    let ratio = layout.ratio.value();
    let upright = photo_local(layout.frame(id).unwrap(), ratio, 0.2, 0.4);
    assert!((upright.0 - 0.2).abs() < 1.0e-5);
    assert!((upright.1 - 0.4).abs() < 1.0e-5);
    for _ in 0..4 {
        layout.turn_photo(id, Turn::Clockwise);
        assert!(cover_contains_frame(layout.frame(id).unwrap(), ratio));
    }
    let angle = layout.frame(id).unwrap().photo.as_ref().unwrap().rotation;
    assert!(angle.abs() < 1.0e-4, "{angle}");
    layout.turn_photo(id, Turn::Counterclockwise);
    let ccw = layout.frame(id).unwrap().photo.as_ref().unwrap().rotation;
    assert!((ccw + std::f32::consts::FRAC_PI_2).abs() < 1.0e-4, "{ccw}");
    layout.set_rotation(id, f32::NAN);
    assert!((layout.frame(id).unwrap().photo.as_ref().unwrap().rotation - ccw).abs() < 1.0e-4);

    layout.set_rotation(id, std::f32::consts::FRAC_PI_4);
    let frame = layout.frame(id).unwrap();
    let (width, height) = displayed_cover(frame, ratio).unwrap();
    assert!(width > frame.w * 1.2 && height > frame.h * 1.2);
    assert!(cover_contains_frame(frame, ratio));
    let above = rotation_from_pointer(frame, ratio, 0.5, -1.0);
    assert!(above.abs() < 1.0e-4, "{above}");
    let right = rotation_from_pointer(frame, ratio, 2.0, 0.5);
    assert!(
        (right - std::f32::consts::FRAC_PI_2).abs() < 1.0e-4,
        "{right}"
    );

    let mut tall = Layout::new(AspectRatio::FOUR_FIVE);
    let tall_id = tall.add_frame();
    tall.fill_frame(tall_id, "night.jpg", 40, 40);
    let tall_ratio = tall.ratio.value();
    let tall_frame = tall.frame(tall_id).unwrap();
    let cx = tall_frame.x + tall_frame.w * 0.5;
    let cy = tall_frame.y + tall_frame.h * 0.5;
    let pixel_right = rotation_from_pointer(tall_frame, tall_ratio, cx + 1.0 / tall_ratio, cy);
    assert!(
        (pixel_right - std::f32::consts::FRAC_PI_2).abs() < 1.0e-4,
        "{pixel_right}"
    );
}

#[test]
fn clockwise_turn_moves_the_bottom_of_the_photo_to_the_left() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "split", 64, 64);
    let mut image = RgbaImage::new(64, 64);
    for (_, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = if y < 32 {
            Rgba([220, 20, 20, 255])
        } else {
            Rgba([20, 160, 40, 255])
        };
    }
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(image));
    let upright = render_layout(&layout, 0, &images, 64);
    assert!(pixel(&upright, 32, 12)[0] > 180);
    assert!(pixel(&upright, 32, 52)[1] > 120);

    layout.turn_photo(id, Turn::Clockwise);
    let turned = render_layout(&layout, 0, &images, 64);
    let left = pixel(&turned, 12, 32);
    let right = pixel(&turned, 52, 32);
    assert!(left[1] > 120 && left[0] < 80, "{left:?}");
    assert!(right[0] > 180 && right[1] < 80, "{right:?}");

    layout.turn_photo(id, Turn::Counterclockwise);
    layout.turn_photo(id, Turn::Counterclockwise);
    let other = render_layout(&layout, 0, &images, 64);
    let left = pixel(&other, 12, 32);
    let right = pixel(&other, 52, 32);
    assert!(left[0] > 180 && left[1] < 80, "{left:?}");
    assert!(right[1] > 120 && right[0] < 80, "{right:?}");
}

#[test]
fn free_rotation_still_fills_the_frame() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.set_frame_rect(id, 0.15, 0.2, 0.7, 0.55);
    layout.fill_frame(id, "red", 48, 32);
    layout.set_rotation(id, std::f32::consts::FRAC_PI_4);
    layout.set_pan(id, 0.15, 0.85);
    let ratio = layout.ratio.value();
    assert!(cover_contains_frame(layout.frame(id).unwrap(), ratio));
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(solid(48, 32, [240, 30, 30, 255])));
    let rendered = render_layout(&layout, 0, &images, 80);
    assert_eq!(pixel(&rendered, 2, 2), [255, 255, 255, 255]);
    let (width, height) = layout.ratio.export_size(80);
    let x0 = (0.15 * width as f32).ceil() as u32 + 2;
    let x1 = (0.85 * width as f32).floor() as u32 - 2;
    let y0 = (0.2 * height as f32).ceil() as u32 + 2;
    let y1 = (0.75 * height as f32).floor() as u32 - 2;
    for y in y0..y1 {
        for x in x0..x1 {
            let sample = pixel(&rendered, x, y);
            assert!(sample[0] > 200, "{x},{y} {sample:?}");
        }
    }
}

#[test]
fn proxy_shrink_keeps_original_dimensions() {
    let shrunk = shrink_long_edge(solid(4096, 1024, [9, 8, 7, 255]), PROXY_LONG_EDGE);
    assert_eq!((shrunk.width(), shrunk.height()), (2048, 512));
    let kept = shrink_long_edge(solid(32, 16, [1, 2, 3, 255]), PROXY_LONG_EDGE);
    assert_eq!((kept.width(), kept.height()), (32, 16));

    let raster = raster_from_bytes(&png_bytes(&solid(4096, 1024, [9, 8, 7, 255]))).unwrap();
    assert_eq!((raster.width, raster.height), (4096, 1024));
    assert_eq!((raster.proxy.width(), raster.proxy.height()), (2048, 512));
    assert_eq!(pixel(&raster.proxy, 10, 10), [9, 8, 7, 255]);
}

#[test]
fn proxy_buffer_samples_the_original_split() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    layout.set_canvas_count(2);
    let id = layout.add_frame();
    layout.set_frame_rect(id, 0.0, 0.25, 2.0, 0.5);
    layout.fill_frame(id, "split", 40, 10);
    let mut proxy = RgbaImage::new(20, 5);
    for (x, _, pixel) in proxy.enumerate_pixels_mut() {
        *pixel = if x < 10 {
            Rgba([220, 20, 20, 255])
        } else {
            Rgba([20, 40, 220, 255])
        };
    }
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(proxy));
    let first = render_layout(&layout, 0, &images, 40);
    let second = render_layout(&layout, 1, &images, 40);
    assert_eq!(pixel(&first, 8, 20), [220, 20, 20, 255]);
    assert_eq!(pixel(&first, 30, 20), [220, 20, 20, 255]);
    assert_eq!(pixel(&second, 8, 20), [20, 40, 220, 255]);
}

#[test]
fn export_reports_each_slide_once() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    layout.set_canvas_count(3);
    let id = layout.add_frame();
    layout.fill_frame(id, "solid", 8, 8);
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(solid(8, 8, [20, 30, 40, 255])));
    let seen = std::sync::atomic::AtomicUsize::new(0);
    let files = export_layout_canvases_with(
        &layout,
        &images,
        ExportSettings {
            width: 8,
            quality: 90,
            format: ExportFormat::Png,
        },
        &|done, total| {
            assert_eq!(total, 3);
            assert!((1..=3).contains(&done));
            seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        },
    )
    .unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(seen.load(std::sync::atomic::Ordering::Relaxed), 3);
}

#[test]
fn parallel_decode_keeps_every_frame() {
    let mut encoded = std::collections::HashMap::new();
    for index in 0..5u8 {
        let bytes = png_bytes(&solid(4, 3, [index * 40, 10, 20, 255]));
        encoded.insert(FrameId(u64::from(index)), Arc::from(bytes));
    }
    let seen = std::sync::atomic::AtomicUsize::new(0);
    let images = decode_frame_images(&encoded, &|done, total| {
        assert_eq!(total, 5);
        assert!((1..=5).contains(&done));
        seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    })
    .unwrap();
    assert_eq!(seen.load(std::sync::atomic::Ordering::Relaxed), 5);
    for index in 0..5u8 {
        let image = &images[&FrameId(u64::from(index))];
        assert_eq!(image.dimensions(), (4, 3));
        assert_eq!(pixel(image, 1, 1), [index * 40, 10, 20, 255]);
    }
}

#[test]
fn jpeg_export_keeps_a_flat_color() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "red", 32, 32);
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(solid(32, 32, [200, 40, 40, 255])));
    let files = export_layout_canvases(
        &layout,
        &images,
        ExportSettings {
            width: 32,
            quality: 90,
            format: ExportFormat::Jpeg,
        },
    )
    .unwrap();
    let decoded = image::load_from_memory(&files[0].bytes).unwrap().to_rgba8();
    let sample = pixel(&decoded, 16, 16);
    assert!(
        sample[0] > 180 && sample[1] < 80 && sample[2] < 80,
        "{sample:?}"
    );
}

#[test]
fn encoded_bytes_export_matches_the_decoded_image() {
    let image = solid(8, 8, [12, 34, 56, 255]);
    let bytes = png_bytes(&image);
    let decoded = image_from_bytes(&bytes).unwrap();
    assert_eq!(decoded.dimensions(), (8, 8));
    assert_eq!(pixel(&decoded, 3, 3), [12, 34, 56, 255]);

    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "solid", 8, 8);
    let mut from_image = FrameImages::new();
    from_image.insert(id, Arc::new(image));
    let mut from_bytes = FrameImages::new();
    from_bytes.insert(id, Arc::new(decoded));
    let settings = ExportSettings {
        width: 16,
        quality: 100,
        format: ExportFormat::Png,
    };
    let exported = export_layout_canvases(&layout, &from_image, settings).unwrap();
    let decoded_export = export_layout_canvases(&layout, &from_bytes, settings).unwrap();
    assert_eq!(exported.len(), decoded_export.len());
    assert_eq!(exported[0].bytes, decoded_export[0].bytes);
}

#[test]
fn wheel_zoom_scales_the_photo_around_the_pointer() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "square.png", 40, 40);
    let ratio = layout.ratio.value();
    let frame = layout.frame(id).unwrap();
    assert!((frame.photo.as_ref().unwrap().zoom - 1.0).abs() < 1e-6);
    let before = cover_fit(frame, ratio).unwrap();
    let (local_x, local_y) = photo_local(frame, ratio, 0.25, 0.3);
    let u = (local_x - before.x) / before.w;
    let v = (local_y - before.y) / before.h;

    layout.zoom_photo(id, 0.25, 0.3, 2.0);
    let frame = layout.frame(id).unwrap();
    let photo = frame.photo.as_ref().unwrap();
    assert!((photo.zoom - 2.0).abs() < 1e-4);
    assert!((photo.pan_x - 0.25).abs() < 1e-3);
    assert!((photo.pan_y - 0.3).abs() < 1e-3);
    let after = cover_fit(frame, ratio).unwrap();
    assert!((after.w - before.w * 2.0).abs() < 1e-3);
    assert!((after.h - before.h * 2.0).abs() < 1e-3);
    assert!(((local_x - after.x) / after.w - u).abs() < 1e-3);
    assert!(((local_y - after.y) / after.h - v).abs() < 1e-3);
    assert!(cover_contains_frame(frame, ratio));

    let src_before = clip_frame_to_canvas(frame, 0, ratio).unwrap().src[2];
    layout.zoom_photo(id, 0.5, 0.5, 2.0);
    let frame = layout.frame(id).unwrap();
    assert!((frame.photo.as_ref().unwrap().zoom - 4.0).abs() < 1e-3);
    let src_after = clip_frame_to_canvas(frame, 0, ratio).unwrap().src[2];
    assert!((src_after * 2.0 - src_before).abs() < 1.0);
    assert!(cover_contains_frame(frame, ratio));

    layout.zoom_photo(id, 0.5, 0.5, 0.01);
    let frame = layout.frame(id).unwrap();
    assert!((frame.photo.as_ref().unwrap().zoom - 1.0).abs() < 1e-4);
    assert!(cover_contains_frame(frame, ratio));

    layout.zoom_photo(id, 0.5, 0.5, 10_000.0);
    let held = layout.frame(id).unwrap().photo.as_ref().unwrap().zoom;
    assert!((held - MAX_PHOTO_ZOOM).abs() < 1e-4);
    layout.zoom_photo(id, 0.5, 0.5, f32::NAN);
    assert!((layout.frame(id).unwrap().photo.as_ref().unwrap().zoom - held).abs() < 1e-4);
    layout.set_rotation(id, 0.4);
    layout.zoom_photo(id, 0.4, 0.6, 0.5);
    let frame = layout.frame(id).unwrap();
    let zoom = frame.photo.as_ref().unwrap().zoom;
    assert!((1.0..=MAX_PHOTO_ZOOM).contains(&zoom));
    assert!(cover_contains_frame(frame, ratio));
}

#[test]
fn zoom_in_shows_the_middle_of_the_photo_and_zoom_out_restores_it() {
    let mut layout = Layout::new(AspectRatio::SQUARE);
    let id = layout.add_frame();
    layout.fill_frame(id, "target", 40, 40);
    let mut image = RgbaImage::from_pixel(40, 40, Rgba([20, 160, 40, 255]));
    for y in 10..30 {
        for x in 10..30 {
            image.put_pixel(x, y, Rgba([220, 20, 20, 255]));
        }
    }
    let mut images = FrameImages::new();
    images.insert(id, Arc::new(image));
    let covered = render_layout(&layout, 0, &images, 40);
    assert!(pixel(&covered, 2, 2)[1] > 120);
    assert!(pixel(&covered, 20, 20)[0] > 180);

    layout.zoom_photo(id, 0.5, 0.5, 4.0);
    let zoomed = render_layout(&layout, 0, &images, 40);
    let corner = pixel(&zoomed, 2, 2);
    assert!(corner[0] > 180 && corner[1] < 80, "{corner:?}");
    assert!(cover_contains_frame(
        layout.frame(id).unwrap(),
        layout.ratio.value()
    ));

    layout.zoom_photo(id, 0.5, 0.5, 0.1);
    let frame = layout.frame(id).unwrap();
    assert!((frame.photo.as_ref().unwrap().zoom - 1.0).abs() < 1e-4);
    let restored = render_layout(&layout, 0, &images, 40);
    assert!(pixel(&restored, 2, 2)[1] > 120);
    assert!(pixel(&restored, 20, 20)[0] > 180);
}

fn png_bytes(image: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}
