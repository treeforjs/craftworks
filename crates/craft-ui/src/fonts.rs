//! Font families. Font files are not part of craft-ui: each app embeds Inter and JetBrains Mono
//! (and the optional craft-fonts faces) itself and passes its definitions through [`prepare`].

use egui::{FontDefinitions, FontFamily, FontId};

/// The family name of the medium weight (Inter Medium in the craft apps).
pub const MEDIUM: &str = "medium";
/// The family name of the semibold weight (Inter SemiBold in the craft apps).
pub const SEMIBOLD: &str = "semibold";

/// Make sure the named families craft-ui draws with exist, so a missing face can never panic in
/// egui's font lookup. A family the app did not define falls back to its proportional stack.
pub fn prepare(mut defs: FontDefinitions) -> FontDefinitions {
    let fallback: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for name in [MEDIUM, SEMIBOLD] {
        let family = FontFamily::Name(name.into());
        let stack = defs.families.entry(family).or_default();
        if stack.is_empty() {
            stack.extend(fallback.iter().cloned());
        }
    }
    defs
}

/// Install `defs` (after [`prepare`]). The faces take effect at the next pass, as with
/// `egui::Context::set_fonts`.
pub fn install(ctx: &egui::Context, defs: FontDefinitions) {
    ctx.set_fonts(prepare(defs));
}

fn named(ctx: &egui::Context, name: &str, size: f32) -> FontId {
    let family = FontFamily::Name(name.into());
    let bound = ctx.fonts(|f| f.definitions().families.get(&family).is_some_and(|s| !s.is_empty()));
    if bound { FontId::new(size, family) } else { FontId::proportional(size) }
}

/// Regular weight at `size`.
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Medium weight at `size`, or regular when the app has not installed a medium face yet.
/// Call only while a pass is running (inside a panel or widget).
pub fn medium(ctx: &egui::Context, size: f32) -> FontId {
    named(ctx, MEDIUM, size)
}

/// Semibold weight at `size`, or regular when the app has not installed a semibold face yet.
/// Call only while a pass is running (inside a panel or widget).
pub fn semibold(ctx: &egui::Context, size: f32) -> FontId {
    named(ctx, SEMIBOLD, size)
}

/// The monospace face at `size`, for numbers that change (timecode, coordinates).
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_binds_the_named_families() {
        let defs = prepare(FontDefinitions::default());
        for name in [MEDIUM, SEMIBOLD] {
            let stack = defs.families.get(&FontFamily::Name(name.into()));
            assert!(stack.is_some_and(|s| !s.is_empty()), "{name} is unbound");
        }
    }

    #[test]
    fn prepare_keeps_an_apps_own_stack() {
        let mut defs = FontDefinitions::default();
        defs.families.insert(FontFamily::Name(SEMIBOLD.into()), vec!["Inter-SemiBold".into()]);
        let defs = prepare(defs);
        assert_eq!(defs.families.get(&FontFamily::Name(SEMIBOLD.into())), Some(&vec!["Inter-SemiBold".to_owned()]));
    }

    #[test]
    fn named_weights_fall_back_before_fonts_are_installed() {
        let ctx = egui::Context::default();
        crate::test_pass(&ctx, egui::RawInput::default(), |ui| {
            // No craft families installed: regular is used instead of panicking.
            assert_eq!(semibold(ui.ctx(), 15.0), FontId::proportional(15.0));
            ui.label(egui::RichText::new("x").font(medium(ui.ctx(), 13.0)));
        });
    }
}
