use eframe::egui;
use pudduscroll_core::{
    CANVAS_PRESETS, ExportFormat, MAX_CANVASES, Turn, canvas_file_name, layout_export_width,
    quality_description,
};

use super::draw::{Glyph, dialog_frame, fitted_rect, icon_button, section_scroll, short_name};
use super::history::HistoryKind;
use super::{ACCENT, MAX_GRID_CELLS, MIN_GRID_CELLS, MUTED, PudduScrollApp, RAIL_W, Route, TEXT};

impl PudduScrollApp {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let kind = match self.route {
            Route::Template(_) => "Template",
            Route::Carousel(_) => "Carousel",
            Route::Home => "",
        };
        let name = self.item_name();
        ui.horizontal_centered(|ui| {
            let home = ui.button("Home");
            self.remember("home", home.rect);
            if home.clicked() {
                self.go_home();
            }
            ui.label(egui::RichText::new(kind).color(MUTED));
            ui.label(egui::RichText::new(short_name(&name)).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let export = ui.add_enabled(
                    !self.exporting && self.has_photo(),
                    egui::Button::new("Export"),
                );
                self.remember("export", export.rect);
                if export.clicked() {
                    self.export_open = true;
                    self.schedule_estimate();
                }
                if !self.status.is_empty() {
                    let text = egui::RichText::new(&self.status);
                    let text = if self.exporting {
                        text.strong().color(TEXT)
                    } else {
                        text.color(MUTED)
                    };
                    ui.label(text);
                }
            });
        });
    }

    pub(super) fn canvas_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Canvas");
        section_scroll(ui, "canvas-scroll").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for preset in CANVAS_PRESETS {
                    let label = preset.label();
                    let selected = self.layout().is_some_and(|layout| layout.ratio == preset);
                    let response = ui.selectable_label(selected, &label);
                    self.remember(format!("canvas-{label}"), response.rect);
                    if response.clicked()
                        && self.layout().is_some_and(|layout| layout.ratio != preset)
                    {
                        self.checkpoint();
                        if let Some(layout) = self.layout_mut() {
                            layout.set_ratio(preset);
                        }
                        self.note_geometry();
                    }
                }
            });
            if let Some(layout) = self.layout() {
                let (export_w, export_h) = layout.ratio.export_size(layout_export_width(layout));
                ui.label(
                    egui::RichText::new(format!("{export_w}×{export_h}"))
                        .color(MUTED)
                        .size(13.0),
                );
            }
            ui.horizontal(|ui| {
                let can_remove = self.layout().is_some_and(|layout| layout.canvas_count > 1);
                let remove = ui.add_enabled(can_remove, egui::Button::new("Remove"));
                self.remember("remove-canvas", remove.rect);
                if remove.clicked() && can_remove {
                    self.checkpoint();
                    if self
                        .layout_mut()
                        .is_some_and(pudduscroll_core::Layout::remove_canvas)
                    {
                        self.note_geometry();
                    }
                }
                if let Some(layout) = self.layout() {
                    ui.monospace(format!("{:02} / {MAX_CANVASES:02}", layout.canvas_count));
                }
                let can_add = self
                    .layout()
                    .is_some_and(|layout| layout.canvas_count < MAX_CANVASES);
                let add = ui.add_enabled(can_add, egui::Button::new("Add"));
                self.remember("add-canvas", add.rect);
                if add.clicked() && can_add {
                    self.checkpoint();
                    if self
                        .layout_mut()
                        .is_some_and(pudduscroll_core::Layout::add_canvas)
                    {
                        self.note_geometry();
                    }
                }
            });
        });
    }

    pub(super) fn frame_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Frame");
        section_scroll(ui, "frame-scroll").show(ui, |ui| {
            ui.horizontal(|ui| {
                let grid = ui.checkbox(&mut self.show_grid, "Grid");
                self.remember("grid", grid.rect);
                let snap = ui.checkbox(&mut self.snap_to_grid, "Snap");
                self.remember("snap", snap.rect);
            });
            ui.label("Squares");
            let previous = self.grid_cells();
            let mut cells = previous;
            let slider = ui.add(
                egui::Slider::new(&mut cells, MIN_GRID_CELLS..=MAX_GRID_CELLS).trailing_fill(true),
            );
            self.remember("grid-squares", slider.rect);
            if slider.changed() {
                self.edit_grid(previous, cells);
            }
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.persist();
            }
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let add_frame = ui.button("Add frame");
                self.remember("add-frame", add_frame.rect);
                if add_frame.clicked() && self.layout().is_some() {
                    self.checkpoint();
                    let id = self.layout_mut().map(pudduscroll_core::Layout::add_frame);
                    if let Some(id) = id {
                        self.selected = Some(id);
                        self.edit_frame = true;
                        self.note_geometry();
                    }
                }
                let edit = ui.add_enabled(
                    self.selected.is_some(),
                    egui::Button::new("Edit frame").selected(self.edit_frame),
                );
                self.remember("edit-frame", edit.rect);
                if edit.clicked() {
                    self.edit_frame = !self.edit_frame;
                }
                let remove_frame =
                    ui.add_enabled(self.selected.is_some(), egui::Button::new("Remove frame"));
                self.remember("remove-frame", remove_frame.rect);
                if remove_frame.clicked()
                    && let Some(id) = self.selected
                {
                    self.remove_frame(id);
                }
            });
            if self.edit_frame && self.selected.is_some() {
                ui.horizontal_wrapped(|ui| {
                    for preset in CANVAS_PRESETS {
                        let label = preset.label();
                        let response = ui.button(&label);
                        self.remember(format!("frame-aspect-{label}"), response.rect);
                        if response.clicked()
                            && let Some(id) = self.selected
                            && self.layout().is_some()
                        {
                            self.checkpoint();
                            if let Some(layout) = self.layout_mut() {
                                layout.set_frame_aspect(id, preset.value());
                            }
                            self.note_geometry();
                        }
                    }
                });
            }
        });
    }

    pub(super) fn photo_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Photo");
        section_scroll(ui, "photo-scroll").show(ui, |ui| {
            let photo_ready = self.selected.is_some_and(|id| {
                self.layout()
                    .and_then(|layout| layout.frame(id))
                    .is_some_and(|frame| frame.photo.is_some())
            });
            let border_edit = photo_ready
                .then_some(self.selected)
                .flatten()
                .and_then(|id| {
                    self.layout().and_then(|layout| {
                        layout
                            .frame(id)
                            .map(|frame| (id, frame.border, frame.border_color))
                    })
                });
            if let Some((id, mut border, color)) = border_edit {
                let mut rgb = [color[0], color[1], color[2]];
                ui.label("Border");
                ui.horizontal(|ui| {
                    ui.spacing_mut().slider_width = 96.0;
                    let slider =
                        ui.add(egui::Slider::new(&mut border, 0.0..=1.0).show_value(false));
                    self.remember("border", slider.rect);
                    let mut percent = (border * 100.0).round() as i32;
                    let number = ui.add_sized(
                        [48.0, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut percent).range(0..=100).speed(1.0),
                    );
                    self.remember("border-value", number.rect);
                    let number_changed = number.changed();
                    if number_changed {
                        border = (percent as f32 / 100.0).clamp(0.0, 1.0);
                    }
                    let popup_id = egui::Id::unique("pudduscroll-border-color");
                    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                    let (rect, response) =
                        ui.allocate_exact_size(ui.spacing().interact_size, egui::Sense::click());
                    if ui.is_rect_visible(rect) {
                        let visuals = if open {
                            &ui.visuals().widgets.open
                        } else {
                            ui.style().interact(&response)
                        };
                        let painted = rect.expand(visuals.expansion);
                        ui.painter().rect_filled(
                            painted.shrink(1.0),
                            visuals.corner_radius,
                            egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                        );
                        ui.painter().rect_stroke(
                            painted,
                            visuals.corner_radius,
                            egui::Stroke::new(1.0, visuals.bg_fill),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.remember("border-color", response.rect);
                    let mut swatch = response.interact_rect;
                    if let Some(to_global) = ui.ctx().layer_transform_to_global(response.layer_id) {
                        swatch = to_global * swatch;
                    }
                    let rail_left = ui.ctx().content_rect().right() - RAIL_W;
                    let anchor =
                        egui::Rect::from_x_y_ranges(rail_left..=rail_left, swatch.y_range());
                    let mut picked = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    let mut color_changed = false;
                    egui::Popup::menu(&response)
                        .id(popup_id)
                        .anchor(anchor)
                        .align(egui::RectAlign::LEFT_START)
                        .align_alternatives(&[egui::RectAlign::LEFT, egui::RectAlign::LEFT_END])
                        .gap(8.0)
                        .width(275.0)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.spacing_mut().slider_width = 275.0;
                            color_changed = egui::color_picker::color_picker_color32(
                                ui,
                                &mut picked,
                                egui::color_picker::Alpha::Opaque,
                            );
                        });
                    if color_changed {
                        rgb = [picked.r(), picked.g(), picked.b()];
                    }
                    if slider.changed() || color_changed || number_changed {
                        self.edit_coalesced(HistoryKind::Border(id.0));
                        if let Some(layout) = self.layout_mut() {
                            layout.set_border(id, border);
                            layout.set_border_color(id, [rgb[0], rgb[1], rgb[2], 255]);
                        }
                        self.note_geometry();
                    }
                });
            } else {
                egui::Popup::close_id(ui.ctx(), egui::Id::unique("pudduscroll-border-color"));
            }
            ui.horizontal(|ui| {
                let ccw = icon_square(ui, "rotate-ccw", photo_ready, false, Glyph::RotateCcw)
                    .on_hover_text("CCW 90");
                self.remember("rotate-ccw", ccw.rect);
                if ccw.clicked()
                    && let Some(id) = self.selected
                    && self.layout().is_some_and(|layout| {
                        layout.frame(id).is_some_and(|frame| frame.photo.is_some())
                    })
                {
                    self.checkpoint();
                    if let Some(layout) = self.layout_mut() {
                        layout.turn_photo(id, Turn::Counterclockwise);
                    }
                    self.note_geometry();
                }
                let cw = icon_square(ui, "rotate-cw", photo_ready, false, Glyph::RotateCw)
                    .on_hover_text("CW 90");
                self.remember("rotate-cw", cw.rect);
                if cw.clicked()
                    && let Some(id) = self.selected
                    && self.layout().is_some_and(|layout| {
                        layout.frame(id).is_some_and(|frame| frame.photo.is_some())
                    })
                {
                    self.checkpoint();
                    if let Some(layout) = self.layout_mut() {
                        layout.turn_photo(id, Turn::Clockwise);
                    }
                    self.note_geometry();
                }
                let free = icon_square(
                    ui,
                    "rotate-free",
                    photo_ready,
                    self.free_rotate && photo_ready,
                    Glyph::RotateFree,
                )
                .on_hover_text("Free");
                self.remember("rotate-free", free.rect);
                if free.clicked() {
                    self.free_rotate = !self.free_rotate;
                    if self.free_rotate {
                        self.status = "Drag the handle to rotate".to_string();
                    }
                }
                let remove_photo = icon_square(
                    ui,
                    "remove-photo",
                    photo_ready && matches!(self.route, Route::Carousel(_)),
                    false,
                    Glyph::Bin,
                )
                .on_hover_text("Remove photo");
                self.remember("remove-photo", remove_photo.rect);
                if remove_photo.clicked()
                    && let Some(id) = self.selected
                {
                    self.remove_photo(id);
                }
            });
        });
    }

    pub(super) fn export_modal(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let view = ctx.content_rect().size();
        let modal_w = (view.x * 0.70).clamp(640.0, 1280.0);
        let image_h = (view.y * 0.55).clamp(220.0, (view.y - 340.0).max(220.0));
        let modal_id = egui::Id::unique("export-options");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(modal_w);
                ui.heading("Export");
                ui.add_space(4.0);
                let (area, _) =
                    ui.allocate_exact_size(egui::vec2(modal_w, image_h), egui::Sense::hover());
                self.preview_nav(ui);
                self.paint_preview(ui, area);
                ui.add_space(8.0);
                if let Some(layout) = self.layout() {
                    let (width, height) = layout.ratio.export_size(layout_export_width(layout));
                    let sample = canvas_file_name(&self.item_name(), 0, self.format);
                    ui.label(format!(
                        "{} canvases · {width}×{height}",
                        layout.canvas_count
                    ));
                    ui.label(egui::RichText::new(format!("Files {sample} and on")).color(MUTED));
                }
                ui.add_space(8.0);
                ui.label("Format");
                ui.horizontal(|ui| {
                    let jpeg = ui.selectable_label(self.format == ExportFormat::Jpeg, "JPEG");
                    if jpeg.clicked() && self.format != ExportFormat::Jpeg {
                        self.format = ExportFormat::Jpeg;
                        self.schedule_estimate();
                    }
                    let png = ui.selectable_label(self.format == ExportFormat::Png, "PNG");
                    if png.clicked() && self.format != ExportFormat::Png {
                        self.format = ExportFormat::Png;
                        self.schedule_estimate();
                    }
                });
                ui.add_space(8.0);
                if self.format == ExportFormat::Jpeg {
                    ui.label("Quality");
                    let slider = ui.add(
                        egui::Slider::new(&mut self.quality, 0..=100)
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    if slider.changed() {
                        self.schedule_estimate();
                    }
                    ui.label(quality_description(self.quality, self.format));
                    if self.quality >= 100 {
                        ui.label(egui::RichText::new("Highest JPEG quality.").color(MUTED));
                    } else {
                        ui.label(
                            egui::RichText::new("Stronger compression toward the left.")
                                .color(MUTED),
                        );
                    }
                } else {
                    ui.label(quality_description(self.quality, self.format));
                    ui.label(
                        egui::RichText::new("No extra loss beyond the source. PNG.").color(MUTED),
                    );
                }
                ui.add_space(4.0);
                if self.estimate_for == Some((self.geometry_revision, self.quality, self.format)) {
                    if let Some(mb) = self.estimate_mb {
                        ui.label(
                            egui::RichText::new(format!("About {mb:.2} MB for the carousel"))
                                .strong(),
                        );
                    }
                } else {
                    let label = if self.estimate_label.is_empty() {
                        "Estimating size…".to_string()
                    } else {
                        self.estimate_label.clone()
                    };
                    ui.label(egui::RichText::new(label).color(MUTED));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.export_open = false;
                    }
                    let confirm = ui.add_enabled(
                        !self.exporting,
                        egui::Button::new(
                            egui::RichText::new("Choose folder").color(egui::Color32::BLACK),
                        )
                        .fill(ACCENT),
                    );
                    if confirm.clicked() {
                        self.export_open = false;
                        self.start_export();
                    }
                });
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if response.should_close() {
            self.export_open = false;
        }
    }

    pub(super) fn overwrite_modal(&mut self, ctx: &egui::Context) {
        if self.overwrite_dir.is_none() {
            return;
        }
        let modal_id = egui::Id::unique("overwrite-export");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("Overwrite files?");
                ui.add_space(6.0);
                let line = if self.overwrite_names.len() == 1 {
                    "This file is already in the folder."
                } else {
                    "These files are already in the folder."
                };
                ui.label(line);
                ui.add_space(6.0);
                let height = (self.overwrite_names.len() as f32 * 22.0).clamp(22.0, 176.0);
                section_scroll(ui, "overwrite-files")
                    .max_height(height)
                    .show(ui, |ui| {
                        for name in &self.overwrite_names {
                            ui.label(egui::RichText::new(short_name(name)).color(MUTED));
                        }
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let cancel = ui.button("Cancel");
                    self.remember("cancel-overwrite", cancel.rect);
                    if cancel.clicked() {
                        self.cancel_overwrite();
                    }
                    let overwrite = ui.add(
                        egui::Button::new(
                            egui::RichText::new("Overwrite").color(egui::Color32::BLACK),
                        )
                        .fill(ACCENT),
                    );
                    self.remember("confirm-overwrite", overwrite.rect);
                    if overwrite.clicked() {
                        self.confirm_overwrite();
                    }
                });
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if response.should_close() {
            self.cancel_overwrite();
        }
    }

    fn preview_nav(&mut self, ui: &mut egui::Ui) {
        let count = self.metrics().map_or(1, |(_, count)| count);
        ui.horizontal(|ui| {
            let button = 34.0;
            let label_w = 64.0;
            let gap = ui.spacing().item_spacing.x;
            let cluster = button + gap + label_w + gap + button;
            let spare = ((ui.available_width() - cluster) * 0.5).max(0.0);
            ui.add_space(spare);
            let (left_rect, _) =
                ui.allocate_exact_size(egui::vec2(button, button), egui::Sense::hover());
            let mut index = self.preview_index;
            if icon_button(ui, left_rect, "preview-prev", index > 0, false, Glyph::Left).clicked()
                && index > 0
            {
                index -= 1;
            }
            let (label_rect, _) =
                ui.allocate_exact_size(egui::vec2(label_w, button), egui::Sense::hover());
            ui.painter().text(
                label_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}/{}", index + 1, count),
                egui::FontId::proportional(16.0),
                TEXT,
            );
            let (right_rect, _) =
                ui.allocate_exact_size(egui::vec2(button, button), egui::Sense::hover());
            if icon_button(
                ui,
                right_rect,
                "preview-next",
                index + 1 < count,
                false,
                Glyph::Right,
            )
            .clicked()
                && index + 1 < count
            {
                index += 1;
            }
            self.preview_index = index;
        });
    }

    fn paint_preview(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        self.ensure_preview(ui.ctx());
        let ratio = self.metrics().map_or(0.8, |(ratio, _)| ratio);
        let fitted = fitted_rect(area, ratio, 1.0);
        let painter = ui.painter();
        painter.rect_filled(fitted, 0.0, egui::Color32::WHITE);
        if let Some(texture) = &self.preview_texture {
            painter.image(
                texture.id(),
                fitted,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

fn icon_square(
    ui: &mut egui::Ui,
    salt: &str,
    enabled: bool,
    selected: bool,
    glyph: Glyph,
) -> egui::Response {
    let side = 30.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    icon_button(ui, rect, salt, enabled, selected, glyph)
}
