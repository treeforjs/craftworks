//! Headless screenshots of craft-ui's components arranged as the shared app shell.
//!
//! ```text
//! CRAFT_UI_FONTS=../pdfcraft/assets/fonts cargo run --example gallery -- target/gallery
//! ```
//! Writes `shell-light.png`, `shell-dark.png` and `palette-dark.png` into the directory.
//! `CRAFT_UI_FONTS` (optional) is a folder with Inter-Regular/Medium/SemiBold.ttf and
//! JetBrainsMono-Regular.ttf; without it egui's default faces are used.

use std::sync::Arc;

use craft_ui::{Appearance, EmptyState, Palette, PaletteItem, Theme, Tip, Toast, Toasts, Variant};
use egui::{Color32, FontData, FontDefinitions, FontFamily, Painter, Rect, Stroke, vec2};
use egui_kittest::Harness;

fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let Ok(dir) = std::env::var("CRAFT_UI_FONTS") else { return defs };
    let dir = std::path::PathBuf::from(dir);
    for (name, file, family) in [
        ("Inter", "Inter-Regular.ttf", None),
        ("Inter-Medium", "Inter-Medium.ttf", Some(craft_ui::fonts::MEDIUM)),
        ("Inter-SemiBold", "Inter-SemiBold.ttf", Some(craft_ui::fonts::SEMIBOLD)),
        ("JetBrainsMono", "JetBrainsMono-Regular.ttf", None),
    ] {
        let Ok(bytes) = std::fs::read(dir.join(file)) else { continue };
        defs.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
        match (name, family) {
            ("Inter", _) => defs.families.entry(FontFamily::Proportional).or_default().insert(0, name.into()),
            ("JetBrainsMono", _) => defs.families.entry(FontFamily::Monospace).or_default().insert(0, name.into()),
            (_, Some(f)) => defs.families.entry(FontFamily::Name(f.into())).or_default().insert(0, name.into()),
            _ => {}
        }
    }
    defs
}

/// A tiny stand-in for an app's Lucide painter: enough shapes to read the shell.
fn icons(p: &Painter, r: Rect, name: &str, c: Color32) -> bool {
    let s = Stroke::new(1.5, c);
    let (cx, cy, k) = (r.center().x, r.center().y, r.width() / 2.0);
    match name {
        "search" => {
            p.circle_stroke(egui::pos2(cx - k * 0.15, cy - k * 0.15), k * 0.55, s);
            p.line_segment([egui::pos2(cx + k * 0.25, cy + k * 0.25), egui::pos2(cx + k * 0.8, cy + k * 0.8)], s);
        }
        "chevron-down" => {
            p.line_segment([egui::pos2(cx - k * 0.5, cy - k * 0.2), egui::pos2(cx, cy + k * 0.3)], s);
            p.line_segment([egui::pos2(cx, cy + k * 0.3), egui::pos2(cx + k * 0.5, cy - k * 0.2)], s);
        }
        "chevron-right" => {
            p.line_segment([egui::pos2(cx - k * 0.2, cy - k * 0.5), egui::pos2(cx + k * 0.3, cy)], s);
            p.line_segment([egui::pos2(cx + k * 0.3, cy), egui::pos2(cx - k * 0.2, cy + k * 0.5)], s);
        }
        "pointer" => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(cx - k * 0.5, cy - k * 0.7),
                    egui::pos2(cx + k * 0.5, cy + k * 0.1),
                    egui::pos2(cx, cy + k * 0.1),
                    egui::pos2(cx - k * 0.3, cy + k * 0.7),
                ],
                Color32::TRANSPARENT,
                s,
            ));
        }
        "brush" => {
            p.line_segment([egui::pos2(cx + k * 0.7, cy - k * 0.7), egui::pos2(cx - k * 0.1, cy + k * 0.1)], s);
            p.circle_stroke(egui::pos2(cx - k * 0.35, cy + k * 0.35), k * 0.3, s);
        }
        "type" => {
            p.line_segment([egui::pos2(cx - k * 0.6, cy - k * 0.6), egui::pos2(cx + k * 0.6, cy - k * 0.6)], s);
            p.line_segment([egui::pos2(cx, cy - k * 0.6), egui::pos2(cx, cy + k * 0.7)], s);
        }
        "square" => {
            p.rect_stroke(r.shrink(k * 0.3), 3.0, s, egui::StrokeKind::Middle);
        }
        "pen" => {
            p.line_segment([egui::pos2(cx - k * 0.6, cy + k * 0.6), egui::pos2(cx + k * 0.6, cy - k * 0.6)], s);
            p.circle_stroke(egui::pos2(cx - k * 0.6, cy + k * 0.6), k * 0.15, s);
        }
        "layers" => {
            for dy in [-0.35, 0.0, 0.35] {
                p.line_segment([egui::pos2(cx - k * 0.7, cy + k * dy), egui::pos2(cx + k * 0.7, cy + k * dy)], s);
            }
        }
        _ => return false,
    }
    true
}

fn shell(ui: &mut egui::Ui, theme: &Theme, toasts: &mut Toasts, palette: &mut Palette) {
    let t = craft_ui::apply(ui.ctx(), theme);
    craft_ui::icons::set_painter(ui.ctx(), icons);
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, t.bg);
    let top = Rect::from_min_size(full.min, vec2(full.width(), 48.0));
    let status = Rect::from_min_max(egui::pos2(full.left(), full.bottom() - 26.0), full.max);
    let rail = Rect::from_min_max(egui::pos2(full.left(), top.bottom()), egui::pos2(full.left() + 52.0, status.top()));
    let insp = Rect::from_min_max(egui::pos2(full.right() - 300.0, top.bottom()), egui::pos2(full.right(), status.top()));
    let canvas = Rect::from_min_max(egui::pos2(rail.right(), top.bottom()), egui::pos2(insp.left(), status.top()));
    let p = ui.painter().clone();
    for r in [top, status, rail, insp] {
        p.rect_filled(r, 0.0, t.surface);
    }
    p.hline(full.x_range(), top.bottom(), Stroke::new(1.0, t.divider));
    p.hline(full.x_range(), status.top(), Stroke::new(1.0, t.divider));
    p.vline(rail.right(), rail.y_range(), Stroke::new(1.0, t.divider));
    p.vline(insp.left(), insp.y_range(), Stroke::new(1.0, t.divider));

    ui.scope_builder(egui::UiBuilder::new().max_rect(top.shrink2(vec2(12.0, 10.0))), |ui| {
        ui.horizontal_centered(|ui| {
            craft_ui::button(ui, "Home", Variant::Ghost);
            ui.add_space(4.0);
            craft_ui::button(ui, "poster.craft", Variant::Secondary);
            craft_ui::button(ui, "flyer.craft", Variant::Ghost);
            let w = ui.available_width();
            ui.add_space((w * 0.5 - 330.0).max(8.0));
            craft_ui::search_trigger(ui, "Search or run a command", 320.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                craft_ui::button(ui, "Export", Variant::Primary);
                craft_ui::button(ui, "Share", Variant::Secondary);
            });
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(rail.shrink(8.0)), |ui| {
        ui.vertical(|ui| {
            for (i, (icon, name, key)) in
                [("pointer", "Select", "V"), ("brush", "Brush", "B"), ("pen", "Pen", "P"), ("type", "Text", "T"), ("square", "Shape", "U")]
                    .into_iter()
                    .enumerate()
            {
                craft_ui::icon_button(ui, icon, &Tip::new(name).shortcut(key), i == 1);
            }
        });
    });
    // Canvas: a document with a selection and the action bar under it.
    let page = Rect::from_center_size(canvas.center() - vec2(0.0, 30.0), vec2(canvas.width() * 0.62, canvas.height() * 0.62));
    p.rect_filled(page.translate(vec2(0.0, 2.0)), 2.0, t.shadow);
    p.rect_filled(page, 2.0, Color32::from_rgb(0xFA, 0xF7, 0xF2));
    let sel = Rect::from_min_size(page.min + vec2(page.width() * 0.2, page.height() * 0.25), vec2(page.width() * 0.45, page.height() * 0.3));
    p.rect_filled(sel, 4.0, Color32::from_rgb(0xF2, 0xA6, 0x5A));
    p.rect_stroke(sel.expand(3.0), 2.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Middle);
    let bar = Rect::from_center_size(egui::pos2(sel.center().x, sel.bottom() + 34.0), vec2(300.0, 40.0));
    p.rect(bar.translate(vec2(0.0, 3.0)), 10.0, t.shadow, Stroke::NONE, egui::StrokeKind::Inside);
    p.rect(bar, 10.0, t.surface_raised, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 6.0))), |ui| {
        ui.horizontal_centered(|ui| {
            for l in ["Fill", "Align", "Duplicate", "Mask"] {
                craft_ui::button(ui, l, Variant::Ghost);
            }
        });
    });
    // Inspector.
    ui.scope_builder(egui::UiBuilder::new().max_rect(insp.shrink2(vec2(12.0, 8.0))), |ui| {
        craft_ui::panel_header(ui, "Rectangle");
        craft_ui::section(ui, "g-pos", "Position and size", true, |ui| {
            egui::Grid::new("pos").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                let (mut x, mut y, mut w, mut h) = (120.0, 84.0, 320.0, 180.0);
                ui.label("X");
                ui.add(egui::DragValue::new(&mut x).suffix(" px"));
                ui.label("Y");
                ui.add(egui::DragValue::new(&mut y).suffix(" px"));
                ui.end_row();
                ui.label("W");
                ui.add(egui::DragValue::new(&mut w).suffix(" px"));
                ui.label("H");
                ui.add(egui::DragValue::new(&mut h).suffix(" px"));
                ui.end_row();
            });
        });
        craft_ui::section(ui, "g-app", "Appearance", true, |ui| {
            let mut o = 0.85f32;
            ui.horizontal(|ui| {
                ui.label("Opacity");
                ui.add(egui::Slider::new(&mut o, 0.0..=1.0).show_value(true));
            });
            let mut i = 0;
            craft_ui::segmented(ui, &mut i, &["Fill", "Stroke", "Both"]);
        });
        craft_ui::section(ui, "g-fx", "Effects", false, |ui| ui.label(""));
        ui.add_space(8.0);
        craft_ui::panel_header(ui, "Layers");
        craft_ui::empty_state(
            ui,
            &EmptyState {
                icon: Some("layers"),
                title: "Only one layer",
                body: "Add a layer to paint without touching this one.",
                action: Some("New layer"),
            },
        );
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(status.shrink2(vec2(12.0, 0.0))), |ui| {
        ui.horizontal_centered(|ui| {
            ui.label(egui::RichText::new("100%   ·   RGB, 8-bit   ·   1920 × 1080 px").size(12.0).color(t.text_muted));
        });
    });
    toasts.show(ui.ctx(), 26.0 + 8.0);
    let items = vec![
        PaletteItem::command("file.export", "Export").shortcut("⇧⌘E").detail("File"),
        PaletteItem::command("layer.new", "New layer").shortcut("⇧⌘N").detail("Layer"),
        PaletteItem::command("edit.transform", "Free transform").shortcut("⌘T").detail("Edit").keywords("rotate scale"),
        PaletteItem::command("tool.brush", "Brush").shortcut("B").kind(craft_ui::PaletteItemKind::Tool).icon("brush"),
        PaletteItem::command("settings.appearance", "Appearance").kind(craft_ui::PaletteItemKind::Setting),
        PaletteItem::command("layer.mask", "Add layer mask").detail("Layer").enabled(false),
    ];
    palette.show(ui.ctx(), &items);
}

fn shot(dir: &std::path::Path, name: &str, appearance: Appearance, with_palette: bool) -> Result<(), String> {
    let theme = Theme { appearance, ..Theme::default() };
    let mut toasts = Toasts::default();
    toasts.push(Toast::undo("Deleted 3 layers"));
    let mut palette = Palette::default();
    if with_palette {
        palette.open();
        palette.query = "e".into();
    }
    let defs = fonts();
    let mut first = true;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 760.0)).with_pixels_per_point(2.0).build_ui(move |ui| {
        if first {
            craft_ui::fonts::install(ui.ctx(), defs.clone());
            first = false;
        }
        shell(ui, &theme, &mut toasts, &mut palette);
    });
    harness.run_steps(4);
    let image = harness.render()?;
    let path = dir.join(name);
    image.save(&path).map_err(|e| e.to_string())?;
    eprintln!("gallery: wrote {}", path.display());
    Ok(())
}

fn main() -> Result<(), String> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "target/gallery".into()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    shot(&dir, "shell-light.png", Appearance::Light, false)?;
    shot(&dir, "shell-dark.png", Appearance::Dark, false)?;
    shot(&dir, "shell-high-contrast.png", Appearance::HighContrast, false)?;
    shot(&dir, "palette-dark.png", Appearance::Dark, true)?;
    Ok(())
}
