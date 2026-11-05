use eframe::egui;
use pudduscroll_core::{
    FrameId, FrameRect, Handle, border_thickness, canvas_at, cover_fit, frame_local,
    frame_rotation_from_pointer, pan_for_origin, photo_local, resize_frame, rotation_from_pointer,
    snap_frame, snap_frame_resize, unrotate_about,
};

use super::draw::{
    cursor_for, editor_strip_size, fit_editor_zoom, frame_screen, frame_screen_corners, grid_step,
    next_editor_zoom, norm_rect, paint_alignment_grid, paint_clipped_image, paint_frame,
    paint_frame_outline, paint_loading, paint_photo_border, paint_plus, paint_resize_handle,
    paint_rotate_handle, paint_rotated_photo, paint_spun_border, paint_spun_fill,
    paint_spun_outline, paint_spun_photo, photo_screen, rotate_around, rotate_handle_geometry,
    rotate_handle_hit, screen_to_norm, spun_handle_at, spun_points, zoom_strip_pan,
};
use super::{
    ACCENT, CANVAS_BG, Drag, EDITOR_ZOOM_SPEED, OUTSIDE_FRAME_ALPHA, PudduScrollApp, Route,
};

impl PudduScrollApp {
    pub(super) fn sync_zoom_basis(&mut self) {
        let Some(layout) = self.layout() else {
            return;
        };
        let basis = (layout.ratio.width, layout.ratio.height);
        if self.zoom_basis == basis {
            return;
        }
        self.zoom_basis = basis;
        self.strip_pan = egui::Vec2::ZERO;
        self.strip_wheel_pan = false;
        self.center_strip = true;
    }

    pub(super) fn apply_wheel_zoom(&mut self, ui: &egui::Ui, bounds: egui::Rect, size: egui::Vec2) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let (pointer, scroll, alt_wheel) = ui.input(|input| {
            let alt_wheel = input.modifiers.alt
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::MouseWheel { modifiers, .. } if modifiers.alt
                    )
                });
            (
                input.pointer.latest_pos(),
                input.smooth_scroll_delta,
                alt_wheel,
            )
        });
        if scroll == egui::Vec2::ZERO {
            self.strip_wheel_pan = false;
            return;
        }
        let Some(pointer) = pointer else {
            return;
        };
        if !bounds.contains(pointer) {
            return;
        }
        if ui
            .layer_id_at(pointer)
            .is_some_and(|layer| layer != ui.layer_id())
        {
            return;
        }
        ui.input_mut(|input| {
            input.smooth_scroll_delta = egui::Vec2::ZERO;
        });
        if alt_wheel {
            self.strip_wheel_pan = true;
        }
        if self.strip_wheel_pan {
            self.strip_pan += egui::vec2(scroll.x + scroll.y, 0.0);
            self.center_strip = false;
            self.gesture = "view".to_string();
            return;
        }
        if self.zoom_edited_frame(pointer, scroll.y) {
            return;
        }
        let zoom = next_editor_zoom(self.editor_zoom, scroll.y);
        if (zoom - self.editor_zoom).abs() < 1.0e-4 {
            return;
        }
        let new_size = editor_strip_size(bounds, zoom, ratio, count);
        self.strip_pan = zoom_strip_pan(self.strip_pan, size, new_size, pointer - bounds.min);
        self.editor_zoom = zoom;
        self.center_strip = false;
    }

    pub(super) fn zoom_edited_frame(&mut self, pointer: egui::Pos2, scroll_y: f32) -> bool {
        if self.crop_session.is_some() || !self.edit_frame || scroll_y.abs() <= f32::EPSILON {
            return false;
        }
        let Some(strip) = self.strip_rect else {
            return false;
        };
        let Some((_, count)) = self.metrics() else {
            return false;
        };
        let Some(id) = self.selected else {
            return false;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)) else {
            return false;
        };
        let Some(photo) = frame.photo.as_ref() else {
            return false;
        };
        let ratio = self.metrics().map_or(1.0, |(ratio, _)| ratio);
        let (nx, ny) = screen_to_norm(strip, count, pointer);
        let (local_x, local_y) = frame_local(frame, ratio, nx, ny);
        if local_x < frame.x
            || local_y < frame.y
            || local_x > frame.x + frame.w
            || local_y > frame.y + frame.h
        {
            return false;
        }
        let before = (photo.zoom, photo.pan_x, photo.pan_y);
        let origin = self.zoom_origin();
        let factor = (scroll_y * EDITOR_ZOOM_SPEED).exp();
        if let Some(layout) = self.layout_mut() {
            layout.zoom_photo(id, nx, ny, factor);
        }
        let changed = self
            .layout()
            .and_then(|layout| layout.frame(id))
            .and_then(|frame| frame.photo.as_ref())
            .is_some_and(|photo| {
                (photo.zoom - before.0).abs() > 1.0e-4
                    || (photo.pan_x - before.1).abs() > 1.0e-4
                    || (photo.pan_y - before.2).abs() > 1.0e-4
            });
        if changed {
            self.remember_zoom(origin);
            self.gesture = "zoom".to_string();
            self.note_geometry();
        }
        true
    }

    pub(super) fn space_held(ctx: &egui::Context) -> bool {
        !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_down(egui::Key::Space))
    }

    pub(super) fn apply_space_pan(&mut self, ui: &egui::Ui, bounds: egui::Rect) {
        if self.export_open
            || self.overwrite_dir.is_some()
            || self.create_open
            || self.confirm.is_some()
            || !Self::space_held(ui.ctx())
        {
            return;
        }
        let (down, released, delta, press, latest) = ui.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.primary_down(),
                pointer.primary_released(),
                pointer.delta(),
                pointer.press_origin(),
                pointer.latest_pos(),
            )
        });
        let over_editor = latest.is_some_and(|pos| bounds.contains(pos));
        let origin = press.filter(|origin| {
            bounds.contains(*origin)
                && ui
                    .layer_id_at(*origin)
                    .is_none_or(|layer| layer == ui.layer_id())
        });
        let blocked = self.drag.is_some()
            || self
                .crop_session
                .as_ref()
                .is_some_and(|session| session.drag.is_some());
        if over_editor && !blocked {
            ui.ctx().set_cursor_icon(if down && origin.is_some() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
        if blocked || self.center_strip {
            return;
        }
        if origin.is_none() {
            return;
        }
        if !down {
            if released {
                ui.ctx().stop_dragging();
            }
            return;
        }
        ui.ctx().set_dragged_id(egui::Id::unique("strip-pan"));
        if delta == egui::Vec2::ZERO {
            return;
        }
        self.strip_pan += delta;
        self.center_strip = false;
        self.gesture = "view".to_string();
    }

    fn reveal_canvas_pan(&mut self, bounds: egui::Rect, size: egui::Vec2, count: usize) {
        let Some(index) = self.reveal_canvas.take() else {
            return;
        };
        let index = index.min(count.saturating_sub(1));
        let canvas_w = size.x / count.max(1) as f32;
        let left = self.strip_pan.x + index as f32 * canvas_w;
        let right = left + canvas_w;
        let view_w = bounds.width();
        let visible = left >= -0.5 && right <= view_w + 0.5;
        if visible {
            return;
        }
        if canvas_w >= view_w || left < 0.0 {
            self.strip_pan.x = -(index as f32) * canvas_w;
        } else {
            self.strip_pan.x += view_w - right;
        }
    }

    pub(super) fn editor(&mut self, ui: &mut egui::Ui, bounds: egui::Rect) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let bounds = bounds.intersect(ui.clip_rect());
        ui.shrink_clip_rect(bounds);
        self.view_rect = Some(bounds);
        self.sync_zoom_basis();
        if self.center_strip {
            self.editor_zoom = fit_editor_zoom(bounds, ratio, count);
        }
        let size = editor_strip_size(bounds, self.editor_zoom, ratio, count);
        if self.center_strip {
            self.strip_pan = (bounds.size() - size) * 0.5;
            self.center_strip = false;
        }
        self.reveal_canvas_pan(bounds, size, count);
        self.apply_wheel_zoom(ui, bounds, size);
        let size = editor_strip_size(bounds, self.editor_zoom, ratio, count);
        self.apply_space_pan(ui, bounds);
        let _ = ui.allocate_rect(bounds, egui::Sense::hover());
        let strip_rect = egui::Rect::from_min_size(bounds.min + self.strip_pan, size);
        ui.scope_builder(egui::UiBuilder::new().max_rect(strip_rect), |ui| {
            ui.set_clip_rect(bounds.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            self.paint_strip(ui, size, bounds);
        });
    }

    pub(super) fn paint_strip(&mut self, ui: &mut egui::Ui, size: egui::Vec2, bounds: egui::Rect) {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        self.strip_rect = Some(rect);
        let layer = ui.layer_id();
        let ctx = ui.ctx().clone();
        self.handle_strip(&ctx, rect, bounds, layer);
        self.plus_rects.clear();
        let clip = bounds.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(clip);
        self.draw_strip(ui, &painter, rect, clip);
        self.paint_crop_handles(&painter, rect, clip);
        self.paint_frame_handles(&painter, rect, clip);
        self.paint_rotate_handle(&painter, rect, clip);
        self.strip_cursor(&response, rect);
    }

    pub(super) fn handle_strip(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        editor: egui::Rect,
        layer: egui::LayerId,
    ) {
        if self.crop_session.is_some() {
            self.drag_crop(ctx, rect, editor);
            return;
        }
        self.drag_frame(ctx, rect, editor, layer);
    }

    pub(super) fn hits_frame_or_handle(&self, rect: egui::Rect, pos: egui::Pos2) -> bool {
        if self.plus_hit(pos) || self.gesture_at(rect, pos).is_some() {
            return true;
        }
        let count = self.metrics().map_or(1, |(_, count)| count);
        let (nx, ny) = screen_to_norm(rect, count, pos);
        self.layout()
            .and_then(|layout| layout.frame_at(nx, ny))
            .is_some()
    }

    pub(super) fn plus_hit(&self, pos: egui::Pos2) -> bool {
        self.plus_rects.iter().any(|(_, rect)| rect.contains(pos))
    }

    pub(super) fn gesture_at(&self, rect: egui::Rect, pos: egui::Pos2) -> Option<Drag> {
        let layout = self.layout()?;
        let ratio = layout.ratio.value();
        let count = layout.canvas_count;
        if self.edit_frame
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
        {
            let screen = frame_screen(rect, count, frame);
            if let Some(handle) = spun_handle_at(screen, frame.rotation, pos) {
                return Some(Drag::ResizeFrame {
                    id,
                    handle,
                    x: frame.x,
                    y: frame.y,
                    w: frame.w,
                    h: frame.h,
                });
            }
        }
        if self.frame_rotate
            && self.crop_session.is_none()
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
        {
            let screen = frame_screen(rect, count, frame);
            if frame_rotate_hit(screen, frame.rotation, pos) {
                return Some(Drag::RotateFrame { id });
            }
        }
        if self.free_rotate
            && self.crop_session.is_none()
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
            && let Some(photo) = frame.photo.as_ref()
        {
            let screen = frame_screen(rect, count, frame);
            if rotate_handle_hit(screen, photo.rotation, pos) {
                return Some(Drag::Rotate { id });
            }
        }
        let (nx, ny) = screen_to_norm(rect, count, pos);
        let id = layout.frame_at(nx, ny)?;
        let frame = layout.frame(id)?;
        if self.edit_frame {
            return Some(Drag::MoveFrame {
                id,
                grab_x: nx - frame.x,
                grab_y: ny - frame.y,
            });
        }
        let fit = cover_fit(frame, ratio)?;
        let (fx, fy) = frame_local(frame, ratio, nx, ny);
        let (local_x, local_y) = photo_local(frame, ratio, fx, fy);
        Some(Drag::Pan {
            id,
            grab_x: local_x - fit.x,
            grab_y: local_y - fit.y,
        })
    }

    pub(super) fn drag_frame(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        editor: egui::Rect,
        layer: egui::LayerId,
    ) {
        if self.export_open || self.overwrite_dir.is_some() || self.confirm.is_some() {
            return;
        }
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
        if self.drag.is_none() && pressed && !Self::space_held(ctx) {
            if let Some(origin) = press.or(interact).or(latest) {
                if editor.contains(origin) && ctx.layer_id_at(origin).is_none_or(|hit| hit == layer)
                {
                    let frame_hit = self.hits_frame_or_handle(rect, origin);
                    if frame_hit {
                        self.active_canvas = self.canvas_for_press(rect, origin);
                    } else {
                        self.active_canvas = self.canvas_index_at(rect, origin);
                    }
                    let hit = self.active_canvas.is_some() || frame_hit;
                    if hit && !self.plus_hit(origin) {
                        self.begin_drag(rect, origin);
                    }
                }
            }
        }
        if self.drag.is_some() && (down || released) {
            if let Some(pos) = interact.or(latest).or(press) {
                self.update_drag(rect, pos);
            }
        }
        match self.drag {
            Some(
                Drag::MoveFrame { .. }
                | Drag::Pan { .. }
                | Drag::Rotate { .. }
                | Drag::RotateFrame { .. },
            ) => {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                if down {
                    ctx.set_dragged_id(egui::Id::unique("frame-gesture"));
                }
            }
            Some(Drag::ResizeFrame { handle, .. }) => {
                ctx.set_cursor_icon(cursor_for(handle));
                if down {
                    ctx.set_dragged_id(egui::Id::unique("frame-gesture"));
                }
            }
            None => {
                if Self::space_held(ctx) {
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
                } else if let Some(pos) = latest.or(interact).or(press) {
                    if editor.contains(pos)
                        && let Some(icon) = self.cursor_at(rect, pos)
                    {
                        ctx.set_cursor_icon(icon);
                    }
                }
            }
        }
        if released {
            if let Some(drag) = self.drag.take() {
                match drag {
                    Drag::Pan { .. } => self.gesture = "pan".to_string(),
                    Drag::Rotate { .. } | Drag::RotateFrame { .. } => {
                        self.gesture = "rotate".to_string();
                    }
                    Drag::MoveFrame { id, .. } => {
                        if self.show_grid && self.snap_to_grid {
                            self.snap_released_frame(id);
                            self.gesture = "snap".to_string();
                        } else {
                            self.gesture = "frame".to_string();
                        }
                    }
                    Drag::ResizeFrame { id, handle, .. } => {
                        if self.show_grid && self.snap_to_grid {
                            self.snap_released_resize(id, handle);
                            self.gesture = "snap".to_string();
                        } else {
                            self.gesture = "frame".to_string();
                        }
                    }
                }
                self.seal_drag();
                self.note_geometry();
                ctx.stop_dragging();
            }
        }
    }

    pub(super) fn snap_released_frame(&mut self, id: FrameId) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let (x, y) = snap_frame(&frame, grid_step(self.grid_cells()));
        if let Some(layout) = self.layout_mut() {
            layout.set_frame_rect(id, x, y, frame.w, frame.h);
        }
    }

    pub(super) fn snap_released_resize(&mut self, id: FrameId, handle: Handle) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let (x, y, w, h) = snap_frame_resize(
            handle,
            frame.x,
            frame.y,
            frame.w,
            frame.h,
            grid_step(self.grid_cells()),
        );
        if let Some(layout) = self.layout_mut() {
            layout.set_frame_rect(id, x, y, w, h);
        }
    }

    pub(super) fn begin_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        if let Some(drag) = self.gesture_at(rect, pos) {
            self.begin_history_drag();
            self.selected = Some(drag.id());
            self.drag = Some(drag);
            return;
        }
        let count = self.metrics().map_or(1, |(_, count)| count);
        let (nx, ny) = screen_to_norm(rect, count, pos);
        if let Some(id) = self.layout().and_then(|layout| layout.frame_at(nx, ny)) {
            self.selected = Some(id);
        } else {
            self.selected = None;
            self.edit_frame = false;
            self.drag = None;
        }
    }

    pub(super) fn update_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let (nx, ny) = screen_to_norm(rect, count, pos);
        match self.drag {
            Some(Drag::MoveFrame { id, grab_x, grab_y }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                if let Some(layout) = self.layout_mut() {
                    layout.set_frame_rect(id, nx - grab_x, ny - grab_y, frame.w, frame.h);
                }
            }
            Some(Drag::ResizeFrame {
                id,
                handle,
                x,
                y,
                w,
                h,
            }) => {
                let lock = matches!(handle, Handle::Ne | Handle::Nw | Handle::Se | Handle::Sw);
                let rotation = self
                    .layout()
                    .and_then(|layout| layout.frame(id))
                    .map_or(0.0, |frame| frame.rotation);
                let center_x = x + w * 0.5;
                let center_y = y + h * 0.5;
                let (lx, ly) = unrotate_about(center_x, center_y, rotation, ratio, nx, ny);
                let resized = resize_frame(FrameRect { x, y, w, h }, handle, lx, ly, lock);
                if let Some(layout) = self.layout_mut() {
                    layout.set_frame_rect(id, resized.x, resized.y, resized.w, resized.h);
                }
            }
            Some(Drag::Pan { id, grab_x, grab_y }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                let (fx, fy) = frame_local(&frame, ratio, nx, ny);
                let (local_x, local_y) = photo_local(&frame, ratio, fx, fy);
                let (pan_x, pan_y) =
                    pan_for_origin(&frame, ratio, local_x - grab_x, local_y - grab_y);
                if let Some(layout) = self.layout_mut() {
                    layout.set_pan(id, pan_x, pan_y);
                }
            }
            Some(Drag::Rotate { id }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                let angle = rotation_from_pointer(&frame, ratio, nx, ny);
                if let Some(layout) = self.layout_mut() {
                    layout.set_rotation(id, angle);
                }
            }
            Some(Drag::RotateFrame { id }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                let angle = frame_rotation_from_pointer(&frame, ratio, nx, ny);
                if let Some(layout) = self.layout_mut() {
                    layout.set_frame_rotation(id, angle);
                }
            }
            None => {}
        }
    }

    pub(super) fn draw_strip(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: egui::Rect,
    ) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let frames = self
            .layout()
            .map(|layout| layout.frames.clone())
            .unwrap_or_default();
        let canvas_w = rect.width() / count.max(1) as f32;
        let active = self.active_canvas.filter(|index| *index < count);
        for index in 0..count {
            let canvas = canvas_screen(rect, count, index);
            let visible = canvas.intersect(clip);
            if !visible.is_positive() {
                continue;
            }
            painter.rect_filled(visible, 0.0, CANVAS_BG);
            if Some(index) != active && visible.contains(canvas.left_top() + egui::vec2(8.0, 6.0)) {
                painter.text(
                    canvas.left_top() + egui::vec2(8.0, 6.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}", index + 1),
                    egui::FontId::proportional(13.0),
                    egui::Color32::from_black_alpha(110),
                );
            }
        }
        let session_id = self.crop_session.as_ref().map(|session| session.id);
        let mut ordered = frames.clone();
        ordered.sort_by_key(|frame| frame.z);
        for frame in &ordered {
            if session_id == Some(frame.id) {
                continue;
            }
            let screen = frame_screen(rect, count, frame);
            let spun = frame.rotation.abs() > 1.0e-4;
            let corners = spun.then(|| frame_screen_corners(rect, count, frame, ratio));
            if frame.photo.is_none() {
                if let Some(corners) = corners {
                    paint_spun_fill(painter, &corners, clip, egui::Color32::from_black_alpha(16));
                } else {
                    fill_outside(painter, screen, rect, clip);
                    let visible = screen.intersect(rect).intersect(clip);
                    if visible.is_positive() {
                        painter.rect_filled(visible, 0.0, egui::Color32::from_black_alpha(16));
                    }
                }
            }
            if let Some(photo) = &frame.photo
                && let Some(asset) = self.assets.get(&frame.id)
                && let Some(fit) = cover_fit(frame, ratio)
            {
                let cover = norm_rect(rect, count, fit.x, fit.y, fit.w, fit.h);
                let uv = egui::Rect::from_min_max(
                    egui::pos2(photo.crop.x, photo.crop.y),
                    egui::pos2(photo.crop.x + photo.crop.w, photo.crop.y + photo.crop.h),
                );
                let faded = egui::Color32::from_white_alpha(OUTSIDE_FRAME_ALPHA);
                if let Some(corners) = corners {
                    paint_spun_photo(painter, rect, count, frame, ratio, clip, asset.texture.id());
                    let color = frame.border_color;
                    paint_spun_border(
                        painter,
                        &corners,
                        border_thickness(frame.border, canvas_w, rect.height()),
                        egui::Color32::from_rgba_unmultiplied(
                            color[0], color[1], color[2], color[3],
                        ),
                    );
                } else if photo.rotation.abs() > 1.0e-4 {
                    paint_rotated_photo(
                        painter,
                        rect,
                        count,
                        frame,
                        ratio,
                        clip,
                        asset.texture.id(),
                    );
                } else {
                    paint_clipped_image(
                        painter,
                        cover,
                        clip.intersect(screen),
                        uv,
                        asset.texture.id(),
                        faded,
                    );
                    paint_clipped_image(
                        painter,
                        cover,
                        clip.intersect(photo_screen(rect, count, frame)),
                        uv,
                        asset.texture.id(),
                        egui::Color32::WHITE,
                    );
                }
                if corners.is_none() {
                    paint_photo_border(
                        painter,
                        screen,
                        clip,
                        frame,
                        egui::vec2(canvas_w, rect.height()),
                    );
                }
            }
        }
        if self.show_grid {
            paint_alignment_grid(painter, rect, count, clip, self.grid_cells());
        }
        for index in 1..count {
            let x = rect.min.x + index as f32 * canvas_w;
            if x < clip.left() || x > clip.right() {
                continue;
            }
            let y0 = rect.top().max(clip.top());
            let y1 = rect.bottom().min(clip.bottom());
            painter.line_segment(
                [egui::pos2(x, y0), egui::pos2(x, y1)],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(18, 18, 18)),
            );
        }
        for frame in &ordered {
            if session_id == Some(frame.id) {
                continue;
            }
            let screen = frame_screen(rect, count, frame);
            let shown = frame.photo.is_some() && self.assets.contains_key(&frame.id);
            let corners = (frame.rotation.abs() > 1.0e-4)
                .then(|| frame_screen_corners(rect, count, frame, ratio));
            if let Some(corners) = corners {
                if !shown {
                    paint_spun_outline(painter, &corners, egui::Color32::from_rgb(42, 36, 30));
                }
                if self.selected == Some(frame.id) {
                    paint_spun_outline(painter, &corners, ACCENT);
                }
            } else {
                if !shown {
                    paint_frame_outline(
                        painter,
                        screen,
                        rect,
                        count,
                        clip,
                        egui::Color32::from_rgb(42, 36, 30),
                        false,
                    );
                }
                if self.selected == Some(frame.id) {
                    paint_frame_outline(painter, screen, rect, count, clip, ACCENT, true);
                }
            }
            let loading_here = matches!(
                self.route,
                Route::Carousel(carousel_id) if self.loading.contains_key(&(carousel_id, frame.id))
            );
            if loading_here {
                paint_loading(ui, screen, clip);
            } else if frame.photo.is_none() && matches!(self.route, Route::Carousel(_)) {
                let center = screen.center();
                let radius = 16.0;
                let hit =
                    egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
                self.plus_rects.push((frame.id, hit));
                paint_plus(painter, center, radius);
                let response = ui.allocate_rect(hit, egui::Sense::click());
                self.remember(format!("plus-{}", frame.id.0), response.rect);
                if response.clicked() && !Self::space_held(ui.ctx()) {
                    let id = frame.id;
                    self.fill_frame(ui.ctx(), id);
                }
            }
        }
        if self.crop_session.is_some() {
            self.paint_session_crop(painter, rect, clip);
        }
        self.paint_active_canvas(painter, rect, clip, count);
    }

    pub(super) fn paint_frame_handles(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        clip: egui::Rect,
    ) {
        if self.crop_session.is_some() || !self.edit_frame {
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        let Some((_, count)) = self.metrics() else {
            return;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)) else {
            return;
        };
        let screen = frame_screen(strip, count, frame);
        let pad = screen.width().max(screen.height());
        if !screen.expand(pad).intersects(clip.expand(24.0)) {
            return;
        }
        let painter = painter.with_clip_rect(clip);
        for (_, pos) in spun_points(screen, frame.rotation) {
            paint_resize_handle(&painter, pos, 9.0, clip);
        }
    }

    pub(super) fn paint_rotate_handle(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        clip: egui::Rect,
    ) {
        if self.crop_session.is_some() || (!self.free_rotate && !self.frame_rotate) {
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        let Some((_, count)) = self.metrics() else {
            return;
        };
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)) else {
            return;
        };
        let screen = frame_screen(strip, count, frame);
        let painter = painter.with_clip_rect(clip);
        if self.frame_rotate {
            let (edge, handle) = rotate_handle_geometry(screen, 0.0);
            let center = screen.center();
            let edge = rotate_around(center, edge, frame.rotation);
            let handle = rotate_around(center, handle, frame.rotation);
            painter.line_segment([edge, handle], egui::Stroke::new(2.0, ACCENT));
            painter.circle_filled(handle, 7.0, egui::Color32::WHITE);
            painter.circle_stroke(handle, 7.0, egui::Stroke::new(2.0, ACCENT));
        }
        if self.free_rotate
            && let Some(photo) = frame.photo.as_ref()
        {
            paint_rotate_handle(&painter, screen, photo.rotation);
        }
    }

    pub(super) fn strip_cursor(&self, response: &egui::Response, rect: egui::Rect) {
        let cropping = self
            .crop_session
            .as_ref()
            .is_some_and(|session| session.drag.is_some());
        if self.drag.is_none() && !cropping && Self::space_held(&response.ctx) {
            let down = response.ctx.input(|input| input.pointer.primary_down());
            response.clone().on_hover_cursor(if down {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
            return;
        }
        if self.crop_session.is_some() {
            if let Some(icon) = self.crop_cursor(response, rect) {
                response.clone().on_hover_cursor(icon);
            }
            return;
        }
        let icon = match self.drag {
            Some(
                Drag::MoveFrame { .. }
                | Drag::Pan { .. }
                | Drag::Rotate { .. }
                | Drag::RotateFrame { .. },
            ) => Some(egui::CursorIcon::Grabbing),
            Some(Drag::ResizeFrame { handle, .. }) => Some(cursor_for(handle)),
            None => self.hover_cursor(response, rect),
        };
        if let Some(icon) = icon {
            response.clone().on_hover_cursor(icon);
        }
    }

    pub(super) fn hover_cursor(
        &self,
        response: &egui::Response,
        rect: egui::Rect,
    ) -> Option<egui::CursorIcon> {
        let pos = response
            .hover_pos()
            .or_else(|| response.ctx.input(|input| input.pointer.hover_pos()))?;
        self.cursor_at(rect, pos)
    }

    pub(super) fn cursor_at(&self, rect: egui::Rect, pos: egui::Pos2) -> Option<egui::CursorIcon> {
        let layout = self.layout()?;
        let count = layout.canvas_count;
        if self.frame_rotate
            && self.crop_session.is_none()
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
        {
            let screen = frame_screen(rect, count, frame);
            if frame_rotate_hit(screen, frame.rotation, pos) {
                return Some(egui::CursorIcon::Grab);
            }
        }
        if self.free_rotate
            && self.crop_session.is_none()
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
            && let Some(photo) = frame.photo.as_ref()
        {
            let screen = frame_screen(rect, count, frame);
            if rotate_handle_hit(screen, photo.rotation, pos) {
                return Some(egui::CursorIcon::Grab);
            }
        }
        if self.edit_frame
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
        {
            let screen = frame_screen(rect, count, frame);
            if let Some(handle) = spun_handle_at(screen, frame.rotation, pos) {
                return Some(cursor_for(handle));
            }
        }
        let (nx, ny) = screen_to_norm(rect, count, pos);
        let id = layout.frame_at(nx, ny)?;
        if self.edit_frame || layout.frame(id).is_some_and(|frame| frame.photo.is_some()) {
            Some(egui::CursorIcon::Grab)
        } else {
            Some(egui::CursorIcon::PointingHand)
        }
    }

    pub(super) fn canvas_under_frame(&self, id: FrameId) -> Option<usize> {
        let layout = self.layout()?;
        let frame = layout.frame(id)?;
        let count = layout.canvas_count;
        if count == 0 {
            return None;
        }
        let center = frame.x + frame.w * 0.5;
        let index = if center.is_finite() {
            center.floor() as isize
        } else {
            0
        };
        Some(index.clamp(0, count as isize - 1) as usize)
    }

    fn canvas_for_press(&self, strip: egui::Rect, pos: egui::Pos2) -> Option<usize> {
        let count = self.metrics()?.1;
        let (x, y) = screen_to_norm(strip, count, pos);
        if let Some(column) = column_at(count, x) {
            return Some(column);
        }
        let id = self
            .layout()
            .and_then(|layout| layout.frame_at(x, y))
            .or(self.selected)?;
        self.canvas_under_frame(id)
    }

    fn canvas_index_at(&self, strip: egui::Rect, pos: egui::Pos2) -> Option<usize> {
        let count = self.metrics()?.1;
        let (x, y) = screen_to_norm(strip, count, pos);
        canvas_at(count, x, y)
    }

    fn paint_active_canvas(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: egui::Rect,
        count: usize,
    ) {
        let Some(index) = self.active_canvas.filter(|index| *index < count) else {
            return;
        };
        let canvas = canvas_screen(rect, count, index);
        paint_frame(painter, canvas, clip, 3.0, ACCENT);
        let badge = egui::Rect::from_min_size(
            canvas.left_top() + egui::vec2(8.0, 6.0),
            egui::vec2(28.0, 20.0),
        );
        if !canvas.intersect(clip).intersects(badge) {
            return;
        }
        painter.rect_filled(badge, 4.0, ACCENT);
        painter.text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            format!("{}", index + 1),
            egui::FontId::proportional(13.0),
            egui::Color32::BLACK,
        );
    }
}

fn column_at(count: usize, x: f32) -> Option<usize> {
    if count == 0 || !x.is_finite() || x < 0.0 || x > count as f32 {
        return None;
    }
    if (x - count as f32).abs() <= f32::EPSILON {
        Some(count - 1)
    } else {
        Some(x.floor() as usize)
    }
}

fn frame_rotate_hit(screen: egui::Rect, angle: f32, pos: egui::Pos2) -> bool {
    let (_, handle) = rotate_handle_geometry(screen, 0.0);
    let handle = rotate_around(screen.center(), handle, angle);
    handle.distance(pos) <= 16.0
}

fn canvas_screen(strip: egui::Rect, count: usize, index: usize) -> egui::Rect {
    let canvas_w = strip.width() / count.max(1) as f32;
    egui::Rect::from_min_size(
        egui::pos2(strip.min.x + index as f32 * canvas_w, strip.min.y),
        egui::vec2(canvas_w, strip.height()),
    )
}

fn fill_outside(painter: &egui::Painter, screen: egui::Rect, strip: egui::Rect, clip: egui::Rect) {
    let visible = screen.intersect(clip);
    if !visible.is_positive() {
        return;
    }
    let color = egui::Color32::from_black_alpha(
        u8::try_from(u16::from(OUTSIDE_FRAME_ALPHA) * 16 / 255).unwrap_or(6),
    );
    let inside = visible.intersect(strip);
    if !inside.is_positive() {
        painter.rect_filled(visible, 0.0, color);
        return;
    }
    let bands = [
        egui::Rect::from_min_max(
            visible.left_top(),
            egui::pos2(visible.right(), inside.top()),
        ),
        egui::Rect::from_min_max(
            egui::pos2(visible.left(), inside.bottom()),
            visible.right_bottom(),
        ),
        egui::Rect::from_min_max(
            egui::pos2(visible.left(), inside.top()),
            egui::pos2(inside.left(), inside.bottom()),
        ),
        egui::Rect::from_min_max(
            egui::pos2(inside.right(), inside.top()),
            egui::pos2(visible.right(), inside.bottom()),
        ),
    ];
    for band in bands {
        if band.is_positive() {
            painter.rect_filled(band, 0.0, color);
        }
    }
}
