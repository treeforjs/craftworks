//! Craftworks desktop launcher and installer.

#[cfg(not(target_arch = "wasm32"))]
mod launcher;

#[cfg(not(target_arch = "wasm32"))]
pub use launcher::Launcher;
