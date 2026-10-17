//! Icons. craft-ui ships no icon files: each app registers a painter that draws its Lucide icons
//! by name, and craft-ui components call it. Without one, components draw text only.

use egui::{Color32, Painter, Rect};

/// Paints the icon `name` into `rect` in `color`. Return false when the name is unknown.
pub type IconPainter = fn(painter: &Painter, rect: Rect, name: &str, color: Color32) -> bool;

fn key() -> egui::Id {
    egui::Id::new("craft-ui-icon-painter")
}

/// Register the app's icon painter.
pub fn set_painter(ctx: &egui::Context, painter: IconPainter) {
    ctx.data_mut(|d| d.insert_temp(key(), painter));
}

/// Paint `name` into `rect` with the registered painter. False when no painter is registered or
/// it does not know the name, so callers can leave the space empty.
pub fn paint(ui: &egui::Ui, rect: Rect, name: &str, color: Color32) -> bool {
    let painter: Option<IconPainter> = ui.ctx().data(|d| d.get_temp(key()));
    painter.is_some_and(|p| p(ui.painter(), rect, name, color))
}
