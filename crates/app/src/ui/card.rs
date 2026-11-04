use eframe::egui;

use super::draw::fitted_rect;
use super::skeleton;
use super::{HAIRLINE, PANEL, SKELETON_BONE};

pub(super) const CARD_INNER_W: f32 = 212.0;
const CARD_INNER_H: f32 = 248.0;
const COVER_H: f32 = 132.0;
const COVER_RADIUS: f32 = 10.0;
const CARD_BUTTON_H: f32 = 32.0;
const CARD_MARGIN: f32 = 12.0;
const CARD_STROKE: f32 = 1.0;

#[derive(Clone, Copy)]
pub(super) struct CardImage {
    id: egui::TextureId,
    width: f32,
    height: f32,
}

pub(super) struct Card {
    pub kind: &'static str,
    pub id: u64,
    pub image: Option<CardImage>,
    pub fill: egui::Color32,
    pub loading: bool,
}

pub(super) struct ShownCard {
    pub rect: egui::Rect,
    pub cover: egui::Rect,
    pub opened: bool,
}

impl Card {
    pub(super) fn show(
        self,
        ui: &mut egui::Ui,
        contents: impl FnOnce(&mut egui::Ui, &mut egui::Ui),
    ) -> ShownCard {
        let painter = ui.painter().clone();
        let mut cover_state = (egui::Rect::NOTHING, false, false);
        let drawn = ui
            .allocate_ui_with_layout(
                card_outer_size(),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    card_frame().show(ui, |ui| {
                        ui.set_width(CARD_INNER_W);
                        let (body, _) = ui.allocate_exact_size(
                            egui::vec2(CARD_INNER_W, CARD_INNER_H),
                            egui::Sense::hover(),
                        );
                        let mut content = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt((self.kind, "body", self.id))
                                .max_rect(body)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        let (cover, response) = content.allocate_exact_size(
                            egui::vec2(CARD_INNER_W, COVER_H),
                            egui::Sense::click(),
                        );
                        response
                            .clone()
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        cover_state = (cover, response.hovered(), response.clicked());
                        content.add_space(8.0);
                        let button_rect = egui::Rect::from_min_max(
                            egui::pos2(body.left(), body.bottom() - CARD_BUTTON_H),
                            body.right_bottom(),
                        );
                        let mut buttons = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt((self.kind, "actions", self.id))
                                .max_rect(button_rect)
                                .layout(egui::Layout::bottom_up(egui::Align::Min)),
                        );
                        contents(&mut content, &mut buttons);
                    })
                },
            )
            .inner;
        let (cover, hovered, opened) = cover_state;
        let scale = hover_scale(
            ui.ctx(),
            ui.make_persistent_id(egui::IdSalt::new((self.kind, "cover", self.id))),
            hovered,
        );
        let shown = egui::Rect::from_center_size(cover.center(), cover.size() * scale);
        let show_loading = self.loading && self.image.is_none();
        paint_rounded_image(&painter, shown, COVER_RADIUS * scale, self.fill, self.image);
        if show_loading {
            skeleton::paint(
                ui.ctx(),
                &painter,
                shown,
                COVER_RADIUS * scale,
                SKELETON_BONE,
            );
            ui.ctx().request_repaint();
        }
        ShownCard {
            rect: drawn.response.rect,
            cover,
            opened,
        }
    }
}

pub(super) fn image_stamp(texture: &egui::TextureHandle) -> CardImage {
    let size = texture.size();
    CardImage {
        id: texture.id(),
        width: size[0] as f32,
        height: size[1] as f32,
    }
}

fn card_outer_size() -> egui::Vec2 {
    let pad = (CARD_MARGIN + CARD_STROKE) * 2.0;
    egui::vec2(CARD_INNER_W + pad, CARD_INNER_H + pad)
}

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(CARD_MARGIN)
        .corner_radius(12)
        .stroke(egui::Stroke::new(CARD_STROKE, HAIRLINE))
}

fn hover_scale(ctx: &egui::Context, id: egui::Id, hovered: bool) -> f32 {
    let t = ctx.animate_bool_with_time(id, hovered, 0.18);
    1.0 + 0.06 * t
}

fn paint_rounded_image(
    painter: &egui::Painter,
    rect: egui::Rect,
    radius: f32,
    fill: egui::Color32,
    image: Option<CardImage>,
) {
    painter.rect_filled(rect, radius, fill);
    let Some(image) = image else {
        return;
    };
    let fitted = fitted_rect(rect, image.width, image.height);
    painter.add(
        egui::epaint::RectShape::filled(fitted, radius, egui::Color32::WHITE).with_texture(
            image.id,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        ),
    );
}
