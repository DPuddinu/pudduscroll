use eframe::egui;
use pudduscroll_core::{
    FrameId, FrameRect, Handle, canvas_at, canvases_touched, cover_fit, pan_for_origin,
    photo_local, resize_frame, rotation_from_pointer, snap_frame, snap_frame_resize,
};

use super::draw::{
    center_on_strip, cursor_for, editor_strip_size, frame_screen, grid_step, handle_at,
    handle_points, next_editor_zoom, norm_rect, paint_alignment_grid, paint_clipped_image,
    paint_frame, paint_loading, paint_photo_border, paint_plus, paint_resize_handle,
    paint_rotate_handle, paint_rotated_photo, photo_screen, place_strip, rotate_handle_hit,
    screen_to_norm,
};
use super::{
    ACCENT, CANVAS_BG, CanvasSelect, Drag, EDITOR_ZOOM_SPEED, OUTSIDE_FRAME_ALPHA, PudduScrollApp,
    Route,
};

const CANVAS_SELECT_DRAG: f32 = 4.0;

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
        self.strip_lead = egui::Vec2::ZERO;
        self.strip_pad = egui::Vec2::ZERO;
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
        if self.zoom_edited_frame(pointer, scroll.y) {
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
        let pads = self.frame_overflow(new_size);
        let (lead, offset) = place_strip(
            desired,
            new_size,
            view,
            egui::vec2(pads[0], pads[1]),
            egui::vec2(pads[2], pads[3]),
        );
        self.strip_lead = lead;
        self.pending_scroll = Some(offset);
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
        let screen = frame_screen(strip, count, frame);
        if !screen.contains(pointer) {
            return false;
        }
        let before = (photo.zoom, photo.pan_x, photo.pan_y);
        let origin = self.zoom_origin();
        let (nx, ny) = screen_to_norm(strip, count, pointer);
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

    pub(super) fn apply_space_pan(&mut self, ui: &egui::Ui, bounds: egui::Rect, size: egui::Vec2) {
        if self.export_open
            || self.overwrite_dir.is_some()
            || self.create_open
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
        let Some(view) = self.view_rect else {
            return;
        };
        let over_editor = latest.is_some_and(|pos| bounds.contains(pos));
        let origin = press.filter(|origin| {
            view.contains(*origin)
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
        let id = ui.make_persistent_id(egui::IdSalt::new("canvas-strip"));
        let stored = egui::scroll_area::State::load(ui.ctx(), id)
            .map_or(egui::Vec2::ZERO, |state| state.clamped_offset());
        let pads = self.frame_overflow(size);
        let offset = self.pending_scroll.unwrap_or(stored);
        let screen = view.min + self.strip_pad + self.strip_lead - offset;
        let (lead, scroll) = place_strip(
            screen + delta,
            size,
            view,
            egui::vec2(pads[0], pads[1]),
            egui::vec2(pads[2], pads[3]),
        );
        self.strip_lead = lead;
        self.pending_scroll = Some(scroll);
        self.center_strip = false;
        self.gesture = "view".to_string();
    }

    fn frame_overflow(&self, size: egui::Vec2) -> [f32; 4] {
        let Some(layout) = self.layout() else {
            return [0.0; 4];
        };
        let count = layout.canvas_count.max(1) as f32;
        let mut left = 0.0f32;
        let mut top = 0.0f32;
        let mut right = 0.0f32;
        let mut bottom = 0.0f32;
        for frame in &layout.frames {
            left = left.max(-frame.x);
            top = top.max(-frame.y);
            right = right.max(frame.x + frame.w - count);
            bottom = bottom.max(frame.y + frame.h - 1.0);
        }
        let canvas_w = size.x / count;
        let canvas_h = size.y;
        [
            left.max(0.0) * canvas_w,
            top.max(0.0) * canvas_h,
            right.max(0.0) * canvas_w,
            bottom.max(0.0) * canvas_h,
        ]
    }

    fn sync_scroll(
        &mut self,
        ui: &egui::Ui,
        bounds: egui::Rect,
        size: egui::Vec2,
        count: usize,
    ) -> [f32; 4] {
        let pads = self.frame_overflow(size);
        let pad_min = egui::vec2(pads[0], pads[1]);
        let pad_max = egui::vec2(pads[2], pads[3]);
        let view_size = self
            .view_rect
            .map_or_else(|| bounds.size(), |rect| rect.size());
        if self.center_strip {
            let (lead, scroll) = center_on_strip(view_size, size, pad_min, pad_max);
            self.strip_lead = lead;
            self.pending_scroll = Some(scroll);
            self.center_strip = false;
        } else {
            self.strip_lead.x = self
                .strip_lead
                .x
                .clamp(0.0, (view_size.x - size.x).max(0.0));
            self.strip_lead.y = self
                .strip_lead
                .y
                .clamp(0.0, (view_size.y - size.y).max(0.0));
            let delta = pad_min - self.strip_pad;
            if self.pending_scroll.is_none() && delta != egui::Vec2::ZERO {
                let content = egui::vec2(
                    pad_min.x + self.strip_lead.x + size.x + pad_max.x,
                    pad_min.y + self.strip_lead.y + size.y + pad_max.y,
                );
                let max_scroll = (content - view_size).max(egui::Vec2::ZERO);
                self.pending_scroll = Some(clamp_scroll(strip_offset(ui) + delta, max_scroll));
            }
        }
        self.strip_pad = pad_min;
        let Some(index) = self.reveal_canvas.take() else {
            return pads;
        };
        let index = index.min(count.saturating_sub(1));
        let mut offset = self.pending_scroll.unwrap_or_else(|| strip_offset(ui));
        let canvas_w = size.x / count.max(1) as f32;
        let left = pad_min.x + self.strip_lead.x + index as f32 * canvas_w;
        let right = left + canvas_w;
        let content_w = pad_min.x + self.strip_lead.x + size.x + pad_max.x;
        let max_x = (content_w - view_size.x).max(0.0);
        let visible = left >= offset.x - 0.5 && right <= offset.x + view_size.x + 0.5;
        if !visible {
            offset.x = if canvas_w >= view_size.x || left < offset.x {
                left
            } else {
                right - view_size.x
            }
            .clamp(0.0, max_x);
            self.pending_scroll = Some(offset);
        }
        pads
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
        self.apply_space_pan(ui, bounds, size);
        let pads = self.sync_scroll(ui, bounds, size, count);
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
            self.strip_lead.x = self.strip_lead.x.clamp(0.0, (view.x - size.x).max(0.0));
            self.strip_lead.y = self.strip_lead.y.clamp(0.0, (view.y - size.y).max(0.0));
            ui.add_space(pads[1]);
            ui.add_space(self.strip_lead.y);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.add_space(pads[0]);
                ui.add_space(self.strip_lead.x);
                self.paint_strip(ui, size, bounds);
                ui.add_space(pads[2]);
            });
            ui.add_space(pads[3]);
        });
        self.view_rect = Some(output.inner_rect);
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
        self.paint_canvas_marquee(&painter, clip);
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

    pub(super) fn claims_strip_pointer(&self, ctx: &egui::Context, editor: egui::Rect) -> bool {
        if Self::space_held(ctx) {
            return true;
        }
        if self.crop_session.is_some() || self.export_open || self.overwrite_dir.is_some() {
            return false;
        }
        if self.drag.is_some() || self.canvas_marquee.is_some() {
            return true;
        }
        let Some(strip) = self.strip_rect else {
            return false;
        };
        let (down, select, pos) = ctx.input(|input| {
            let pointer = &input.pointer;
            (
                pointer.primary_down(),
                input.modifiers.command || input.modifiers.ctrl,
                pointer
                    .press_origin()
                    .or(pointer.interact_pos())
                    .or(pointer.latest_pos()),
            )
        });
        let Some(pos) = pos else {
            return false;
        };
        if down && select && editor.contains(pos) {
            return true;
        }
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
        let (local_x, local_y) = photo_local(frame, ratio, nx, ny);
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
        if self.export_open || self.overwrite_dir.is_some() {
            self.canvas_marquee = None;
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
        if self.drag.is_none() && self.canvas_marquee.is_none() && pressed && !Self::space_held(ctx)
        {
            if let Some(origin) = press.or(interact).or(latest) {
                if editor.contains(origin) && ctx.layer_id_at(origin).is_none_or(|hit| hit == layer)
                {
                    if Self::canvas_modifier(ctx) {
                        self.begin_canvas_select(origin);
                    } else if !self.plus_hit(origin) {
                        self.selected_canvases.clear();
                        self.begin_drag(rect, origin);
                    }
                }
            }
        }
        if let Some(select) = self.canvas_marquee.as_mut()
            && let Some(pos) = interact.or(latest).or(press)
        {
            select.current = pos;
        }
        if released && self.canvas_marquee.is_some() {
            self.commit_canvas_select(rect);
        }
        if self.canvas_marquee.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            if down {
                ctx.set_dragged_id(egui::Id::unique("canvas-select"));
            }
            return;
        }
        if self.drag.is_some() && (down || released) {
            if let Some(pos) = interact.or(latest).or(press) {
                self.update_drag(rect, pos);
            }
        }
        match self.drag {
            Some(Drag::MoveFrame { .. } | Drag::Pan { .. } | Drag::Rotate { .. }) => {
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
                    Drag::Rotate { .. } => self.gesture = "rotate".to_string(),
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
                let resized = resize_frame(FrameRect { x, y, w, h }, handle, nx, ny, lock);
                if let Some(layout) = self.layout_mut() {
                    layout.set_frame_rect(id, resized.x, resized.y, resized.w, resized.h);
                }
            }
            Some(Drag::Pan { id, grab_x, grab_y }) => {
                let Some(frame) = self.layout().and_then(|layout| layout.frame(id)).cloned() else {
                    return;
                };
                let (local_x, local_y) = photo_local(&frame, ratio, nx, ny);
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
                fill_outside(painter, screen, rect, clip);
                let visible = screen.intersect(rect).intersect(clip);
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
                let faded = egui::Color32::from_white_alpha(OUTSIDE_FRAME_ALPHA);
                if photo.rotation.abs() > 1.0e-4 {
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
                paint_photo_border(
                    painter,
                    screen,
                    clip,
                    frame,
                    egui::vec2(canvas_w, rect.height()),
                );
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
            if !shown {
                paint_frame(
                    painter,
                    screen,
                    clip,
                    2.0,
                    egui::Color32::from_rgb(42, 36, 30),
                );
            }
            if self.selected == Some(frame.id) {
                paint_frame(painter, screen, clip, 2.0, ACCENT);
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
                if response.clicked()
                    && !Self::space_held(ui.ctx())
                    && !Self::canvas_modifier(ui.ctx())
                {
                    let id = frame.id;
                    self.fill_frame(ui.ctx(), id);
                }
            }
        }
        if self.crop_session.is_some() {
            self.paint_session_crop(painter, rect, clip);
        }
        self.paint_canvas_selection(painter, rect, clip, count);
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

    pub(super) fn paint_rotate_handle(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        clip: egui::Rect,
    ) {
        if !self.free_rotate || self.crop_session.is_some() {
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
        let Some(photo) = frame.photo.as_ref() else {
            return;
        };
        let screen = frame_screen(strip, count, frame);
        let painter = painter.with_clip_rect(clip);
        paint_rotate_handle(&painter, screen, photo.rotation);
    }

    pub(super) fn strip_cursor(&self, response: &egui::Response, rect: egui::Rect) {
        if self.canvas_marquee.is_some() {
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::Crosshair);
            return;
        }
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
            Some(Drag::MoveFrame { .. } | Drag::Pan { .. } | Drag::Rotate { .. }) => {
                Some(egui::CursorIcon::Grabbing)
            }
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

    fn canvas_modifier(ctx: &egui::Context) -> bool {
        ctx.input(|input| input.modifiers.command || input.modifiers.ctrl)
    }

    fn begin_canvas_select(&mut self, origin: egui::Pos2) {
        self.canvas_marquee = Some(CanvasSelect {
            origin,
            current: origin,
        });
    }

    fn commit_canvas_select(&mut self, strip: egui::Rect) {
        let Some(select) = self.canvas_marquee.take() else {
            return;
        };
        let count = self.metrics().map_or(0, |(_, count)| count);
        if count == 0 {
            return;
        }
        if select.origin.distance(select.current) <= CANVAS_SELECT_DRAG {
            let (x, y) = screen_to_norm(strip, count, select.origin);
            let Some(index) = canvas_at(count, x, y) else {
                return;
            };
            if let Some(pos) = self
                .selected_canvases
                .iter()
                .position(|item| *item == index)
            {
                self.selected_canvases.remove(pos);
            } else {
                self.selected_canvases.push(index);
                self.selected_canvases.sort_unstable();
            }
            return;
        }
        let (x0, y0) = screen_to_norm(strip, count, select.origin);
        let (x1, y1) = screen_to_norm(strip, count, select.current);
        self.selected_canvases = canvases_touched(count, x0, y0, x1, y1);
    }

    fn highlighted_canvases(&self, strip: egui::Rect, count: usize) -> Vec<usize> {
        let Some(select) = self.canvas_marquee else {
            return self.selected_canvases.clone();
        };
        if select.origin.distance(select.current) <= CANVAS_SELECT_DRAG {
            return self.selected_canvases.clone();
        }
        let (x0, y0) = screen_to_norm(strip, count, select.origin);
        let (x1, y1) = screen_to_norm(strip, count, select.current);
        canvases_touched(count, x0, y0, x1, y1)
    }

    fn paint_canvas_selection(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        clip: egui::Rect,
        count: usize,
    ) {
        let canvas_w = rect.width() / count.max(1) as f32;
        for index in self.highlighted_canvases(rect, count) {
            if index >= count {
                continue;
            }
            let canvas = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + index as f32 * canvas_w, rect.min.y),
                egui::vec2(canvas_w, rect.height()),
            );
            paint_frame(painter, canvas, clip, 2.0, ACCENT);
        }
    }

    fn paint_canvas_marquee(&self, painter: &egui::Painter, clip: egui::Rect) {
        let Some(select) = self.canvas_marquee else {
            return;
        };
        let area = egui::Rect::from_two_pos(select.origin, select.current);
        let painter = painter.with_clip_rect(clip);
        painter.rect_filled(area, 0.0, ACCENT.gamma_multiply(0.28));
        painter.rect_stroke(
            area,
            0.0,
            egui::Stroke::new(1.0, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
}

fn strip_offset(ui: &egui::Ui) -> egui::Vec2 {
    let id = ui.make_persistent_id(egui::IdSalt::new("canvas-strip"));
    egui::scroll_area::State::load(ui.ctx(), id)
        .map_or(egui::Vec2::ZERO, |state| state.clamped_offset())
}

fn clamp_scroll(offset: egui::Vec2, max_scroll: egui::Vec2) -> egui::Vec2 {
    egui::vec2(
        offset.x.clamp(0.0, max_scroll.x),
        offset.y.clamp(0.0, max_scroll.y),
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
