//! In-app updates for the craft apps.
//!
//! One flow for every app: ask GitHub Releases for the newest version, pick the asset that
//! matches how this copy was installed, download it, check it against the release's
//! `SHA256SUMS.txt` and that list's Ed25519 signature, and install it when the app quits (or
//! restart into it right away).
//!
//! - [`Version`] compares release versions; [`Release`] reads GitHub's answer and keeps only
//!   download links inside the app's own repository.
//! - [`InstallKind`] says how this copy was installed (MSI, AppImage, deb, rpm, Flatpak, portable
//!   zip, tarball, macOS app) and which asset updates it.
//! - [`verify`] checks the checksum list's signature and a downloaded file's hash.
//! - [`AutoCheck`] decides when the next background check is due, so starting the app never waits
//!   on the network.
//! - `Updater` (desktop only) runs the whole flow on a background thread; the app polls it once per
//!   frame and draws its state.
//!
//! The web build compiles everything except the network and the installers.

// Tests may unwrap; library code may not (see `[lints]` in Cargo.toml).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

mod error;
mod install_kind;
mod release;
mod schedule;
pub mod verify;
mod version;

#[cfg(not(target_arch = "wasm32"))]
mod install;
#[cfg(not(target_arch = "wasm32"))]
mod net;
#[cfg(not(target_arch = "wasm32"))]
mod updater;

pub use error::Error;
pub use install_kind::{Arch, InstallKind, Os};
pub use release::{Asset, Release, newest};
pub use schedule::AutoCheck;
pub use version::Version;

#[cfg(not(target_arch = "wasm32"))]
pub use install::{Applied, Prepared};
#[cfg(not(target_arch = "wasm32"))]
pub use net::{GitHub, Source};
#[cfg(not(target_arch = "wasm32"))]
pub use updater::{Offer, State, Updater};

/// What the updater needs to know about the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    /// The lowercase name used in asset file names and package names, such as `pdfcraft`.
    pub id: String,
    /// The name people read, such as `PdfCraft`.
    pub display_name: String,
    /// `owner/repo` on GitHub, such as `treeforjs/pdfcraft`.
    pub repo: String,
    /// The running version (`env!("CARGO_PKG_VERSION")`).
    pub current: String,
    /// The Ed25519 public key that signs `SHA256SUMS.txt`, base64 of its 32 bytes. Without one
    /// the app can tell people about a new version but never installs anything itself.
    pub public_key: Option<String>,
    /// Offer pre-releases (`1.2.0-beta.1`) as well as releases.
    pub prereleases: bool,
}

impl AppInfo {
    /// Where every release of the app is listed.
    pub fn releases_page(&self) -> String {
        format!("https://github.com/{}/releases", self.repo)
    }

    /// The public key as bytes, if one is set and well formed.
    pub fn key(&self) -> Option<[u8; 32]> {
        verify::decode_public_key(self.public_key.as_deref()?).ok()
    }
}
