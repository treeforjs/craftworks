//! Removal of the apps' older, registered Windows Installer packages.
//!
//! Current and earlier WiX packages use the names in `APPS`. Keep the old publisher
//! as compatibility data. We never execute registry uninstall strings.

use std::ffi::OsString;
use std::io;
use std::path::{Component, PathBuf};
use std::process::Command;

use craft_update::{Error, Version};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

use super::{Identity, LegacyInstall, LegacyMethod, Removal};
use crate::App;

const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const WINDOWS_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const PUBLISHER: &str = "Craft contributors";
// Compatibility with already installed MSI packages; never displayed as current branding.
const LEGACY_PUBLISHER: &str = "Learning Machines LLC";
const REGISTRY_VIEWS: [u32; 2] = [KEY_WOW64_64KEY, KEY_WOW64_32KEY];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    fn open(self) -> RegKey {
        RegKey::predef(match self {
            Self::CurrentUser => HKEY_CURRENT_USER,
            Self::LocalMachine => HKEY_LOCAL_MACHINE,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::CurrentUser => "this user",
            Self::LocalMachine => "all users",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegistryEntry {
    name: String,
    publisher: String,
    version: Option<String>,
    location: Option<String>,
    windows_installer: u32,
}

/// An opaque Windows Installer identity, including the registry snapshot shown to the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowsTarget {
    hive: Hive,
    view: u32,
    key: String,
    product_code: String,
    entry: RegistryEntry,
}

pub(crate) fn scan(app: App) -> Result<Vec<LegacyInstall>, Error> {
    let mut installs = Vec::new();
    for hive in [Hive::CurrentUser, Hive::LocalMachine] {
        for view in REGISTRY_VIEWS {
            let Some(parent) = optional_key(&hive.open(), UNINSTALL_KEY, view)? else { continue };
            for key in parent.enum_keys() {
                let key = key.map_err(registry_error)?;
                // Other uninstall records have no bearing on our catalog. In particular,
                // EXE uninstallers and the Craftworks hub are never candidates.
                if !valid_product_code(&key) {
                    continue;
                }
                if let Some(entry) = read_entry(&parent, &key, view)?
                    && let Some(install) = identify(app, hive, view, key, entry)
                    && !installs.iter().any(|old: &LegacyInstall| same_package(&old.identity, &install.identity))
                {
                    installs.push(install);
                }
            }
        }
    }
    Ok(installs)
}

fn same_package(first: &Identity, second: &Identity) -> bool {
    match (first, second) {
        (Identity::Windows(first), Identity::Windows(second)) => first.hive == second.hive && first.product_code == second.product_code,
        _ => false,
    }
}

fn optional_key(parent: &RegKey, key: &str, view: u32) -> Result<Option<RegKey>, Error> {
    match parent.open_subkey_with_flags(key, KEY_READ | view) {
        Ok(key) => Ok(Some(key)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        // The 64-bit view does not exist on 32-bit Windows.
        Err(error) if view == KEY_WOW64_64KEY && error.raw_os_error() == Some(87) => Ok(None),
        Err(error) => Err(registry_error(error)),
    }
}

fn optional_string(key: &RegKey, name: &str) -> Result<Option<String>, Error> {
    match key.get_value::<String, _>(name) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound || error.kind() == io::ErrorKind::InvalidData => Ok(None),
        Err(error) => Err(registry_error(error)),
    }
}

fn read_entry(parent: &RegKey, name: &str, view: u32) -> Result<Option<RegistryEntry>, Error> {
    let Some(key) = optional_key(parent, name, view)? else { return Ok(None) };
    let Some(name) = optional_string(&key, "DisplayName")? else { return Ok(None) };
    let Some(publisher) = optional_string(&key, "Publisher")? else { return Ok(None) };
    let windows_installer = match key.get_value::<u32, _>("WindowsInstaller") {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound || error.kind() == io::ErrorKind::InvalidData => return Ok(None),
        Err(error) => return Err(registry_error(error)),
    };
    Ok(Some(RegistryEntry {
        name,
        publisher,
        version: optional_string(&key, "DisplayVersion")?,
        location: optional_string(&key, "InstallLocation")?,
        windows_installer,
    }))
}

fn identify(app: App, hive: Hive, view: u32, key: String, entry: RegistryEntry) -> Option<LegacyInstall> {
    if !REGISTRY_VIEWS.contains(&view)
        || entry.windows_installer != 1
        || ![PUBLISHER, LEGACY_PUBLISHER].iter().any(|publisher| entry.publisher.eq_ignore_ascii_case(publisher))
        || !matches_name(app, &entry)
        || !valid_product_code(&key)
    {
        return None;
    }
    let location = entry
        .location
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && !path.components().any(|part| matches!(part, Component::ParentDir)));
    Some(LegacyInstall {
        app,
        version: entry.version.clone(),
        description: format!("Windows Installer package for {}", hive.label()),
        location,
        method: LegacyMethod::WindowsMsi,
        identity: Identity::Windows(WindowsTarget { hive, view, product_code: key.to_ascii_uppercase(), key, entry }),
    })
}

fn matches_name(app: App, entry: &RegistryEntry) -> bool {
    if entry.name.eq_ignore_ascii_case(app.name) {
        return true;
    }
    // Accept a version suffix only when it exactly agrees with DisplayVersion.
    // Do not mistake a helper, SDK, another product, or a substring for the app.
    let Some((name, suffix)) = entry.name.rsplit_once(' ') else { return false };
    name.eq_ignore_ascii_case(app.name)
        && entry.version.as_deref() == Some(suffix)
        && Version::parse(suffix).is_some()
        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn valid_product_code(value: &str) -> bool {
    value.len() == 38
        && value.bytes().enumerate().all(|(index, byte)| match index {
            0 => byte == b'{',
            37 => byte == b'}',
            9 | 14 | 19 | 24 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

pub(crate) fn uninstall(install: &LegacyInstall) -> Result<Removal, Error> {
    remove_checked(
        install,
        |target| {
            let Some(parent) = optional_key(&target.hive.open(), UNINSTALL_KEY, target.view)? else { return Ok(None) };
            read_entry(&parent, &target.key, target.view)
        },
        |target| {
            let mut command = uninstall_command(msiexec()?, target)?;
            command.status().map(|status| status.code()).map_err(|error| {
                if error.raw_os_error() == Some(1223) {
                    Error::Cancelled
                } else {
                    Error::Install(format!("Windows could not start the old app's removal: {error}"))
                }
            })
        },
    )
}

fn remove_checked(
    install: &LegacyInstall,
    read: impl FnOnce(&WindowsTarget) -> Result<Option<RegistryEntry>, Error>,
    run: impl FnOnce(&WindowsTarget) -> Result<Option<i32>, Error>,
) -> Result<Removal, Error> {
    let Identity::Windows(target) = &install.identity else {
        return Err(Error::Unsupported("This copy is not a Windows Installer package.".into()));
    };
    let current = read(target)?.and_then(|entry| identify(install.app, target.hive, target.view, target.key.clone(), entry));
    if current.as_ref() != Some(install) {
        return Err(Error::Install("The old installation changed or was removed. Refresh the library before removing it.".into()));
    }
    removal_result(run(target)?)
}

fn uninstall_command(executable: PathBuf, target: &WindowsTarget) -> Result<Command, Error> {
    if !valid_product_code(&target.product_code) {
        return Err(Error::Install("The old installation has an invalid Windows Installer product code.".into()));
    }
    let mut command = Command::new(executable);
    command.args([OsString::from("/x"), OsString::from(&target.product_code), OsString::from("/passive"), OsString::from("/norestart")]);
    Ok(command)
}

fn msiexec() -> Result<PathBuf, Error> {
    // Read the administrator-owned Windows location instead of trusting PATH or an
    // environment variable when launching a program that may request elevation.
    let root = Hive::LocalMachine.open();
    let key = optional_key(&root, WINDOWS_KEY, KEY_WOW64_64KEY)?
        .or(optional_key(&root, WINDOWS_KEY, KEY_WOW64_32KEY)?)
        .ok_or_else(|| Error::Install("Windows did not provide its system directory.".into()))?;
    let directory = optional_string(&key, "SystemRoot")?
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && !path.components().any(|part| matches!(part, Component::ParentDir)))
        .ok_or_else(|| Error::Install("Windows did not provide a usable system directory.".into()))?;
    let executable = directory.join("System32").join("msiexec.exe");
    if !executable.is_file() {
        return Err(Error::Install("Windows Installer is unavailable on this computer.".into()));
    }
    Ok(executable)
}

fn removal_result(code: Option<i32>) -> Result<Removal, Error> {
    match code {
        Some(0 | 1605) => Ok(Removal { restart_required: false, ..Removal::default() }),
        Some(1641 | 3010) => Ok(Removal { restart_required: true, ..Removal::default() }),
        Some(1602 | 1223) => Err(Error::Cancelled),
        Some(1618) => Err(Error::Install("Windows is installing or removing another app. Wait for it to finish, then try again.".into())),
        Some(code) => {
            Err(Error::Install(format!("Windows Installer could not remove the old app (code {code}). Try removing it in Windows Settings.")))
        }
        None => Err(Error::Install("Windows Installer stopped before confirming that the old app was removed.".into())),
    }
}

fn registry_error(error: io::Error) -> Error {
    Error::Install(format!("Craftworks could not read installed apps from Windows: {error}"))
}

#[cfg(test)]
mod tests;
