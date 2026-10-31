use std::fmt::Write;

use eframe::egui;
use pudduscroll_core::{
    Crop, CropPreset, FrameId, begin_crop_rect, crop_lock_ratio, move_crop_inside,
    resize_crop_inside,
};

use super::draw::{
    crop_handle_at, crop_to_screen, crop_unchanged, cursor_for, frame_screen, full_image_screen,
    handle_points, image_to_norm, paint_clipped_image, paint_crop_veil, paint_frame,
    paint_resize_handle,
};
use super::{CropGesture, CropSession, PudduScrollApp};

impl PudduScrollApp {
    pub(super) fn open_crop_session(&mut self, id: FrameId, preset: CropPreset) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let Some(photo) = frame.photo else {
            return;
        };
        let current = photo.crop;
        if let Some(session) = self.crop_session.as_mut() {
            if session.id == id {
                if session.preset != preset {
                    session.preset = preset;
                    session.crop =
                        begin_crop_rect(current, preset, photo.image_width, photo.image_height);
                    session.drag = None;
                }
                return;
            }
        }
        self.drag = None;
        self.status = "Cropping · Enter or click off the photo applies · Esc cancels".to_string();
        self.crop_session = Some(CropSession {
            id,
            preset,
            crop: begin_crop_rect(current, preset, photo.image_width, photo.image_height),
            drag: None,
        });
    }

    pub(super) fn commit_crop_session(&mut self) {
        let Some(session) = self.crop_session.take() else {
            return;
        };
        let Some(frame) = self
            .layout()
            .and_then(|layout| layout.frame(session.id))
            .cloned()
        else {
            return;
        };
        let Some(photo) = frame.photo else {
            return;
        };
        if photo.preset == session.preset && crop_unchanged(photo.crop, session.crop) {
            return;
        }
        self.checkpoint();
        if let Some(layout) = self.layout_mut() {
            layout.set_photo_crop(session.id, session.crop, session.preset);
        }
        self.note_geometry();
        self.status = "Crop updated".to_string();
    }

    pub(super) fn drag_crop(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: egui::Rect) {
        if self.export_open || self.overwrite_dir.is_some() || self.confirm.is_some() {
            return;
        }
        let Some(id) = self.crop_session.as_ref().map(|session| session.id) else {
            return;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            self.crop_session = None;
            self.crop_press = None;
            return;
        };
        let Some(photo) = frame.photo.as_ref() else {
            self.crop_session = None;
            self.crop_press = None;
            return;
        };
        let count = self.metrics().map_or(1, |(_, count)| count);
        let image_rect = full_image_screen(frame_screen(rect, count, &frame), photo.crop);
        let preset = self
            .crop_session
            .as_ref()
            .map_or(CropPreset::Freehand, |session| session.preset);
        let lock = crop_lock_ratio(preset, photo.image_width, photo.image_height);
        if !image_rect.is_positive() {
            return;
        }
        let crop_now = self
            .crop_session
            .as_ref()
            .map_or_else(Crop::full, |session| session.crop);
        let crop_rect = crop_to_screen(image_rect, crop_now);
        let (pressed, down, released, press, interact, latest) = ctx.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.primary_pressed(),
                pointer.primary_down(),
                pointer.primary_released(),
                pointer.press_origin(),
                pointer.interact_pos(),
                pointer.latest_pos(),
            )
        });
        let space = Self::space_held(ctx);
        let mut gesture = self.crop_session.as_ref().and_then(|session| session.drag);
        if gesture.is_none() && pressed && !space {
            let origin = press.or(interact).or(latest);
            if let Some(origin) = origin {
                if editor.contains(origin)
                    && let Some(handle) = crop_handle_at(crop_rect, origin)
                {
                    gesture = Some(CropGesture::Resize {
                        handle,
                        origin: crop_now,
                    });
                } else if editor.contains(origin) && crop_rect.contains(origin) {
                    let (nx, ny) = image_to_norm(image_rect, origin);
                    gesture = Some(CropGesture::Move {
                        grab_x: nx - crop_now.x,
                        grab_y: ny - crop_now.y,
                    });
                }
            }
        }
        if gesture.is_none() && down && !space {
            if let Some(origin) = press.or(interact).or(latest) {
                if editor.contains(origin)
                    && let Some(handle) = crop_handle_at(crop_rect, origin)
                {
                    gesture = Some(CropGesture::Resize {
                        handle,
                        origin: crop_now,
                    });
                }
            }
        }
        if let Some(session) = self.crop_session.as_mut() {
            session.drag = gesture;
            let pos = interact.or(latest).or(press);
            match gesture {
                Some(CropGesture::Resize { handle, origin }) => {
                    if let Some(pos) = pos {
                        let (nx, ny) = image_to_norm(image_rect, pos);
                        session.crop = resize_crop_inside(origin, handle, nx, ny, lock);
                    }
                    ctx.set_cursor_icon(cursor_for(handle));
                    if down {
                        ctx.set_dragged_id(egui::Id::unique("crop-gesture"));
                    }
                }
                Some(CropGesture::Move { grab_x, grab_y }) => {
                    if let Some(pos) = pos {
                        let (nx, ny) = image_to_norm(image_rect, pos);
                        session.crop = move_crop_inside(session.crop, nx - grab_x, ny - grab_y);
                    }
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    if down {
                        ctx.set_dragged_id(egui::Id::unique("crop-gesture"));
                    }
                }
                None => {
                    if space {
                        if let Some(pos) = latest.or(interact).or(press) {
                            if editor.contains(pos) {
                                let grabbing =
                                    down && press.is_some_and(|origin| editor.contains(origin));
                                ctx.set_cursor_icon(if grabbing {
                                    egui::CursorIcon::Grabbing
                                } else {
                                    egui::CursorIcon::Grab
                                });
                            }
                        }
                    } else if let Some(pos) = latest.or(interact) {
                        if editor.contains(pos)
                            && let Some(handle) = crop_handle_at(crop_rect, pos)
                        {
                            ctx.set_cursor_icon(cursor_for(handle));
                        }
                    }
                }
            }
            if released {
                if gesture.is_some() {
                    ctx.stop_dragging();
                }
                session.drag = None;
            }
        }
    }

    pub(super) fn paint_session_crop(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        clip: egui::Rect,
    ) {
        let Some(session) = self.crop_session.as_ref() else {
            return;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(session.id)) else {
            return;
        };
        let Some(photo) = frame.photo.as_ref() else {
            return;
        };
        let Some(texture) = self.assets.get(&session.id).map(|asset| asset.texture.id()) else {
            return;
        };
        let count = self.metrics().map_or(1, |(_, count)| count);
        let image = full_image_screen(frame_screen(strip, count, frame), photo.crop);
        let visible = image.intersect(clip);
        if !visible.is_positive() {
            return;
        }
        let crop_rect = crop_to_screen(image, session.crop);
        let full_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        paint_clipped_image(painter, image, clip, full_uv, texture, egui::Color32::WHITE);
        paint_crop_veil(painter, visible, crop_rect);
        let inside_uv = egui::Rect::from_min_max(
            egui::pos2(session.crop.x, session.crop.y),
            egui::pos2(
                session.crop.x + session.crop.w,
                session.crop.y + session.crop.h,
            ),
        );
        paint_clipped_image(
            painter,
            crop_rect,
            clip,
            inside_uv,
            texture,
            egui::Color32::WHITE,
        );
        paint_frame(
            painter,
            image,
            clip,
            1.0,
            egui::Color32::from_rgb(70, 70, 70),
        );
        paint_frame(painter, crop_rect, clip, 2.0, egui::Color32::WHITE);
    }

    pub(super) fn paint_crop_handles(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        clip: egui::Rect,
    ) {
        let Some(crop_rect) = self.session_crop_screen(strip) else {
            return;
        };
        let painter = painter.with_clip_rect(clip);
        for (_, pos) in handle_points(crop_rect) {
            paint_resize_handle(&painter, pos, 11.0, clip);
        }
    }

    pub(super) fn crop_cursor(
        &self,
        response: &egui::Response,
        rect: egui::Rect,
    ) -> Option<egui::CursorIcon> {
        let gesture = self.crop_session.as_ref().and_then(|session| session.drag);
        match gesture {
            Some(CropGesture::Move { .. }) => return Some(egui::CursorIcon::Grabbing),
            Some(CropGesture::Resize { handle, .. }) => return Some(cursor_for(handle)),
            None => {}
        }
        let pos = response
            .hover_pos()
            .or_else(|| response.ctx.input(|input| input.pointer.hover_pos()))?;
        let crop_rect = self.session_crop_screen(rect)?;
        if let Some(handle) = crop_handle_at(crop_rect, pos) {
            return Some(cursor_for(handle));
        }
        if crop_rect.contains(pos) {
            return Some(egui::CursorIcon::Grab);
        }
        None
    }

    pub(super) fn session_image_screen(&self, strip: egui::Rect) -> Option<egui::Rect> {
        let id = self.crop_session.as_ref()?.id;
        let count = self.layout()?.canvas_count;
        let frame = self.layout()?.frame(id)?;
        let photo = frame.photo.as_ref()?;
        let image = full_image_screen(frame_screen(strip, count, frame), photo.crop);
        image.is_positive().then_some(image)
    }

    pub(super) fn session_crop_screen(&self, strip: egui::Rect) -> Option<egui::Rect> {
        let image = self.session_image_screen(strip)?;
        let crop = self.crop_session.as_ref()?.crop;
        Some(crop_to_screen(image, crop))
    }

    pub(super) fn write_crop_probe(&self, ctx: &egui::Context, editor: egui::Rect) {
        let Some(strip) = self.strip_rect else {
            return;
        };
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let canvas_w = strip.width() / count.max(1) as f32;
        let mut text = format!(
            "ppp={}\nviewport={:?}\neditor={:?}\nstrip={:?}\n",
            ctx.pixels_per_point(),
            ctx.viewport_rect(),
            editor,
            strip
        );
        for index in 0..count {
            let canvas = egui::Rect::from_min_size(
                egui::pos2(strip.min.x + index as f32 * canvas_w, strip.min.y),
                egui::vec2(canvas_w, strip.height()),
            );
            let _ = writeln!(text, "canvas {index} {canvas:?}");
        }
        if let Some(layout) = self.layout() {
            for frame in &layout.frames {
                let screen = frame_screen(strip, count, frame);
                let _ = writeln!(
                    text,
                    "frame {} x={} y={} w={} h={} crop={:?} screen={:?} ratio={ratio}",
                    frame.id.0,
                    frame.x,
                    frame.y,
                    frame.w,
                    frame.h,
                    frame.photo.as_ref().map(|photo| photo.crop),
                    screen
                );
            }
        }
        if let Some(session) = &self.crop_session {
            let screen = self.session_crop_screen(strip);
            let _ = writeln!(
                text,
                "session {:?} crop={:?} screen={:?}",
                session.preset, session.crop, screen
            );
        }
        let _ = std::fs::write(
            r"C:\Users\dario\AppData\Local\Temp\pudduscroll-crop.txt",
            text,
        );
    }

    pub(super) fn pointer_hits_crop_photo(&self, pos: egui::Pos2) -> bool {
        let Some(strip) = self.strip_rect else {
            return true;
        };
        let Some(image) = self.session_image_screen(strip) else {
            return true;
        };
        if image.contains(pos) {
            return true;
        }
        self.session_crop_screen(strip)
            .is_some_and(|crop_rect| crop_handle_at(crop_rect, pos).is_some())
    }

    pub(super) fn apply_crop_click(&mut self, ctx: &egui::Context, editor_rect: egui::Rect) {
        if self.export_open
            || self.overwrite_dir.is_some()
            || self.confirm.is_some()
            || self.crop_session.is_none()
        {
            self.crop_press = None;
            return;
        }
        let (down, clicked, origin, latest) = ctx.input(|input| {
            let pointer = &input.pointer;
            let pressed_at = input.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    ..
                } if *button == egui::PointerButton::Primary => Some(*pos),
                _ => None,
            });
            (
                pointer.primary_down(),
                pointer.primary_clicked(),
                pointer.press_origin().or(pressed_at),
                pointer.latest_pos(),
            )
        });
        if let Some(origin) = origin {
            self.crop_press = Some(origin);
        }
        if !clicked {
            if !down {
                self.crop_press = None;
            }
            return;
        }
        let Some(release) = latest else {
            self.crop_press = None;
            return;
        };
        let press = self.crop_press.take().unwrap_or(release);
        if !editor_rect.contains(press) || !editor_rect.contains(release) {
            return;
        }
        if self.pointer_hits_crop_photo(press) || self.pointer_hits_crop_photo(release) {
            return;
        }
        self.commit_crop_session();
        ctx.request_repaint();
    }
}
