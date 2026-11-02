use std::time::Duration;

use eframe::egui;

use super::draw::dialog_frame;
use super::{ACCENT, PudduScrollApp, TEXT};

const TOAST_LIFE: Duration = Duration::from_millis(4_000);
const TOAST_MARGIN: f32 = 24.0;
const TOAST_BOTTOM: f32 = 32.0;
const TOAST_W: f32 = 360.0;
const TOAST_GAP: f32 = 14.0;
const TOAST_LIMIT: usize = 3;
const TOAST_OMEGA: f32 = 27.2;

#[derive(Debug)]
struct Toast {
    id: u64,
    text: String,
    life: Option<Duration>,
    undo: bool,
    open: bool,
    motion: Spring,
}

#[derive(Debug, Default)]
struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    fn step(&mut self, target: f32, dt: f32) -> bool {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return !self.resting(target);
        }
        let y0 = self.value - target;
        let v0 = self.velocity;
        let decay = (-TOAST_OMEGA * dt).exp();
        let kick = v0 + TOAST_OMEGA * y0;
        self.value = target + (y0 + kick * dt) * decay;
        self.velocity = (v0 - TOAST_OMEGA * kick * dt) * decay;
        if self.resting(target) {
            self.value = target;
            self.velocity = 0.0;
            return false;
        }
        true
    }

    fn resting(&self, target: f32) -> bool {
        (self.value - target).abs() < 0.001 && self.velocity.abs() < 0.02
    }
}

#[derive(Debug, Default)]
pub(super) struct Toaster {
    next: u64,
    items: Vec<Toast>,
    hover: bool,
}

impl Toaster {
    fn push(&mut self, text: String, life: Option<Duration>, undo: bool) {
        if text.is_empty() {
            return;
        }
        if let Some(toast) = self.items.iter_mut().find(|toast| toast.text == text) {
            toast.life = life;
            toast.undo = undo;
            toast.open = true;
            return;
        }
        if self.items.iter().filter(|toast| toast.open).count() >= TOAST_LIMIT
            && let Some(oldest) = self.items.iter_mut().find(|toast| toast.open)
        {
            oldest.open = false;
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.items.push(Toast {
            id,
            text,
            life,
            undo,
            open: true,
            motion: Spring::default(),
        });
    }

    fn close_undo(&mut self) {
        for toast in &mut self.items {
            if toast.undo {
                toast.open = false;
            }
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if self.hover || !ctx.input(|input| input.focused) {
            return;
        }
        let step = Duration::from_secs_f32(ctx.input(|input| input.stable_dt).max(0.0));
        for toast in &mut self.items {
            if !toast.open {
                continue;
            }
            let Some(life) = toast.life.as_mut() else {
                continue;
            };
            *life = life.saturating_sub(step);
            if life.is_zero() {
                toast.open = false;
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context) -> bool {
        self.tick(ctx);
        if self.items.is_empty() {
            self.hover = false;
            return false;
        }
        let focused = ctx.input(|input| input.focused);
        let dt = ctx.input(|input| input.stable_dt);
        let mut hover = false;
        let mut undo = false;
        let mut dismiss = Vec::new();
        let mut finished = Vec::new();
        let mut follow = false;
        let inset = toast_inset();
        let area = egui::Area::new(egui::Id::unique("toasts"))
            .order(egui::Order::Debug)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-inset.x, -inset.y))
            .fade_in(false)
            .interactable(true)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    ui.spacing_mut().item_spacing.y = TOAST_GAP;
                    for index in (0..self.items.len()).rev() {
                        let toast = &mut self.items[index];
                        let target = if toast.open { 1.0 } else { 0.0 };
                        if toast.motion.step(target, dt) {
                            follow = true;
                        }
                        if !toast.open && toast.motion.resting(0.0) {
                            finished.push(toast.id);
                            continue;
                        }
                        let shown = toast.motion.value.clamp(0.0, 1.0);
                        let toast_id = toast.id;
                        let text = toast.text.clone();
                        let undoable = toast.undo;
                        ui.push_id(toast_id, |ui| {
                            let layer_id = ui.layer_id();
                            let start = ui.ctx().graphics(|gfx| {
                                gfx.get(layer_id).map_or(
                                    egui::layers::ShapeIdx(0),
                                    egui::layers::PaintList::next_idx,
                                )
                            });
                            ui.set_opacity(shown);
                            ui.set_max_width(TOAST_W);
                            let mut acted = false;
                            let inner = toast_frame().show(ui, |ui| {
                                acted = toast_body(ui, &text, undoable);
                            });
                            let transform =
                                toast_transform(inner.response.rect.right_bottom(), shown);
                            if transform != egui::emath::TSTransform::IDENTITY {
                                ui.ctx().graphics_mut(|gfx| {
                                    let layer = gfx.entry(layer_id);
                                    let end = layer.next_idx();
                                    layer.transform_after_rounding_range(start, end, transform);
                                });
                            }
                            if undoable {
                                if inner.response.container_hovered() {
                                    hover = true;
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if inner.response.container_clicked() || acted {
                                    dismiss.push(toast_id);
                                }
                                if acted {
                                    undo = true;
                                }
                            } else {
                                let response = inner.response.interact(egui::Sense::click());
                                if response.hovered() {
                                    hover = true;
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if response.clicked() {
                                    dismiss.push(toast_id);
                                }
                            }
                        });
                    }
                });
            });
        hover |= area.response.hovered();
        for id in dismiss {
            if let Some(toast) = self.items.iter_mut().find(|toast| toast.id == id) {
                toast.open = false;
            }
        }
        self.items.retain(|toast| !finished.contains(&toast.id));
        self.hover = hover;
        if focused
            && !hover
            && self
                .items
                .iter()
                .any(|toast| toast.open && toast.life.is_some())
        {
            follow = true;
        }
        if follow {
            ctx.request_repaint();
        }
        undo
    }
}

fn toast_body(ui: &mut egui::Ui, text: &str, undoable: bool) -> bool {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    if !undoable {
        ui.label(egui::RichText::new(text).color(TEXT).size(14.0));
        return false;
    }
    let mut acted = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let undo = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Undo")
                            .color(egui::Color32::BLACK)
                            .size(14.0),
                    )
                    .fill(ACCENT),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if undo.clicked() {
                acted = true;
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.label(egui::RichText::new(text).color(TEXT).size(14.0));
            });
        });
    });
    acted
}

fn toast_shadow() -> egui::Shadow {
    egui::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: egui::Color32::from_black_alpha(96),
    }
}

fn toast_inset() -> egui::Vec2 {
    let edge = toast_shadow().margin();
    egui::vec2(TOAST_MARGIN + edge.right, TOAST_BOTTOM + edge.bottom)
}

fn toast_transform(pivot: egui::Pos2, shown: f32) -> egui::emath::TSTransform {
    let scale = 1.0 - 0.06 * (1.0 - shown);
    egui::emath::TSTransform::new((1.0 - scale) * pivot.to_vec2(), scale)
}

fn toast_frame() -> egui::Frame {
    dialog_frame().shadow(toast_shadow())
}

impl PudduScrollApp {
    pub(super) fn toast(&mut self, message: impl Into<String>) {
        self.toaster.push(message.into(), Some(TOAST_LIFE), false);
    }

    pub(super) fn toast_undo(&mut self, message: impl Into<String>) {
        self.toaster.push(message.into(), Some(TOAST_LIFE), true);
    }

    pub(super) fn toast_persist(&mut self, message: impl Into<String>) {
        self.toaster.push(message.into(), None, false);
    }

    pub(super) fn dismiss_undo_toasts(&mut self) {
        self.toaster.close_undo();
    }

    pub(super) fn show_toasts(&mut self, ctx: &egui::Context) {
        if self.toaster.show(ctx) {
            self.undo_edit(ctx);
        }
    }

    pub(super) fn toast_messages(&self) -> impl Iterator<Item = &str> + '_ {
        self.toaster
            .items
            .iter()
            .filter(|toast| toast.open)
            .map(|toast| toast.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    use super::{
        Spring, TOAST_BOTTOM, TOAST_LIFE, TOAST_MARGIN, Toaster, toast_inset, toast_shadow,
        toast_transform,
    };

    #[test]
    fn the_toast_keeps_a_margin_under_the_shadow() {
        let inset = toast_inset();
        let edge = toast_shadow().margin();
        let right = inset.x - edge.right;
        let bottom = inset.y - edge.bottom;
        assert!((right - TOAST_MARGIN).abs() < f32::EPSILON);
        assert!((bottom - TOAST_BOTTOM).abs() < f32::EPSILON);
    }

    #[test]
    fn the_toast_scales_in_place_without_sliding() {
        let pivot = egui::pos2(400.0, 300.0);
        let settled = toast_transform(pivot, 1.0);
        assert_eq!(settled, egui::emath::TSTransform::IDENTITY);
        let hidden = toast_transform(pivot, 0.0);
        assert!((hidden.scaling - 0.94).abs() < 0.001);
        let corner = hidden.mul_pos(pivot);
        assert!((corner.x - pivot.x).abs() < 0.001);
        assert!((corner.y - pivot.y).abs() < 0.001);
    }

    #[test]
    fn the_spring_arrives_quickly_and_reverses_in_flight() {
        let mut spring = Spring::default();
        let frame = 1.0 / 60.0;
        for _ in 0..13 {
            spring.step(1.0, frame);
        }
        assert!(spring.value > 0.95, "{}", spring.value);
        assert!(spring.value < 1.02, "{}", spring.value);
        let going = spring.velocity;
        spring.step(0.0, frame);
        assert!(spring.velocity < going);
    }

    #[test]
    fn a_newer_edit_closes_the_undo_toast() {
        let mut toaster = Toaster::default();
        toaster.push("Frame deleted".into(), Some(TOAST_LIFE), true);
        toaster.push("Choose a cover".into(), Some(TOAST_LIFE), false);
        toaster.close_undo();
        let open: Vec<_> = toaster
            .items
            .iter()
            .filter(|toast| toast.open)
            .map(|toast| toast.text.as_str())
            .collect();
        assert_eq!(open, ["Choose a cover"]);
    }
}
