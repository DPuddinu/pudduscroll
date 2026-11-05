use eframe::egui;
use pudduscroll_core::{CROP_PRESETS, Crop, FrameId, LayerMove, render_layout};

const EXPORT_PREVIEW_WIDTH: u32 = 1280;
const TOOL_COUNT: usize = 4;
const TOOL_COLS: usize = 3;
const TOOL_CELL_H: f32 = 64.0;

fn tool_rows(count: usize) -> usize {
    count.div_ceil(TOOL_COLS)
}
const TOOL_MIN_H: f32 = 72.0;
const LAYERS_MIN_H: f32 = 200.0;

use super::draw::{
    Glyph, apply_rail_drag, counted, icon_button, paint_glyph, rail_pane_heights, rail_splitter,
    rail_weights_from, section_scroll, short_name,
};
use super::{
    ACCENT, COVER_EMPTY, HAIRLINE, IncomingPhoto, MUTED, PANEL, PudduScrollApp, RailTool, Route,
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
                    if let Some(index) = self.canvas_under_frame(id) {
                        self.active_canvas = Some(index);
                    }
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
                    for (slot, (glyph, movement, enabled)) in [
                        (Glyph::Up, LayerMove::Up, index > 0),
                        (Glyph::Down, LayerMove::Down, index + 1 < count),
                        (Glyph::Top, LayerMove::Top, index > 0),
                        (Glyph::Bottom, LayerMove::Bottom, index + 1 < count),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(
                                origin_x + slot as f32 * (button + gap),
                                row.center().y - button * 0.5,
                            ),
                            egui::vec2(button, button),
                        );
                        if icon_button(ui, rect, ("layer-move", id.0, slot), enabled, false, glyph)
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
                    if preset_response.clicked() {
                        self.open_crop_session(id, preset);
                        ui.ctx().request_repaint();
                    }
                }
            });
        });
        ui.add_space(8.0);
    }

    pub(super) fn snapshot_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.0);
        ui.heading("Snapshots");
        let Route::Carousel(carousel_id) = self.route else {
            ui.label("Open a carousel to use snapshots.");
            return;
        };
        let create = ui.button("New snapshot");
        self.remember("new-snapshot", create.rect);
        if create.clicked() {
            self.open_snapshot_modal();
        }
        ui.add_space(6.0);
        let rows: Vec<(u64, String)> = self
            .library
            .carousels
            .iter()
            .find(|carousel| carousel.id == carousel_id)
            .map(|carousel| {
                carousel
                    .snapshots
                    .iter()
                    .map(|snapshot| (snapshot.id, snapshot.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        ui.label(egui::RichText::new(counted(rows.len(), "snapshot", "snapshots")).color(MUTED));
        if rows.is_empty() {
            ui.label("No snapshots yet.");
            return;
        }
        section_scroll(ui, "snapshot-list").show(ui, |ui| {
            for (id, name) in rows {
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 40.0),
                    egui::Sense::click(),
                );
                if response.clicked() {
                    self.apply_named_snapshot(id, ctx);
                }
                response
                    .clone()
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                ui.painter().text(
                    row.min + egui::vec2(8.0, 11.0),
                    egui::Align2::LEFT_TOP,
                    short_name(&name),
                    egui::FontId::proportional(14.0),
                    TEXT,
                );
                let pointer_on_row = ui.input(|input| {
                    input
                        .pointer
                        .latest_pos()
                        .is_some_and(|pos| row.contains(pos))
                });
                if ui.rect_contains_pointer(row) || pointer_on_row {
                    let button = 22.0;
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(row.right() - 4.0 - button, row.center().y - button * 0.5),
                        egui::vec2(button, button),
                    );
                    if icon_button(ui, rect, ("snap-del", id), true, false, Glyph::Bin).clicked() {
                        self.confirm = Some(super::Confirm::DeleteSnapshot(id));
                    }
                }
            }
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
        let ctx = rail_ui.ctx().clone();
        let mut content = rail_ui.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-content")
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        content.set_clip_rect(inner);

        let tiles_h = tool_rows(TOOL_COUNT) as f32 * TOOL_CELL_H;
        let tiles_rect = egui::Rect::from_min_size(
            content.cursor().left_top(),
            egui::vec2(inner.width(), tiles_h),
        );
        self.paint_tool_tiles(&mut content, tiles_rect);
        content.allocate_rect(tiles_rect, egui::Sense::hover());
        content.painter().hline(
            tiles_rect.x_range(),
            tiles_rect.bottom(),
            egui::Stroke::new(1.0, HAIRLINE),
        );

        let mins = [TOOL_MIN_H, LAYERS_MIN_H];
        let body = (inner.height() - tiles_h).max(0.0);
        let mut heights = rail_pane_heights(body, self.rail_weights, mins);
        let mut drag = None;

        let tool_rect = egui::Rect::from_min_size(
            content.cursor().left_top(),
            egui::vec2(inner.width(), heights[0]),
        );
        let mut tool_ui = content.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-tool-body")
                .max_rect(tool_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        tool_ui.set_clip_rect(tool_rect);
        egui::ScrollArea::vertical()
            .id_salt("rail-tool-scroll")
            .show(&mut tool_ui, |ui| match self.rail_tool {
                RailTool::Canvas => self.canvas_panel(ui),
                RailTool::Photo => self.photo_panel(ui),
                RailTool::Crop => self.crop_panel(ui),
                RailTool::Snapshots => self.snapshot_panel(ui, &ctx),
            });
        content.allocate_rect(tool_rect, egui::Sense::hover());

        let (delta, stopped) = rail_splitter(&mut content);
        if let Some(delta) = delta {
            drag = Some(delta);
        }

        let layers_rect = egui::Rect::from_min_size(
            content.cursor().left_top(),
            egui::vec2(inner.width(), heights[1]),
        );
        let mut layers_ui = content.new_child(
            egui::UiBuilder::new()
                .id_salt("rail-layers-body")
                .max_rect(layers_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        layers_ui.set_clip_rect(layers_rect);
        self.layer_panel(&mut layers_ui);
        content.allocate_rect(layers_rect, egui::Sense::hover());

        if let Some(delta) = drag {
            heights = apply_rail_drag(heights, 0, delta, mins);
            self.rail_weights = rail_weights_from(heights);
        }
        if stopped {
            self.settings.rail = self.rail_weights;
            if self.can_store {
                self.save_settings();
            }
        }
    }

    fn paint_tool_tiles(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let tools = [
            (RailTool::Canvas, "canvas", Glyph::ToolFrame),
            (RailTool::Photo, "photo", Glyph::ToolPhoto),
            (RailTool::Crop, "crop", Glyph::ToolCrop),
            (RailTool::Snapshots, "snapshots", Glyph::ToolSnapshot),
        ];
        let rows = tool_rows(tools.len());
        let grid_h = (rows as f32 * TOOL_CELL_H).min(rect.height());
        let grid_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), grid_h));
        let cell_w = grid_rect.width() / TOOL_COLS as f32;
        for (index, (tool, label, glyph)) in tools.into_iter().enumerate() {
            let col = index % TOOL_COLS;
            let row = index / TOOL_COLS;
            let cell = egui::Rect::from_min_size(
                egui::pos2(
                    grid_rect.left() + col as f32 * cell_w,
                    grid_rect.top() + row as f32 * TOOL_CELL_H,
                ),
                egui::vec2(cell_w, TOOL_CELL_H),
            );
            if cell.bottom() > rect.bottom() + 0.5 {
                continue;
            }
            let selected = self.rail_tool == tool;
            let response = ui.interact(
                cell,
                ui.scope_id().with(("rail-tool", label)),
                egui::Sense::click(),
            );
            if response.clicked() {
                self.rail_tool = tool;
            }
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if selected {
                ui.painter().rect_filled(cell.shrink(2.0), 6.0, SELECT_BG);
            }
            let icon = egui::Rect::from_center_size(
                egui::pos2(cell.center().x, cell.top() + 22.0),
                egui::vec2(22.0, 22.0),
            );
            paint_glyph(
                ui.painter(),
                icon,
                glyph,
                if selected { ACCENT } else { TEXT },
            );
            ui.painter().text(
                egui::pos2(cell.center().x, cell.bottom() - 14.0),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(13.0),
                if selected { ACCENT } else { MUTED },
            );
            self.remember(format!("rail-tool-{label}"), cell);
        }
    }
}
