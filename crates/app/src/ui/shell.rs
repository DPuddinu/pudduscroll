use std::fmt::Write;

use eframe::egui;
use pudduscroll_core::cover_contains_frame;

use super::draw::{frame_screen, handle_name, handle_points};
use super::{BG, Drag, PANEL, PudduScrollApp, RAIL_W, Route, TEXT, TOOLBAR_H};

impl PudduScrollApp {
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
        self.write_crop_probe(ui.ctx(), editor_rect);
        self.apply_crop_click(ui.ctx(), editor_rect);
    }

    pub(super) fn write_probe(&self, ctx: &egui::Context) {
        let mut text = String::new();
        let view = ctx.viewport_rect();
        let _ = writeln!(text, "ppp {}", ctx.pixels_per_point());
        let _ = writeln!(
            text,
            "viewport {} {} {} {}",
            view.min.x,
            view.min.y,
            view.width(),
            view.height()
        );
        let _ = writeln!(
            text,
            "library {}",
            crate::library::library_directory().display()
        );
        match self.route {
            Route::Home => {
                let _ = writeln!(text, "screen home");
            }
            Route::Template(id) => {
                let _ = writeln!(text, "screen template {id}");
            }
            Route::Carousel(id) => {
                let _ = writeln!(text, "screen carousel {id}");
            }
        }
        let _ = writeln!(
            text,
            "grid {} snap {} cells {} edit_frame {} free {}",
            u8::from(self.show_grid),
            u8::from(self.snap_to_grid),
            self.grid_cells(),
            u8::from(self.edit_frame),
            u8::from(self.free_rotate)
        );
        let drag = match self.drag {
            Some(Drag::MoveFrame { .. }) => "move",
            Some(Drag::ResizeFrame { .. }) => "resize",
            Some(Drag::Pan { .. }) => "pan",
            Some(Drag::Rotate { .. }) => "rotate",
            None => "none",
        };
        let _ = writeln!(text, "drag {drag}");
        let _ = writeln!(text, "gesture {}", self.gesture);
        let _ = writeln!(text, "painted {}", self.painted);
        let _ = writeln!(text, "status {}", self.status.replace(['\n', '\r'], " "));
        let _ = writeln!(
            text,
            "cover_loading {}",
            u8::from(self.cover_load.is_some())
        );
        for (carousel_id, frame_id) in self.loading.keys() {
            let _ = writeln!(text, "loading {carousel_id} {}", frame_id.0);
        }
        for template in &self.library.templates {
            let _ = writeln!(
                text,
                "template {} \"{}\" {} {} {}",
                template.id,
                template.name.replace('"', "'"),
                template.layout.canvas_count,
                template.layout.frames.len(),
                u8::from(true)
            );
        }
        for carousel in &self.library.carousels {
            let photos = carousel
                .layout
                .frames
                .iter()
                .filter(|frame| frame.photo.is_some())
                .count();
            let _ = writeln!(
                text,
                "carousel {} \"{}\" {} {} {}",
                carousel.id,
                carousel.name.replace('"', "'"),
                carousel.layout.canvas_count,
                carousel.layout.frames.len(),
                photos
            );
        }
        let strip = self.strip_rect;
        if let Some(layout) = self.layout() {
            let ratio = layout.ratio.value();
            for frame in &layout.frames {
                let covers = u8::from(frame.photo.is_some() && cover_contains_frame(frame, ratio));
                let photo = frame.photo.as_ref();
                let _ = writeln!(
                    text,
                    "frame {} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {} {} {:.6}",
                    frame.id.0,
                    frame.x,
                    frame.y,
                    frame.w,
                    frame.h,
                    photo.map_or(0.5, |photo| photo.pan_x),
                    photo.map_or(0.5, |photo| photo.pan_y),
                    u8::from(photo.is_some()),
                    covers,
                    photo.map_or(0.0, |photo| photo.rotation)
                );
                if let Some(strip) = strip {
                    let screen = frame_screen(strip, layout.canvas_count, frame);
                    let _ = writeln!(
                        text,
                        "screen {} {} {} {} {}",
                        frame.id.0,
                        screen.min.x,
                        screen.min.y,
                        screen.width(),
                        screen.height()
                    );
                }
            }
        }
        if self.edit_frame
            && let Some(strip) = strip
            && let Some(id) = self.selected
        {
            let count = self.metrics().map_or(1, |(_, count)| count);
            if let Some(frame) = self.layout().and_then(|layout| layout.frame(id)) {
                let screen = frame_screen(strip, count, frame);
                for (handle, pos) in handle_points(screen) {
                    let _ = writeln!(text, "handle {} {} {}", handle_name(handle), pos.x, pos.y);
                }
            }
        }
        for (name, rect) in &self.widgets {
            let _ = writeln!(
                text,
                "widget {name} {} {} {} {}",
                rect.min.x,
                rect.min.y,
                rect.width(),
                rect.height()
            );
        }
        let _ = std::fs::write(
            r"C:\Users\dario\AppData\Local\Temp\pudduscroll-ui.txt",
            text,
        );
    }
}

impl eframe::App for PudduScrollApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::color::note_display(frame);
        self.painted = self.painted.wrapping_add(1);
        let ctx = ui.ctx().clone();
        self.poll_jobs(&ctx);
        let full = ui.max_rect();
        if !self.library_ready {
            ui.painter().rect_filled(full, 0.0, BG);
            ui.painter().text(
                full.center(),
                egui::Align2::CENTER_CENTER,
                &self.status,
                egui::FontId::proportional(18.0),
                TEXT,
            );
            self.write_probe(&ctx);
            return;
        }
        self.handle_keys(&ctx);
        self.widgets.clear();
        if full.width() < 900.0 || full.height() < 640.0 {
            self.write_probe(&ctx);
            return;
        }
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
        self.export_modal(&ctx);
        self.overwrite_modal(&ctx);
        self.write_probe(&ctx);
    }

    fn on_exit(&mut self) {
        self.flush_store();
    }
}
