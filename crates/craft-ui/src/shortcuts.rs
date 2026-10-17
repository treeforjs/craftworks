//! The shortcuts every craft app shares. Tool keys (V, B, T…) and app-specific commands stay in
//! each app; these are the ones a person should never have to relearn between apps.

use egui::{Key, KeyboardShortcut, Modifiers};

/// Open the command palette (⌘K / Ctrl+K).
pub const PALETTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);

/// One shared shortcut and the command id each app binds it to.
#[derive(Clone, Copy, Debug)]
pub struct Shared {
    /// The command id. Apps whose ids differ map them, but the key stays.
    pub command: &'static str,
    /// What the shortcut sheet and palette call it.
    pub label: &'static str,
    /// The key combination.
    pub shortcut: KeyboardShortcut,
}

const fn s(command: &'static str, label: &'static str, modifiers: Modifiers, key: Key) -> Shared {
    Shared { command, label, shortcut: KeyboardShortcut::new(modifiers, key) }
}

const CMD_SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: true };

/// The shared shortcuts, in the order the shortcut sheet lists them.
pub const SHARED: &[Shared] = &[
    s("app.palette", "Search commands", Modifiers::COMMAND, Key::K),
    s("app.settings", "Settings", Modifiers::COMMAND, Key::Comma),
    s("file.new", "New", Modifiers::COMMAND, Key::N),
    s("file.open", "Open", Modifiers::COMMAND, Key::O),
    s("file.save", "Save", Modifiers::COMMAND, Key::S),
    s("file.save_as", "Save as", CMD_SHIFT, Key::S),
    s("file.export", "Export", CMD_SHIFT, Key::E),
    s("file.close", "Close", Modifiers::COMMAND, Key::W),
    s("edit.undo", "Undo", Modifiers::COMMAND, Key::Z),
    s("edit.redo", "Redo", CMD_SHIFT, Key::Z),
    s("edit.select_all", "Select all", Modifiers::COMMAND, Key::A),
    s("view.fit", "Fit to window", Modifiers::COMMAND, Key::Num0),
    s("view.actual_size", "Actual size", Modifiers::COMMAND, Key::Num1),
    s("view.zoom_in", "Zoom in", Modifiers::COMMAND, Key::Plus),
    s("view.zoom_out", "Zoom out", Modifiers::COMMAND, Key::Minus),
    s("view.toggle_panels", "Show or hide panels", Modifiers::NONE, Key::Tab),
    s("help.shortcuts", "Keyboard shortcuts", Modifiers::SHIFT, Key::Questionmark),
];

/// The shared shortcut for `command`, if it has one.
pub fn for_command(command: &str) -> Option<KeyboardShortcut> {
    SHARED.iter().find(|s| s.command == command).map(|s| s.shortcut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_shared_commands_use_the_same_keys() {
        for (i, a) in SHARED.iter().enumerate() {
            for b in SHARED.iter().skip(i + 1) {
                assert!(a.shortcut != b.shortcut, "{} and {} share a shortcut", a.command, b.command);
                assert_ne!(a.command, b.command);
            }
        }
    }

    #[test]
    fn palette_is_command_k() {
        assert_eq!(for_command("app.palette"), Some(PALETTE));
    }
}
