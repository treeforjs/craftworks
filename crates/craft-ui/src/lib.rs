//! craft-ui: the shared design system of the craft apps (PdfCraft, PhotoCraft, VectorCraft,
//! FilmCraft, EffectCraft, DesignCraft, LightCraft).
//!
//! One set of tokens and one set of components, so the same action looks, sits and is named the
//! same in every app. The design direction is in `DESIGN.md`.
//!
//! ```no_run
//! fn frame(ui: &mut egui::Ui, theme: &craft_ui::Theme) {
//!     let t = craft_ui::apply(ui.ctx(), theme); // every frame, or when the theme changes
//!     if craft_ui::button(ui, "Export", craft_ui::Variant::Primary).clicked() {
//!         // run "file.export" through the app's command registry
//!     }
//!     ui.label(egui::RichText::new("Saved").color(t.text_muted));
//! }
//! ```

pub mod controls;
pub mod fonts;
pub mod icons;
pub mod layout;
pub mod palette;
pub mod shortcuts;
pub mod style;
pub mod toast;
pub mod tokens;

pub use controls::{Tip, Variant, button, button_with_icon, icon_button, search_trigger, segmented, with_tip};
pub use layout::{EmptyState, empty_state, panel_header, section};
pub use palette::{Item as PaletteItem, ItemKind as PaletteItemKind, Palette};
pub use style::{Theme, apply, tokens};
pub use toast::{Toast, Toasts};
pub use tokens::{Appearance, Density, Mode, Tokens, contrast};

/// Settings › Appearance: the appearance and density pickers every app shows the same way.
/// Returns true when either changed.
pub fn appearance_settings(ui: &mut egui::Ui, theme: &mut Theme) -> bool {
    let t = style::tokens(ui.ctx());
    let mut changed = false;
    egui::Grid::new("craft-ui-appearance").num_columns(2).spacing([tokens::space::LG, tokens::space::SM]).show(ui, |ui| {
        ui.label(egui::RichText::new("Appearance").color(t.text_muted));
        let mut i = Appearance::ALL.iter().position(|a| *a == theme.appearance).unwrap_or(0);
        let labels: Vec<&str> = Appearance::ALL.iter().map(|a| a.label()).collect();
        if segmented(ui, &mut i, &labels).changed() {
            theme.appearance = Appearance::ALL.get(i).copied().unwrap_or_default();
            changed = true;
        }
        ui.end_row();
        ui.label(egui::RichText::new("Density").color(t.text_muted));
        let mut i = Density::ALL.iter().position(|d| *d == theme.density).unwrap_or(0);
        let labels: Vec<&str> = Density::ALL.iter().map(|d| d.label()).collect();
        if segmented(ui, &mut i, &labels).changed() {
            theme.density = Density::ALL.get(i).copied().unwrap_or_default();
            changed = true;
        }
        ui.end_row();
        ui.label(egui::RichText::new("Motion").color(t.text_muted));
        changed |= ui.checkbox(&mut theme.reduced_motion, "Reduce motion").changed();
        ui.end_row();
    });
    changed
}

/// Run one egui pass in a test and drop its output the way egui expects.
#[cfg(test)]
pub(crate) fn test_pass(ctx: &egui::Context, input: egui::RawInput, f: impl FnMut(&mut egui::Ui)) {
    let mut out = ctx.run_ui(input, f);
    out.textures_delta.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every component draws in every mode and density without panicking, including with no
    /// fonts or icon painter installed and in a zero-sized window.
    #[test]
    fn gallery_draws_in_every_mode() {
        for appearance in Appearance::ALL {
            for density in Density::ALL {
                for size in [egui::vec2(1024.0, 768.0), egui::vec2(0.0, 0.0)] {
                    let ctx = egui::Context::default();
                    let mut theme = Theme { appearance, density, ..Theme::default() };
                    let mut toasts = Toasts::default();
                    toasts.push(Toast::undo("Deleted 3 layers"));
                    let mut palette = Palette::default();
                    palette.open();
                    let items = [PaletteItem::command("file.export", "Export").shortcut("⌘⇧E")];
                    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
                    crate::test_pass(&ctx, input, |ui| {
                        apply(ui.ctx(), &theme);
                        button(ui, "Export", Variant::Primary);
                        button(ui, "Cancel", Variant::Secondary);
                        button(ui, "More", Variant::Ghost);
                        button(ui, "Delete forever", Variant::Danger);
                        icon_button(ui, "brush", &Tip::new("Brush").shortcut("B").detail("Paint with the foreground colour"), true);
                        search_trigger(ui, "Search or run a command", 260.0);
                        let mut seg = 1;
                        segmented(ui, &mut seg, &["Fit", "Fill", "Stretch"]);
                        section(ui, "pos", "Position and size", true, |ui| ui.label("x"));
                        section(ui, "fx", "Effects", false, |ui| ui.label("y"));
                        panel_header(ui, "Layers");
                        empty_state(
                            ui,
                            &EmptyState { icon: Some("layers"), title: "No layers yet", body: "Add a layer to start.", action: Some("New layer") },
                        );
                        appearance_settings(ui, &mut theme);
                        toasts.show(ui.ctx(), 28.0);
                        palette.show(ui.ctx(), &items);
                    });
                }
            }
        }
    }
}
