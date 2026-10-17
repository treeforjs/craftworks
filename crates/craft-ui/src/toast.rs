//! Toasts: short notices at the bottom centre, with an optional action such as Undo.
//!
//! Prefer an Undo toast to an "Are you sure?" dialog: do the action, then offer to reverse it.

use egui::{Align2, CornerRadius, Sense, Stroke, StrokeKind, vec2};

use crate::tokens::{motion, radius, space, type_size};
use crate::{fonts, style};

/// How a toast is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    /// A neutral notice ("Exported 3 pages").
    #[default]
    Info,
    /// Something finished well.
    Success,
    /// Something failed; the message says what to do next.
    Error,
}

/// One toast.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    /// What happened, in one short sentence.
    pub message: String,
    /// A button label and the command id it runs ("Undo", "edit.undo").
    pub action: Option<(String, String)>,
    /// Colour of the leading mark.
    pub kind: Kind,
    shown_at: Option<f64>,
}

impl Toast {
    /// A neutral toast.
    pub fn info(message: impl Into<String>) -> Self {
        Self { message: message.into(), action: None, kind: Kind::Info, shown_at: None }
    }

    /// A success toast.
    pub fn success(message: impl Into<String>) -> Self {
        Self { kind: Kind::Success, ..Self::info(message) }
    }

    /// An error toast.
    pub fn error(message: impl Into<String>) -> Self {
        Self { kind: Kind::Error, ..Self::info(message) }
    }

    /// Add an action button that runs `command` (a command id the app dispatches).
    pub fn with_action(mut self, label: impl Into<String>, command: impl Into<String>) -> Self {
        self.action = Some((label.into(), command.into()));
        self
    }

    /// The standard Undo toast after a destructive action.
    pub fn undo(message: impl Into<String>) -> Self {
        Self::info(message).with_action("Undo", "edit.undo")
    }
}

/// The toasts an app is showing. Keep one in the app state; push to it, show it every frame.
#[derive(Clone, Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

/// At most this many toasts are visible; older ones leave first.
pub const MAX_VISIBLE: usize = 3;

impl Toasts {
    /// Show `toast`. A toast with the same message replaces the older one.
    pub fn push(&mut self, toast: Toast) {
        self.items.retain(|t| t.message != toast.message);
        self.items.push(toast);
        let extra = self.items.len().saturating_sub(MAX_VISIBLE);
        self.items.drain(..extra);
    }

    /// The toasts currently queued, oldest first.
    pub fn items(&self) -> &[Toast] {
        &self.items
    }

    /// Remove every toast.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Drop toasts older than [`motion::TOAST_SECONDS`] at time `now` (seconds, as egui's
    /// `InputState::time`). A toast's clock starts the first time it is seen here.
    pub fn expire(&mut self, now: f64) {
        for t in &mut self.items {
            t.shown_at.get_or_insert(now);
        }
        self.items.retain(|t| t.shown_at.is_none_or(|s| now - s < motion::TOAST_SECONDS));
    }

    /// Draw the toasts and return the command id of an action the user clicked. `bottom_inset`
    /// lifts them above a status bar or timeline.
    pub fn show(&mut self, ctx: &egui::Context, bottom_inset: f32) -> Option<String> {
        let now = ctx.input(|i| i.time);
        let hovered_any = ctx.data(|d| d.get_temp::<bool>(egui::Id::new("craft-ui-toast-hover"))).unwrap_or(false);
        if hovered_any {
            // Keep them while the pointer is over one: restart every clock.
            for t in &mut self.items {
                t.shown_at = Some(now);
            }
        }
        self.expire(now);
        if self.items.is_empty() {
            return None;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let t = style::tokens(ctx);
        let screen = ctx.content_rect();
        let mut y = screen.bottom() - bottom_inset - space::LG;
        let mut clicked = None;
        let mut dismissed = None;
        let mut hovered = false;
        for (i, toast) in self.items.iter().enumerate().rev() {
            let area = egui::Area::new(egui::Id::new("craft-ui-toast").with(i))
                .order(egui::Order::Tooltip)
                .pivot(Align2::CENTER_BOTTOM)
                .fixed_pos(egui::pos2(screen.center().x, y))
                .interactable(true);
            let shown = area.show(ctx, |ui| {
                let frame = egui::Frame::NONE
                    .fill(t.surface_raised)
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(CornerRadius::same(radius::LG))
                    .inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8))
                    .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow });
                frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mark = match toast.kind {
                            Kind::Info => t.accent,
                            Kind::Success => t.success,
                            Kind::Error => t.danger,
                        };
                        let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, mark);
                        ui.label(egui::RichText::new(&toast.message).size(type_size::BODY).color(t.text));
                        if let Some((label, command)) = &toast.action {
                            let b = ui.add(
                                egui::Button::new(egui::RichText::new(label).font(fonts::medium(ui.ctx(), type_size::BODY)).color(t.accent_text))
                                    .frame(false),
                            );
                            if b.clicked() {
                                clicked = Some(command.clone());
                                dismissed = Some(i);
                            }
                        }
                        let close = ui.add(egui::Button::new(egui::RichText::new("×").size(type_size::TITLE).color(t.text_muted)).frame(false));
                        let close = close.on_hover_text("Dismiss");
                        if close.clicked() {
                            dismissed = Some(i);
                        }
                    });
                });
            });
            hovered |= shown.response.contains_pointer();
            y = shown.response.rect.top() - space::SM;
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("craft-ui-toast-hover"), hovered));
        if let Some(i) = dismissed
            && i < self.items.len()
        {
            self.items.remove(i);
        }
        let _ = StrokeKind::Inside;
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_expire_after_their_time() {
        let mut ts = Toasts::default();
        ts.push(Toast::undo("Deleted 3 layers"));
        ts.expire(10.0);
        assert_eq!(ts.items().len(), 1);
        ts.expire(10.0 + motion::TOAST_SECONDS - 0.1);
        assert_eq!(ts.items().len(), 1);
        ts.expire(10.0 + motion::TOAST_SECONDS + 0.1);
        assert!(ts.items().is_empty());
    }

    #[test]
    fn at_most_three_and_no_duplicates() {
        let mut ts = Toasts::default();
        for i in 0..5 {
            ts.push(Toast::info(format!("n{i}")));
        }
        ts.push(Toast::info("n4"));
        let msgs: Vec<&str> = ts.items().iter().map(|t| t.message.as_str()).collect();
        assert_eq!(msgs, ["n2", "n3", "n4"]);
    }

    #[test]
    fn undo_runs_edit_undo() {
        let t = Toast::undo("Deleted page 4");
        assert_eq!(t.action, Some(("Undo".to_owned(), "edit.undo".to_owned())));
    }

    #[test]
    fn show_draws_without_panicking() {
        let ctx = egui::Context::default();
        let mut ts = Toasts::default();
        ts.push(Toast::error("Couldn't save: the disk is full. Free some space and try again."));
        ts.push(Toast::undo("Deleted 3 layers"));
        for _ in 0..3 {
            crate::test_pass(&ctx, egui::RawInput::default(), |ui| {
                assert_eq!(ts.show(ui.ctx(), 28.0), None);
            });
        }
        assert_eq!(ts.items().len(), 2);
    }
}
