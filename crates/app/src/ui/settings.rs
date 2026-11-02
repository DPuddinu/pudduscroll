use eframe::egui;
use pudduscroll_core::{ExportFormat, quality_description};

use crate::settings::{
    Command, QUALITY_MAX, QUALITY_MIN, normalize_shortcut, redo_alias, shortcut_rank,
};

use super::{ACCENT, MUTED, PudduScrollApp, Route};

impl PudduScrollApp {
    pub(super) fn apply_system_menu(&mut self, ctx: &egui::Context) {
        let Some(command) = crate::window_menu::take() else {
            return;
        };
        match command {
            crate::window_menu::SystemCommand::NewCarousel => self.new_carousel(ctx),
            crate::window_menu::SystemCommand::Settings => self.open_settings(),
        }
    }

    pub(super) fn settings_screen(&mut self, ui: &mut egui::Ui, full: egui::Rect) {
        ui.painter().rect_filled(full, 0.0, super::BG);
        let inner = full.shrink(16.0);
        let mut page = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("settings")
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        page.horizontal(|ui| {
            ui.label(egui::RichText::new("Settings").strong().size(22.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let back = ui.button("Back");
                back.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                self.remember("settings-back", back.rect);
                if back.clicked() {
                    self.settings_open = false;
                    self.rebind = None;
                }
            });
        });
        page.add_space(18.0);
        egui::ScrollArea::vertical()
            .id_salt("settings-scroll")
            .auto_shrink([false, false])
            .show(&mut page, |ui| {
                ui.heading("Export");
                ui.add_space(8.0);
                self.export_default_controls(ui, "settings");
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.add_space(8.0);
                    ui.label("Folder");
                    if let Some(folder) = &self.settings.folder {
                        ui.label(
                            egui::RichText::new(folder.display().to_string())
                                .color(MUTED)
                                .size(13.0),
                        );
                    } else {
                        ui.label(egui::RichText::new("No folder yet.").color(MUTED));
                    }
                    ui.horizontal(|ui| {
                        let choose = ui.add(
                            egui::Button::new(
                                egui::RichText::new("Choose folder").color(egui::Color32::BLACK),
                            )
                            .fill(ACCENT),
                        );
                        choose
                            .clone()
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        self.remember("settings-choose-folder", choose.rect);
                        if choose.clicked() {
                            self.choose_default_folder();
                        }
                        if self.settings.folder.is_some() {
                            let clear = ui.button("Clear");
                            clear
                                .clone()
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            self.remember("settings-clear-folder", clear.rect);
                            if clear.clicked() {
                                self.settings.folder = None;
                                self.save_settings();
                            }
                        }
                    })
                };
                ui.add_space(18.0);
                ui.heading("Keybindings");
                ui.add_space(8.0);
                self.keybinding_rows(ui);
            });
    }

    pub(super) fn export_default_controls(&mut self, ui: &mut egui::Ui, salt: &str) {
        ui.label("Format");
        ui.horizontal(|ui| {
            let jpeg = ui.selectable_label(self.settings.format == ExportFormat::Jpeg, "JPEG");
            jpeg.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
            self.remember(format!("{salt}-jpeg"), jpeg.rect);
            if jpeg.clicked() && self.settings.format != ExportFormat::Jpeg {
                self.settings.format = ExportFormat::Jpeg;
                self.schedule_estimate();
                self.save_settings();
            }
            let png = ui.selectable_label(self.settings.format == ExportFormat::Png, "PNG");
            png.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
            self.remember(format!("{salt}-png"), png.rect);
            if png.clicked() && self.settings.format != ExportFormat::Png {
                self.settings.format = ExportFormat::Png;
                self.schedule_estimate();
                self.save_settings();
            }
        });
        ui.add_space(8.0);
        if self.settings.format == ExportFormat::Jpeg {
            ui.label("Quality");
            let slider = ui.add(
                egui::Slider::new(&mut self.settings.quality, QUALITY_MIN..=QUALITY_MAX)
                    .show_value(false)
                    .trailing_fill(true),
            );
            self.remember(format!("{salt}-quality"), slider.rect);
            if slider.changed() {
                self.schedule_estimate();
            }
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.save_settings();
            }
            ui.label(quality_description(
                self.settings.quality,
                self.settings.format,
            ));
            if self.settings.quality >= QUALITY_MAX {
                ui.label(egui::RichText::new("Highest JPEG quality.").color(MUTED));
            } else {
                ui.label(egui::RichText::new("Stronger compression toward the left.").color(MUTED));
            }
        } else {
            ui.label(quality_description(
                self.settings.quality,
                self.settings.format,
            ));
            ui.label(egui::RichText::new("No extra loss beyond the source. PNG.").color(MUTED));
        }
    }

    pub(super) fn new_carousel(&mut self, ctx: &egui::Context) {
        let id = self.library.create_carousel();
        self.rebind = None;
        self.settings_open = false;
        self.note_geometry();
        self.enter(Route::Carousel(id), ctx);
        self.selected = self
            .layout()
            .and_then(|layout| layout.frames.first().map(|frame| frame.id));
        self.edit_frame = self.selected.is_some();
    }

    pub(super) fn save_settings(&mut self) {
        if let Err(error) = self.settings.save() {
            self.toast(error);
        }
    }

    pub(super) fn capture_rebind(&mut self, ctx: &egui::Context) -> bool {
        let Some(command) = self.rebind else {
            return false;
        };
        let pressed = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, modifiers)) = pressed else {
            return true;
        };
        ctx.input_mut(|input| {
            input.consume_key(modifiers, key);
        });
        if modifier_key(key) {
            return true;
        }
        if key == egui::Key::Escape && modifiers.is_none() {
            self.rebind = None;
            return true;
        }
        let shortcut = normalize_shortcut(egui::KeyboardShortcut::new(modifiers, key));
        match self.settings.assign(command, shortcut) {
            Ok(()) => {
                self.rebind = None;
                self.save_settings();
            }
            Err(owner) => {
                let label = owner.label();
                self.toast(format!("Used by {label}"));
            }
        }
        true
    }

    pub(super) fn dispatch_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        let mut ready: Vec<Command> = Command::ALL
            .into_iter()
            .filter(|command| *command != Command::Paste && self.command_available(*command))
            .collect();
        ready.sort_by_key(|command| {
            std::cmp::Reverse(shortcut_rank(self.settings.binding(*command)))
        });
        for command in ready {
            let shortcut = self.settings.binding(command);
            if ctx.input_mut(|input| input.consume_shortcut(&shortcut)) {
                self.apply_command(command, ctx);
                return true;
            }
        }
        let alias = redo_alias();
        if self.command_available(Command::Redo)
            && self.settings.binding(Command::Redo) != alias
            && self.settings.owner(alias).is_none()
            && ctx.input_mut(|input| input.consume_shortcut(&alias))
        {
            self.apply_command(Command::Redo, ctx);
            return true;
        }
        false
    }

    fn open_settings(&mut self) {
        if !self.settings_open {
            self.toggle_settings();
        }
    }

    fn toggle_settings(&mut self) {
        if self.settings_open {
            self.settings_open = false;
            self.rebind = None;
            return;
        }
        self.export_open = false;
        self.link_open = false;
        self.create_open = false;
        self.confirm = None;
        self.cancel_overwrite();
        self.rebind = None;
        self.settings_open = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn choose_default_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Export folder");
        if let Some(dir) = &self.settings.folder
            && dir.is_dir()
        {
            dialog = dialog.set_directory(dir);
        }
        let Some(dir) = dialog.pick_folder() else {
            return;
        };
        self.settings.folder = Some(dir);
        self.save_settings();
    }

    fn keybinding_rows(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("keybindings")
            .num_columns(3)
            .spacing(egui::vec2(16.0, 8.0))
            .show(ui, |ui| {
                for command in Command::ALL {
                    ui.label(command.label());
                    if self.rebind == Some(command) {
                        ui.label(egui::RichText::new("Press a key").color(ACCENT).size(13.0));
                    } else {
                        ui.label(
                            egui::RichText::new(self.binding_label(command, ui.ctx()))
                                .color(MUTED)
                                .size(13.0),
                        );
                    }
                    if command.rebindable() {
                        let title = if self.rebind == Some(command) {
                            "Cancel"
                        } else {
                            "Change"
                        };
                        let change = ui.button(title);
                        change
                            .clone()
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        self.remember(format!("bind-{}", command.id()), change.rect);
                        if change.clicked() {
                            self.rebind = if self.rebind == Some(command) {
                                None
                            } else {
                                Some(command)
                            };
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
    }

    fn binding_label(&self, command: Command, ctx: &egui::Context) -> String {
        let mut text = shortcut_text(self.settings.binding(command), ctx);
        if command == Command::Redo {
            let alias = redo_alias();
            if self.settings.binding(Command::Redo) != alias && self.settings.owner(alias).is_none()
            {
                text.push_str(", ");
                text.push_str(&shortcut_text(alias, ctx));
            }
        }
        text
    }

    fn command_available(&self, command: Command) -> bool {
        if self.rebind.is_some() {
            return false;
        }
        let editor = !matches!(self.route, Route::Home) && !self.settings_open;
        let modal = self.modal_open();
        match command {
            Command::NewCarousel | Command::Settings => !modal,
            Command::Export => editor && !modal && !self.exporting && self.has_photo(),
            Command::Undo | Command::Redo | Command::Delete => editor,
            Command::AddCanvas => {
                editor && !modal && self.layout().is_some() && self.active_canvas.is_some()
            }
            Command::AddFrame | Command::ToggleGrid => editor && !modal && self.layout().is_some(),
            Command::Home => !modal && (self.settings_open || !matches!(self.route, Route::Home)),
            Command::Paste => false,
        }
    }

    fn modal_open(&self) -> bool {
        self.export_open
            || self.link_open
            || self.create_open
            || self.overwrite_dir.is_some()
            || self.confirm.is_some()
    }

    fn apply_command(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::NewCarousel => self.new_carousel(ctx),
            Command::Settings => self.toggle_settings(),
            Command::Export => {
                self.export_open = true;
                self.schedule_estimate();
            }
            Command::Undo => {
                if self.crop_session.take().is_some() {
                    self.crop_press = None;
                    self.status = "Crop canceled".to_string();
                } else {
                    self.undo_edit(ctx);
                }
            }
            Command::Redo => {
                self.crop_session = None;
                self.crop_press = None;
                self.redo_edit(ctx);
            }
            Command::Delete => self.delete_selection(),
            Command::AddFrame => self.add_frame_command(),
            Command::AddCanvas => self.add_canvas_command(),
            Command::ToggleGrid => self.show_grid = !self.show_grid,
            Command::Home => {
                self.rebind = None;
                self.settings_open = false;
                if !matches!(self.route, Route::Home) {
                    self.go_home();
                }
            }
            Command::Paste => self.paste_clipboard(ctx),
        }
    }

    pub(super) fn add_frame_command(&mut self) {
        if self.layout().is_none() {
            return;
        }
        self.checkpoint();
        let id = self.layout_mut().map(pudduscroll_core::Layout::add_frame);
        if let Some(id) = id {
            self.selected = Some(id);
            self.edit_frame = true;
            self.note_geometry();
        }
    }

    pub(super) fn add_canvas_command(&mut self) {
        self.add_canvas_beside(true);
    }

    pub(super) fn add_canvas_beside(&mut self, to_the_right: bool) {
        let Some(count) = self.layout().map(|layout| layout.canvas_count) else {
            return;
        };
        if count == 0 || count >= pudduscroll_core::MAX_CANVASES {
            return;
        }
        let Some(active) = self.active_canvas.filter(|index| *index < count) else {
            return;
        };
        let index = if to_the_right { active + 1 } else { active };
        self.checkpoint();
        let added = self
            .layout_mut()
            .is_some_and(|layout| layout.insert_canvas(index));
        if !added {
            return;
        }
        if self.preview_index >= index {
            self.preview_index += 1;
        }
        self.active_canvas = Some(if to_the_right { active } else { active + 1 });
        self.reveal_canvas = Some(index);
        self.note_geometry();
    }
}

fn shortcut_text(shortcut: egui::KeyboardShortcut, ctx: &egui::Context) -> String {
    let mut text = egui::ModifierNames::NAMES.format(&shortcut.modifiers, ctx.os().is_mac());
    if !text.is_empty() {
        text.push('+');
    }
    text.push_str(shortcut.logical_key.symbol_or_name());
    text
}

fn modifier_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}
