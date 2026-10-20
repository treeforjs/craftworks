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
    match eframe::run_native("Craftworks", options, Box::new(|cc| Ok(Box::new(craftworks::Launcher::new(cc))))) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Craftworks couldn't open its window: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
