use eframe::egui;

use super::{BG, HAIRLINE, HOME_NAV_W, PANEL, SKELETON_BONE};
const CARD_INNER_W: f32 = 212.0;
const CARD_GAP: f32 = 8.0;
const COVER_H: f32 = 132.0;
const CARD_MARGIN: f32 = 12.0;
const CARD_STROKE: f32 = 1.0;
const COVER_RADIUS: f32 = 10.0;

pub(super) fn home(ui: &mut egui::Ui, full: egui::Rect) {
    ui.painter().rect_filled(full, 0.0, BG);
    let nav_rect = egui::Rect::from_min_max(
        full.left_top(),
        egui::pos2(full.left() + HOME_NAV_W, full.bottom()),
    );
    let main_rect = egui::Rect::from_min_max(
        egui::pos2(nav_rect.right(), full.top()),
        full.right_bottom(),
    );
    ui.painter().rect_filled(nav_rect, 0.0, PANEL);
    let mut nav = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("library-skeleton-nav")
            .max_rect(nav_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    nav.add_space(16.0);
    nav.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_width((ui.available_width() - 12.0).max(1.0));
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
            bone(ui, egui::vec2(ui.available_width().max(1.0), 36.0), 6.0);
            bone(ui, egui::vec2(ui.available_width().max(1.0), 36.0), 6.0);
        });
    });
    let mut main = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("library-skeleton-main")
            .max_rect(main_rect.shrink(16.0))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    section(&mut main, "Carousels", 3);
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
    let name_h = ui.spacing().interact_size.y;
    let meta_h = ui.text_style_height(&egui::TextStyle::Body);
    let inner_h = COVER_H + name_h + meta_h + CARD_GAP * 2.0;
    let pad = (CARD_MARGIN + CARD_STROKE) * 2.0;
    let outer = egui::vec2(CARD_INNER_W + pad, inner_h + pad);
    ui.allocate_ui_with_layout(outer, egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.push_id(("skeleton-card", index), |ui| {
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(CARD_MARGIN)
                .corner_radius(12)
                .stroke(egui::Stroke::new(CARD_STROKE, HAIRLINE))
                .show(ui, |ui| {
                    ui.set_width(CARD_INNER_W);
                    ui.spacing_mut().item_spacing.y = CARD_GAP;
                    bone(ui, egui::vec2(CARD_INNER_W, COVER_H), COVER_RADIUS);
                    bone(ui, egui::vec2(148.0, name_h), 4.0);
                    bone(ui, egui::vec2(112.0, meta_h), 4.0);
                });
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
