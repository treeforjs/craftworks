//! Previous installations remain separate from the verified, managed library.
//!
//! ZIP folders are moved intact into a neighbouring backup directory only after an explicit
//! user action. Registered Windows MSI copies are removed through Windows Installer.

use std::path::{Path, PathBuf};

use craft_update::Error;

use crate::{APPS, App};

mod portable;
#[cfg(windows)]
mod windows;

/// How a previous copy was installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyMethod {
    /// An unpacked application folder; retirement archives the entire folder.
    PortableFolder,
    /// A registered Windows Installer package.
    WindowsMsi,
}

impl LegacyMethod {
    /// A short description suitable for the launcher.
    pub fn label(self) -> &'static str {
        match self {
            Self::PortableFolder => "ZIP or app folder",
            Self::WindowsMsi => "Windows installer",
        }
    }
}

/// A validated previous copy. Its removal identity cannot be supplied by UI text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyInstall {
    /// The catalog app this copy belongs to.
    pub app: App,
    /// A version reported by its native installer, if available.
    pub version: Option<String>,
    /// What the user is being offered to remove or archive.
    pub description: String,
    /// The original installation location, if the installer records one.
    pub location: Option<PathBuf>,
    /// The installer type.
    pub method: LegacyMethod,
    pub(crate) identity: Identity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Identity {
    Portable(portable::PortableTarget),
    #[cfg(windows)]
    Windows(windows::WindowsTarget),
}

/// The successful retirement of a previous copy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Removal {
    /// Windows Installer needs a reboot to finish removing files.
    pub restart_required: bool,
    /// An archived ZIP folder, including any settings and documents it contained.
    pub backup: Option<PathBuf>,
}

/// Finds native installer entries and dedicated app folders in common user locations.
/// Never runs an old executable or recursively searches a user's drive.
pub fn scan(app: App) -> Result<Vec<LegacyInstall>, Error> {
    validate_app(app)?;
    let mut found = portable::scan(app);
    #[cfg(windows)]
    found.extend(windows::scan(app)?);
    found.sort_by(|a, b| a.location.cmp(&b.location).then_with(|| a.description.cmp(&b.description)));
    found.dedup();
    Ok(found)
}

/// Validates a dedicated old app folder, or the app executable inside it, chosen by the user.
/// Shared folders and managed installations are refused.
pub fn inspect_portable(app: App, path: &Path) -> Result<LegacyInstall, Error> {
    validate_app(app)?;
    portable::inspect(app, path)
}

/// Revalidates a previous copy, then archives its folder or invokes its native uninstaller.
/// This must be called only after the user has reviewed and confirmed the exact target.
pub fn uninstall(install: &LegacyInstall) -> Result<Removal, Error> {
    validate_app(install.app)?;
    match &install.identity {
        Identity::Portable(_) => portable::archive(install),
        #[cfg(windows)]
        Identity::Windows(_) => windows::uninstall(install),
    }
}

fn validate_app(app: App) -> Result<(), Error> {
    if APPS.contains(&app) { Ok(()) } else { Err(Error::Invalid("That app is not in the Craftworks catalog.".into())) }
}
