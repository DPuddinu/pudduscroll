use eframe::egui;
use pudduscroll_core::{CROP_PRESETS, Crop, CropPreset, FrameId, LayerMove, render_layout};

use super::draw::{
    Glyph, counted, icon_button, preview_slots, rail_section, section_heights, short_name,
};
use super::{ACCENT, MUTED, PANEL, PudduScrollApp, Route, SELECT_BG, TEXT};

impl PudduScrollApp {
    pub(super) fn layer_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Layers");
        let rows: Vec<(FrameId, String, Option<Crop>)> = self
            .layout()
            .map(|layout| {
                layout
                    .frames_front_to_back()
                    .into_iter()
                    .map(|frame| {
                        let name = frame
                            .photo
                            .as_ref()
                            .map_or_else(|| "Frame".to_string(), |photo| photo.name.clone());
                        let crop = frame.photo.as_ref().map(|photo| photo.crop);
                        (frame.id, name, crop)
                    })
                    .collect()
            })
            .unwrap_or_default();
        ui.label(egui::RichText::new(counted(rows.len(), "frame", "frames")).color(MUTED));
        ui.add_space(4.0);
        if rows.is_empty() {
            ui.label("No frames yet.");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            let count = rows.len();
            for (index, (id, name, crop)) in rows.iter().enumerate() {
                let id = *id;
                let selected = self.selected == Some(id);
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 48.0),
                    egui::Sense::click(),
                );
                if response.clicked() {
                    self.selected = Some(id);
                }
                {
                    let painter = ui.painter();
                    if selected {
                        painter.rect_filled(row, 6.0, SELECT_BG);
                    }
                    let thumb = egui::Rect::from_min_size(
                        row.min + egui::vec2(6.0, 4.0),
                        egui::vec2(40.0, 40.0),
                    );
                    painter.rect_filled(thumb, 2.0, egui::Color32::from_rgb(36, 36, 36));
                    if let Some(crop) = crop
                        && let Some(asset) = self.assets.get(&id)
                    {
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(crop.x, crop.y),
                            egui::pos2(crop.x + crop.w, crop.y + crop.h),
                        );
                        painter.image(asset.texture.id(), thumb, uv, egui::Color32::WHITE);
                    }
                    painter.text(
                        row.min + egui::vec2(54.0, 15.0),
                        egui::Align2::LEFT_TOP,
                        short_name(name),
                        egui::FontId::proportional(14.0),
                        if selected { ACCENT } else { TEXT },
                    )
                };
                let pointer_on_row = ui.input(|input| {
                    input
                        .pointer
                        .latest_pos()
                        .is_some_and(|pos| row.contains(pos))
                });
                if ui.rect_contains_pointer(row) || pointer_on_row {
                    let button = 22.0;
                    let gap = 3.0;
                    let cluster_w = button * 4.0 + gap * 3.0;
                    let origin_x = row.right() - 4.0 - cluster_w;
                    let cover = egui::Rect::from_min_max(
                        egui::pos2(origin_x - 6.0, row.top()),
                        row.right_bottom(),
                    );
                    ui.painter().rect_filled(
                        cover,
                        egui::CornerRadius {
                            nw: 0,
                            ne: 6,
                            sw: 0,
                            se: 6,
                        },
                        if selected { SELECT_BG } else { PANEL },
                    );
                    let moves = [
                        (Glyph::Up, LayerMove::Up, index > 0),
                        (Glyph::Down, LayerMove::Down, index + 1 < count),
                        (Glyph::Top, LayerMove::Top, index > 0),
                        (Glyph::Bottom, LayerMove::Bottom, index + 1 < count),
                    ];
                    let y = row.center().y - button * 0.5;
                    for (slot, (glyph, movement, enabled)) in moves.into_iter().enumerate() {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(origin_x + (button + gap) * slot as f32, y),
                            egui::vec2(button, button),
                        );
                        if icon_button(ui, rect, ("layer-move", id.0, slot), enabled, glyph)
                            .clicked()
                            && enabled
                            && self
                                .layout_mut()
                                .is_some_and(|layout| layout.move_layer(id, movement))
                        {
                            self.note_geometry();
                        }
                    }
                }
            }
        });
        ui.add_space(8.0);
    }

    pub(super) fn crop_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Crop");
        if matches!(self.route, Route::Template(_)) {
            ui.label("Frames stay empty in a template.");
            return;
        }
        let Some(id) = self.selected else {
            ui.label("Select a frame to crop its photo.");
            return;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            ui.label("Select a frame to crop its photo.");
            return;
        };
        let Some(photo) = frame.photo.clone() else {
            ui.label("This frame is empty.");
            return;
        };
        ui.label(short_name(&photo.name));
        let shown = self
            .crop_session
            .as_ref()
            .filter(|session| session.id == id)
            .map_or(photo.crop, |session| session.crop);
        let ratio = shown.pixel_aspect(photo.image_width, photo.image_height);
        ui.label(egui::RichText::new(format!("Visible {ratio:.2}")).color(MUTED));
        ui.add_space(4.0);
        {
            let mut scroll = egui::style::ScrollStyle::solid();
            scroll.bar_width = 8.0;
            scroll.bar_inner_margin = 0.0;
            scroll.bar_outer_margin = 0.0;
            ui.spacing_mut().scroll = scroll;
        };
        egui::ScrollArea::vertical()
            .id_salt("crop-list")
            .auto_shrink([false, false])
            .content_margin(0.0)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                for preset in CROP_PRESETS {
                    let armed = self
                        .crop_session
                        .as_ref()
                        .filter(|session| session.id == id)
                        .map(|session| session.preset);
                    let selected = armed.unwrap_or(photo.preset) == preset;
                    let preset_response = ui.selectable_label(selected, preset.label());
                    if preset == CropPreset::Square {
                        let _ = std::fs::write(
                            r"C:\Users\dario\AppData\Local\Temp\pudduscroll-square.txt",
                            format!("{:?}", preset_response.rect),
                        );
                    }
                    if preset_response.clicked() {
                        self.open_crop_session(id, preset);
                        ui.ctx().request_repaint();
                    }
                }
            });
        ui.add_space(8.0);
    }

    pub(super) fn preview_panel(&mut self, ui: &mut egui::Ui) {
        let count = self.metrics().map_or(1, |(_, count)| count);
        ui.horizontal(|ui| {
            ui.heading("Preview");
            ui.label(
                egui::RichText::new(format!("{}/{}", self.preview_index + 1, count)).size(16.0),
            );
        });
        let area = ui.available_rect_before_wrap();
        if !area.is_positive() {
            return;
        }
        ui.allocate_rect(area, egui::Sense::hover());
        let ratio = self.metrics().map_or(0.8, |(ratio, _)| ratio);
        let slots = preview_slots(area, ratio);
        let can_prev = self.preview_index > 0;
        if icon_button(ui, slots.left, "preview-prev", can_prev, Glyph::Left).clicked()
            && self.preview_index > 0
        {
            self.preview_index -= 1;
        }
        self.draw_phone(ui, slots.phone);
        let can_next = self.preview_index + 1 < count;
        if icon_button(ui, slots.right, "preview-next", can_next, Glyph::Right).clicked()
            && self.preview_index + 1 < count
        {
            self.preview_index += 1;
        }
    }

    pub(super) fn draw_phone(&mut self, ui: &mut egui::Ui, bezel: egui::Rect) {
        self.ensure_preview(ui.ctx());
        let screen_w = (bezel.width() - 28.0).max(1.0);
        let screen_h = (bezel.height() - 40.0).max(1.0);
        let painter = ui.painter();
        painter.rect_filled(bezel, 24.0, egui::Color32::from_rgb(10, 10, 10));
        painter.circle_filled(
            egui::pos2(bezel.center().x, bezel.top() + 14.0),
            4.0,
            egui::Color32::from_rgb(48, 48, 48),
        );
        let screen = egui::Rect::from_center_size(
            bezel.center() + egui::vec2(0.0, 6.0),
            egui::vec2(screen_w, screen_h),
        );
        painter.rect_filled(screen, 3.0, egui::Color32::WHITE);
        if let Some(texture) = &self.preview_texture {
            painter.image(
                texture.id(),
                screen,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    pub(super) fn ensure_preview(&mut self, ctx: &egui::Context) {
        let Some(layout) = self.layout().cloned() else {
            return;
        };
        if layout.canvas_count == 0 {
            return;
        }
        if self.preview_index >= layout.canvas_count {
            self.preview_index = layout.canvas_count - 1;
        }
        let key = (self.geometry_revision, self.preview_index);
        if self.preview_key == Some(key) {
            return;
        }
        if ctx.input(|input| input.pointer.any_down()) && self.preview_texture.is_some() {
            ctx.request_repaint();
            return;
        }
        let images = self.image_map();
        let rendered = render_layout(&layout, self.preview_index, &images, 640);
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [rendered.width() as usize, rendered.height() as usize],
            rendered.as_raw(),
        );
        self.preview_texture = Some(ctx.load_texture(
            format!("preview-{}-{}", key.0, key.1),
            color,
            egui::TextureOptions::LINEAR,
        ));
        self.preview_key = Some(key);
    }

    pub(super) fn paint_rail(&mut self, ui: &mut egui::Ui, rail_rect: egui::Rect) {
        if !rail_rect.is_positive() {
            return;
        }
        let layer = egui::LayerId::debug();
        ui.ctx().memory_mut(|mem| {
            mem.areas_mut().set_state(
                layer,
                egui::AreaState {
                    pivot_pos: Some(rail_rect.left_top()),
                    pivot: egui::Align2::LEFT_TOP,
                    size: Some(rail_rect.size()),
                    interactable: true,
                    last_became_visible_at: None,
                },
            );
        });
        let mut rail_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-ui")
                .max_rect(rail_rect)
                .layer_id(layer)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        rail_ui.set_clip_rect(rail_rect);
        rail_ui.painter().rect_filled(rail_rect, 0.0, PANEL);
        let inner = egui::Rect::from_min_max(
            egui::pos2(rail_rect.left() + 12.0, rail_rect.top() + 8.0),
            egui::pos2(rail_rect.right(), rail_rect.bottom() - 8.0),
        );
        let mut content = rail_ui.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-content")
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        content.set_clip_rect(inner);
        let (preview_h, crop_h, layers_h) = section_heights(content.available_height());
        rail_section(&mut content, preview_h, "preview", false, |ui| {
            self.preview_panel(ui);
        });
        rail_section(&mut content, crop_h, "crop", true, |ui| self.crop_panel(ui));
        rail_section(&mut content, layers_h, "layers", true, |ui| {
            self.layer_panel(ui);
        });
    }
}
