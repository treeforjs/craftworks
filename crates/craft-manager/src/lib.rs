//! Per-user installation and updates for the Craft apps.
//!
//! The desktop manager verifies a signed checksum list before downloading any executable.
//! Windows uses portable ZIP packages and Linux uses AppImages. Each update gets a new
//! directory; an atomic, append-only receipt activates it without replacing running binaries.
//! Existing versions remain until the app is uninstalled. App settings stay in each app's
//! normal user profile, separate from these managed binaries.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

use std::path::PathBuf;

use craft_update::{Asset, Release};

#[cfg(not(target_arch = "wasm32"))]
mod manager;
#[cfg(not(target_arch = "wasm32"))]
mod package;

/// Discover previous installations and retire them without adopting their files.
#[cfg(not(target_arch = "wasm32"))]
pub mod legacy;

/// Preview and remove an installed Craftworks launcher without touching source checkouts.
#[cfg(not(target_arch = "wasm32"))]
pub mod self_removal;

#[cfg(not(target_arch = "wasm32"))]
pub use manager::Manager;

/// One app in the fixed, first-party catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct App {
    /// The app's lowercase package and executable name.
    pub id: &'static str,
    /// Its display name.
    pub name: &'static str,
    /// A short description for the library.
    pub summary: &'static str,
    /// Its official GitHub repository, as `owner/repo`.
    pub repo: &'static str,
}

/// Apps managed by Craftworks. Previous installations can be retired but are never adopted.
pub const APPS: [App; 3] = [
    App { id: "pdfcraft", name: "PdfCraft", summary: "Read, edit and create PDF documents.", repo: "treeforjs/pdfcraft" },
    App { id: "vectorcraft", name: "VectorCraft", summary: "Draw and edit vector artwork.", repo: "treeforjs/vectorcraft" },
    App { id: "photocraft", name: "PhotoCraft", summary: "Edit and develop photographs.", repo: "treeforjs/photocraft" },
];

/// The active, owned installation of an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The active release version.
    pub version: String,
    /// The executable inside the manager's own installation directory.
    pub executable: PathBuf,
}

/// The newest published release and whether this machine can install it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Availability {
    /// The newest stable published release, if any.
    pub release: Option<Release>,
    /// Its package for this operating system and architecture, if any.
    pub asset: Option<Asset>,
    /// Why installation is unavailable, in words suitable for the UI.
    pub blocked: Option<String>,
}
