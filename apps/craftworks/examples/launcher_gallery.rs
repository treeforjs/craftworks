//! Network-free Light, Dark and compact screenshots of the launcher.
//! Run: cargo run -p craftworks --example launcher_gallery -- target/launcher-gallery

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), String> {
    use craft_ui::Appearance;
    use egui_kittest::Harness;
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "target/launcher-gallery".into()));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    for (name, mut app, size) in [
        ("launcher-light.png", craftworks::Launcher::preview(Appearance::Light), egui::vec2(920.0, 1120.0)),
        ("launcher-dark.png", craftworks::Launcher::preview(Appearance::Dark), egui::vec2(920.0, 1120.0)),
        ("launcher-compact.png", craftworks::Launcher::preview(Appearance::Light), egui::vec2(360.0, 640.0)),
        ("launcher-uninstall-light.png", craftworks::Launcher::preview_uninstall(Appearance::Light), egui::vec2(920.0, 920.0)),
        ("launcher-uninstall-dark.png", craftworks::Launcher::preview_uninstall(Appearance::Dark), egui::vec2(920.0, 920.0)),
        ("launcher-uninstall-compact.png", craftworks::Launcher::preview_uninstall(Appearance::Light), egui::vec2(360.0, 640.0)),
        ("launcher-update-light.png", craftworks::Launcher::preview_launcher_update(Appearance::Light), egui::vec2(920.0, 1120.0)),
        ("launcher-update-dark.png", craftworks::Launcher::preview_launcher_update(Appearance::Dark), egui::vec2(920.0, 1120.0)),
        ("launcher-update-compact.png", craftworks::Launcher::preview_launcher_update(Appearance::Light), egui::vec2(360.0, 800.0)),
        ("launcher-checking.png", craftworks::Launcher::preview_launcher_checking(Appearance::Dark), egui::vec2(920.0, 1120.0)),
        ("launcher-offline.png", craftworks::Launcher::preview_offline(Appearance::Light), egui::vec2(920.0, 1200.0)),
        ("launcher-no-key.png", craftworks::Launcher::preview_without_key(Appearance::Dark), egui::vec2(920.0, 1240.0)),
        ("launcher-previous-light.png", craftworks::Launcher::preview_previous(Appearance::Light), egui::vec2(920.0, 1450.0)),
        ("launcher-previous-dark.png", craftworks::Launcher::preview_previous(Appearance::Dark), egui::vec2(920.0, 1450.0)),
    ] {
        let mut harness = Harness::builder().with_size(size).with_pixels_per_point(1.0).build_ui(move |ui| app.draw(ui));
        harness.run_steps(4);
        let image = harness.render()?;
        image.save(dir.join(name)).map_err(|error| error.to_string())?;
        eprintln!("Wrote {}", dir.join(name).display());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
