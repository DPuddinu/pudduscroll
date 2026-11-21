use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Image {
    #[must_use]
    pub fn filled(width: u32, height: u32, pixel: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width * height {
            pixels.extend_from_slice(&pixel);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    #[must_use]
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        debug_assert_eq!(pixels.len(), width as usize * height as usize * 4);
        Self {
            width,
            height,
            pixels,
        }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn get(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return [0, 0, 0, 0];
        }
        let index = (y as u32 * self.width + x as u32) as usize * 4;
        [
            self.pixels[index],
            self.pixels[index + 1],
            self.pixels[index + 2],
            self.pixels[index + 3],
        ]
    }

    pub fn set(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = (y * self.width + x) as usize * 4;
        self.pixels[index..index + 4].copy_from_slice(&pixel);
    }

    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, pixel: [u8; 4]) {
        let y1 = y.saturating_add(h).min(self.height);
        let x1 = x.saturating_add(w).min(self.width);
        for row in y..y1 {
            for col in x..x1 {
                self.set(col, row, pixel);
            }
        }
    }

    pub fn load_png(path: &Path) -> Result<Self, image::ImageError> {
        let image = image::open(path)?.into_rgba8();
        let width = image.width();
        let height = image.height();
        Ok(Self {
            width,
            height,
            pixels: image.into_raw(),
        })
    }

    pub fn save_png(&self, path: &Path) -> Result<(), image::ImageError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image::save_buffer(
            path,
            &self.pixels,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )
    }

    pub(crate) fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
}

pub(crate) fn rgb(pixel: [u8; 4]) -> [u8; 3] {
    [pixel[0], pixel[1], pixel[2]]
}

pub(crate) fn dist_sq(a: [u8; 3], b: [u8; 3]) -> i32 {
    let dr = i32::from(a[0]) - i32::from(b[0]);
    let dg = i32::from(a[1]) - i32::from(b[1]);
    let db = i32::from(a[2]) - i32::from(b[2]);
    dr * dr + dg * dg + db * db
}
