use eframe::egui;
use pudduscroll_core::{AspectRatio, CANVAS_PRESETS};

use super::card::{self, CARD_INNER_W, Card};
use super::draw::{counted, dialog_frame, fitted_rect, section_scroll, short_name};
use super::{
    ACCENT, BG, CANVAS_BG, Confirm, DecodeTarget, HomeAction, MUTED, PudduScrollApp, Route,
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
        let mut action = None;
        home.horizontal(|ui| {
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
        home.add_space(8.0);
        if self.library.templates.is_empty() {
            home.label(egui::RichText::new("No templates yet.").color(MUTED));
        }
        let template_count = self.library.templates.len();
        home.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
            for index in 0..template_count {
                if let Some(next) = self.template_card(ui, index) {
                    action = Some(next);
                }
            }
        });
        home.add_space(18.0);
        home.horizontal(|ui| {
            ui.heading("Carousels");
            let import = ui.add_enabled(!self.importing, egui::Button::new("Import"));
            self.remember("import-carousel", import.rect);
            if import.clicked() {
                self.import_package();
            }
        });
        home.add_space(8.0);
        let carousel_count = self.library.carousels.len();
        section_scroll(&mut home, "carousels").show(&mut home, |ui| {
            if carousel_count == 0 {
                ui.label(egui::RichText::new("No carousels yet.").color(MUTED));
                return;
            }
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
        let proxy = self.library.templates[index].cover.screen();
        self.queue_cover(id, proxy);
        ctx.request_repaint();
    }

    pub(super) fn ensure_carousel_cover(&mut self, ctx: &egui::Context, index: usize) {
        let id = self.library.carousels[index].id;
        if self.cover_textures.contains_key(&id) || self.cover_pending.contains_key(&id) {
            return;
        }
        let Some(cover) = self.library.carousels[index].cover.as_ref() else {
            return;
        };
        let proxy = cover.screen();
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
        self.ensure_carousel_cover(ui.ctx(), index);
        let canvases = self.library.carousels[index].layout.canvas_count;
        let frames = self.library.carousels[index].layout.frames.len();
        let image = self.cover_textures.get(&id).map(card::image_stamp);
        let loading = image.is_none() && self.cover_pending.contains_key(&id);
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
                self.confirm = Some(Confirm::DeleteTemplate(id));
            }
            HomeAction::OpenCarousel(id) => self.enter(Route::Carousel(id), ctx),
            HomeAction::DuplicateCarousel(id) => {
                self.library.duplicate_carousel(id);
                self.note_geometry();
            }
            HomeAction::DeleteCarousel(id) => {
                self.confirm = Some(Confirm::DeleteCarousel(id));
            }
        }
    }

    pub(super) fn create_template(&mut self, ctx: &egui::Context) {
        let name = self.create_name.trim().to_string();
        if name.is_empty() {
            self.toast("Name the template");
            return;
        }
        let Some(cover) = self.create_cover.take() else {
            self.toast("Choose a cover");
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
                    if choose.clicked() {
                        self.pick_image(ctx, "Choose cover", DecodeTarget::Cover);
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

    pub(super) fn confirm_modal(&mut self, ctx: &egui::Context) {
        if self.confirm.is_none() {
            return;
        }
        let Some(copy) = self.confirm_copy() else {
            self.confirm = None;
            return;
        };
        let modal_id = egui::Id::unique("confirm-delete");
        let mut accept = false;
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading(copy.title);
                ui.add_space(6.0);
                ui.label(&copy.body);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let cancel = ui.button("Cancel");
                    cancel
                        .clone()
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.remember("confirm-cancel", cancel.rect);
                    if cancel.clicked() {
                        self.confirm = None;
                    }
                    let action = ui.add(
                        egui::Button::new(
                            egui::RichText::new(copy.action).color(egui::Color32::BLACK),
                        )
                        .fill(ACCENT),
                    );
                    action
                        .clone()
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.remember("confirm-accept", action.rect);
                    if action.clicked() {
                        accept = true;
                    }
                });
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if accept {
            if let Some(confirm) = self.confirm.take() {
                self.apply_confirm(confirm);
            }
        } else if response.should_close() {
            self.confirm = None;
        }
    }

    fn confirm_copy(&self) -> Option<ConfirmCopy> {
        match self.confirm.as_ref()? {
            Confirm::DeleteCarousel(id) => {
                let name = self
                    .library
                    .carousels
                    .iter()
                    .find(|carousel| carousel.id == *id)?
                    .name
                    .clone();
                Some(ConfirmCopy {
                    title: "Delete carousel?",
                    body: format!("{} will be removed.", short_name(&name)),
                    action: "Delete",
                })
            }
            Confirm::DeleteTemplate(id) => {
                let name = self
                    .library
                    .templates
                    .iter()
                    .find(|template| template.id == *id)?
                    .name
                    .clone();
                Some(ConfirmCopy {
                    title: "Delete template?",
                    body: format!("{} will be removed.", short_name(&name)),
                    action: "Delete",
                })
            }
            Confirm::RemoveCanvases(indices) => {
                if indices.is_empty() {
                    return None;
                }
                let (title, body) = if indices.len() == 1 {
                    ("Remove canvas?", "This canvas will be removed.")
                } else {
                    ("Remove canvases?", "These canvases will be removed.")
                };
                Some(ConfirmCopy {
                    title,
                    body: body.to_string(),
                    action: "Remove",
                })
            }
            Confirm::RemoveFrame(_) => Some(ConfirmCopy {
                title: "Remove frame?",
                body: "This frame will be removed.".to_string(),
                action: "Remove",
            }),
            Confirm::RemovePhoto(_) => Some(ConfirmCopy {
                title: "Remove photo?",
                body: "This photo will be removed.".to_string(),
                action: "Remove",
            }),
        }
    }

    fn apply_confirm(&mut self, confirm: Confirm) {
        match confirm {
            Confirm::DeleteTemplate(id) => {
                self.library.delete_template(id);
                self.cover_textures.remove(&id);
                self.cover_pending.remove(&id);
                self.note_geometry();
            }
            Confirm::DeleteCarousel(id) => {
                self.library.delete_carousel(id);
                self.cover_textures.remove(&id);
                self.cover_pending.remove(&id);
                self.note_geometry();
            }
            Confirm::RemoveCanvases(indices) => self.remove_canvases(&indices),
            Confirm::RemoveFrame(id) => self.remove_frame(id),
            Confirm::RemovePhoto(id) => self.remove_photo(id),
        }
    }
}

struct ConfirmCopy {
    title: &'static str,
    body: String,
    action: &'static str,
}
