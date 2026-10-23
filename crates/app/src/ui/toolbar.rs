use eframe::egui;
use pudduscroll_core::{
    CANVAS_PRESETS, MAX_CANVASES, canvas_file_name, layout_export_width, quality_description,
};

use super::draw::{dialog_frame, short_name};
use super::{ACCENT, MUTED, PudduScrollApp, Route};

impl PudduScrollApp {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let kind = match self.route {
            Route::Template(_) => "Template",
            Route::Carousel(_) => "Carousel",
            Route::Home => "",
        };
        let name = self.item_name();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let home = ui.button("Home");
                self.remember("home", home.rect);
                if home.clicked() {
                    self.go_home();
                }
                ui.label(egui::RichText::new(kind).color(MUTED));
                ui.label(egui::RichText::new(short_name(&name)).strong());
                ui.separator();
                for preset in CANVAS_PRESETS {
                    let selected = self.layout().is_some_and(|layout| layout.ratio == preset);
                    let response = ui.selectable_label(selected, preset.label());
                    self.remember(format!("canvas-{}", preset.label()), response.rect);
                    if response.clicked() {
                        if let Some(layout) = self.layout_mut() {
                            layout.set_ratio(preset);
                        }
                        self.note_geometry();
                    }
                }
                if let Some(layout) = self.layout() {
                    let (export_w, export_h) =
                        layout.ratio.export_size(layout_export_width(layout));
                    ui.label(
                        egui::RichText::new(format!("{export_w}×{export_h}"))
                            .color(MUTED)
                            .size(13.0),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Canvases");
                let can_remove = self.layout().is_some_and(|layout| layout.canvas_count > 1);
                let remove = ui.add_enabled(can_remove, egui::Button::new("Remove"));
                self.remember("remove-canvas", remove.rect);
                if remove.clicked()
                    && self
                        .layout_mut()
                        .is_some_and(pudduscroll_core::Layout::remove_canvas)
                {
                    self.note_geometry();
                }
                if let Some(layout) = self.layout() {
                    ui.monospace(format!("{:02} / {MAX_CANVASES:02}", layout.canvas_count));
                }
                let can_add = self
                    .layout()
                    .is_some_and(|layout| layout.canvas_count < MAX_CANVASES);
                let add = ui.add_enabled(can_add, egui::Button::new("Add"));
                self.remember("add-canvas", add.rect);
                if add.clicked()
                    && self
                        .layout_mut()
                        .is_some_and(pudduscroll_core::Layout::add_canvas)
                {
                    self.note_geometry();
                }
                ui.separator();
                let grid = ui.checkbox(&mut self.show_grid, "Grid");
                self.remember("grid", grid.rect);
                let snap = ui.checkbox(&mut self.snap_to_grid, "Snap");
                self.remember("snap", snap.rect);
                ui.separator();
                let add_frame = ui.button("Add frame");
                self.remember("add-frame", add_frame.rect);
                if add_frame.clicked() {
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
                if self.edit_frame && self.selected.is_some() {
                    for preset in CANVAS_PRESETS {
                        let response = ui.button(preset.label());
                        self.remember(format!("frame-aspect-{}", preset.label()), response.rect);
                        if response.clicked() {
                            let id = self.selected;
                            if let Some(id) = id
                                && let Some(layout) = self.layout_mut()
                            {
                                layout.set_frame_aspect(id, preset.value());
                            }
                            self.note_geometry();
                        }
                    }
                }
                let remove_frame =
                    ui.add_enabled(self.selected.is_some(), egui::Button::new("Remove frame"));
                self.remember("remove-frame", remove_frame.rect);
                if remove_frame.clicked()
                    && let Some(id) = self.selected
                {
                    self.remove_frame(id);
                }
                let photo_ready = self.selected.is_some_and(|id| {
                    self.layout()
                        .and_then(|layout| layout.frame(id))
                        .is_some_and(|frame| frame.photo.is_some())
                });
                let remove_photo = ui.add_enabled(
                    photo_ready && matches!(self.route, Route::Carousel(_)),
                    egui::Button::new("Remove photo"),
                );
                self.remember("remove-photo", remove_photo.rect);
                if remove_photo.clicked()
                    && let Some(id) = self.selected
                {
                    self.remove_photo(id);
                }
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
                    ui.label(egui::RichText::new(&self.status).color(MUTED));
                }
            });
        });
    }

    pub(super) fn export_modal(&mut self, ctx: &egui::Context) {
        if !self.export_open {
            return;
        }
        let modal_id = egui::Id::unique("export-options");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("Export");
                ui.add_space(4.0);
                if let Some(layout) = self.layout() {
                    let (width, height) = layout.ratio.export_size(layout_export_width(layout));
                    let sample = canvas_file_name(0, self.quality);
                    ui.label(format!(
                        "{} canvases · {width}×{height}",
                        layout.canvas_count
                    ));
                    ui.label(egui::RichText::new(format!("Files {sample} and on")).color(MUTED));
                }
                ui.add_space(8.0);
                ui.label("Quality");
                let slider = ui.add(
                    egui::Slider::new(&mut self.quality, 0..=100)
                        .show_value(false)
                        .trailing_fill(true),
                );
                if slider.changed() {
                    self.schedule_estimate();
                }
                ui.label(quality_description(self.quality));
                if self.quality >= 100 {
                    ui.label(
                        egui::RichText::new("No extra loss beyond the source. PNG.").color(MUTED),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("Stronger compression toward the left.").color(MUTED),
                    );
                }
                ui.add_space(4.0);
                if self.estimate_for == Some((self.geometry_revision, self.quality)) {
                    if let Some(mb) = self.estimate_mb {
                        ui.label(
                            egui::RichText::new(format!("About {mb:.2} MB for the carousel"))
                                .strong(),
                        );
                    }
                } else {
                    ui.label(egui::RichText::new("Estimating size…").color(MUTED));
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
}
