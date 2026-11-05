use std::time::Duration;

use eframe::egui;

use super::{BG, MENU_H, PANEL, PudduScrollApp, RAIL_W, Route, TEXT, TOOLBAR_H};
#[cfg(windows)]
use super::{HAIRLINE, SELECT_BG};

#[cfg(windows)]
const CAPTION_BUTTON_W: f32 = 46.0;
#[cfg(windows)]
const CAPTION_ICON: f32 = 16.0;
const MENU_PAD_X: f32 = 7.0;
const TITLE_BAR_PAD: i8 = 4;

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
    style.spacing.button_padding = egui::vec2(MENU_PAD_X, 0.0);
    style.spacing.item_spacing = egui::vec2(0.0, 0.0);
}

fn menu_chip_width(ui: &egui::Ui, label: &str) -> f32 {
    let font_id = ui.style().text_styles[&egui::TextStyle::Button].clone();
    let width = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout(label.to_owned(), font_id, TEXT, f32::INFINITY))
        .size()
        .x;
    width + MENU_PAD_X * 2.0
}

fn title_menu_button<R>(
    ui: &mut egui::Ui,
    label: &str,
    row_h: f32,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::Response, Option<egui::InnerResponse<R>>) {
    let chip_w = menu_chip_width(ui, label);
    let mut menu = None;
    ui.allocate_ui_with_layout(
        egui::vec2(chip_w, row_h),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            menu = Some(
                egui::menu::MenuButton::from_button(
                    egui::Button::new(egui::RichText::new(label).color(TEXT))
                        .min_size(egui::vec2(chip_w, row_h)),
                )
                .ui(ui, body),
            );
        },
    );
    menu.unwrap_or_else(|| {
        let response = ui.allocate_response(egui::vec2(0.0, row_h), egui::Sense::hover());
        (response, None)
    })
}

fn menu_item_style(style: &mut egui::Style) {
    menu_chrome_style(style);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.item_spacing = egui::vec2(0.0, 2.0);
}

impl PudduScrollApp {
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut action = None;
        let chrome = egui::Frame::new()
            .fill(BG)
            .inner_margin(egui::Margin::same(TITLE_BAR_PAD));
        let menu = egui::MenuBar::new()
            .style(menu_bar_style)
            .config(egui::menu::MenuConfig::new().style(menu_item_style));
        egui::Panel::top("app-menu")
            .exact_size(MENU_H)
            .resizable(false)
            .show_separator_line(true)
            .frame(chrome)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let bar = ui.max_rect();
                let reserved = caption_reserve();
                let lead = caption_lead();
                let mut menu_left = bar.left() + lead;
                let mut menu_right = menu_left;
                let menu_rect = egui::Rect::from_min_max(
                    egui::pos2(bar.left() + lead, bar.top()),
                    egui::pos2(bar.right() - reserved, bar.bottom()),
                );
                let row_h = menu_rect.height();
                let mut menu_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("menu-items")
                        .max_rect(menu_rect)
                        .layout(
                            egui::Layout::left_to_right(egui::Align::Center)
                                .with_cross_align(egui::Align::Center),
                        ),
                );
                menu_ui.set_clip_rect(menu_rect);
                menu_ui.style_mut().spacing.interact_size.y = row_h;
                menu.ui(&mut menu_ui, |ui| {
                    let (file, _) = title_menu_button(ui, "File", row_h, |ui| {
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
                    let (help, _) = title_menu_button(ui, "Help", row_h, |ui| {
                        if ui
                            .button(egui::RichText::new("About").color(TEXT))
                            .clicked()
                        {
                            action = Some(MenuAction::About);
                            ui.close();
                        }
                    });
                    menu_left = file.rect.left();
                    menu_right = file.rect.right().max(help.rect.right());
                });
                #[cfg(windows)]
                {
                    paint_caption_icon(ui, bar);
                    let buttons = egui::Rect::from_min_max(
                        egui::pos2(bar.right() - reserved, bar.top()),
                        bar.right_bottom(),
                    );
                    caption_buttons(ui, buttons);
                    let leading = egui::Rect::from_min_max(
                        bar.left_top(),
                        egui::pos2(menu_left, bar.bottom()),
                    );
                    if leading.width() > 0.0 {
                        caption_drag(ui, leading, "caption-drag-lead");
                    }
                    let drag = egui::Rect::from_min_max(
                        egui::pos2(menu_right, bar.top()),
                        egui::pos2(buttons.left(), bar.bottom()),
                    );
                    if drag.width() > 0.0 {
                        caption_drag(ui, drag, "caption-drag-gap");
                    }
                }
                #[cfg(not(windows))]
                let _ = (menu_left, menu_right);
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

fn caption_lead() -> f32 {
    #[cfg(windows)]
    {
        MENU_PAD_X + CAPTION_ICON + MENU_PAD_X
    }
    #[cfg(not(windows))]
    {
        0.0
    }
}

fn caption_reserve() -> f32 {
    #[cfg(windows)]
    {
        CAPTION_BUTTON_W * 3.0
    }
    #[cfg(not(windows))]
    {
        0.0
    }
}

#[cfg(windows)]
fn round_window(frame: &eframe::Frame) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use winit::platform::windows::{CornerPreference, WindowExtWindows};

    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(window) = frame.winit_window() {
        window.set_corner_preference(CornerPreference::Round);
        DONE.store(true, Ordering::Relaxed);
    }
}

#[cfg(windows)]
fn caption_icon(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::unique("caption-icon");
    if let Some(handle) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }
    let decoded = image::load_from_memory(include_bytes!("../../assets/icon.png"))
        .expect("caption icon")
        .to_rgba8();
    let handle = super::draw::texture_from_preview(ctx, "caption-icon", &decoded);
    ctx.data_mut(|data| data.insert_temp(id, handle.clone()));
    handle
}

#[cfg(windows)]
fn paint_caption_icon(ui: &mut egui::Ui, bar: egui::Rect) {
    let rect = egui::Rect::from_center_size(
        egui::pos2(bar.left() + MENU_PAD_X + CAPTION_ICON * 0.5, bar.center().y),
        egui::vec2(CAPTION_ICON, CAPTION_ICON),
    );
    let texture = caption_icon(ui.ctx());
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

#[cfg(windows)]
fn caption_drag(ui: &mut egui::Ui, bar: egui::Rect, salt: &str) {
    let response = ui.interact(bar, egui::Id::unique(salt), egui::Sense::click_and_drag());
    if response.double_clicked() {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[cfg(windows)]
fn caption_buttons(ui: &mut egui::Ui, rect: egui::Rect) {
    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    let kinds = [
        ("caption-min", Caption::Minimize),
        (
            "caption-max",
            if maximized {
                Caption::Restore
            } else {
                Caption::Maximize
            },
        ),
        ("caption-close", Caption::Close),
    ];
    let width = rect.width() / 3.0;
    let mut x = rect.left();
    for (salt, kind) in kinds {
        let button =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, rect.height()));
        x += width;
        let response = ui.interact(button, egui::Id::unique(salt), egui::Sense::click());
        let cover = if response.hovered() { SELECT_BG } else { BG };
        if response.hovered() {
            ui.painter().rect_filled(button, 0.0, cover);
        }
        paint_caption(ui.painter(), button, kind, cover);
        if response.clicked() {
            let command = match kind {
                Caption::Minimize => egui::ViewportCommand::Minimized(true),
                Caption::Maximize | Caption::Restore => {
                    egui::ViewportCommand::Maximized(!maximized)
                }
                Caption::Close => egui::ViewportCommand::Close,
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

#[cfg(windows)]
fn paint_caption(painter: &egui::Painter, rect: egui::Rect, kind: Caption, cover: egui::Color32) {
    let stroke = egui::Stroke::new(1.0, TEXT);
    let glyph = egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0));
    match kind {
        Caption::Minimize => {
            let y = rect.center().y;
            painter.line_segment(
                [egui::pos2(glyph.left(), y), egui::pos2(glyph.right(), y)],
                stroke,
            );
        }
        Caption::Maximize => {
            painter.rect_stroke(glyph, 0.0, stroke, egui::StrokeKind::Inside);
        }
        Caption::Restore => {
            let back = glyph.translate(egui::vec2(2.0, -2.0));
            let front = glyph.translate(egui::vec2(-2.0, 2.0));
            painter.rect_stroke(back, 0.0, stroke, egui::StrokeKind::Inside);
            painter.rect_filled(front.shrink(0.5), 0.0, cover);
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
        }
        Caption::Close => {
            painter.line_segment([glyph.left_top(), glyph.right_bottom()], stroke);
            painter.line_segment([glyph.right_top(), glyph.left_bottom()], stroke);
        }
    }
}

#[cfg(windows)]
fn window_resize(ui: &mut egui::Ui, window: egui::Rect) {
    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    if maximized {
        return;
    }
    let band = 4.0;
    let corner = 8.0;
    let below = window.top() + MENU_H;
    resize_band(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(window.left() + corner, window.top()),
            egui::pos2(window.right() - caption_reserve(), window.top() + band),
        ),
        "edge-n",
        egui::CursorIcon::ResizeVertical,
        egui::ResizeDirection::North,
    );
    resize_band(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(window.left() + corner, window.bottom() - band),
            egui::pos2(window.right() - corner, window.bottom()),
        ),
        "edge-s",
        egui::CursorIcon::ResizeVertical,
        egui::ResizeDirection::South,
    );
    resize_band(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(window.left(), below),
            egui::pos2(window.left() + band, window.bottom() - corner),
        ),
        "edge-w",
        egui::CursorIcon::ResizeHorizontal,
        egui::ResizeDirection::West,
    );
    resize_band(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(window.right() - band, below),
            egui::pos2(window.right(), window.bottom() - corner),
        ),
        "edge-e",
        egui::CursorIcon::ResizeHorizontal,
        egui::ResizeDirection::East,
    );
    resize_band(
        ui,
        egui::Rect::from_min_size(window.left_top(), egui::vec2(corner, corner)),
        "edge-nw",
        egui::CursorIcon::ResizeNwSe,
        egui::ResizeDirection::NorthWest,
    );
    resize_band(
        ui,
        egui::Rect::from_min_size(
            egui::pos2(window.left(), window.bottom() - corner),
            egui::vec2(corner, corner),
        ),
        "edge-sw",
        egui::CursorIcon::ResizeNeSw,
        egui::ResizeDirection::SouthWest,
    );
    resize_band(
        ui,
        egui::Rect::from_min_size(
            egui::pos2(window.right() - corner, window.bottom() - corner),
            egui::vec2(corner, corner),
        ),
        "edge-se",
        egui::CursorIcon::ResizeNwSe,
        egui::ResizeDirection::SouthEast,
    );
    ui.painter().rect_stroke(
        window,
        0.0,
        egui::Stroke::new(1.0, HAIRLINE),
        egui::StrokeKind::Inside,
    );
}

#[cfg(not(windows))]
fn window_resize(_ui: &mut egui::Ui, _window: egui::Rect) {}

#[cfg(windows)]
fn resize_band(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    salt: &str,
    cursor: egui::CursorIcon,
    direction: egui::ResizeDirection,
) {
    let response = ui
        .interact(rect, egui::Id::unique(salt), egui::Sense::drag())
        .on_hover_cursor(cursor);
    if response.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

impl eframe::App for PudduScrollApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::color::note_display(frame);
        #[cfg(windows)]
        round_window(frame);
        self.painted = self.painted.wrapping_add(1);
        let ctx = ui.ctx().clone();
        let window = ui.max_rect();
        self.poll_jobs(&ctx);
        self.refresh_paste_offer(&ctx);
        self.menu_bar(ui, &ctx);
        if !self.library_ready {
            let full = ui.available_rect_before_wrap();
            super::skeleton::home(ui, full);
            self.show_toasts(&ctx);
            window_resize(ui, window);
            return;
        }
        self.handle_keys(&ctx);
        self.widgets.clear();
        let full = ui.available_rect_before_wrap();
        if full.width() < 900.0 || full.height() < 640.0 {
            self.show_toasts(&ctx);
            window_resize(ui, window);
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
        window_resize(ui, window);
    }

    fn on_exit(&mut self) {
        self.refresh_open_carousel_cover();
        self.flush_store();
    }
}
