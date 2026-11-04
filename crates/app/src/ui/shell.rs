use std::time::Duration;

use eframe::egui;

use super::{BG, MENU_H, PANEL, PudduScrollApp, RAIL_W, Route, TEXT, TOOLBAR_H};

enum MenuAction {
    NewCarousel,
    Settings,
    Quit,
    About,
}

fn menu_chrome_style(style: &mut egui::Style) {
    egui::menu::menu_style(style);
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    let radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.noninteractive.corner_radius = radius;
    style.visuals.widgets.inactive.corner_radius = radius;
    style.visuals.widgets.hovered.corner_radius = radius;
    style.visuals.widgets.active.corner_radius = radius;
    style.visuals.widgets.open.corner_radius = radius;
}

fn menu_bar_style(style: &mut egui::Style) {
    menu_chrome_style(style);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.item_spacing = egui::vec2(0.0, 0.0);
}

fn menu_item_style(style: &mut egui::Style) {
    menu_chrome_style(style);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.item_spacing = egui::vec2(0.0, 2.0);
}

impl PudduScrollApp {
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut action = None;
        let chrome = egui::Frame::new().fill(BG).inner_margin(0);
        let menu = egui::MenuBar::new()
            .style(menu_bar_style)
            .config(egui::menu::MenuConfig::new().style(menu_item_style));
        egui::Panel::top("app-menu")
            .exact_size(MENU_H)
            .resizable(false)
            .show_separator_line(true)
            .frame(chrome)
            .show(ui, |ui| {
                menu.ui(ui, |ui| {
                    ui.menu_button(egui::RichText::new("File").color(TEXT), |ui| {
                        if ui
                            .button(egui::RichText::new("New carousel").color(TEXT))
                            .clicked()
                        {
                            action = Some(MenuAction::NewCarousel);
                            ui.close();
                        }
                        if ui
                            .button(egui::RichText::new("Settings").color(TEXT))
                            .clicked()
                        {
                            action = Some(MenuAction::Settings);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(egui::RichText::new("Quit").color(TEXT)).clicked() {
                            action = Some(MenuAction::Quit);
                            ui.close();
                        }
                    });
                    ui.menu_button(egui::RichText::new("Help").color(TEXT), |ui| {
                        if ui
                            .button(egui::RichText::new("About").color(TEXT))
                            .clicked()
                        {
                            action = Some(MenuAction::About);
                            ui.close();
                        }
                    });
                });
            });
        match action {
            Some(MenuAction::NewCarousel) => self.new_carousel(ctx),
            Some(MenuAction::Settings) => self.open_settings(),
            Some(MenuAction::Quit) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(MenuAction::About) => self.about_open = true,
            None => {}
        }
    }

    pub(super) fn editor_screen(&mut self, ui: &mut egui::Ui, full: egui::Rect) {
        let chrome = egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::symmetric(12, 6));
        egui::Panel::top("toolbar")
            .exact_size(TOOLBAR_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(chrome)
            .show(ui, |ui| self.toolbar(ui));
        if self.route == Route::Home {
            return;
        }
        let rail_rect = egui::Rect::from_min_max(
            egui::pos2(full.right() - RAIL_W, full.top() + TOOLBAR_H),
            full.right_bottom(),
        );
        let editor_rect = egui::Rect::from_min_max(
            egui::pos2(full.left(), rail_rect.top()),
            egui::pos2(rail_rect.left(), full.bottom()),
        );
        let mut editor_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("editor")
                .max_rect(editor_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        editor_ui.set_clip_rect(editor_rect);
        editor_ui.painter().rect_filled(editor_rect, 0.0, BG);
        self.editor(&mut editor_ui, editor_rect);
        self.paint_rail(ui, rail_rect);
        self.apply_crop_click(ui.ctx(), editor_rect);
    }
}

impl eframe::App for PudduScrollApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::color::note_display(frame);
        self.painted = self.painted.wrapping_add(1);
        let ctx = ui.ctx().clone();
        self.poll_jobs(&ctx);
        self.refresh_paste_offer(&ctx);
        if !self.library_ready {
            let full = ui.max_rect();
            super::skeleton::home(ui, full);
            self.show_toasts(&ctx);
            return;
        }
        self.handle_keys(&ctx);
        self.widgets.clear();
        self.menu_bar(ui, &ctx);
        let full = ui.available_rect_before_wrap();
        if full.width() < 900.0 || full.height() < 640.0 {
            self.show_toasts(&ctx);
            return;
        }
        if self.settings_open {
            self.settings_screen(ui, full);
        } else {
            let action = if self.route == Route::Home {
                self.home_screen(ui, full)
            } else {
                self.editor_screen(ui, full);
                None
            };
            if let Some(action) = action {
                self.apply_home(&action, &ctx);
            }
            self.create_modal(&ctx);
            self.snapshot_modal(&ctx);
            self.link_modal(&ctx);
            self.export_modal(&ctx);
            self.overwrite_modal(&ctx);
            self.confirm_modal(&ctx);
        }
        self.about_modal(&ctx);
        self.update_modal(&ctx);
        if self.sharing || self.importing {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.show_toasts(&ctx);
    }

    fn on_exit(&mut self) {
        self.refresh_open_carousel_cover();
        self.flush_store();
    }
}
