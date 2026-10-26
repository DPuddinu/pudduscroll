use std::path::{Path, PathBuf};

use eframe::egui;
use image::RgbaImage;
use image::imageops::overlay;
use pudduscroll_core::{
    CoverFit, Crop, EncodedCanvas, ExportError, ExportFormat, ExportSettings, Frame, FrameImages,
    Handle, Layout, border_thickness, canvas_file_name, cover_fit, export_layout_canvases_with,
    layout_export_width, photo_local, photo_on_canvases, render_layout, write_canvases,
};

use super::{
    ACCENT, BG, CROP_HANDLE_HIT, EDITOR_ZOOM_SPEED, MAX_EDITOR_ZOOM, MAX_GRID_CELLS,
    MIN_EDITOR_ZOOM, MIN_GRID_CELLS, OUTSIDE_FRAME_ALPHA, PANEL, RAIL_SECTION_MIN, RAIL_SECTIONS,
    RAIL_SPLIT, TEXT,
};

pub(super) fn counted(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

pub(super) fn handle_name(handle: Handle) -> &'static str {
    match handle {
        Handle::N => "N",
        Handle::S => "S",
        Handle::E => "E",
        Handle::W => "W",
        Handle::Ne => "Ne",
        Handle::Nw => "Nw",
        Handle::Se => "Se",
        Handle::Sw => "Sw",
    }
}

pub(super) fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(16)
        .corner_radius(8)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 58, 58)))
}

pub(super) fn fitted_rect(bounds: egui::Rect, width: f32, height: f32) -> egui::Rect {
    let aspect = width / height.max(1.0);
    let mut fitted_w = bounds.width();
    let mut fitted_h = fitted_w / aspect.max(0.05);
    if fitted_h > bounds.height() {
        fitted_h = bounds.height();
        fitted_w = fitted_h * aspect;
    }
    egui::Rect::from_center_size(bounds.center(), egui::vec2(fitted_w, fitted_h))
}

pub(super) fn paint_plus(painter: &egui::Painter, center: egui::Pos2, radius: f32) {
    painter.circle_filled(center, radius, egui::Color32::from_rgb(28, 28, 28));
    painter.circle_stroke(center, radius, egui::Stroke::new(2.0, ACCENT));
    let arm = radius * 0.46;
    let stroke = egui::Stroke::new(2.0, ACCENT);
    painter.line_segment(
        [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
        stroke,
    );
    painter.line_segment(
        [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
        stroke,
    );
}

pub(super) fn carousel_preview_image(layout: &Layout, images: &FrameImages) -> RgbaImage {
    let canvas_w = 140u32;
    let (width, height) = layout.ratio.export_size(canvas_w);
    let count = layout.canvas_count.max(1) as u32;
    let mut output =
        RgbaImage::from_pixel(width * count, height, image::Rgba([255, 255, 255, 255]));
    for index in 0..layout.canvas_count {
        let canvas = render_layout(layout, index, images, canvas_w);
        overlay(
            &mut output,
            &canvas,
            i64::from(index as u32) * i64::from(width),
            0,
        );
    }
    output
}

pub(super) fn crop_unchanged(a: Crop, b: Crop) -> bool {
    (a.x - b.x).abs() < 1.0e-4
        && (a.y - b.y).abs() < 1.0e-4
        && (a.w - b.w).abs() < 1.0e-4
        && (a.h - b.h).abs() < 1.0e-4
}

pub(super) fn full_image_screen(placed: egui::Rect, crop: Crop) -> egui::Rect {
    let w = placed.width() / crop.w.max(0.02);
    let h = placed.height() / crop.h.max(0.02);
    egui::Rect::from_min_size(
        egui::pos2(placed.min.x - crop.x * w, placed.min.y - crop.y * h),
        egui::vec2(w, h),
    )
}

pub(super) fn image_to_norm(image: egui::Rect, pos: egui::Pos2) -> (f32, f32) {
    (
        (pos.x - image.min.x) / image.width().max(1.0),
        (pos.y - image.min.y) / image.height().max(1.0),
    )
}

pub(super) fn crop_to_screen(image: egui::Rect, crop: Crop) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            image.min.x + crop.x * image.width(),
            image.min.y + crop.y * image.height(),
        ),
        egui::pos2(
            image.min.x + (crop.x + crop.w) * image.width(),
            image.min.y + (crop.y + crop.h) * image.height(),
        ),
    )
}

pub(super) fn paint_crop_veil(painter: &egui::Painter, image: egui::Rect, crop: egui::Rect) {
    let veil = egui::Color32::from_black_alpha(140);
    let crop = crop.intersect(image);
    if !crop.is_positive() {
        painter.rect_filled(image, 0.0, veil);
        return;
    }
    let regions = [
        egui::Rect::from_min_max(image.left_top(), egui::pos2(image.right(), crop.top())),
        egui::Rect::from_min_max(
            egui::pos2(image.left(), crop.bottom()),
            image.right_bottom(),
        ),
        egui::Rect::from_min_max(egui::pos2(image.left(), crop.top()), crop.left_bottom()),
        egui::Rect::from_min_max(crop.right_top(), egui::pos2(image.right(), crop.bottom())),
    ];
    for region in regions {
        if region.is_positive() {
            painter.rect_filled(region, 0.0, veil);
        }
    }
}

const fn rail_room(total: f32) -> f32 {
    (total - RAIL_SPLIT * (RAIL_SECTIONS - 1) as f32).max(0.0)
}

const fn rail_min(room: f32) -> f32 {
    if room <= 0.0 {
        0.0
    } else {
        RAIL_SECTION_MIN.min(room / RAIL_SECTIONS as f32)
    }
}

pub(super) fn rail_heights(total: f32, weights: [f32; RAIL_SECTIONS]) -> [f32; RAIL_SECTIONS] {
    let room = rail_room(total);
    let min = rail_min(room);
    let sum = weights.into_iter().sum::<f32>().max(f32::EPSILON);
    let mut heights = weights.map(|weight| room * (weight / sum));
    for _ in 0..RAIL_SECTIONS {
        let mut deficit = 0.0;
        let mut spare = 0.0;
        for height in &heights {
            if *height < min {
                deficit += min - *height;
            } else {
                spare += *height - min;
            }
        }
        if deficit <= 0.01 || spare <= 0.01 {
            break;
        }
        for height in &mut heights {
            if *height < min {
                *height = min;
            } else {
                let extra = *height - min;
                *height -= deficit * (extra / spare);
            }
        }
    }
    for height in &mut heights {
        if *height < min {
            *height = min;
        }
    }
    heights
}

pub(super) fn apply_rail_drag(
    mut heights: [f32; RAIL_SECTIONS],
    index: usize,
    delta: f32,
    total: f32,
) -> [f32; RAIL_SECTIONS] {
    if index + 1 >= RAIL_SECTIONS {
        return heights;
    }
    let pair = heights[index] + heights[index + 1];
    let min = rail_min(rail_room(total)).min(pair * 0.5);
    if pair <= min * 2.0 + 0.01 {
        return heights;
    }
    let grown = (heights[index] + delta).clamp(min, pair - min);
    heights[index] = grown;
    heights[index + 1] = pair - grown;
    heights
}

pub(super) fn rail_splitter(ui: &mut egui::Ui) -> Option<f32> {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), RAIL_SPLIT),
        egui::Sense::drag(),
    );
    let hot = response.hovered() || response.dragged();
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let color = if hot {
        egui::Color32::from_rgb(120, 110, 98)
    } else {
        egui::Color32::from_rgb(48, 48, 48)
    };
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0, color),
    );
    let delta = response.drag_delta().y;
    response.dragged().then_some(delta)
}

pub(super) fn rail_section(
    ui: &mut egui::Ui,
    height: f32,
    id: &str,
    divider: bool,
    add: impl FnOnce(&mut egui::Ui),
) {
    let width = ui.available_width();
    let (_space_id, rect) = ui.allocate_space(egui::vec2(width, height.max(0.0)));
    let clip = rect.intersect(ui.clip_rect());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(clip);
    if divider && clip.height() > 1.0 {
        child.painter().hline(
            clip.x_range(),
            clip.top(),
            egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 48, 48)),
        );
    }
    add(&mut child);
}

pub(super) fn paint_frame(
    painter: &egui::Painter,
    rect: egui::Rect,
    clip: egui::Rect,
    width: f32,
    color: egui::Color32,
) {
    let rect = rect.intersect(clip.shrink(1.0));
    if width <= 0.0 || rect.width() <= width * 2.0 || rect.height() <= width * 2.0 {
        return;
    }
    let bands = [
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.right(), rect.top() + width),
        ),
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - width),
            rect.right_bottom(),
        ),
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + width),
            egui::pos2(rect.left() + width, rect.bottom() - width),
        ),
        egui::Rect::from_min_max(
            egui::pos2(rect.right() - width, rect.top() + width),
            egui::pos2(rect.right(), rect.bottom() - width),
        ),
    ];
    for band in bands {
        if band.is_positive() {
            painter.rect_filled(band, 0.0, color);
        }
    }
}

pub(super) fn paint_resize_handle(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    clip: egui::Rect,
) {
    let handle =
        egui::Rect::from_center_size(center, egui::vec2(size, size)).intersect(clip.shrink(1.0));
    if handle.width() < 2.0 || handle.height() < 2.0 {
        return;
    }
    painter.rect_filled(handle, 2.0, egui::Color32::WHITE);
    paint_frame(painter, handle, handle, 1.5, ACCENT);
}

pub(super) fn photo_screen(strip: egui::Rect, count: usize, frame: &Frame) -> egui::Rect {
    let Some(bounds) = photo_on_canvases(frame, count) else {
        return egui::Rect::NOTHING;
    };
    norm_rect(strip, count, bounds.x, bounds.y, bounds.w, bounds.h)
}

pub(super) fn paint_photo_border(
    painter: &egui::Painter,
    screen: egui::Rect,
    clip: egui::Rect,
    frame: &Frame,
    canvas_size: egui::Vec2,
) {
    let thickness = border_thickness(frame.border, canvas_size.x, canvas_size.y);
    if thickness <= 0.5 {
        return;
    }
    let color = frame.border_color;
    let painter = painter.with_clip_rect(clip);
    painter.rect_stroke(
        screen,
        0.0,
        egui::Stroke::new(
            thickness,
            egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
        ),
        egui::StrokeKind::Inside,
    );
}

pub(super) fn paint_rotated_photo(
    painter: &egui::Painter,
    strip: egui::Rect,
    count: usize,
    frame: &Frame,
    ratio: f32,
    clip: egui::Rect,
    texture: egui::TextureId,
) {
    let Some(photo) = frame.photo.as_ref() else {
        return;
    };
    let Some(fit) = cover_fit(frame, ratio) else {
        return;
    };
    let screen = frame_screen(strip, count, frame);
    let passes = [
        (
            clip.intersect(screen),
            egui::Color32::from_white_alpha(OUTSIDE_FRAME_ALPHA),
        ),
        (
            clip.intersect(photo_screen(strip, count, frame)),
            egui::Color32::WHITE,
        ),
    ];
    for (visible, tint) in passes {
        if !visible.is_positive() {
            continue;
        }
        let outline = [
            screen.left_top(),
            screen.right_top(),
            screen.right_bottom(),
            screen.left_bottom(),
        ];
        let stamps: Vec<Stamp> = outline
            .into_iter()
            .map(|pos| Stamp {
                pos,
                uv: photo_uv(strip, count, frame, ratio, fit, photo.crop, pos),
            })
            .collect();
        let stamps = clip_stamps(&stamps, visible);
        if stamps.len() >= 3 {
            let mut mesh = egui::Mesh::with_texture(texture);
            for stamp in &stamps {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: stamp.pos,
                    uv: stamp.uv,
                    color: tint,
                });
            }
            let last = stamps.len() as u32;
            for index in 1..last - 1 {
                mesh.add_triangle(0, index, index + 1);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

fn photo_uv(
    strip: egui::Rect,
    count: usize,
    frame: &Frame,
    ratio: f32,
    fit: CoverFit,
    crop: Crop,
    pos: egui::Pos2,
) -> egui::Pos2 {
    let (x, y) = screen_to_norm(strip, count, pos);
    let (local_x, local_y) = photo_local(frame, ratio, x, y);
    let u = if fit.w.abs() <= 1.0e-6 {
        0.0
    } else {
        (local_x - fit.x) / fit.w
    };
    let v = if fit.h.abs() <= 1.0e-6 {
        0.0
    } else {
        (local_y - fit.y) / fit.h
    };
    egui::pos2(crop.x + u * crop.w, crop.y + v * crop.h)
}

#[derive(Clone, Copy)]
struct Stamp {
    pos: egui::Pos2,
    uv: egui::Pos2,
}

fn clip_stamps(poly: &[Stamp], clip: egui::Rect) -> Vec<Stamp> {
    let poly = clip_stamp_edge(poly, clip.left(), true, true);
    let poly = clip_stamp_edge(&poly, clip.right(), true, false);
    let poly = clip_stamp_edge(&poly, clip.top(), false, true);
    clip_stamp_edge(&poly, clip.bottom(), false, false)
}

fn clip_stamp_edge(input: &[Stamp], edge: f32, vertical: bool, keep_greater: bool) -> Vec<Stamp> {
    let inside = |stamp: &Stamp| {
        let value = if vertical { stamp.pos.x } else { stamp.pos.y };
        if keep_greater {
            value >= edge - 0.01
        } else {
            value <= edge + 0.01
        }
    };
    let mut output = Vec::new();
    if input.is_empty() {
        return output;
    }
    let mut previous_index = input.len() - 1;
    for index in 0..input.len() {
        let previous = input[previous_index];
        let current = input[index];
        let previous_in = inside(&previous);
        let current_in = inside(&current);
        if current_in {
            if !previous_in {
                output.push(stamp_cross(previous, current, edge, vertical));
            }
            output.push(current);
        } else if previous_in {
            output.push(stamp_cross(previous, current, edge, vertical));
        }
        previous_index = index;
    }
    output
}

fn stamp_cross(from: Stamp, to: Stamp, edge: f32, vertical: bool) -> Stamp {
    let (start, end) = if vertical {
        (from.pos.x, to.pos.x)
    } else {
        (from.pos.y, to.pos.y)
    };
    let delta = end - start;
    let t = if delta.abs() < 1.0e-6 {
        0.0
    } else {
        ((edge - start) / delta).clamp(0.0, 1.0)
    };
    Stamp {
        pos: from.pos + (to.pos - from.pos) * t,
        uv: egui::pos2(
            from.uv.x + (to.uv.x - from.uv.x) * t,
            from.uv.y + (to.uv.y - from.uv.y) * t,
        ),
    }
}

pub(super) fn rotate_handle_geometry(screen: egui::Rect, angle: f32) -> (egui::Pos2, egui::Pos2) {
    let center = screen.center();
    let (sin, cos) = angle.sin_cos();
    let dir = egui::vec2(sin, -cos);
    let half_w = screen.width() * 0.5;
    let half_h = screen.height() * 0.5;
    let along_x = if dir.x.abs() < 1.0e-4 {
        f32::INFINITY
    } else {
        half_w / dir.x.abs()
    };
    let along_y = if dir.y.abs() < 1.0e-4 {
        f32::INFINITY
    } else {
        half_h / dir.y.abs()
    };
    let edge = center + dir * along_x.min(along_y);
    (edge, edge + dir * 28.0)
}

pub(super) fn rotate_handle_hit(screen: egui::Rect, angle: f32, pos: egui::Pos2) -> bool {
    let (_, handle) = rotate_handle_geometry(screen, angle);
    handle.distance(pos) <= 16.0
}

pub(super) fn paint_rotate_handle(painter: &egui::Painter, screen: egui::Rect, angle: f32) {
    let (edge, handle) = rotate_handle_geometry(screen, angle);
    painter.line_segment([edge, handle], egui::Stroke::new(2.0, ACCENT));
    painter.circle_filled(handle, 7.0, egui::Color32::WHITE);
    painter.circle_stroke(handle, 7.0, egui::Stroke::new(2.0, ACCENT));
}

pub(super) fn paint_clipped_image(
    painter: &egui::Painter,
    screen: egui::Rect,
    clip: egui::Rect,
    uv: egui::Rect,
    texture: egui::TextureId,
    tint: egui::Color32,
) {
    let visible = screen.intersect(clip);
    if !visible.is_positive() || screen.width() <= 0.0 || screen.height() <= 0.0 {
        return;
    }
    let sx = uv.width() / screen.width();
    let sy = uv.height() / screen.height();
    let clipped_uv = egui::Rect::from_min_max(
        egui::pos2(
            uv.min.x + (visible.min.x - screen.min.x) * sx,
            uv.min.y + (visible.min.y - screen.min.y) * sy,
        ),
        egui::pos2(
            uv.min.x + (visible.max.x - screen.min.x) * sx,
            uv.min.y + (visible.max.y - screen.min.y) * sy,
        ),
    );
    painter.image(texture, visible, clipped_uv, tint);
}

pub(super) fn export_folder(
    layout: &Layout,
    images: &FrameImages,
    quality: u8,
    format: ExportFormat,
    on_slide: &(impl Fn(usize, usize) + Sync),
) -> Result<Vec<EncodedCanvas>, String> {
    export_layout_canvases_with(
        layout,
        images,
        ExportSettings {
            width: layout_export_width(layout),
            quality,
            format,
        },
        on_slide,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn save_folder(
    dir: &Path,
    name: &str,
    encoded: &[EncodedCanvas],
    format: ExportFormat,
) -> Result<String, String> {
    let count = encoded.len();
    write_canvases(dir, name, format, encoded).map_err(|error: ExportError| error.to_string())?;
    let sample = canvas_file_name(name, 0, format);
    Ok(format!(
        "Exported {count} images to {} ({sample})",
        dir.display()
    ))
}

pub(super) fn pick_image_path(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
        .pick_file()
}

pub(super) fn texture_from_preview(
    ctx: &egui::Context,
    name: impl Into<String>,
    preview: &RgbaImage,
) -> egui::TextureHandle {
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [preview.width() as usize, preview.height() as usize],
        preview.as_raw(),
    );
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

pub(super) fn paint_loading(
    ui: &egui::Ui,
    painter: &egui::Painter,
    screen: egui::Rect,
    clip: egui::Rect,
) {
    let center = screen.center();
    if !clip.contains(center) {
        return;
    }
    let spin = egui::Rect::from_center_size(center, egui::vec2(28.0, 28.0));
    egui::Spinner::new()
        .size(28.0)
        .color(ACCENT)
        .paint_at(ui, spin);
    let label =
        egui::Rect::from_center_size(center + egui::vec2(0.0, 28.0), egui::vec2(88.0, 22.0));
    let visible = label.intersect(clip);
    if !visible.is_positive() {
        return;
    }
    painter.rect_filled(visible, 4.0, egui::Color32::from_rgb(28, 28, 28));
    painter.text(
        label.center(),
        egui::Align2::CENTER_CENTER,
        "Loading",
        egui::FontId::proportional(14.0),
        TEXT,
    );
}

pub(super) fn next_editor_zoom(current: f32, scroll_y: f32) -> f32 {
    (current * (scroll_y * EDITOR_ZOOM_SPEED).exp()).clamp(MIN_EDITOR_ZOOM, MAX_EDITOR_ZOOM)
}

pub(super) fn grid_step(cells: i32) -> f32 {
    1.0 / cells.clamp(MIN_GRID_CELLS, MAX_GRID_CELLS) as f32
}

pub(super) fn paint_alignment_grid(
    painter: &egui::Painter,
    strip: egui::Rect,
    count: usize,
    clip: egui::Rect,
    cells: i32,
) {
    let count = count.max(1);
    let cells = cells.clamp(MIN_GRID_CELLS, MAX_GRID_CELLS);
    let canvas_w = strip.width() / count as f32;
    let divisions = cells as f32;
    let dark = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(150));
    let light = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(120));
    for index in 0..count {
        let canvas = egui::Rect::from_min_size(
            egui::pos2(strip.min.x + index as f32 * canvas_w, strip.min.y),
            egui::vec2(canvas_w, strip.height()),
        );
        let visible = canvas.intersect(clip);
        if !visible.is_positive() {
            continue;
        }
        let painter = painter.with_clip_rect(visible);
        let step_x = canvas.width() / divisions;
        let step_y = canvas.height() / divisions;
        for line in 0..=cells {
            let x = canvas.min.x + line as f32 * step_x;
            let y = canvas.min.y + line as f32 * step_y;
            let vertical = [egui::pos2(x, canvas.top()), egui::pos2(x, canvas.bottom())];
            let horizontal = [egui::pos2(canvas.left(), y), egui::pos2(canvas.right(), y)];
            painter.line_segment(vertical, dark);
            painter.line_segment(vertical, light);
            painter.line_segment(horizontal, dark);
            painter.line_segment(horizontal, light);
        }
    }
}

pub(super) fn editor_strip_size(
    bounds: egui::Rect,
    zoom: f32,
    ratio: f32,
    count: usize,
) -> egui::Vec2 {
    let fit_h = (bounds.height() - 36.0).max(120.0);
    let canvas_h = fit_h * zoom;
    let canvas_w = canvas_h * ratio;
    egui::vec2(canvas_w * count.max(1) as f32, canvas_h)
}

pub(super) fn place_strip(
    desired: egui::Pos2,
    size: egui::Vec2,
    view: egui::Rect,
    pad_min: egui::Vec2,
    pad_max: egui::Vec2,
) -> (egui::Vec2, egui::Vec2) {
    let mut lead = egui::Vec2::ZERO;
    let mut scroll = egui::Vec2::ZERO;
    let want = [desired.x - view.min.x, desired.y - view.min.y];
    let len = [size.x, size.y];
    let view_len = [view.width(), view.height()];
    let before = [pad_min.x, pad_min.y];
    let after = [pad_max.x, pad_max.y];
    for axis in 0..2 {
        if len[axis] <= view_len[axis] {
            let slack = (view_len[axis] - len[axis]).max(0.0);
            let lead_ideal = want[axis] - before[axis];
            if (0.0..=slack).contains(&lead_ideal) {
                lead[axis] = lead_ideal;
            } else if lead_ideal < 0.0 {
                let max_scroll = (before[axis] + len[axis] + after[axis] - view_len[axis]).max(0.0);
                scroll[axis] = (before[axis] - want[axis]).clamp(0.0, max_scroll);
            } else {
                lead[axis] = slack;
                let content = before[axis] + slack + len[axis] + after[axis];
                let max_scroll = (content - view_len[axis]).max(0.0);
                scroll[axis] = (before[axis] + slack - want[axis]).clamp(0.0, max_scroll);
            }
        } else {
            let max_scroll = (before[axis] + len[axis] + after[axis] - view_len[axis]).max(0.0);
            scroll[axis] = (before[axis] - want[axis]).clamp(0.0, max_scroll);
        }
    }
    (lead, scroll)
}

pub(super) fn center_on_strip(
    view: egui::Vec2,
    size: egui::Vec2,
    pad_min: egui::Vec2,
    pad_max: egui::Vec2,
) -> (egui::Vec2, egui::Vec2) {
    let mut lead = egui::Vec2::ZERO;
    let mut scroll = egui::Vec2::ZERO;
    let strip = [size.x, size.y];
    let before = [pad_min.x, pad_min.y];
    let after = [pad_max.x, pad_max.y];
    let view_len = [view.x, view.y];
    for axis in 0..2 {
        let content = before[axis] + strip[axis] + after[axis];
        let max_scroll = (content - view_len[axis]).max(0.0);
        if strip[axis] <= view_len[axis] {
            let ideal = (view_len[axis] - strip[axis]) * 0.5;
            if before[axis] <= ideal {
                lead[axis] = ideal - before[axis];
            } else {
                scroll[axis] = (before[axis] - ideal).clamp(0.0, max_scroll);
            }
        } else {
            scroll[axis] = before[axis].clamp(0.0, max_scroll);
        }
    }
    (lead, scroll)
}

pub(super) fn screen_to_norm(strip: egui::Rect, count: usize, pos: egui::Pos2) -> (f32, f32) {
    let canvas_w = strip.width() / count.max(1) as f32;
    let x = (pos.x - strip.min.x) / canvas_w.max(1.0);
    let y = (pos.y - strip.min.y) / strip.height().max(1.0);
    (x, y)
}

pub(super) fn norm_rect(
    strip: egui::Rect,
    count: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> egui::Rect {
    let canvas_w = strip.width() / count.max(1) as f32;
    egui::Rect::from_min_max(
        egui::pos2(strip.min.x + x * canvas_w, strip.min.y + y * strip.height()),
        egui::pos2(
            strip.min.x + (x + w) * canvas_w,
            strip.min.y + (y + h) * strip.height(),
        ),
    )
}

pub(super) fn frame_screen(strip: egui::Rect, count: usize, frame: &Frame) -> egui::Rect {
    norm_rect(strip, count, frame.x, frame.y, frame.w, frame.h)
}

pub(super) fn handle_points(rect: egui::Rect) -> [(Handle, egui::Pos2); 8] {
    let center = rect.center();
    [
        (Handle::Nw, rect.left_top()),
        (Handle::Ne, rect.right_top()),
        (Handle::Sw, rect.left_bottom()),
        (Handle::Se, rect.right_bottom()),
        (Handle::N, egui::pos2(center.x, rect.top())),
        (Handle::S, egui::pos2(center.x, rect.bottom())),
        (Handle::W, egui::pos2(rect.left(), center.y)),
        (Handle::E, egui::pos2(rect.right(), center.y)),
    ]
}

pub(super) fn handle_target(point: egui::Pos2) -> egui::Rect {
    egui::Rect::from_center_size(point, egui::vec2(20.0, 20.0))
}

pub(super) fn handle_at(rect: egui::Rect, pos: egui::Pos2) -> Option<Handle> {
    handle_points(rect)
        .into_iter()
        .find(|(_, point)| handle_target(*point).contains(pos))
        .map(|(handle, _)| handle)
}

pub(super) fn crop_handle_target(point: egui::Pos2) -> egui::Rect {
    egui::Rect::from_center_size(
        point,
        egui::vec2(CROP_HANDLE_HIT * 2.0, CROP_HANDLE_HIT * 2.0),
    )
}

pub(super) fn crop_handle_at(rect: egui::Rect, pos: egui::Pos2) -> Option<Handle> {
    handle_points(rect)
        .into_iter()
        .find(|(_, point)| crop_handle_target(*point).contains(pos))
        .map(|(handle, _)| handle)
}

pub(super) fn cursor_for(handle: Handle) -> egui::CursorIcon {
    match handle {
        Handle::N | Handle::S => egui::CursorIcon::ResizeVertical,
        Handle::E | Handle::W => egui::CursorIcon::ResizeHorizontal,
        Handle::Ne | Handle::Sw => egui::CursorIcon::ResizeNeSw,
        Handle::Nw | Handle::Se => egui::CursorIcon::ResizeNwSe,
    }
}

pub(super) fn short_name(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if index == 28 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    Left,
    Right,
    Up,
    Down,
    Top,
    Bottom,
    RotateCcw,
    RotateCw,
    RotateFree,
    Bin,
}

pub(super) fn icon_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
    enabled: bool,
    selected: bool,
    glyph: Glyph,
) -> egui::Response {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(salt).max_rect(rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
    ));
    child.set_clip_rect(rect.expand(1.0).intersect(child.clip_rect()));
    child.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
    let icon = egui::Atom::paint(rect.size() * 0.56, move |ui, args| {
        paint_glyph(
            ui.painter(),
            args.rect,
            glyph,
            icon_ink(args.fallback_text_color),
        );
    });
    child.add_enabled(
        enabled,
        egui::Button::new(icon)
            .selected(selected)
            .min_size(rect.size())
            .corner_radius(egui::CornerRadius::same(6)),
    )
}

pub(super) fn icon_ink(color: egui::Color32) -> egui::Color32 {
    let on_accent = color.r() > 200
        && color.g() > 120
        && color.g() < 210
        && color.b() < 140
        && color.r() > color.b();
    if on_accent {
        egui::Color32::BLACK
    } else {
        color
    }
}

pub(super) fn paint_glyph(
    painter: &egui::Painter,
    rect: egui::Rect,
    glyph: Glyph,
    color: egui::Color32,
) {
    let width = (rect.width().min(rect.height()) * 0.14).clamp(1.4, 2.15);
    let stroke = egui::Stroke::new(width, color);
    match glyph {
        Glyph::Left => chevron(painter, rect, std::f32::consts::PI, stroke),
        Glyph::Right => chevron(painter, rect, 0.0, stroke),
        Glyph::Up => chevron(painter, rect, -std::f32::consts::FRAC_PI_2, stroke),
        Glyph::Down => chevron(painter, rect, std::f32::consts::FRAC_PI_2, stroke),
        Glyph::Top => capped(painter, rect, -std::f32::consts::FRAC_PI_2, stroke),
        Glyph::Bottom => capped(painter, rect, std::f32::consts::FRAC_PI_2, stroke),
        Glyph::RotateCcw => circular_arrow(painter, rect, false, stroke),
        Glyph::RotateCw => circular_arrow(painter, rect, true, stroke),
        Glyph::RotateFree => free_rotate(painter, rect, stroke),
        Glyph::Bin => paint_bin(painter, rect, stroke),
    }
}

fn paint_bin(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let center = rect.center();
    let span = rect.width().min(rect.height());
    let lid = center.y - span * 0.22;
    painter.line_segment(
        [
            egui::pos2(center.x - span * 0.36, lid),
            egui::pos2(center.x + span * 0.36, lid),
        ],
        stroke,
    );
    let handle = lid - span * 0.14;
    painter.line(
        vec![
            egui::pos2(center.x - span * 0.12, lid),
            egui::pos2(center.x - span * 0.12, handle),
            egui::pos2(center.x + span * 0.12, handle),
            egui::pos2(center.x + span * 0.12, lid),
        ],
        stroke,
    );
    let top = lid + span * 0.08;
    let bottom = center.y + span * 0.38;
    painter.line(
        vec![
            egui::pos2(center.x - span * 0.28, top),
            egui::pos2(center.x + span * 0.28, top),
            egui::pos2(center.x + span * 0.2, bottom),
            egui::pos2(center.x - span * 0.2, bottom),
            egui::pos2(center.x - span * 0.28, top),
        ],
        stroke,
    );
}

pub(super) fn section_scroll(ui: &mut egui::Ui, salt: &str) -> egui::ScrollArea {
    let mut scroll = egui::style::ScrollStyle::solid();
    scroll.bar_width = 8.0;
    scroll.bar_inner_margin = 0.0;
    scroll.bar_outer_margin = 0.0;
    ui.spacing_mut().scroll = scroll;
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .content_margin(0.0)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
}

fn circular_arrow(
    painter: &egui::Painter,
    rect: egui::Rect,
    clockwise: bool,
    stroke: egui::Stroke,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.3;
    let end = -std::f32::consts::FRAC_PI_2;
    let sweep = if clockwise {
        std::f32::consts::PI * 1.45
    } else {
        -std::f32::consts::PI * 1.45
    };
    let arc_end = end - sweep.signum() * 0.42;
    let start = end - sweep;
    let steps = 16;
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|step| {
            let t = start + (arc_end - start) * (step as f32 / steps as f32);
            center + egui::vec2(t.cos(), t.sin()) * radius
        })
        .collect();
    painter.line(points, stroke);
    let tangent = if clockwise {
        end + std::f32::consts::FRAC_PI_2
    } else {
        end - std::f32::consts::FRAC_PI_2
    };
    let reach = radius * 0.62;
    let (sin, cos) = tangent.sin_cos();
    let forward = egui::vec2(cos, sin);
    let tip = center + egui::vec2(end.cos(), end.sin()) * radius + forward * reach * 0.15;
    chevron_at(painter, tip - forward * reach, tangent, reach, stroke);
}

fn free_rotate(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let span = rect.width().min(rect.height());
    let radius = span * 0.24;
    let center = rect.center() + egui::vec2(0.0, radius * 0.28);
    painter.circle_stroke(center, radius, stroke);
    let edge = center - egui::vec2(0.0, radius);
    let knob = edge - egui::vec2(0.0, radius * 0.7);
    painter.line_segment([edge, knob], stroke);
    painter.circle_filled(knob, stroke.width * 1.05, stroke.color);
}

pub(super) fn chevron(painter: &egui::Painter, rect: egui::Rect, angle: f32, stroke: egui::Stroke) {
    let reach = rect.width().min(rect.height()) * 0.34;
    chevron_at(painter, rect.center(), angle, reach, stroke);
}

pub(super) fn chevron_at(
    painter: &egui::Painter,
    center: egui::Pos2,
    angle: f32,
    reach: f32,
    stroke: egui::Stroke,
) {
    let (sin, cos) = angle.sin_cos();
    let point = |x: f32, y: f32| center + egui::vec2(x * cos - y * sin, x * sin + y * cos) * reach;
    painter.line(
        vec![point(-0.65, -1.0), point(1.0, 0.0), point(-0.65, 1.0)],
        stroke,
    );
}

pub(super) fn capped(painter: &egui::Painter, rect: egui::Rect, angle: f32, stroke: egui::Stroke) {
    let along = egui::vec2(angle.cos(), angle.sin());
    let side = egui::vec2(-along.y, along.x);
    let half = rect.width().min(rect.height()) * 0.42;
    let bar_center = rect.center() + along * half * 0.72;
    let bar_half = side * half * 0.85;
    painter.line(vec![bar_center - bar_half, bar_center + bar_half], stroke);
    let reach = rect.width().min(rect.height()) * 0.24;
    chevron_at(
        painter,
        rect.center() - along * reach * 0.35,
        angle,
        reach,
        stroke,
    );
}

pub(super) fn apply_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = egui::Color32::from_rgb(12, 12, 12);
    visuals.faint_bg_color = egui::Color32::from_rgb(32, 32, 32);
    visuals.override_text_color = None;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(38, 38, 38);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 46, 42);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(64, 52, 42);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}
