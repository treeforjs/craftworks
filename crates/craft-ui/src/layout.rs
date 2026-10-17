//! Building blocks of panels: inspector sections, panel headers and empty states.

use egui::{Align2, CornerRadius, Response, Sense, Ui, vec2};

use crate::controls::{self, Variant};
use crate::tokens::{space, type_size};
use crate::{fonts, icons, style};

/// A collapsible inspector section with a title row. Returns `Some` with the body's result
/// while it is open. The open state is remembered per `id_salt`.
pub fn section<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    default_open: bool,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let t = style::tokens(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let mut open = ui.data_mut(|d| d.get_persisted::<bool>(id)).unwrap_or(default_open);
    let h = t.control_height();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, title));
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(crate::tokens::radius::SM), t.hover);
    }
    let chevron = if open { "chevron-down" } else { "chevron-right" };
    let cr = egui::Rect::from_center_size(egui::pos2(rect.left() + 10.0, rect.center().y), vec2(12.0, 12.0));
    if !icons::paint(ui, cr, chevron, t.text_muted) {
        ui.painter().text(cr.center(), Align2::CENTER_CENTER, if open { "▾" } else { "▸" }, fonts::regular(type_size::SMALL), t.text_muted);
    }
    ui.painter().text(
        egui::pos2(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        fonts::semibold(ui.ctx(), type_size::BODY),
        t.text,
    );
    let out = if open {
        let r = ui.indent(id.with("body"), |ui| {
            ui.add_space(space::XS);
            let r = body(ui);
            ui.add_space(space::SM);
            r
        });
        Some(r.inner)
    } else {
        None
    };
    ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), egui::Stroke::new(1.0, t.divider));
    ui.add_space(1.0);
    out
}

/// A panel's title row.
pub fn panel_header(ui: &mut Ui, title: &str) {
    let t = style::tokens(ui.ctx());
    ui.add_space(space::SM);
    ui.label(egui::RichText::new(title).font(fonts::semibold(ui.ctx(), type_size::TITLE)).color(t.text));
    ui.add_space(space::XS);
}

/// What an empty panel says: what the panel is for, and the first step.
#[derive(Clone, Copy, Debug)]
pub struct EmptyState<'a> {
    /// An icon from the app's icon painter.
    pub icon: Option<&'a str>,
    /// What belongs here, in a few words ("No layers yet").
    pub title: &'a str,
    /// One sentence on what the panel is for or how to fill it.
    pub body: &'a str,
    /// The label of the button for the first step, if there is one.
    pub action: Option<&'a str>,
}

/// Draw an empty state centred in the available space. Returns the action button's response.
pub fn empty_state(ui: &mut Ui, e: &EmptyState<'_>) -> Option<Response> {
    let t = style::tokens(ui.ctx());
    let mut action = None;
    ui.vertical_centered(|ui| {
        ui.add_space(space::XXL);
        if let Some(icon) = e.icon {
            let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
            icons::paint(ui, r, icon, t.text_faint);
            ui.add_space(space::MD);
        }
        ui.label(egui::RichText::new(e.title).font(fonts::semibold(ui.ctx(), type_size::TITLE)).color(t.text));
        ui.add_space(space::XS);
        ui.label(egui::RichText::new(e.body).size(type_size::BODY).color(t.text_muted));
        if let Some(label) = e.action {
            ui.add_space(space::LG);
            action = Some(controls::button(ui, label, Variant::Primary));
        }
        ui.add_space(space::XXL);
    });
    action
}
