//! Buttons, icon buttons, tooltips, segmented choices and the top bar's search trigger.

use egui::{Align2, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::tokens::{Tokens, radius, space, type_size};
use crate::{fonts, icons, style};

/// The four button kinds. One `Primary` per view at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Variant {
    /// The main action of a view: accent fill.
    Primary,
    /// Other actions: outlined.
    #[default]
    Secondary,
    /// Low-emphasis actions in bars and lists: no outline until hovered.
    Ghost,
    /// Actions that destroy data and cannot be undone.
    Danger,
}

fn measure(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER).size().x
}

fn focus_ring(ui: &Ui, rect: Rect, resp: &Response, t: &Tokens) {
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), CornerRadius::same(radius::MD + 2), Stroke::new(2.0, t.focus), StrokeKind::Outside);
    }
}

/// A text button with an optional leading icon.
pub fn button(ui: &mut Ui, label: &str, variant: Variant) -> Response {
    button_with_icon(ui, None, label, variant)
}

/// A button with a leading icon from the app's icon painter.
pub fn button_with_icon(ui: &mut Ui, icon: Option<&str>, label: &str, variant: Variant) -> Response {
    let t = style::tokens(ui.ctx());
    let font = fonts::medium(ui.ctx(), type_size::BODY);
    let h = t.control_height();
    let icon_w = if icon.is_some() { 16.0 + space::SM } else { 0.0 };
    let w = measure(ui, label, font.clone()) + icon_w + 2.0 * space::MD;
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(h), h), Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered() && enabled;
        let pressed = resp.is_pointer_button_down_on() && enabled;
        let (fill, stroke, fg) = match variant {
            Variant::Primary => (if hovered { t.accent_hover } else { t.accent }, Stroke::NONE, t.text_on_accent),
            Variant::Danger => (if hovered { t.danger.gamma_multiply(0.9) } else { t.danger }, Stroke::NONE, t.text_on_accent),
            Variant::Secondary => (
                if pressed {
                    t.pressed
                } else if hovered {
                    t.hover
                } else {
                    t.surface
                },
                Stroke::new(1.0, t.border),
                t.text,
            ),
            Variant::Ghost => (
                if pressed {
                    t.pressed
                } else if hovered {
                    t.hover
                } else {
                    egui::Color32::TRANSPARENT
                },
                Stroke::NONE,
                t.text,
            ),
        };
        let fg = if enabled { fg } else { t.text_disabled };
        let fill = if enabled || matches!(variant, Variant::Ghost) { fill } else { t.hover };
        ui.painter().rect(rect, CornerRadius::same(radius::MD), fill, stroke, StrokeKind::Inside);
        let mut x = rect.left() + space::MD;
        if let Some(name) = icon {
            let r = Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0));
            icons::paint(ui, r, name, fg);
            x += icon_w;
        }
        ui.painter().text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, label, font, fg);
        focus_ring(ui, rect, &resp, &t);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A square icon-only button, for tool rails and bars. Always has a tooltip: name, shortcut and
/// one line of explanation.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &Tip<'_>, selected: bool) -> Response {
    let t = style::tokens(ui.ctx());
    let s = t.density.tool_size();
    let (rect, resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, tip.name));
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            t.accent_soft
        } else if resp.is_pointer_button_down_on() {
            t.pressed
        } else if resp.hovered() {
            t.hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(radius::MD), fill);
        let fg = if !enabled {
            t.text_disabled
        } else if selected {
            t.accent_text
        } else {
            t.icon
        };
        let icon_rect = Rect::from_center_size(rect.center(), vec2(20.0, 20.0));
        if !icons::paint(ui, icon_rect, icon, fg) {
            // No painter: the first letter keeps the button recognisable.
            let letter: String = tip.name.chars().take(1).collect();
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, fonts::medium(ui.ctx(), type_size::BODY), fg);
        }
        focus_ring(ui, rect, &resp, &t);
    }
    with_tip(resp, tip)
}

/// What a tooltip says.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tip<'a> {
    /// The name, as the menu and palette show it.
    pub name: &'a str,
    /// The shortcut, already formatted for the platform ("⌘K", "Ctrl+K"), if any.
    pub shortcut: Option<&'a str>,
    /// One line on what it does, if the name alone is not clear.
    pub detail: Option<&'a str>,
}

impl<'a> Tip<'a> {
    /// A tooltip with just a name.
    pub fn new(name: &'a str) -> Self {
        Self { name, shortcut: None, detail: None }
    }

    /// Add the shortcut.
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Add one line of explanation.
    pub fn detail(mut self, detail: &'a str) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// Attach the standard tooltip to any response.
pub fn with_tip(resp: Response, tip: &Tip<'_>) -> Response {
    let tip = *tip;
    resp.on_hover_ui(move |ui| {
        let t = style::tokens(ui.ctx());
        ui.set_max_width(280.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tip.name).font(fonts::medium(ui.ctx(), type_size::BODY)).color(t.text));
            if let Some(k) = tip.shortcut {
                ui.label(egui::RichText::new(k).size(type_size::SMALL).color(t.text_faint));
            }
        });
        if let Some(d) = tip.detail {
            ui.label(egui::RichText::new(d).size(type_size::SMALL).color(t.text_muted));
        }
    })
}

/// A row of mutually exclusive choices. Returns the response of the row; `selected` changes when
/// the user picks another option.
pub fn segmented(ui: &mut Ui, selected: &mut usize, options: &[&str]) -> Response {
    let t = style::tokens(ui.ctx());
    let h = t.control_height();
    let font = fonts::regular(type_size::BODY);
    let widths: Vec<f32> = options.iter().map(|o| measure(ui, o, font.clone()) + 2.0 * space::MD).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    ui.painter().rect(rect, CornerRadius::same(radius::MD), t.field, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let mut x = rect.left() + 2.0;
    for (i, (label, w)) in options.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), vec2(*w, h - 4.0));
        let id = resp.id.with(i);
        let seg = ui.interact(r, id, Sense::click());
        let on = *selected == i;
        seg.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, *label));
        if seg.clicked() && !on {
            *selected = i;
            resp.mark_changed();
        }
        let fill = if on {
            t.surface_raised
        } else if seg.hovered() {
            t.hover
        } else {
            egui::Color32::TRANSPARENT
        };
        let stroke = if on { Stroke::new(1.0, t.border) } else { Stroke::NONE };
        ui.painter().rect(r, CornerRadius::same(radius::SM), fill, stroke, StrokeKind::Inside);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), if on { t.text } else { t.text_muted });
        focus_ring(ui, r, &seg, &t);
        x += w;
    }
    resp
}

/// The search field in the top bar. It is a button that opens the command palette, showing the
/// palette's shortcut on the right.
pub fn search_trigger(ui: &mut Ui, placeholder: &str, width: f32) -> Response {
    let t = style::tokens(ui.ctx());
    let h = t.control_height();
    let (rect, resp) = ui.allocate_exact_size(vec2(width.max(h * 3.0), h), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, placeholder));
    if ui.is_rect_visible(rect) {
        let fill = if resp.hovered() { t.hover } else { t.field };
        ui.painter().rect(rect, CornerRadius::same(radius::MD), fill, Stroke::new(1.0, t.border), StrokeKind::Inside);
        let icon_rect = Rect::from_center_size(egui::pos2(rect.left() + space::MD + 7.0, rect.center().y), vec2(14.0, 14.0));
        let has_icon = icons::paint(ui, icon_rect, "search", t.text_muted);
        let text_left = if has_icon { icon_rect.right() + space::SM } else { rect.left() + space::MD };
        let shortcut = ui.ctx().format_shortcut(&crate::shortcuts::PALETTE);
        let shortcut = ui.painter().layout_no_wrap(shortcut, fonts::regular(type_size::SMALL), t.text_faint);
        let sx = rect.right() - space::MD - shortcut.size().x;
        let clip = Rect::from_min_max(egui::pos2(text_left, rect.top()), egui::pos2((sx - space::SM).max(text_left), rect.bottom()));
        let mut job = egui::text::LayoutJob::simple_singleline(placeholder.to_owned(), fonts::regular(type_size::BODY), t.text_faint);
        job.wrap = egui::text::TextWrapping::truncate_at_width(clip.width());
        let galley = ui.painter().layout_job(job);
        let gy = rect.center().y - galley.size().y * 0.5;
        ui.painter().with_clip_rect(clip).galley(egui::pos2(text_left, gy), galley, t.text_faint);
        let shy = rect.center().y - shortcut.size().y * 0.5;
        ui.painter().with_clip_rect(rect).galley(egui::pos2(sx, shy), shortcut, t.text_faint);
        focus_ring(ui, rect, &resp, &t);
    }
    resp.on_hover_cursor(egui::CursorIcon::Text)
}
