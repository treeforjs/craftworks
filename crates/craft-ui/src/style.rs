//! Turning tokens into egui's style, and reading them back from any widget.

use egui::{Color32, CornerRadius, Stroke, Visuals};

use crate::fonts;
use crate::tokens::{Appearance, Density, Mode, Tokens, motion, radius, type_size};

fn key() -> egui::Id {
    egui::Id::new("craft-ui-tokens")
}

/// What the user chose in Settings › Appearance, plus the app's dark preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Theme {
    /// System, Light, Cream, Dim, Dark or High contrast.
    pub appearance: Appearance,
    /// Comfortable or Compact.
    pub density: Density,
    /// Draw dark when the appearance is System and the OS reports nothing. True for photo, video
    /// and motion apps, where a dark surround helps judge colour.
    pub prefer_dark: bool,
    /// Turn transitions off (the OS reduced-motion setting, or the app's own switch).
    pub reduced_motion: bool,
}

impl Theme {
    /// The mode this theme draws in right now.
    pub fn mode(&self, ctx: &egui::Context) -> Mode {
        self.appearance.resolve(ctx.system_theme(), self.prefer_dark)
    }
}

/// The tokens the last [`apply`] installed (light, comfortable before the first one).
pub fn tokens(ctx: &egui::Context) -> Tokens {
    ctx.data(|d| d.get_temp::<Tokens>(key())).unwrap_or_default()
}

/// Install `theme`: store its tokens for [`tokens`] and set egui's visuals, spacing and text
/// styles to match. Cheap enough to call every frame; call it at least when the theme or the
/// OS appearance changes.
pub fn apply(ctx: &egui::Context, theme: &Theme) -> Tokens {
    let t = Tokens::new(theme.mode(ctx), theme.density);
    if ctx.data(|d| d.get_temp::<Tokens>(key())) == Some(t) {
        return t;
    }
    ctx.data_mut(|d| d.insert_temp(key(), t));
    ctx.set_theme(if t.is_dark() { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(visuals(&t));
    let h = t.control_height();
    let compact = theme.density == Density::Compact;
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = if compact { egui::vec2(6.0, 4.0) } else { egui::vec2(8.0, 6.0) };
        s.spacing.button_padding = if compact { egui::vec2(8.0, 3.0) } else { egui::vec2(10.0, 5.0) };
        s.spacing.interact_size = egui::vec2(h, h);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.window_margin = egui::Margin::same(16);
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.floating = true;
        s.text_styles.insert(egui::TextStyle::Body, fonts::regular(type_size::BODY));
        s.text_styles.insert(egui::TextStyle::Button, fonts::regular(type_size::BODY));
        s.text_styles.insert(egui::TextStyle::Small, fonts::regular(type_size::SMALL));
        s.text_styles.insert(egui::TextStyle::Monospace, fonts::mono(type_size::SMALL));
        // Proportional, not semibold: the app's semibold face may not be installed yet.
        s.text_styles.insert(egui::TextStyle::Heading, fonts::regular(type_size::HEADING));
        s.interaction.tooltip_delay = motion::TOOLTIP_DELAY;
        s.animation_time = if theme.reduced_motion { 0.0 } else { motion::FAST };
    });
    t
}

/// egui visuals for `t`.
pub fn visuals(t: &Tokens) -> Visuals {
    let mut v = if t.is_dark() { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.surface;
    v.window_fill = t.surface_raised;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.extreme_bg_color = t.field;
    v.text_edit_bg_color = Some(t.field);
    v.faint_bg_color = t.hover;
    v.code_bg_color = t.hover;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent_text;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.danger;
    v.override_text_color = Some(t.text);
    v.window_corner_radius = CornerRadius::same(radius::XL);
    v.menu_corner_radius = CornerRadius::same(radius::LG);
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: t.shadow };
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow };
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(radius::MD);
        w.fg_stroke = Stroke::new(1.0, t.text);
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_fill = t.surface;
    v.widgets.noninteractive.weak_bg_fill = t.surface;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_muted);
    // Slider rails and checkbox boxes: visible on every surface.
    v.widgets.inactive.bg_fill = t.pressed;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.active.bg_fill = t.pressed;
    v.widgets.active.weak_bg_fill = t.pressed;
    v.widgets.active.bg_stroke = Stroke::new(1.0, t.focus);
    v.widgets.open.bg_fill = t.hover;
    v.widgets.open.weak_bg_fill = t.hover;
    v.widgets.open.bg_stroke = Stroke::new(1.0, t.border);
    v.slider_trailing_fill = true;
    v.text_options.subpixel_binning = false;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_stores_the_tokens_it_drew() {
        let ctx = egui::Context::default();
        let theme = Theme { appearance: Appearance::Dark, density: Density::Compact, ..Theme::default() };
        crate::test_pass(&ctx, egui::RawInput::default(), |ui| {
            let t = apply(ui.ctx(), &theme);
            assert_eq!(tokens(ui.ctx()), t);
            assert_eq!(t.mode, Mode::Dark);
            assert_eq!(t.control_height(), 24.0);
            assert!(ui.ctx().global_style().visuals.dark_mode);
        });
    }

    #[test]
    fn tokens_default_before_apply() {
        let ctx = egui::Context::default();
        assert_eq!(tokens(&ctx), Tokens::default());
    }
}
