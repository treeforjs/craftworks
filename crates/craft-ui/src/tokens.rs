//! Design tokens: the one colour, type, spacing, radius and motion scale every craft app uses.
//!
//! Values are our own (see `DESIGN.md`); nothing here is measured from another vendor's UI.
//! `tests::contrast_meets_wcag_aa` keeps every text and control colour readable.

use egui::Color32;

/// What the user picked in Settings › Appearance. Saved in each app's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Appearance {
    /// Follow the operating system's light or dark setting.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
    /// A softer dark: mid greys instead of near-black, for long sessions.
    Dim,
    /// Warm paper tones with a red accent: light, but easier on the eyes than white.
    Cream,
    /// Dark with maximum contrast and a yellow accent.
    HighContrast,
}

impl Appearance {
    /// Every choice, in the order Settings shows them.
    pub const ALL: [Appearance; 6] =
        [Appearance::System, Appearance::Light, Appearance::Cream, Appearance::Dim, Appearance::Dark, Appearance::HighContrast];

    /// The label Settings shows.
    pub fn label(self) -> &'static str {
        match self {
            Appearance::System => "System",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
            Appearance::Dim => "Dim",
            Appearance::Cream => "Cream",
            Appearance::HighContrast => "High contrast",
        }
    }

    /// A stable id for settings files, the CLI and the control channel.
    pub fn id(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
            Appearance::Dim => "dim",
            Appearance::Cream => "cream",
            Appearance::HighContrast => "high-contrast",
        }
    }

    /// Parse an id written by [`Appearance::id`], or a theme name saved by an older build of any
    /// craft app ("darkest", "medium-light", "pro", "studio-light" and so on), mapped to the
    /// nearest choice. Unknown names give `None` so the caller keeps its default.
    pub fn parse(name: &str) -> Option<Self> {
        let n: String = name.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if n.is_empty() {
            return None;
        }
        Some(match n.as_str() {
            "system" | "auto" | "os" => Appearance::System,
            "light" | "day" => Appearance::Light,
            "dark" | "night" => Appearance::Dark,
            // older builds' mid greys ("Medium", "Medium Dark", "ProMedium", "grey") were the dim choice
            "dim" | "medium" | "mediumdark" | "promedium" | "grey" | "gray" | "dusk" => Appearance::Dim,
            "cream" | "warm" | "sepia" | "paper" => Appearance::Cream,
            "highcontrast" | "contrast" | "hc" => Appearance::HighContrast,
            _ if n.contains("contrast") => Appearance::HighContrast,
            _ if n.contains("light") => Appearance::Light,
            // Older builds' dark greys: darkest, darker, mediumdark, pro, promedium, studio, classic,
            // adwaitadark, solarizeddark and the like.
            _ if n.contains("dark") || n.contains("pro") || n.contains("studio") || n.contains("medium") || n.contains("classic") => Appearance::Dark,
            _ => return None,
        })
    }

    /// The mode to draw: `system` is what the OS reports (`egui::Context::system_theme`), and
    /// `prefer_dark` is the app's choice when the OS says nothing (photo, video and motion apps
    /// prefer dark, document apps light).
    pub fn resolve(self, system: Option<egui::Theme>, prefer_dark: bool) -> Mode {
        match self {
            Appearance::Light => Mode::Light,
            Appearance::Dark => Mode::Dark,
            Appearance::Dim => Mode::Dim,
            Appearance::Cream => Mode::Cream,
            Appearance::HighContrast => Mode::HighContrast,
            Appearance::System => match system {
                Some(egui::Theme::Light) => Mode::Light,
                Some(egui::Theme::Dark) => Mode::Dark,
                None if prefer_dark => Mode::Dark,
                None => Mode::Light,
            },
        }
    }
}

/// The colours actually drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// Light surfaces, dark text.
    #[default]
    Light,
    /// Dark surfaces, light text.
    Dark,
    /// Mid-grey surfaces, light text: between Light and Dark.
    Dim,
    /// Warm cream surfaces, dark brown text, red accent.
    Cream,
    /// Black surfaces, white text, yellow accent.
    HighContrast,
}

impl Mode {
    /// True for the modes drawn on dark surfaces.
    pub fn is_dark(self) -> bool {
        !matches!(self, Mode::Light | Mode::Cream)
    }
}

/// How tightly controls are packed. Settings › Appearance › Density.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Density {
    /// 28 px controls. The default.
    #[default]
    Comfortable,
    /// 24 px controls, for dense professional layouts.
    Compact,
}

impl Density {
    /// Both choices, in the order Settings shows them.
    pub const ALL: [Density; 2] = [Density::Comfortable, Density::Compact];

    /// The label Settings shows.
    pub fn label(self) -> &'static str {
        match self {
            Density::Comfortable => "Comfortable",
            Density::Compact => "Compact",
        }
    }

    /// A stable id for settings files.
    pub fn id(self) -> &'static str {
        match self {
            Density::Comfortable => "comfortable",
            Density::Compact => "compact",
        }
    }

    /// Parse an id written by [`Density::id`].
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "comfortable" | "default" | "normal" => Some(Density::Comfortable),
            "compact" | "dense" => Some(Density::Compact),
            _ => None,
        }
    }

    /// Height of buttons, fields and rows.
    pub fn control_height(self) -> f32 {
        match self {
            Density::Comfortable => 28.0,
            Density::Compact => 24.0,
        }
    }

    /// Height of an icon button in a tool rail.
    pub fn tool_size(self) -> f32 {
        match self {
            Density::Comfortable => 36.0,
            Density::Compact => 30.0,
        }
    }
}

/// The 4 px spacing grid.
pub mod space {
    /// 4 px: inside compact controls.
    pub const XS: f32 = 4.0;
    /// 8 px: between related items.
    pub const SM: f32 = 8.0;
    /// 12 px: panel padding.
    pub const MD: f32 = 12.0;
    /// 16 px: between groups.
    pub const LG: f32 = 16.0;
    /// 24 px: between sections.
    pub const XL: f32 = 24.0;
    /// 32 px: around empty states and dialogs.
    pub const XXL: f32 = 32.0;
}

/// Corner radii.
pub mod radius {
    /// Fields, chips, small swatches.
    pub const SM: u8 = 4;
    /// Buttons and controls.
    pub const MD: u8 = 6;
    /// Panels, cards, menus, popovers.
    pub const LG: u8 = 10;
    /// Dialogs and the command palette.
    pub const XL: u8 = 14;
}

/// Type sizes in points. The UI face is Inter; numbers that change use JetBrains Mono.
pub mod type_size {
    /// Captions, badges.
    pub const CAPTION: f32 = 11.0;
    /// Secondary text, shortcuts, status bar.
    pub const SMALL: f32 = 12.0;
    /// Body text, buttons, fields.
    pub const BODY: f32 = 13.0;
    /// Panel and dialog titles (semibold).
    pub const TITLE: f32 = 15.0;
    /// Page headings such as Home (semibold).
    pub const HEADING: f32 = 18.0;
}

/// Motion durations in seconds. Zero when the user asks for reduced motion.
pub mod motion {
    /// Hover and press feedback.
    pub const FAST: f32 = 0.12;
    /// Panels and popovers opening.
    pub const NORMAL: f32 = 0.2;
    /// How long a toast stays when nobody hovers it.
    pub const TOAST_SECONDS: f64 = 6.0;
    /// Delay before a tooltip shows.
    pub const TOOLTIP_DELAY: f32 = 0.5;
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Every colour of one mode, plus the density. Read with [`crate::tokens`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// The mode these colours draw.
    pub mode: Mode,
    /// The density sizes come from.
    pub density: Density,
    /// Window background and pasteboard.
    pub bg: Color32,
    /// Panels and bars.
    pub surface: Color32,
    /// Popovers, menus, dialogs, toasts.
    pub surface_raised: Color32,
    /// Text and number fields.
    pub field: Color32,
    /// Field and card outlines.
    pub border: Color32,
    /// Separators between regions.
    pub divider: Color32,
    /// Hovered controls and rows.
    pub hover: Color32,
    /// Pressed controls.
    pub pressed: Color32,
    /// Body text.
    pub text: Color32,
    /// Labels and secondary text.
    pub text_muted: Color32,
    /// Hints and placeholders (still WCAG AA on every surface).
    pub text_faint: Color32,
    /// Disabled text (exempt from contrast rules, like WCAG).
    pub text_disabled: Color32,
    /// Text and icons on an `accent` fill.
    pub text_on_accent: Color32,
    /// Icons on surfaces.
    pub icon: Color32,
    /// Primary buttons, selection outline.
    pub accent: Color32,
    /// Hovered primary buttons.
    pub accent_hover: Color32,
    /// Links and accent-coloured text.
    pub accent_text: Color32,
    /// Selected rows, the active tool's background.
    pub accent_soft: Color32,
    /// The keyboard focus ring.
    pub focus: Color32,
    /// Success status.
    pub success: Color32,
    /// Warning status.
    pub warning: Color32,
    /// Errors and destructive actions.
    pub danger: Color32,
    /// Shadow under floating things.
    pub shadow: Color32,
}

impl Tokens {
    /// The tokens of `mode` at `density`.
    pub fn new(mode: Mode, density: Density) -> Self {
        match mode {
            Mode::Light => Self {
                mode,
                density,
                bg: hex(0xE8E8EC),
                surface: hex(0xF3F3F5),
                surface_raised: hex(0xF8F8F9),
                field: hex(0xFAFAFB),
                border: hex(0xC8C8D0),
                divider: hex(0xDADAE0),
                hover: hex(0xE2E2E7),
                pressed: hex(0xD6D6DD),
                text: hex(0x1D1D21),
                text_muted: hex(0x4C4C56),
                text_faint: hex(0x5A5A64),
                text_disabled: hex(0xA4A4AC),
                text_on_accent: hex(0xFFFFFF),
                icon: hex(0x44444C),
                accent: hex(0x4F5BD5),
                accent_hover: hex(0x4350C6),
                accent_text: hex(0x3E49BD),
                accent_soft: hex(0xDCDFF7),
                focus: hex(0x4F5BD5),
                success: hex(0x1B7F45),
                warning: hex(0xA35F00),
                danger: hex(0xC0353A),
                shadow: Color32::from_black_alpha(36),
            },
            Mode::Dark => Self {
                mode,
                density,
                bg: hex(0x141416),
                surface: hex(0x1C1C1F),
                surface_raised: hex(0x252529),
                field: hex(0x111113),
                border: hex(0x3A3A41),
                divider: hex(0x2A2A2F),
                hover: hex(0x2C2C31),
                pressed: hex(0x36363C),
                text: hex(0xECECEF),
                text_muted: hex(0xA8A8B2),
                text_faint: hex(0x9898A2),
                text_disabled: hex(0x5C5C64),
                text_on_accent: hex(0xFFFFFF),
                icon: hex(0xC8C8D0),
                accent: hex(0x5A66E6),
                accent_hover: hex(0x4C58D6),
                accent_text: hex(0xA3ACFF),
                accent_soft: hex(0x272B4E),
                focus: hex(0x8D97FA),
                success: hex(0x4CC27E),
                warning: hex(0xE7A13A),
                danger: hex(0xF06A6A),
                shadow: Color32::from_black_alpha(110),
            },
            Mode::Dim => Self {
                mode,
                density,
                bg: hex(0x232328),
                surface: hex(0x2A2A30),
                surface_raised: hex(0x333339),
                field: hex(0x1F1F24),
                border: hex(0x4A4A53),
                divider: hex(0x393940),
                hover: hex(0x36363D),
                pressed: hex(0x414149),
                text: hex(0xEEEEF1),
                text_muted: hex(0xBDBDC6),
                text_faint: hex(0xAEAEB8),
                text_disabled: hex(0x6E6E77),
                text_on_accent: hex(0xFFFFFF),
                icon: hex(0xD2D2DA),
                accent: hex(0x5A66E6),
                accent_hover: hex(0x4C58D6),
                accent_text: hex(0xB4BBFF),
                accent_soft: hex(0x3B4070),
                focus: hex(0x9EA6FF),
                success: hex(0x62CF8F),
                warning: hex(0xECAE52),
                danger: hex(0xF58585),
                shadow: Color32::from_black_alpha(90),
            },
            Mode::Cream => Self {
                mode,
                density,
                bg: hex(0xF5F1E8),
                surface: hex(0xFAF7F1),
                surface_raised: hex(0xFDFBF7),
                field: hex(0xFFFDFA),
                border: hex(0xDDD5C6),
                divider: hex(0xEAE4D8),
                hover: hex(0xEFE9DE),
                pressed: hex(0xE5DDCF),
                text: hex(0x2A221B),
                text_muted: hex(0x5C5045),
                text_faint: hex(0x665A4E),
                text_disabled: hex(0xABA092),
                text_on_accent: hex(0xFFFFFF),
                icon: hex(0x4A3F35),
                accent: hex(0xB3261E),
                accent_hover: hex(0x9C1F18),
                accent_text: hex(0x9E2119),
                accent_soft: hex(0xF6DDD7),
                focus: hex(0xB3261E),
                success: hex(0x2E7D46),
                warning: hex(0x9A5B00),
                danger: hex(0x8E1B2C),
                shadow: Color32::from_black_alpha(30),
            },
            Mode::HighContrast => Self {
                mode,
                density,
                bg: hex(0x000000),
                surface: hex(0x000000),
                surface_raised: hex(0x0E0E0E),
                field: hex(0x000000),
                border: hex(0xBDBDBD),
                divider: hex(0x8A8A8A),
                hover: hex(0x262626),
                pressed: hex(0x3A3A3A),
                text: hex(0xFFFFFF),
                text_muted: hex(0xE6E6E6),
                text_faint: hex(0xCCCCCC),
                text_disabled: hex(0x8A8A8A),
                text_on_accent: hex(0x000000),
                icon: hex(0xFFFFFF),
                accent: hex(0xFFD23F),
                accent_hover: hex(0xFFDF73),
                accent_text: hex(0xFFD23F),
                accent_soft: hex(0x3A3000),
                focus: hex(0xFFD23F),
                success: hex(0x5EE08F),
                warning: hex(0xFFB547),
                danger: hex(0xFF7A7A),
                shadow: Color32::from_black_alpha(0),
            },
        }
    }

    /// True when these tokens draw on dark surfaces.
    pub fn is_dark(&self) -> bool {
        self.mode.is_dark()
    }

    /// Height of buttons, fields and rows at this density.
    pub fn control_height(&self) -> f32 {
        self.density.control_height()
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Self::new(Mode::Light, Density::Comfortable)
    }
}

/// WCAG 2 contrast ratio between two opaque colours (1.0 to 21.0).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn channel(v: u8) -> f32 {
        let s = f32::from(v) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    }
    let lum = |c: Color32| 0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b());
    let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
    x.max(y) / x.min(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [Mode; 5] = [Mode::Light, Mode::Cream, Mode::Dim, Mode::Dark, Mode::HighContrast];

    #[test]
    fn contrast_meets_wcag_aa() {
        for mode in MODES {
            let t = Tokens::new(mode, Density::Comfortable);
            let surfaces = [("bg", t.bg), ("surface", t.surface), ("surface_raised", t.surface_raised), ("field", t.field), ("hover", t.hover)];
            let texts = [("text", t.text), ("text_muted", t.text_muted), ("text_faint", t.text_faint), ("accent_text", t.accent_text)];
            for (fg_name, fg) in texts {
                for (bg_name, bg) in surfaces {
                    let r = contrast(fg, bg);
                    assert!(r >= 4.5, "{mode:?}: {fg_name} on {bg_name} is {r:.2}:1, text needs 4.5:1");
                }
            }
            for (bg_name, bg) in [("accent", t.accent), ("accent_hover", t.accent_hover)] {
                let r = contrast(t.text_on_accent, bg);
                assert!(r >= 4.5, "{mode:?}: text_on_accent on {bg_name} is {r:.2}:1");
            }
            let r = contrast(t.text, t.accent_soft);
            assert!(r >= 4.5, "{mode:?}: text on accent_soft is {r:.2}:1");
            for (name, c) in
                [("accent", t.accent), ("focus", t.focus), ("icon", t.icon), ("success", t.success), ("warning", t.warning), ("danger", t.danger)]
            {
                for (bg_name, bg) in [("bg", t.bg), ("surface", t.surface)] {
                    let r = contrast(c, bg);
                    assert!(r >= 3.0, "{mode:?}: {name} on {bg_name} is {r:.2}:1, UI parts need 3:1");
                }
            }
        }
    }

    #[test]
    fn appearance_ids_round_trip() {
        for a in Appearance::ALL {
            assert_eq!(Appearance::parse(a.id()), Some(a));
            assert_eq!(Appearance::parse(a.label()), Some(a));
        }
        for d in Density::ALL {
            assert_eq!(Density::parse(d.id()), Some(d));
        }
    }

    #[test]
    fn older_theme_names_map_to_the_nearest_choice() {
        for (old, new) in [
            ("darkest", Appearance::Dark),
            ("Medium Dark", Appearance::Dim),
            ("medium", Appearance::Dim),
            ("medium-light", Appearance::Light),
            ("ProMedium", Appearance::Dim),
            ("studio-light", Appearance::Light),
            ("AdwaitaDark", Appearance::Dark),
            ("high_contrast", Appearance::HighContrast),
            ("Sepia", Appearance::Cream),
            ("warm", Appearance::Cream),
        ] {
            assert_eq!(Appearance::parse(old), Some(new), "{old}");
        }
        assert_eq!(Appearance::parse(""), None);
        assert_eq!(Appearance::parse("teal"), None);
    }

    #[test]
    fn system_follows_the_os_then_the_app_preference() {
        assert_eq!(Appearance::System.resolve(Some(egui::Theme::Light), true), Mode::Light);
        assert_eq!(Appearance::System.resolve(Some(egui::Theme::Dark), false), Mode::Dark);
        assert_eq!(Appearance::System.resolve(None, true), Mode::Dark);
        assert_eq!(Appearance::System.resolve(None, false), Mode::Light);
        assert_eq!(Appearance::HighContrast.resolve(Some(egui::Theme::Light), false), Mode::HighContrast);
    }
}
