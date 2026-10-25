use std::sync::Arc;
use std::thread;

use eframe::egui;
use pudduscroll_core::{AspectRatio, CANVAS_PRESETS, FrameImages};

use super::card::{self, CARD_INNER_W, Card};
use super::draw::{
    carousel_preview_image, counted, dialog_frame, fitted_rect, pick_image_path, short_name,
};
use super::{
    ACCENT, BG, CANVAS_BG, DecodeTarget, HomeAction, IncomingPhoto, MUTED, PudduScrollApp, Route,
};

impl PudduScrollApp {
    pub(super) fn home_screen(
        &mut self,
        ui: &mut egui::Ui,
        full: egui::Rect,
    ) -> Option<HomeAction> {
        ui.painter().rect_filled(full, 0.0, BG);
        let inner = full.shrink(16.0);
        let mut home = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("home")
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        home.horizontal(|ui| {
            ui.label(egui::RichText::new("PudduScroll").strong().size(22.0));
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(&self.status).color(MUTED));
            }
        });
        home.add_space(8.0);
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("home-scroll")
            .auto_shrink([false, false])
            .show(&mut home, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Templates");
                    let response = ui.button("New template");
                    self.remember("new-template", response.rect);
                    if response.clicked() {
                        self.create_name = "Template".to_string();
                        self.create_ratio = AspectRatio::FOUR_FIVE;
                        self.create_cover = None;
                        self.create_cover_texture = None;
                        self.cover_load = None;
                        self.create_open = true;
                    }
                });
                ui.add_space(8.0);
                if self.library.templates.is_empty() {
                    ui.label(egui::RichText::new("No templates yet.").color(MUTED));
                }
                let template_count = self.library.templates.len();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    for index in 0..template_count {
                        if let Some(next) = self.template_card(ui, index) {
                            action = Some(next);
                        }
                    }
                });
                ui.add_space(18.0);
                ui.heading("Carousels");
                ui.add_space(8.0);
                if self.library.carousels.is_empty() {
                    ui.label(egui::RichText::new("No carousels yet.").color(MUTED));
                }
                let carousel_count = self.library.carousels.len();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    for index in 0..carousel_count {
                        if let Some(next) = self.carousel_card(ui, index) {
                            action = Some(next);
                        }
                    }
                });
            });
        if self.name_dirty {
            self.name_dirty = false;
            self.persist();
        }
        action
    }

    pub(super) fn ensure_cover_texture(&mut self, ctx: &egui::Context, index: usize) {
        let id = self.library.templates[index].id;
        if self.cover_textures.contains_key(&id) || self.cover_pending.contains_key(&id) {
            return;
        }
        let proxy = Arc::clone(&self.library.templates[index].cover.proxy);
        self.queue_cover(id, proxy);
        ctx.request_repaint();
    }

    pub(super) fn template_card(&mut self, ui: &mut egui::Ui, index: usize) -> Option<HomeAction> {
        let id = self.library.templates[index].id;
        let name = self.library.templates[index].name.clone();
        let canvases = self.library.templates[index].layout.canvas_count;
        let frames = self.library.templates[index].layout.frames.len();
        self.ensure_cover_texture(ui.ctx(), index);
        let image = self.cover_textures.get(&id).map(card::image_stamp);
        let loading = self.cover_pending.contains_key(&id);
        let mut action = None;
        let shown = Card {
            kind: "template",
            id,
            image,
            fill: egui::Color32::from_rgb(36, 36, 36),
            loading,
        }
        .show(ui, |content, buttons| {
            content.label(egui::RichText::new(short_name(&name)).strong().size(16.0));
            content.label(
                egui::RichText::new(format!(
                    "{} · {}",
                    counted(canvases, "canvas", "canvases"),
                    counted(frames, "frame", "frames")
                ))
                .color(MUTED),
            );
            buttons.horizontal(|ui| {
                let edit = ui.button("Edit");
                self.remember(format!("template-edit-{id}"), edit.rect);
                if edit.clicked() {
                    action = Some(HomeAction::EditTemplate(id));
                }
                let delete = ui.button("Delete");
                self.remember(format!("template-delete-{id}"), delete.rect);
                if delete.clicked() {
                    action = Some(HomeAction::DeleteTemplate(id));
                }
            });
        });
        self.remember(format!("template-open-{id}"), shown.cover);
        self.remember(format!("template-card-{id}"), shown.rect);
        if shown.opened {
            action = Some(HomeAction::OpenTemplate(id));
        }
        action
    }

    pub(super) fn carousel_card(&mut self, ui: &mut egui::Ui, index: usize) -> Option<HomeAction> {
        let id = self.library.carousels[index].id;
        self.ensure_carousel_preview(ui.ctx(), index);
        let canvases = self.library.carousels[index].layout.canvas_count;
        let frames = self.library.carousels[index].layout.frames.len();
        let image = self
            .card_previews
            .get(&id)
            .map(|(_, texture)| card::image_stamp(texture));
        let loading = image.is_none() && self.card_jobs.contains_key(&id);
        let mut action = None;
        let shown = Card {
            kind: "carousel",
            id,
            image,
            fill: CANVAS_BG,
            loading,
        }
        .show(ui, |content, buttons| {
            let (changed, name_rect) = {
                let carousel = &mut self.library.carousels[index];
                let response = content.add(
                    egui::TextEdit::singleline(&mut carousel.name)
                        .id_salt(("carousel-name", id))
                        .desired_width(CARD_INNER_W),
                );
                (response.changed(), response.rect)
            };
            self.remember(format!("carousel-name-{id}"), name_rect);
            if changed {
                self.name_dirty = true;
            }
            content.label(
                egui::RichText::new(format!(
                    "{} · {}",
                    counted(canvases, "canvas", "canvases"),
                    counted(frames, "frame", "frames")
                ))
                .color(MUTED),
            );
            buttons.horizontal(|ui| {
                let duplicate = ui.button("Duplicate");
                self.remember(format!("carousel-duplicate-{id}"), duplicate.rect);
                if duplicate.clicked() {
                    action = Some(HomeAction::DuplicateCarousel(id));
                }
                let delete = ui.button("Delete");
                self.remember(format!("carousel-delete-{id}"), delete.rect);
                if delete.clicked() {
                    action = Some(HomeAction::DeleteCarousel(id));
                }
            });
        });
        self.remember(format!("carousel-open-{id}"), shown.cover);
        self.remember(format!("carousel-card-{id}"), shown.rect);
        if shown.opened {
            action = Some(HomeAction::OpenCarousel(id));
        }
        action
    }

    pub(super) fn ensure_carousel_preview(&mut self, ctx: &egui::Context, index: usize) {
        let id = self.library.carousels[index].id;
        let visuals = self.library.visuals;
        if self
            .card_previews
            .get(&id)
            .is_some_and(|(stamp, _)| *stamp == visuals)
            || self.card_jobs.get(&id) == Some(&visuals)
        {
            return;
        }
        let carousel = &self.library.carousels[index];
        let layout = carousel.layout.clone();
        let images: FrameImages = carousel
            .photos
            .iter()
            .map(|(frame_id, blob)| (*frame_id, Arc::clone(&blob.proxy)))
            .collect();
        self.card_jobs.insert(id, visuals);
        let tx = self.photo_tx.clone();
        thread::spawn(move || {
            let image = carousel_preview_image(&layout, &images);
            let _ = tx.send(IncomingPhoto::CardPreview {
                carousel_id: id,
                visuals,
                image,
            });
        });
        ctx.request_repaint();
    }

    pub(super) fn apply_home(&mut self, action: &HomeAction, ctx: &egui::Context) {
        match *action {
            HomeAction::OpenTemplate(id) => {
                if let Some(carousel_id) = self.library.instantiate(id) {
                    self.note_geometry();
                    self.enter(Route::Carousel(carousel_id), ctx);
                }
            }
            HomeAction::EditTemplate(id) => self.enter(Route::Template(id), ctx),
            HomeAction::DeleteTemplate(id) => {
                self.library.delete_template(id);
                self.cover_textures.remove(&id);
                self.cover_pending.remove(&id);
                self.note_geometry();
            }
            HomeAction::OpenCarousel(id) => self.enter(Route::Carousel(id), ctx),
            HomeAction::DuplicateCarousel(id) => {
                self.library.duplicate_carousel(id);
                self.note_geometry();
            }
            HomeAction::DeleteCarousel(id) => {
                self.library.delete_carousel(id);
                self.card_previews.remove(&id);
                self.card_jobs.remove(&id);
                self.note_geometry();
            }
        }
    }

    pub(super) fn create_template(&mut self, ctx: &egui::Context) {
        let name = self.create_name.trim().to_string();
        if name.is_empty() {
            self.status = "Name the template".to_string();
            return;
        }
        let Some(cover) = self.create_cover.take() else {
            self.status = "Choose a cover".to_string();
            return;
        };
        let ratio = self.create_ratio;
        let id = self.library.create_template(name, ratio, cover);
        self.create_cover_texture = None;
        self.create_open = false;
        self.note_geometry();
        self.enter(Route::Template(id), ctx);
        self.status = String::new();
    }

    pub(super) fn create_modal(&mut self, ctx: &egui::Context) {
        if !self.create_open {
            return;
        }
        let modal_id = egui::Id::unique("create-template");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("New template");
                ui.add_space(6.0);
                ui.label("Name");
                let name = ui.add(
                    egui::TextEdit::singleline(&mut self.create_name)
                        .id_salt("template-name")
                        .desired_width(360.0),
                );
                self.remember("template-name", name.rect);
                ui.add_space(6.0);
                ui.label("Cover");
                ui.horizontal(|ui| {
                    let choose = ui.button("Choose cover");
                    self.remember("choose-cover", choose.rect);
                    if choose.clicked()
                        && let Some(path) = pick_image_path("Choose cover")
                    {
                        self.spawn_decode(ctx, path, DecodeTarget::Cover);
                    }
                    if let Some(cover) = &self.create_cover {
                        ui.label(egui::RichText::new(short_name(&cover.name)).color(MUTED));
                    }
                });
                if self.cover_load.is_some() {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0).color(ACCENT));
                        ui.label("Loading");
                    });
                } else if let Some(texture) = &self.create_cover_texture {
                    let size = texture.size();
                    let bounds =
                        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(120.0, 90.0));
                    let fitted = fitted_rect(bounds, size[0] as f32, size[1] as f32);
                    ui.allocate_exact_size(egui::vec2(120.0, 90.0), egui::Sense::hover());
                    ui.painter().image(
                        texture.id(),
                        fitted,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                ui.add_space(6.0);
                ui.label("Canvas aspect");
                ui.horizontal(|ui| {
                    for preset in CANVAS_PRESETS {
                        let response =
                            ui.selectable_label(self.create_ratio == preset, preset.label());
                        self.remember(format!("aspect-{}", preset.label()), response.rect);
                        if response.clicked() {
                            self.create_ratio = preset;
                        }
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let cancel = ui.button("Cancel");
                    self.remember("cancel-template", cancel.rect);
                    if cancel.clicked() {
                        self.create_open = false;
                        self.create_cover = None;
                        self.create_cover_texture = None;
                        self.cover_load = None;
                    }
                    let ready = self.cover_load.is_none()
                        && self.create_cover.is_some()
                        && !self.create_name.trim().is_empty();
                    let create = ui.add_enabled(
                        ready,
                        egui::Button::new(
                            egui::RichText::new("Create template").color(egui::Color32::BLACK),
                        )
                        .fill(ACCENT),
                    );
                    self.remember("create-template", create.rect);
                    if create.clicked() {
                        self.create_template(ctx);
                    }
                });
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if response.should_close() {
            self.create_open = false;
            self.cover_load = None;
        }
    }
}
