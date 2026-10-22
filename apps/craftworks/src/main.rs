#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("Craftworks {}", env!("CARGO_PKG_VERSION"));
                return std::process::ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!(
                    "Craftworks {}\n\nInstall, update and open PdfCraft, VectorCraft and PhotoCraft.\n\nUsage: craftworks [--version | --help]\n\nApp installs are stored in your user data directory; administrator access is not needed.",
                    env!("CARGO_PKG_VERSION")
                );
                return std::process::ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("Unknown option {arg}. Run craftworks --help for usage.");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_decorations(true).with_inner_size([920.0, 800.0]).with_min_inner_size([360.0, 360.0]),
        ..Default::default()
    };
    let (uninstall_tx, uninstall_rx) = std::sync::mpsc::channel();
    let result =
        eframe::run_native("Craftworks", options, Box::new(move |cc| Ok(Box::new(craftworks::Launcher::new_with_uninstall(cc, uninstall_tx)))));
    match result {
        Ok(()) => {
            // run_native has dropped FileStorage, whose Drop joins its last persistence write.
            // Removing app.ron earlier would allow the final save to recreate it.
            if let Ok(plan) = uninstall_rx.try_recv()
                && let Err(error) = plan.finish()
            {
                eprintln!("Craftworks couldn't finish uninstalling: {error}");
                show_removal_error(error.to_string());
                return std::process::ExitCode::FAILURE;
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Craftworks couldn't open its window: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn show_removal_error(message: String) {
    struct RemovalError(String);
    impl eframe::App for RemovalError {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            craft_ui::apply(ui.ctx(), &craft_ui::Theme::default());
            ui.heading("Uninstall could not finish");
            ui.add(egui::Label::new(&self.0).wrap());
            ui.label("Some selected cleanup may already have completed. Remaining files were kept. You can reinstall Craftworks or retry the registered uninstaller.");
            if craft_ui::button(ui, "Close", craft_ui::Variant::Secondary).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        fn persist_egui_memory(&self) -> bool {
            false
        }
    }
    // A failed removal must not create another saved application profile.
    let options = eframe::NativeOptions {
        persistence_path: Some(std::path::PathBuf::new()),
        persist_window: false,
        viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 240.0]),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Craftworks uninstall",
        options,
        Box::new(move |cc| {
            craft_ui::fonts::install(&cc.egui_ctx, egui::FontDefinitions::default());
            Ok(Box::new(RemovalError(message)))
        }),
    );
}

#[cfg(target_arch = "wasm32")]
fn main() {}
