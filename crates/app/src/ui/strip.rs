use eframe::egui;
use pudduscroll_core::{
    FrameId, FrameRect, Handle, cover_fit, pan_for_origin, resize_frame, snap_frame_to_grid,
    snap_resized_frame_to_grid,
};

use super::draw::{
    cursor_for, editor_strip_size, frame_screen, handle_at, handle_points, next_editor_zoom,
    norm_rect, paint_alignment_grid, paint_clipped_image, paint_frame, paint_loading, paint_plus,
    paint_resize_handle, place_strip, screen_to_norm,
};
use super::{ACCENT, CANVAS_BG, DEFAULT_EDITOR_ZOOM, Drag, PudduScrollApp, Route};

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
        self.editor_zoom = DEFAULT_EDITOR_ZOOM;
        self.strip_lead = egui::Vec2::ZERO;
        self.pending_scroll = Some(egui::Vec2::ZERO);
        self.strip_wheel_pan = false;
        self.center_strip = true;
    }

    pub(super) fn apply_wheel_zoom(&mut self, ui: &egui::Ui, bounds: egui::Rect) {
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
        let Some(strip) = self.strip_rect else {
            return;
        };
        let Some(view) = self.view_rect else {
            return;
        };
        let Some(pointer) = pointer else {
            return;
        };
        if !strip.contains(pointer) {
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
            let id = ui.make_persistent_id(egui::IdSalt::new("canvas-strip"));
            let mut offset = egui::scroll_area::State::load(ui.ctx(), id)
                .map_or(egui::Vec2::ZERO, |state| state.clamped_offset());
            offset.x = (offset.x - (scroll.x + scroll.y)).max(0.0);
            self.pending_scroll = Some(offset);
            self.center_strip = false;
            return;
        }
        let zoom = next_editor_zoom(self.editor_zoom, scroll.y);
        if (zoom - self.editor_zoom).abs() < 1.0e-4 {
            return;
        }
        let fx = ((pointer.x - strip.min.x) / strip.width().max(1.0)).clamp(0.0, 1.0);
        let fy = ((pointer.y - strip.min.y) / strip.height().max(1.0)).clamp(0.0, 1.0);
        self.editor_zoom = zoom;
        let new_size = editor_strip_size(bounds, zoom, ratio, count);
        let desired = pointer - egui::vec2(fx * new_size.x, fy * new_size.y);
        let (lead, offset) = place_strip(desired, new_size, view);
        self.strip_lead = lead;
        self.pending_scroll = Some(offset);
        self.center_strip = false;
    }

    pub(super) fn editor(&mut self, ui: &mut egui::Ui, bounds: egui::Rect) {
        let Some((ratio, count)) = self.metrics() else {
            return;
        };
        let bounds = bounds.intersect(ui.clip_rect());
        ui.shrink_clip_rect(bounds);
        self.sync_zoom_basis();
        self.apply_wheel_zoom(ui, bounds);
        let size = editor_strip_size(bounds, self.editor_zoom, ratio, count);
        let mut scroll_source = egui::scroll_area::ScrollSource::SCROLL_BAR;
        if self.crop_session.is_none() && !self.claims_strip_pointer(ui.ctx(), bounds) {
            scroll_source |= egui::scroll_area::ScrollSource::DRAG;
        }
        let mut area = egui::ScrollArea::both()
            .id_salt("canvas-strip")
            .auto_shrink([false, false])
            .scroll_source(scroll_source);
        if let Some(offset) = self.pending_scroll.take() {
            area = area.scroll_offset(offset);
        }
        let output = area.show(ui, |ui| {
            ui.shrink_clip_rect(bounds);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let view = ui.available_size();
            if self.center_strip {
                self.strip_lead.x = ((view.x - size.x) * 0.5).max(0.0);
                self.strip_lead.y = ((view.y - size.y) * 0.5).max(0.0);
                self.center_strip = false;
            } else {
                self.strip_lead.x = self.strip_lead.x.clamp(0.0, (view.x - size.x).max(0.0));
                self.strip_lead.y = self.strip_lead.y.clamp(0.0, (view.y - size.y).max(0.0));
            }
            ui.add_space(self.strip_lead.y);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.add_space(self.strip_lead.x);
                self.paint_strip(ui, size, bounds);
            });
        });
        self.view_rect = Some(output.inner_rect);
    }

    pub(super) fn paint_strip(&mut self, ui: &mut egui::Ui, size: egui::Vec2, bounds: egui::Rect) {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        self.strip_rect = Some(rect);
        let ctx = ui.ctx().clone();
        self.handle_strip(&ctx, rect, bounds);
        self.plus_rects.clear();
        let clip = bounds.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(clip);
        self.draw_strip(ui, &painter, rect, clip);
        self.paint_crop_handles(&painter, rect, clip);
        self.paint_frame_handles(&painter, rect, clip);
        self.strip_cursor(&response, rect);
    }

    pub(super) fn handle_strip(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        editor: egui::Rect,
    ) {
        if self.crop_session.is_some() {
            self.drag_crop(ctx, rect, editor);
            return;
        }
        self.drag_frame(ctx, rect, editor);
    }

    pub(super) fn claims_strip_pointer(&self, ctx: &egui::Context, editor: egui::Rect) -> bool {
        if self.crop_session.is_some() || self.export_open {
            return false;
        }
        if self.drag.is_some() {
            return true;
        }
        let Some(strip) = self.strip_rect else {
            return false;
        };
        let (down, pos) = ctx.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.primary_down(),
                pointer
                    .press_origin()
                    .or(pointer.interact_pos())
                    .or(pointer.latest_pos()),
            )
        });
        let Some(pos) = pos else {
            return false;
        };
        down && editor.contains(pos) && self.hits_frame_or_handle(strip, pos)
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
            if let Some(handle) = handle_at(screen, pos) {
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
        Some(Drag::Pan {
            id,
            grab_x: nx - fit.x,
            grab_y: ny - fit.y,
        })
    }

    pub(super) fn drag_frame(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: egui::Rect) {
        if self.export_open {
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
        if self.drag.is_none() && pressed {
            if let Some(origin) = press.or(interact).or(latest) {
                if editor.contains(origin) && !self.plus_hit(origin) {
                    self.begin_drag(rect, origin);
                }
            }
        }
        if self.drag.is_some() && (down || released) {
            if let Some(pos) = interact.or(latest).or(press) {
                self.update_drag(rect, pos);
            }
        }
        match self.drag {
            Some(Drag::MoveFrame { .. } | Drag::Pan { .. }) => {
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
                if let Some(pos) = latest.or(interact).or(press) {
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
                self.note_geometry();
                ctx.stop_dragging();
            }
        }
    }

    pub(super) fn snap_released_frame(&mut self, id: FrameId) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let (x, y) = snap_frame_to_grid(&frame);
        if let Some(layout) = self.layout_mut() {
            layout.set_frame_rect(id, x, y, frame.w, frame.h);
        }
    }

    pub(super) fn snap_released_resize(&mut self, id: FrameId, handle: Handle) {
        let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
            return;
        };
        let (x, y, w, h) = snap_resized_frame_to_grid(handle, frame.x, frame.y, frame.w, frame.h);
        if let Some(layout) = self.layout_mut() {
            layout.set_frame_rect(id, x, y, w, h);
        }
    }

    pub(super) fn begin_drag(&mut self, rect: egui::Rect, pos: egui::Pos2) {
        if let Some(drag) = self.gesture_at(rect, pos) {
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
                let resized = resize_frame(FrameRect { x, y, w, h }, handle, nx, ny, lock);
                if let Some(layout) = self.layout_mut() {
                    layout.set_frame_rect(id, resized.x, resized.y, resized.w, resized.h);
                }
            }
            Some(Drag::Pan { id, grab_x, grab_y }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                let (pan_x, pan_y) = pan_for_origin(&frame, ratio, nx - grab_x, ny - grab_y);
                if let Some(layout) = self.layout_mut() {
                    layout.set_pan(id, pan_x, pan_y);
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
        for index in 0..count {
            let canvas = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + index as f32 * canvas_w, rect.min.y),
                egui::vec2(canvas_w, rect.height()),
            );
            let visible = canvas.intersect(clip);
            if !visible.is_positive() {
                continue;
            }
            painter.rect_filled(visible, 0.0, CANVAS_BG);
            if visible.contains(canvas.left_top() + egui::vec2(8.0, 6.0)) {
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
            if frame.photo.is_none() {
                let visible = screen.intersect(clip);
                if visible.is_positive() {
                    painter.rect_filled(visible, 0.0, egui::Color32::from_black_alpha(16));
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
                paint_clipped_image(
                    painter,
                    cover,
                    clip.intersect(screen),
                    uv,
                    asset.texture.id(),
                );
            }
        }
        if self.show_grid {
            paint_alignment_grid(painter, rect, count, clip);
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
            if !shown {
                let color = if self.selected == Some(frame.id) {
                    ACCENT
                } else {
                    egui::Color32::from_rgb(42, 36, 30)
                };
                paint_frame(painter, screen, clip, 2.0, color);
            }
            let loading_here = matches!(
                self.route,
                Route::Carousel(carousel_id) if self.loading.contains_key(&(carousel_id, frame.id))
            );
            if loading_here {
                paint_loading(ui, painter, screen, clip);
            } else if frame.photo.is_none() && matches!(self.route, Route::Carousel(_)) {
                let center = screen.center();
                let radius = 16.0;
                let hit =
                    egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
                self.plus_rects.push((frame.id, hit));
                paint_plus(painter, center, radius);
                let response = ui.allocate_rect(hit, egui::Sense::click());
                self.remember(format!("plus-{}", frame.id.0), response.rect);
                if response.clicked() {
                    let id = frame.id;
                    self.fill_frame(ui.ctx(), id);
                }
            }
        }
        if self.crop_session.is_some() {
            self.paint_session_crop(painter, rect, clip);
        }
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
        if !screen.intersects(clip.expand(24.0)) {
            return;
        }
        let painter = painter.with_clip_rect(clip);
        for (_, pos) in handle_points(screen) {
            paint_resize_handle(&painter, pos, 9.0, clip);
        }
    }

    pub(super) fn strip_cursor(&self, response: &egui::Response, rect: egui::Rect) {
        if self.crop_session.is_some() {
            if let Some(icon) = self.crop_cursor(response, rect) {
                response.clone().on_hover_cursor(icon);
            }
            return;
        }
        let icon = match self.drag {
            Some(Drag::MoveFrame { .. } | Drag::Pan { .. }) => Some(egui::CursorIcon::Grabbing),
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
        if self.edit_frame
            && let Some(id) = self.selected
            && let Some(frame) = layout.frame(id)
        {
            let screen = frame_screen(rect, count, frame);
            if let Some(handle) = handle_at(screen, pos) {
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
}
