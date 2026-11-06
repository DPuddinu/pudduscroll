use eframe::egui;
use pudduscroll_core::{AspectRatio, CANVAS_PRESETS};

use super::card::{self, CARD_INNER_W, Card};
use super::draw::{counted, dialog_frame, section_scroll, short_name};
use super::{
    ACCENT, BG, CANVAS_BG, COVER_EMPTY, Confirm, HOME_NAV_W, HomeAction, HomeSection, INK, MUTED,
    PANEL, PudduScrollApp, Route, SELECT_BG, TEXT,
};

impl PudduScrollApp {
    pub(super) fn home_screen(
        &mut self,
        ui: &mut egui::Ui,
        full: egui::Rect,
    ) -> Option<HomeAction> {
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
        self.home_nav(ui, nav_rect);
        let mut action = None;
        let mut main = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("home-main")
                .max_rect(main_rect.shrink(16.0))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        match self.home_section {
            HomeSection::Carousels => {
                if let Some(next) = self.home_carousels(&mut main) {
                    action = Some(next);
                }
            }
            HomeSection::Templates => {
                if let Some(next) = self.home_templates(&mut main) {
                    action = Some(next);
                }
            }
        }
        if self.name_dirty {
            self.name_dirty = false;
            self.persist();
        }
        action
    }

    fn home_nav(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let mut nav = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("home-nav")
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        nav.add_space(16.0);
        nav.horizontal(|ui| {
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 12.0).max(1.0));
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
                self.home_nav_item(ui, HomeSection::Carousels, "Carousels", "nav-carousels");
                self.home_nav_item(ui, HomeSection::Templates, "Templates", "nav-templates");
            });
        });
    }

    fn home_nav_item(&mut self, ui: &mut egui::Ui, section: HomeSection, label: &str, id: &str) {
        let selected = self.home_section == section;
        let width = ui.available_width().max(1.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::click());
        response
            .clone()
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if selected {
            ui.painter().rect_filled(rect, 6.0, SELECT_BG);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 6.0, COVER_EMPTY);
        }
        let color = if selected { ACCENT } else { TEXT };
        ui.painter().text(
            rect.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(14.0),
            color,
        );
        self.remember(id, rect);
        if response.clicked() {
            self.home_section = section;
        }
    }

    fn home_carousels(&mut self, ui: &mut egui::Ui) -> Option<HomeAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.heading("Carousels");
            let import = ui.add_enabled(!self.importing, egui::Button::new("Import"));
            self.remember("import-carousel", import.rect);
            if import.clicked() {
                self.import_package();
            }
        });
        ui.add_space(8.0);
        let carousel_count = self.library.carousels.len();
        section_scroll(ui, "carousels").show(ui, |ui| {
            if carousel_count == 0 {
                ui.label(egui::RichText::new("No carousels yet.").color(MUTED));
                ui.add_space(8.0);
                let browse = ui.button("Browse templates");
                self.remember("browse-templates", browse.rect);
                if browse.clicked() {
                    self.home_section = HomeSection::Templates;
                }
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                for index in (0..carousel_count).rev() {
                    if let Some(next) = self.carousel_card(ui, index) {
                        action = Some(next);
                    }
                }
            });
        });
        action
    }

    fn home_templates(&mut self, ui: &mut egui::Ui) -> Option<HomeAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.heading("Templates");
            let response = ui.button("New template");
            self.remember("new-template", response.rect);
            if response.clicked() {
                self.create_name = "Template".to_string();
                self.create_ratio = AspectRatio::FOUR_FIVE;
                self.create_open = true;
            }
        });
        ui.add_space(8.0);
        if self.library.templates.is_empty() {
            ui.label(egui::RichText::new("No templates yet.").color(MUTED));
        }
        let template_count = self.library.templates.len();
        section_scroll(ui, "templates").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                for index in 0..template_count {
                    if let Some(next) = self.template_card(ui, index) {
                        action = Some(next);
                    }
                }
            });
        });
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
            fill: COVER_EMPTY,
            loading,
            actions: true,
        }
        .show(ui, |ui| {
            let name_h = ui.spacing().interact_size.y;
            let (changed, name_rect) = {
                let template = &mut self.library.templates[index];
                let response = ui.add_sized(
                    egui::vec2(CARD_INNER_W, name_h),
                    egui::TextEdit::singleline(&mut template.name)
                        .id_salt(("template-name", id))
                        .desired_width(CARD_INNER_W),
                );
                (response.changed(), response.rect)
            };
            self.remember(format!("template-name-{id}"), name_rect);
            if changed {
                self.name_dirty = true;
            }
            ui.label(
                egui::RichText::new(format!(
                    "{} · {}",
                    counted(canvases, "canvas", "canvases"),
                    counted(frames, "frame", "frames")
                ))
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                let use_template = ui.button("Use");
                self.remember(format!("template-use-{id}"), use_template.rect);
                if use_template.clicked() {
                    action = Some(HomeAction::OpenTemplate(id));
                }
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
            action = Some(HomeAction::EditTemplate(id));
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
            actions: false,
        }
        .show(ui, |ui| {
            let name_h = ui.spacing().interact_size.y;
            let (changed, name_rect) = {
                let carousel = &mut self.library.carousels[index];
                let response = ui.add_sized(
                    egui::vec2(CARD_INNER_W, name_h),
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
            let meta_h = ui.text_style_height(&egui::TextStyle::Body);
            let menu_side = meta_h;
            let label_w = (CARD_INNER_W - menu_side).max(1.0);
            ui.allocate_ui_with_layout(
                egui::vec2(CARD_INNER_W, meta_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    ui.add_sized(
                        egui::vec2(label_w, meta_h),
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "{} · {}",
                                counted(canvases, "canvas", "canvases"),
                                counted(frames, "frame", "frames")
                            ))
                            .color(MUTED),
                        )
                        .selectable(false),
                    );
                    let hot_id =
                        ui.make_persistent_id(egui::IdSalt::new(("carousel-menu-hot", id)));
                    let hot = ui.data(|data| data.get_temp::<bool>(hot_id).unwrap_or(false));
                    let t = ui.ctx().animate_bool_with_time(hot_id, hot, 0.18);
                    let color = MUTED.lerp_to_gamma(TEXT, t);
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(menu_side, menu_side), egui::Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "⋮",
                        egui::FontId::proportional(meta_h),
                        color,
                    );
                    let mut picked = None;
                    egui::Popup::menu(&response).show(|ui| {
                        if ui.button("Duplicate").clicked() {
                            picked = Some(HomeAction::DuplicateCarousel(id));
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            picked = Some(HomeAction::DeleteCarousel(id));
                            ui.close();
                        }
                        if ui.button("Make template").clicked() {
                            picked = Some(HomeAction::MakeTemplate(id));
                            ui.close();
                        }
                    });
                    if let Some(next) = picked {
                        action = Some(next);
                    }
                    let popup_id = egui::Popup::default_response_id(&response);
                    let next = response.hovered() || egui::Popup::is_id_open(ui.ctx(), popup_id);
                    ui.data_mut(|data| data.insert_temp(hot_id, next));
                    if next != hot {
                        ui.ctx().request_repaint();
                    }
                    self.remember(format!("carousel-menu-{id}"), response.rect);
                },
            );
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
            HomeAction::MakeTemplate(id) => {
                if let Some(template_id) = self.library.template_from_carousel(id) {
                    self.note_geometry();
                    self.enter(Route::Template(template_id), ctx);
                }
            }
        }
    }

    pub(super) fn create_template(&mut self, ctx: &egui::Context) {
        let name = self.create_name.trim().to_string();
        if name.is_empty() {
            self.toast("Name the template");
            return;
        }
        let ratio = self.create_ratio;
        let cover = crate::library::blank_template_cover();
        let id = self.library.create_template(name, ratio, cover);
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
                    }
                    let ready = !self.create_name.trim().is_empty();
                    let create = ui.add_enabled(
                        ready,
                        egui::Button::new(egui::RichText::new("Create template").color(INK))
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
                        egui::Button::new(egui::RichText::new(copy.action).color(INK)).fill(ACCENT),
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
                self.apply_confirm(ctx, confirm);
            }
        } else if response.should_close() {
            self.confirm = None;
        }
    }

    pub(super) fn about_modal(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let modal_id = egui::Id::unique("about");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.heading("PudduScroll");
                ui.add_space(6.0);
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label(env!("CARGO_PKG_AUTHORS"));
                ui.label("2026");
                ui.add_space(6.0);
                ui.hyperlink_to("GitHub", "https://github.com/DPuddinu/pudduscroll");
                ui.add_space(12.0);
                let close = ui.button("Close");
                close
                    .clone()
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                self.remember("about-close", close.rect);
                if close.clicked() {
                    self.about_open = false;
                }
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if response.should_close() {
            self.about_open = false;
        }
    }

    pub(super) fn update_modal(&mut self, ctx: &egui::Context) {
        let Some(offer) = self.update_offer.clone() else {
            return;
        };
        let modal_id = egui::Id::unique("update-available");
        let mut accept = false;
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.set_max_width(520.0);
                ui.heading(format!("Version {} is available", offer.version));
                ui.add_space(6.0);
                if offer.notes.is_empty() {
                    ui.label(
                        egui::RichText::new("A newer build is ready to install.").color(MUTED),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            ui.set_min_width(400.0);
                            for (index, line) in offer.notes.lines().enumerate() {
                                if index > 0 && !line.is_empty() {
                                    ui.add_space(6.0);
                                }
                                if !line.is_empty() {
                                    ui.label(egui::RichText::new(line).color(TEXT));
                                }
                            }
                        });
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let later = ui.button("Later");
                    later
                        .clone()
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.remember("update-later", later.rect);
                    if later.clicked() {
                        self.update_offer = None;
                    }
                    let update = ui.add(
                        egui::Button::new(egui::RichText::new("Update").color(INK)).fill(ACCENT),
                    );
                    update
                        .clone()
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.remember("update-accept", update.rect);
                    if update.clicked() {
                        accept = true;
                    }
                });
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if accept {
            self.begin_update();
        } else if response.should_close() {
            self.update_offer = None;
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
            Confirm::LoadImage(_) => Some(ConfirmCopy {
                title: "Swap image?",
                body: "This photo will be replaced.".to_string(),
                action: "Load",
            }),
            Confirm::DeleteSnapshot(id) => {
                let Route::Carousel(carousel_id) = self.route else {
                    return None;
                };
                let name = self
                    .library
                    .carousels
                    .iter()
                    .find(|carousel| carousel.id == carousel_id)?
                    .snapshots
                    .iter()
                    .find(|snapshot| snapshot.id == *id)?
                    .name
                    .clone();
                Some(ConfirmCopy {
                    title: "Delete snapshot?",
                    body: format!("{} will be removed.", short_name(&name)),
                    action: "Delete",
                })
            }
        }
    }

    fn apply_confirm(&mut self, ctx: &egui::Context, confirm: Confirm) {
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
            Confirm::LoadImage(id) => self.fill_frame(ctx, id),
            Confirm::DeleteSnapshot(id) => self.delete_named_snapshot(id),
        }
    }

    pub(super) fn snapshot_modal(&mut self, ctx: &egui::Context) {
        if !self.snapshot_open {
            return;
        }
        let modal_id = egui::Id::unique("new-snapshot");
        let response = egui::Modal::new(modal_id)
            .area(egui::Modal::default_area(modal_id).order(egui::Order::Debug))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("New Snapshot");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Snapshot Name:");
                    let name = ui.add(
                        egui::TextEdit::singleline(&mut self.snapshot_name)
                            .id_salt("snapshot-name")
                            .desired_width(260.0),
                    );
                    self.remember("snapshot-name", name.rect);
                });
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel = ui.button("Cancel");
                    self.remember("cancel-snapshot", cancel.rect);
                    if cancel.clicked() {
                        self.snapshot_open = false;
                    }
                    let ready = !self.snapshot_name.trim().is_empty();
                    let create = ui.add_enabled(ready, egui::Button::new("Create"));
                    self.remember("create-snapshot", create.rect);
                    if create.clicked() {
                        self.create_named_snapshot();
                    }
                });
            });
        ctx.move_to_top(egui::LayerId::new(egui::Order::Debug, modal_id));
        if response.should_close() {
            self.snapshot_open = false;
        }
    }
}

struct ConfirmCopy {
    title: &'static str,
    body: String,
    action: &'static str,
}
