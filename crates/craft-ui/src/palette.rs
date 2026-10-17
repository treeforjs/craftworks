//! The command palette: ⌘K in every craft app. It searches whatever the app lists (commands,
//! tools, settings, recent files, help) and returns the id the user picked; the app runs it
//! through its command registry like any other dispatch.

use egui::{Align2, CornerRadius, Key, Rect, Sense, vec2};

use crate::tokens::{radius, space, type_size};
use crate::{fonts, icons, style};

/// What kind of thing an entry is. Shown as the entry's detail when it has no shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ItemKind {
    /// A command.
    #[default]
    Command,
    /// A tool.
    Tool,
    /// A setting.
    Setting,
    /// A recent file.
    File,
    /// A help topic.
    Help,
}

impl ItemKind {
    /// The word shown beside an entry without a shortcut.
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Command => "Command",
            ItemKind::Tool => "Tool",
            ItemKind::Setting => "Setting",
            ItemKind::File => "Recent file",
            ItemKind::Help => "Help",
        }
    }
}

/// One palette entry.
#[derive(Clone, Debug, Default)]
pub struct Item {
    /// The id returned when it is picked (a command id, a tool id, a path…).
    pub id: String,
    /// What the entry says.
    pub label: String,
    /// Where it lives, such as "Edit › Transform"; searched and shown faintly.
    pub detail: String,
    /// Extra words that should find it ("rotate" for Transform).
    pub keywords: String,
    /// The formatted shortcut, if any.
    pub shortcut: Option<String>,
    /// An icon name for the app's icon painter.
    pub icon: Option<String>,
    /// What kind of entry it is.
    pub kind: ItemKind,
    /// Disabled entries are listed but cannot run.
    pub enabled: bool,
}

impl Item {
    /// An enabled command entry.
    pub fn command(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), enabled: true, ..Self::default() }
    }

    /// Set the shortcut.
    pub fn shortcut(mut self, s: impl Into<String>) -> Self {
        self.shortcut = Some(s.into());
        self
    }

    /// Set where it lives.
    pub fn detail(mut self, s: impl Into<String>) -> Self {
        self.detail = s.into();
        self
    }

    /// Set extra search words.
    pub fn keywords(mut self, s: impl Into<String>) -> Self {
        self.keywords = s.into();
        self
    }

    /// Set the kind.
    pub fn kind(mut self, kind: ItemKind) -> Self {
        self.kind = kind;
        self
    }

    /// Set the icon.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = Some(name.into());
        self
    }

    /// Set whether it can run now.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// The palette's state. Keep one in the app state (it is serde so the control channel can open
/// it and type into it).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    /// Whether it is showing.
    pub open: bool,
    /// What has been typed.
    pub query: String,
    /// The highlighted row.
    pub selected: usize,
}

/// How many rows the palette shows.
pub const MAX_ROWS: usize = 10;

/// A match score for `query` in `text`; lower is better, `None` is no match. Prefix beats word
/// start beats substring beats letters in order.
pub fn score(text: &str, query: &str) -> Option<u32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let h = text.to_lowercase();
    if h.starts_with(&q) {
        return Some(0);
    }
    if let Some(p) = h.find(&q) {
        let at_word = h.get(..p).and_then(|s| s.chars().last()).is_some_and(|c| !c.is_alphanumeric());
        let p = u32::try_from(p).unwrap_or(u32::MAX).min(1000);
        return Some(if at_word { 10 + p } else { 100 + p });
    }
    let mut chars = h.chars();
    q.chars().filter(|c| !c.is_whitespace()).all(|c| chars.any(|x| x == c)).then_some(2000)
}

/// The indices of `items` that match `query`, best first, enabled before disabled.
pub fn matches(items: &[Item], query: &str) -> Vec<usize> {
    let mut hits: Vec<(u32, bool, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let s = [
                score(&it.label, query),
                score(&it.keywords, query).map(|s| s.saturating_add(5)),
                score(&it.detail, query).map(|s| s.saturating_add(50)),
            ]
            .into_iter()
            .flatten()
            .min()?;
            Some((s, !it.enabled, i))
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, _, i)| i).collect()
}

impl Palette {
    /// Open it with an empty query.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    /// Close it.
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }

    /// Toggle on ⌘K. Call once per frame before [`Palette::show`].
    pub fn handle_shortcut(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&crate::shortcuts::PALETTE)) {
            if self.open {
                self.close();
            } else {
                self.open();
            }
        }
    }

    /// Draw the palette if open, and return the id of the entry the user ran.
    pub fn show(&mut self, ctx: &egui::Context, items: &[Item]) -> Option<String> {
        if !self.open {
            return None;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.close();
            return None;
        }
        let t = style::tokens(ctx);
        let hits = matches(items, &self.query);
        let shown: Vec<&Item> = hits.iter().take(MAX_ROWS).filter_map(|&i| items.get(i)).collect();
        let last = shown.len().saturating_sub(1);
        if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
            self.selected = if self.selected >= last { 0 } else { self.selected + 1 };
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
            self.selected = if self.selected == 0 { last } else { self.selected - 1 };
        }
        self.selected = self.selected.min(last);
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        let mut chosen = None;
        let screen = ctx.content_rect();
        let width = (screen.width() - 2.0 * space::LG).clamp(240.0, 600.0);
        let row_h = t.control_height() + space::SM;
        let before = self.query.clone();
        egui::Area::new(egui::Id::new("craft-ui-palette"))
            .order(egui::Order::Foreground)
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(screen.center().x, screen.top() + screen.height().min(800.0) * 0.12))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).corner_radius(CornerRadius::same(radius::XL)).inner_margin(egui::Margin::same(space::SM as i8)).show(
                    ui,
                    |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            icons::paint(ui, r, "search", t.text_muted);
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut self.query)
                                    .id_salt("craft-ui-palette-query")
                                    .hint_text("Search commands, tools and settings")
                                    .frame(egui::Frame::NONE)
                                    .font(fonts::regular(type_size::TITLE))
                                    .desired_width(f32::INFINITY),
                            );
                            field.request_focus();
                        });
                        ui.add_space(space::XS);
                        ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), egui::Stroke::new(1.0, t.divider));
                        ui.add_space(space::XS);
                        if shown.is_empty() {
                            ui.add_space(space::MD);
                            ui.vertical_centered(|ui| ui.label(egui::RichText::new("Nothing matches. Try another word.").color(t.text_muted)));
                            ui.add_space(space::MD);
                        }
                        for (row, item) in shown.iter().enumerate() {
                            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, item.enabled, &item.label));
                            if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                                self.selected = row;
                            }
                            if row == self.selected {
                                ui.painter().rect_filled(rect, CornerRadius::same(radius::MD), t.accent_soft);
                            }
                            let fg = if item.enabled { t.text } else { t.text_disabled };
                            let icon_rect = Rect::from_center_size(egui::pos2(rect.left() + space::MD + 8.0, rect.center().y), vec2(16.0, 16.0));
                            if let Some(icon) = &item.icon {
                                icons::paint(ui, icon_rect, icon, if item.enabled { t.icon } else { t.text_disabled });
                            }
                            let right = item.shortcut.clone().unwrap_or_else(|| item.kind.label().to_owned());
                            let right = ui.painter().layout_no_wrap(right, fonts::regular(type_size::SMALL), t.text_faint);
                            let rx = rect.right() - space::MD - right.size().x;
                            let lx = icon_rect.right() + space::SM;
                            let mut job = egui::text::LayoutJob::default();
                            job.append(&item.label, 0.0, egui::TextFormat::simple(fonts::regular(type_size::BODY), fg));
                            if !item.detail.is_empty() {
                                job.append(&item.detail, space::SM, egui::TextFormat::simple(fonts::regular(type_size::SMALL), t.text_faint));
                            }
                            job.wrap = egui::text::TextWrapping::truncate_at_width((rx - space::SM - lx).max(0.0));
                            let g = ui.painter().layout_job(job);
                            ui.painter().galley(egui::pos2(lx, rect.center().y - g.size().y * 0.5), g, fg);
                            ui.painter().galley(egui::pos2(rx, rect.center().y - right.size().y * 0.5), right, t.text_faint);
                            if resp.clicked() && item.enabled {
                                chosen = Some(item.id.clone());
                            }
                        }
                    },
                );
            });
        if self.query != before {
            self.selected = 0;
        }
        if enter && chosen.is_none() {
            chosen = shown.get(self.selected).filter(|i| i.enabled).map(|i| i.id.clone());
        }
        if chosen.is_some() {
            self.close();
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item> {
        vec![
            Item::command("file.export", "Export").shortcut("⌘⇧E").detail("File"),
            Item::command("layer.new", "New layer").detail("Layer"),
            Item::command("edit.transform", "Free transform").keywords("rotate scale"),
            Item::command("file.export_pdf", "Export as PDF").enabled(false),
        ]
    }

    #[test]
    fn prefix_beats_word_beats_substring_beats_letters() {
        assert!(score("Export", "exp") < score("Quick export", "exp"));
        assert!(score("Quick export", "exp") < score("Reexport", "exp"));
        assert!(score("Reexport", "exp") < score("Expand path", "epa"));
        assert_eq!(score("Layer", "zz"), None);
        assert_eq!(score("anything", "  "), Some(0));
    }

    #[test]
    fn keywords_find_entries() {
        let it = items();
        let ids: Vec<&str> = matches(&it, "rotate").iter().filter_map(|&i| it.get(i)).map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["edit.transform"]);
    }

    #[test]
    fn enabled_entries_come_first_on_equal_scores() {
        let it = items();
        let first = matches(&it, "export").first().and_then(|&i| it.get(i)).map(|i| i.id.clone());
        assert_eq!(first.as_deref(), Some("file.export"));
    }

    #[test]
    fn non_ascii_queries_do_not_panic() {
        assert_eq!(score("Ébauche", "bau"), Some(102));
        let _ = score("新規レイヤー", "レイ");
        let _ = score("İstanbul", "i̇s");
    }

    #[test]
    fn enter_runs_the_highlighted_entry() {
        let ctx = egui::Context::default();
        let it = items();
        let mut p = Palette::default();
        p.open();
        p.query = "layer".into();
        crate::test_pass(&ctx, egui::RawInput::default(), |ui| {
            assert_eq!(p.show(ui.ctx(), &it), None);
        });
        let input = egui::RawInput {
            events: vec![egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        };
        let mut ran = None;
        crate::test_pass(&ctx, input, |ui| ran = p.show(ui.ctx(), &it));
        assert_eq!(ran.as_deref(), Some("layer.new"));
        assert!(!p.open);
    }

    #[test]
    fn empty_list_and_escape() {
        let ctx = egui::Context::default();
        let mut p = Palette::default();
        p.open();
        crate::test_pass(&ctx, egui::RawInput::default(), |ui| assert_eq!(p.show(ui.ctx(), &[]), None));
        let input = egui::RawInput {
            events: vec![egui::Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        };
        crate::test_pass(&ctx, input, |ui| assert_eq!(p.show(ui.ctx(), &[]), None));
        assert!(!p.open);
    }
}
