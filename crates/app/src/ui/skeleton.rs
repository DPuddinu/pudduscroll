use eframe::egui;

use super::{BG, HAIRLINE, PANEL, SKELETON_BONE};
const CARD_INNER_W: f32 = 212.0;
const CARD_INNER_H: f32 = 248.0;
const COVER_H: f32 = 132.0;
const CARD_BUTTON_H: f32 = 32.0;
const CARD_MARGIN: f32 = 12.0;
const CARD_STROKE: f32 = 1.0;
const COVER_RADIUS: f32 = 10.0;

pub(super) fn home(ui: &mut egui::Ui, full: egui::Rect) {
    ui.painter().rect_filled(full, 0.0, BG);
    let inner = full.shrink(16.0);
    let mut home = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("library-skeleton")
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    section(&mut home, "Templates", 3);
    home.add_space(18.0);
    section(&mut home, "Carousels", 3);
    home.add_space(16.0);
    ui.ctx().request_repaint();
}

pub(super) fn bone(ui: &mut egui::Ui, size: egui::Vec2, radius: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    paint(ui.ctx(), ui.painter(), rect, radius, SKELETON_BONE);
    response
}

pub(super) fn paint(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: egui::Rect,
    radius: f32,
    base: egui::Color32,
) {
    if !rect.is_positive() {
        return;
    }
    painter.rect_filled(rect, radius, base);
    let highlight = lift(base);
    let t = phase(ctx);
    let band_w = rect.width().max(32.0) * 0.42;
    let travel = rect.width() + band_w * 2.0;
    let x = rect.left() - band_w + travel * t;
    let band = egui::Rect::from_min_max(
        egui::pos2(x, rect.top()),
        egui::pos2(x + band_w, rect.bottom()),
    );
    let visible = band.intersect(rect).intersect(painter.clip_rect());
    if !visible.is_positive() {
        return;
    }
    let mut mesh = egui::Mesh::default();
    add_shimmer(&mut mesh, visible, x, band_w, highlight);
    let mut clipped = painter.clone();
    clipped.set_clip_rect(rect.intersect(painter.clip_rect()));
    clipped.add(egui::Shape::mesh(mesh));
}

fn section(ui: &mut egui::Ui, title: &str, cards: usize) {
    let row_h = 28.0;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.heading(title);
            bone(ui, egui::vec2(108.0, row_h), 6.0);
        },
    );
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
        for index in 0..cards {
            card(ui, index);
        }
    });
}

fn card(ui: &mut egui::Ui, index: usize) {
    let pad = (CARD_MARGIN + CARD_STROKE) * 2.0;
    let outer = egui::vec2(CARD_INNER_W + pad, CARD_INNER_H + pad);
    ui.allocate_ui_with_layout(outer, egui::Layout::top_down(egui::Align::Min), |ui| {
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(CARD_MARGIN)
            .corner_radius(12)
            .stroke(egui::Stroke::new(CARD_STROKE, HAIRLINE))
            .show(ui, |ui| {
                ui.set_width(CARD_INNER_W);
                let (body, _) = ui.allocate_exact_size(
                    egui::vec2(CARD_INNER_W, CARD_INNER_H),
                    egui::Sense::hover(),
                );
                let mut content = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("skeleton-card", index))
                        .max_rect(body)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                bone(
                    &mut content,
                    egui::vec2(CARD_INNER_W, COVER_H),
                    COVER_RADIUS,
                );
                content.add_space(8.0);
                bone(&mut content, egui::vec2(148.0, 16.0), 4.0);
                content.add_space(8.0);
                bone(&mut content, egui::vec2(112.0, 13.0), 4.0);
                let button_rect = egui::Rect::from_min_max(
                    egui::pos2(body.left(), body.bottom() - CARD_BUTTON_H),
                    body.right_bottom(),
                );
                let mut buttons = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("skeleton-actions", index))
                        .max_rect(button_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                bone(&mut buttons, egui::vec2(72.0, 28.0), 6.0);
                bone(&mut buttons, egui::vec2(64.0, 28.0), 6.0);
            });
    });
}

fn phase(ctx: &egui::Context) -> f32 {
    let time = ctx.input(|input| input.time) as f32;
    (time * 0.85).rem_euclid(1.0)
}

fn lift(base: egui::Color32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        base.r().saturating_add(28),
        base.g().saturating_add(24),
        base.b().saturating_add(20),
        110,
    )
}

fn add_shimmer(
    mesh: &mut egui::Mesh,
    rect: egui::Rect,
    band_left: f32,
    band_w: f32,
    color: egui::Color32,
) {
    let clear = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 0);
    let left = rect.left();
    let right = rect.right();
    let top = rect.top();
    let bottom = rect.bottom();
    let mid = (band_left + band_w * 0.5).clamp(left, right);
    let band_right = band_left + band_w;
    let i = mesh.vertices.len() as u32;
    let colors = [
        sample(left, band_left, mid, band_right, clear, color),
        sample(mid, band_left, mid, band_right, clear, color),
        sample(right, band_left, mid, band_right, clear, color),
    ];
    mesh.vertices.extend([
        vertex(egui::pos2(left, top), colors[0]),
        vertex(egui::pos2(mid, top), colors[1]),
        vertex(egui::pos2(right, top), colors[2]),
        vertex(egui::pos2(left, bottom), colors[0]),
        vertex(egui::pos2(mid, bottom), colors[1]),
        vertex(egui::pos2(right, bottom), colors[2]),
    ]);
    mesh.indices.extend([
        i,
        i + 1,
        i + 4,
        i,
        i + 4,
        i + 3,
        i + 1,
        i + 2,
        i + 5,
        i + 1,
        i + 5,
        i + 4,
    ]);
}

fn vertex(pos: egui::Pos2, color: egui::Color32) -> egui::epaint::Vertex {
    egui::epaint::Vertex {
        pos,
        uv: egui::Pos2::ZERO,
        color,
    }
}

fn sample(
    x: f32,
    left: f32,
    mid: f32,
    right: f32,
    clear: egui::Color32,
    peak: egui::Color32,
) -> egui::Color32 {
    let t = if x <= mid {
        ((x - left) / (mid - left).max(0.001)).clamp(0.0, 1.0)
    } else {
        ((right - x) / (right - mid).max(0.001)).clamp(0.0, 1.0)
    };
    egui::Color32::from_rgba_unmultiplied(
        lerp(clear.r(), peak.r(), t),
        lerp(clear.g(), peak.g(), t),
        lerp(clear.b(), peak.b(), t),
        lerp(clear.a(), peak.a(), t),
    )
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}
