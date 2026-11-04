use eframe::egui;
use pudduscroll_core::{CROP_PRESETS, Crop, CropPreset, FrameId, LayerMove, render_layout};

const EXPORT_PREVIEW_WIDTH: u32 = 1280;

use super::draw::{
    Glyph, apply_rail_drag, counted, icon_button, rail_at_default, rail_bar, rail_fit_body,
    rail_scroll_to, rail_section, rail_splitter, section_scroll, short_name,
};
use super::{
    ACCENT, COVER_EMPTY, IncomingPhoto, MUTED, PANEL, PudduScrollApp, RAIL_SECTIONS, Route,
    SELECT_BG, TEXT,
};

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
        section_scroll(ui, "layers-list").show(ui, |ui| {
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
                    painter.rect_filled(thumb, 2.0, COVER_EMPTY);
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
                        if icon_button(ui, rect, ("layer-move", id.0, slot), enabled, false, glyph)
                            .clicked()
                            && enabled
                            && self.layout().is_some()
                        {
                            self.checkpoint();
                            if self
                                .layout_mut()
                                .is_some_and(|layout| layout.move_layer(id, movement))
                            {
                                self.note_geometry();
                            }
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
        section_scroll(ui, "crop-list").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
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
        });
        ui.add_space(8.0);
    }

    pub(super) fn ensure_preview(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
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
        if self.preview_key == Some(key) || self.preview_job == Some(key) {
            return;
        }
        let index = self.preview_index;
        let images = self.proxy_map();
        self.preview_job = Some(key);
        let tx = self.photo_tx.clone();
        crate::host::spawn(move || {
            let image = render_layout(&layout, index, &images, EXPORT_PREVIEW_WIDTH);
            let _ = tx.send(IncomingPhoto::PhonePreview {
                revision: key.0,
                index,
                image,
            });
        });
        ctx.request_repaint();
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
            egui::pos2(rail_rect.right() - 12.0, rail_rect.bottom() - 8.0),
        );
        let fitting = rail_at_default(self.rail_weights);
        let scroll_id = rail_ui.scope_id().with("rail-scroll");
        let extent_id = rail_ui.scope_id().with("rail-extent");
        let stored_extent = rail_ui
            .ctx()
            .data(|data| data.get_temp::<f32>(extent_id))
            .unwrap_or(0.0);
        let stored_scroll = rail_ui
            .ctx()
            .data(|data| data.get_temp::<f32>(scroll_id))
            .unwrap_or(0.0);
        let wheel = if rail_ui.rect_contains_pointer(rail_rect) {
            rail_ui.input(|input| input.smooth_scroll_delta().y)
        } else {
            0.0
        };
        let mut scroll = rail_scroll_to(stored_extent, inner.height(), stored_scroll, wheel);
        if (scroll - stored_scroll).abs() > 0.1 {
            rail_ui.ctx().request_repaint();
        }
        let mut content = rail_ui.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-content")
                .max_rect(rail_fit_body(inner, scroll))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        content.set_clip_rect(inner);
        let mut heights = if fitting {
            [0.0; RAIL_SECTIONS]
        } else {
            self.rail_weights
        };
        let mut drag = None;
        heights[0] = rail_section(&mut content, heights[0], fitting, "canvas", false, |ui| {
            self.canvas_panel(ui);
        });
        if let Some(delta) = rail_splitter(&mut content)
            && drag.is_none()
        {
            drag = Some((0, delta));
        }
        heights[1] = rail_section(&mut content, heights[1], fitting, "frame", false, |ui| {
            self.frame_panel(ui);
        });
        if let Some(delta) = rail_splitter(&mut content)
            && drag.is_none()
        {
            drag = Some((1, delta));
        }
        heights[2] = rail_section(&mut content, heights[2], fitting, "photo", false, |ui| {
            self.photo_panel(ui);
        });
        if let Some(delta) = rail_splitter(&mut content)
            && drag.is_none()
        {
            drag = Some((2, delta));
        }
        heights[3] = rail_section(&mut content, heights[3], fitting, "crop", false, |ui| {
            self.crop_panel(ui);
        });
        if let Some(delta) = rail_splitter(&mut content)
            && drag.is_none()
        {
            drag = Some((3, delta));
        }
        heights[4] = rail_section(&mut content, heights[4], fitting, "layers", false, |ui| {
            self.layer_panel(ui);
        });
        let measured = content.min_rect();
        let extent = if measured.max.y.is_finite() {
            (measured.max.y - (inner.top() - scroll)).max(0.0)
        } else {
            0.0
        };
        let gutter = egui::Rect::from_min_max(
            egui::pos2(inner.right(), inner.top()),
            egui::pos2(rail_rect.right(), inner.bottom()),
        );
        scroll = rail_bar(&mut rail_ui, gutter, inner.height(), extent, scroll);
        rail_ui.ctx().data_mut(|data| {
            data.insert_temp(scroll_id, scroll);
            data.insert_temp(extent_id, extent);
        });
        if let Some((index, delta)) = drag {
            self.rail_weights = apply_rail_drag(heights, index, delta, inner.height());
        }
    }
}
