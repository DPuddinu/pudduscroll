use std::collections::HashMap;

use crate::image::Image;

#[derive(Default)]
pub struct Raster {
    textures: HashMap<egui::TextureId, Tex>,
}

impl std::fmt::Debug for Raster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Raster")
            .field("textures", &self.textures.len())
            .finish()
    }
}

struct Tex {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 4]>,
}

impl Raster {
    #[must_use]
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        output: &mut egui::FullOutput,
        logical: [u32; 2],
    ) -> Image {
        let mut delta = std::mem::take(&mut output.textures_delta);
        self.apply(&delta);
        let free = std::mem::take(&mut delta.free);
        delta.set.clear();
        let shapes = std::mem::take(&mut output.shapes);
        let pixels_per_point = output.pixels_per_point.max(0.25);
        let primitives = ctx.tessellate(shapes, pixels_per_point);
        let mut image = self.paint(&primitives, pixels_per_point, logical);
        for id in free {
            self.textures.remove(&id);
        }
        straighten(image.pixels_mut());
        image
    }

    fn apply(&mut self, delta: &egui::epaint::textures::TexturesDelta) {
        for (id, patches) in &delta.set {
            for patch in patches {
                self.blit(*id, patch);
            }
        }
    }

    fn blit(&mut self, id: egui::TextureId, patch: &egui::epaint::ImageDelta) {
        let egui::epaint::ImageData::Color(image) = &patch.image;
        let origin = patch.pos.unwrap_or([0, 0]);
        let origin_x = origin[0];
        let origin_y = origin[1];
        if patch.pos.is_none() {
            self.textures.insert(id, Tex::from_image(image));
            return;
        }
        let tex = self.textures.entry(id).or_insert_with(Tex::empty);
        tex.ensure(origin_x + image.size[0], origin_y + image.size[1]);
        for y in 0..image.size[1] {
            for x in 0..image.size[0] {
                let dest = (origin_y + y) * tex.width + (origin_x + x);
                tex.pixels[dest] = image.pixels[y * image.size[0] + x].to_array();
            }
        }
    }

    fn paint(
        &self,
        primitives: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
        logical: [u32; 2],
    ) -> Image {
        let width = (logical[0] as f32 * pixels_per_point).round().max(1.0) as u32;
        let height = (logical[1] as f32 * pixels_per_point).round().max(1.0) as u32;
        let mut image = Image::filled(width, height, [0, 0, 0, 0]);
        for clipped in primitives {
            let egui::epaint::Primitive::Mesh(mesh) = &clipped.primitive else {
                continue;
            };
            let clip = clip_rect(clipped.clip_rect, pixels_per_point, width, height);
            self.mesh(&mut image, clip, mesh, pixels_per_point);
        }
        image
    }

    fn mesh(
        &self,
        image: &mut Image,
        clip: egui::Rect,
        mesh: &egui::epaint::Mesh,
        pixels_per_point: f32,
    ) {
        let mut index = 0;
        while index + 2 < mesh.indices.len() {
            let a = mesh.vertices.get(mesh.indices[index] as usize);
            let b = mesh.vertices.get(mesh.indices[index + 1] as usize);
            let c = mesh.vertices.get(mesh.indices[index + 2] as usize);
            index += 3;
            let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                continue;
            };
            self.triangle(image, clip, [a, b, c], mesh.texture_id, pixels_per_point);
        }
    }

    fn triangle(
        &self,
        image: &mut Image,
        clip: egui::Rect,
        verts: [&egui::epaint::Vertex; 3],
        texture: egui::TextureId,
        pixels_per_point: f32,
    ) {
        let [a, b, c] = verts;
        let ax = a.pos.x * pixels_per_point;
        let ay = a.pos.y * pixels_per_point;
        let bx = b.pos.x * pixels_per_point;
        let by = b.pos.y * pixels_per_point;
        let cx = c.pos.x * pixels_per_point;
        let cy = c.pos.y * pixels_per_point;
        let area = edge(ax, ay, bx, by, cx, cy);
        if area.abs() < 1.0e-4 {
            return;
        }
        let min_x = ax.min(bx).min(cx).floor().max(clip.min.x) as i32;
        let max_x = ax.max(bx).max(cx).ceil().min(clip.max.x) as i32;
        let min_y = ay.min(by).min(cy).floor().max(clip.min.y) as i32;
        let max_y = ay.max(by).max(cy).ceil().min(clip.max.y) as i32;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let e0 = edge(bx, by, cx, cy, px, py);
                let e1 = edge(cx, cy, ax, ay, px, py);
                let e2 = edge(ax, ay, bx, by, px, py);
                if e0.signum() != area.signum() && e0 != 0.0
                    || e1.signum() != area.signum() && e1 != 0.0
                    || e2.signum() != area.signum() && e2 != 0.0
                {
                    continue;
                }
                let w0 = e0 / area;
                let w1 = e1 / area;
                let w2 = e2 / area;
                let color = mix(
                    a.color.to_array(),
                    b.color.to_array(),
                    c.color.to_array(),
                    w0,
                    w1,
                    w2,
                );
                let uv = [
                    a.uv.x * w0 + b.uv.x * w1 + c.uv.x * w2,
                    a.uv.y * w0 + b.uv.y * w1 + c.uv.y * w2,
                ];
                let sampled = tint(self.sample(texture, uv), color);
                blend(image, x, y, sampled);
            }
        }
    }

    fn sample(&self, id: egui::TextureId, uv: [f32; 2]) -> [u8; 4] {
        let Some(tex) = self.textures.get(&id) else {
            return [255, 255, 255, 255];
        };
        if tex.width == 0 || tex.height == 0 {
            return [255, 255, 255, 255];
        }
        let x = (uv[0] * tex.width as f32) as isize;
        let y = (uv[1] * tex.height as f32) as isize;
        let x = x.clamp(0, tex.width as isize - 1) as usize;
        let y = y.clamp(0, tex.height as isize - 1) as usize;
        tex.pixels[y * tex.width + x]
    }
}

impl Tex {
    fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        }
    }

    fn from_image(image: &egui::ColorImage) -> Self {
        Self {
            width: image.size[0],
            height: image.size[1],
            pixels: image.pixels.iter().map(egui::Color32::to_array).collect(),
        }
    }

    fn ensure(&mut self, width: usize, height: usize) {
        if width <= self.width && height <= self.height {
            return;
        }
        let next_w = width.max(self.width);
        let next_h = height.max(self.height);
        let mut pixels = vec![[0, 0, 0, 0]; next_w * next_h];
        for y in 0..self.height {
            for x in 0..self.width {
                pixels[y * next_w + x] = self.pixels[y * self.width + x];
            }
        }
        self.width = next_w;
        self.height = next_h;
        self.pixels = pixels;
    }
}

fn clip_rect(clip: egui::Rect, pixels_per_point: f32, width: u32, height: u32) -> egui::Rect {
    let limit_x = width as f32;
    let limit_y = height as f32;
    let x0 = finite(clip.min.x * pixels_per_point).clamp(0.0, limit_x);
    let y0 = finite(clip.min.y * pixels_per_point).clamp(0.0, limit_y);
    let x1 = finite(clip.max.x * pixels_per_point).clamp(0.0, limit_x);
    let y1 = finite(clip.max.y * pixels_per_point).clamp(0.0, limit_y);
    egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1.max(x0), y1.max(y0)))
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn mix(a: [u8; 4], b: [u8; 4], c: [u8; 4], w0: f32, w1: f32, w2: f32) -> [u8; 4] {
    let mut out = [0; 4];
    for channel in 0..4 {
        let value =
            f32::from(a[channel]) * w0 + f32::from(b[channel]) * w1 + f32::from(c[channel]) * w2;
        out[channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn tint(texture: [u8; 4], color: [u8; 4]) -> [u8; 4] {
    [
        ((u16::from(texture[0]) * u16::from(color[0])) / 255) as u8,
        ((u16::from(texture[1]) * u16::from(color[1])) / 255) as u8,
        ((u16::from(texture[2]) * u16::from(color[2])) / 255) as u8,
        ((u16::from(texture[3]) * u16::from(color[3])) / 255) as u8,
    ]
}

fn blend(image: &mut Image, x: i32, y: i32, src: [u8; 4]) {
    if x < 0 || y < 0 || x as u32 >= image.width() || y as u32 >= image.height() {
        return;
    }
    let dst = image.get(x, y);
    let inv = 255 - u16::from(src[3]);
    let pixel = [
        src[0].saturating_add(((u16::from(dst[0]) * inv + 127) / 255) as u8),
        src[1].saturating_add(((u16::from(dst[1]) * inv + 127) / 255) as u8),
        src[2].saturating_add(((u16::from(dst[2]) * inv + 127) / 255) as u8),
        src[3].saturating_add(((u16::from(dst[3]) * inv + 127) / 255) as u8),
    ];
    image.set(x as u32, y as u32, pixel);
}

fn straighten(pixels: &mut [u8]) {
    for chunk in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(chunk[3]);
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for channel in &mut chunk[..3] {
            *channel = (u16::from(*channel) * 255 / alpha).min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Raster;

    #[test]
    fn fills_an_opaque_rect() {
        let ctx = egui::Context::default();
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(1.0),
                ..Default::default()
            },
        );
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(24.0, 24.0),
            )),
            viewports,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::pos2(4.0, 4.0), egui::vec2(8.0, 8.0)),
                0.0,
                egui::Color32::from_rgb(220, 10, 10),
            );
        });
        let scale = output.pixels_per_point;
        let image = Raster::default().render(&ctx, &mut output, [24, 24]);
        let pixel = image.get((8.0 * scale) as i32, (8.0 * scale) as i32);
        assert!(pixel[0] > 180, "{pixel:?}");
        assert!(pixel[1] < 40, "{pixel:?}");
        let clear = image.get(1, 1);
        assert!(clear[0] < 30, "{clear:?}");
    }
}
