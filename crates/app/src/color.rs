use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use image::{ImageDecoder, RgbaImage};
use moxcms::{ColorProfile, Layout, RenderingIntent, Transform8BitExecutor, TransformOptions};

static MONITOR: Mutex<Option<String>> = Mutex::new(None);
static CANVAS: OnceLock<Canvas> = OnceLock::new();
static TRANSFORMS: Mutex<Option<TransformCache>> = Mutex::new(None);

enum Canvas {
    Profile(Arc<[u8]>),
    Srgb,
}

struct TransformCache {
    space: CacheSpace,
    transforms: HashMap<u64, Arc<Transform8BitExecutor>>,
}

enum CacheSpace {
    Profile(Arc<[u8]>),
    Srgb,
}

impl CacheSpace {
    fn from_space(space: Space<'_>) -> Self {
        match space {
            Space::Srgb => Self::Srgb,
            Space::Profile(bytes) => Self::Profile(Arc::from(bytes)),
        }
    }

    fn matches(&self, space: Space<'_>) -> bool {
        match (self, space) {
            (Self::Srgb, Space::Srgb) => true,
            (Self::Profile(cached), Space::Profile(profile)) => cached.as_ref() == profile,
            _ => false,
        }
    }
}

#[derive(Clone, Copy)]
enum Space<'a> {
    Profile(&'a [u8]),
    Srgb,
}

pub(crate) fn note_display(frame: &eframe::Frame) {
    if CANVAS.get().is_some() {
        return;
    }
    let Some(window) = frame.winit_window() else {
        return;
    };
    let Some(monitor) = window.current_monitor() else {
        return;
    };
    #[cfg(target_os = "macos")]
    let token = {
        use winit::platform::macos::MonitorHandleExtMacOS;
        let id = monitor.native_id();
        (id != 0).then(|| id.to_string())
    };
    #[cfg(not(target_os = "macos"))]
    let token = monitor.name().filter(|name| !name.is_empty());
    let Some(token) = token else {
        return;
    };
    *lock(&MONITOR) = Some(token);
}

pub(crate) fn for_display(proxy: &Arc<RgbaImage>, source: Option<&[u8]>) -> Arc<RgbaImage> {
    match canvas() {
        Canvas::Profile(profile) => Arc::new(adapt(proxy, source, Some(profile.as_ref()))),
        Canvas::Srgb => {
            if source.is_none() {
                Arc::clone(proxy)
            } else {
                Arc::new(adapt_srgb(proxy, source))
            }
        }
    }
}

pub(crate) fn embedded_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let profile = decoder.icc_profile().ok().flatten()?;
    if profile.is_empty() {
        None
    } else {
        Some(profile)
    }
}

fn canvas() -> &'static Canvas {
    CANVAS.get_or_init(|| {
        let monitor = lock(&MONITOR).clone();
        let device = monitor.as_deref();
        if pudduscroll_display::compositor_is_srgb(device) {
            return Canvas::Srgb;
        }
        match pudduscroll_display::profile_bytes(device) {
            Some(bytes) => Canvas::Profile(Arc::from(bytes)),
            None => Canvas::Srgb,
        }
    })
}

fn adapt(image: &RgbaImage, source: Option<&[u8]>, display: Option<&[u8]>) -> RgbaImage {
    let Some(display) = display else {
        return image.clone();
    };
    apply(image, source, Space::Profile(display))
}

fn adapt_srgb(image: &RgbaImage, source: Option<&[u8]>) -> RgbaImage {
    if source.is_none() {
        return image.clone();
    }
    apply(image, source, Space::Srgb)
}

fn apply(image: &RgbaImage, source: Option<&[u8]>, space: Space<'_>) -> RgbaImage {
    if let Space::Profile(display) = space {
        if source.is_some_and(|profile| profile == display) {
            return image.clone();
        }
    }
    let Some(transform) = cached_transform(source, space) else {
        return image.clone();
    };
    let packed = pack_rgb(image.as_raw());
    let mut rgb = vec![0; packed.len()];
    if transform.transform(&packed, &mut rgb).is_err() {
        return image.clone();
    }
    let mut dst = Vec::with_capacity(image.as_raw().len());
    for (color, pixel) in rgb.chunks_exact(3).zip(image.as_raw().chunks_exact(4)) {
        dst.extend_from_slice(color);
        dst.push(pixel[3]);
    }
    RgbaImage::from_raw(image.width(), image.height(), dst).unwrap_or_else(|| image.clone())
}

fn pack_rgb(pixels: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(pixels.len() / 4 * 3);
    for pixel in pixels.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}

fn cached_transform(source: Option<&[u8]>, space: Space<'_>) -> Option<Arc<Transform8BitExecutor>> {
    let key = profile_key(source);
    let mut guard = lock(&TRANSFORMS);
    if guard
        .as_ref()
        .is_none_or(|cache| !cache.space.matches(space))
    {
        *guard = Some(TransformCache {
            space: CacheSpace::from_space(space),
            transforms: HashMap::new(),
        });
    }
    let cache = guard.as_mut()?;
    if let Some(found) = cache.transforms.get(&key) {
        return Some(Arc::clone(found));
    }
    let built = build_transform(source, space)?;
    cache.transforms.insert(key, Arc::clone(&built));
    Some(built)
}

fn profile_key(source: Option<&[u8]>) -> u64 {
    let Some(source) = source else {
        return 0;
    };
    let mut hasher = std::hash::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish().max(1)
}

fn build_transform(source: Option<&[u8]>, space: Space<'_>) -> Option<Arc<Transform8BitExecutor>> {
    let src = match source {
        Some(bytes) => {
            ColorProfile::new_from_slice(bytes).unwrap_or_else(|_| ColorProfile::new_srgb())
        }
        None => ColorProfile::new_srgb(),
    };
    let dst = match space {
        Space::Profile(bytes) => ColorProfile::new_from_slice(bytes).ok()?,
        Space::Srgb => ColorProfile::new_srgb(),
    };
    let options = TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        ..TransformOptions::default()
    };
    src.create_transform_8bit(Layout::Rgb, &dst, Layout::Rgb, options)
        .ok()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{adapt, adapt_srgb, embedded_profile};
    use image::{ImageEncoder, RgbaImage};
    use std::path::PathBuf;

    #[test]
    fn missing_or_unreadable_display_profile_keeps_pixels() {
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([4, 5, 6, 7]));
        assert_eq!(adapt(&image, None, None).get_pixel(0, 0).0, [4, 5, 6, 7]);
        assert_eq!(
            adapt(&image, None, Some(b"not a profile"))
                .get_pixel(0, 0)
                .0,
            [4, 5, 6, 7]
        );
    }

    #[test]
    fn srgb_display_profile_keeps_a_flat_color() {
        let Some(profile) = srgb_profile() else {
            return;
        };
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([180, 40, 90, 128]));
        let pixel = adapt(&image, None, Some(&profile)).get_pixel(0, 0).0;
        assert_eq!(pixel[3], 128);
        for (before, after) in [180u8, 40, 90].into_iter().zip(pixel) {
            assert!(
                (i16::from(before) - i16::from(after)).abs() <= 2,
                "{before} -> {after}"
            );
        }
    }

    #[test]
    fn embedded_profile_round_trips_and_untagged_png_has_none() {
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([9, 8, 7, 255]));
        let mut plain = Vec::new();
        image::DynamicImage::ImageRgba8(image.clone())
            .write_to(
                &mut std::io::Cursor::new(&mut plain),
                image::ImageFormat::Png,
            )
            .unwrap();
        assert!(embedded_profile(&plain).is_none());

        let marker = b"pudduscroll-profile".to_vec();
        let mut tagged = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut tagged);
        encoder.set_icc_profile(marker.clone()).unwrap();
        encoder
            .write_image(image.as_raw(), 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert_eq!(
            embedded_profile(&tagged).as_deref(),
            Some(marker.as_slice())
        );
    }

    #[test]
    fn display_p3_green_changes_on_the_way_to_srgb_and_keeps_alpha() {
        let profile = moxcms::ColorProfile::new_display_p3().encode().unwrap();
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([40, 180, 90, 200]));
        let pixel = adapt_srgb(&image, Some(&profile)).get_pixel(0, 0).0;
        assert_eq!(pixel[3], 200);
        assert_ne!([pixel[0], pixel[1], pixel[2]], [40, 180, 90]);
    }

    #[test]
    fn srgb_source_stays_flat_when_the_window_is_srgb() {
        let profile = moxcms::ColorProfile::new_srgb().encode().unwrap();
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([180, 40, 90, 128]));
        let pixel = adapt_srgb(&image, Some(&profile)).get_pixel(0, 0).0;
        assert_eq!(pixel[3], 128);
        for (before, after) in [180u8, 40, 90].into_iter().zip(pixel) {
            assert!(
                (i16::from(before) - i16::from(after)).abs() <= 2,
                "{before} -> {after}"
            );
        }
    }

    #[test]
    fn system_display_profile_keeps_alpha() {
        let Some(bytes) = pudduscroll_display::profile_bytes(None) else {
            return;
        };
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let pixel = adapt(&image, None, Some(&bytes)).get_pixel(0, 0).0;
        assert_eq!(pixel[3], 255);
    }

    fn srgb_profile() -> Option<Vec<u8>> {
        let root = std::env::var_os("SystemRoot")?;
        let path = PathBuf::from(root)
            .join("System32")
            .join("spool")
            .join("drivers")
            .join("color")
            .join("sRGB Color Space Profile.icm");
        std::fs::read(path).ok()
    }
}
