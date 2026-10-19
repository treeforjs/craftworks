//! Installing a verified download, once the app is ready to quit.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::install_kind::strip_deleted;
use crate::{AppInfo, Error, InstallKind, Version};

/// A downloaded, verified update waiting to be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    /// How this copy is installed (decides how the update is applied).
    pub kind: InstallKind,
    /// The verified file.
    pub file: PathBuf,
    /// The version it installs.
    pub version: Version,
}

/// What [`Prepared::apply`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// The new version is in place; if a restart was asked for, it is starting. Quit now.
    Installed,
    /// The installer starts as soon as this app quits (Windows) and, if a restart was asked for,
    /// opens the new version when it is done. Quit now.
    InstallerStarted,
}

/// Where downloads wait: a folder only this user can write, so nothing can swap a verified
/// file before it is installed.
pub(crate) fn download_dir(app: &AppInfo, kind: &InstallKind) -> Result<PathBuf, Error> {
    // An AppImage is replaced by renaming, which needs the new file on the same disk.
    if let InstallKind::AppImage(path) = kind {
        let dir = path.parent().ok_or_else(|| Error::Install("the AppImage has no folder".into()))?;
        return Ok(dir.to_path_buf());
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    let base = base.filter(|p| p.is_absolute()).ok_or_else(|| Error::Install("no folder to download the update into".into()))?;
    Ok(base.join(&app.display_name).join("updates"))
}

/// The file a download of `asset` is written to in `dir`.
pub(crate) fn download_path(dir: &Path, kind: &InstallKind, asset: &str) -> PathBuf {
    match kind {
        // Hidden beside the AppImage until it replaces it.
        InstallKind::AppImage(_) => dir.join(format!(".{asset}.update")),
        _ => dir.join(asset),
    }
}

/// Makes the download folder (private to this user on Unix) and removes earlier downloads.
pub(crate) fn prepare_dir(dir: &Path, kind: &InstallKind) -> Result<(), Error> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir).map_err(|e| Error::Install(format!("can't create {}: {e}", dir.display())))?;
    if matches!(kind, InstallKind::AppImage(_)) {
        // The AppImage's own folder: leave everything but our hidden partial downloads alone.
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') && name.ends_with(".AppImage.update") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        return Ok(());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_type().is_ok_and(|t| t.is_file()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(())
}

impl Prepared {
    /// Installs the update. Call it when the app is about to quit, after the person has saved
    /// their work: with `restart`, the new version opens once it is installed.
    ///
    /// Blocks while a Linux package manager runs (it asks for a password); returns at once on
    /// Windows, where the installer waits for this app to quit.
    pub fn apply(&self, restart: bool) -> Result<Applied, Error> {
        if !self.file.is_file() {
            return Err(Error::Install("the downloaded update is gone; check for updates again".into()));
        }
        match &self.kind {
            InstallKind::AppImage(target) => self.apply_appimage(target, restart),
            InstallKind::WindowsMsi => self.apply_msi(restart),
            InstallKind::Deb | InstallKind::Rpm => self.apply_package(restart),
            other => Err(Error::Unsupported(format!("{other:?} copies don't update themselves"))),
        }
    }

    fn apply_appimage(&self, target: &Path, restart: bool) -> Result<Applied, Error> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.file, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| Error::Install(format!("can't make the new AppImage runnable: {e}")))?;
        }
        // Atomic on one disk: the running copy keeps working from its open file until it quits.
        std::fs::rename(&self.file, target).map_err(|e| Error::Install(format!("can't replace {}: {e}", target.display())))?;
        if restart {
            spawn_detached(Command::new(target))?;
        }
        Ok(Applied::Installed)
    }

    fn apply_package(&self, restart: bool) -> Result<Applied, Error> {
        let file = self.file.to_string_lossy().into_owned();
        let pkexec = find(&["/usr/bin/pkexec", "/bin/pkexec"])
            .ok_or_else(|| Error::Install("installing needs pkexec (polkit), which isn't installed".into()))?;
        let attempts: Vec<Vec<&str>> = if self.kind == InstallKind::Deb {
            vec![vec!["/usr/bin/apt-get", "install", "-y", "--allow-downgrades", &file], vec!["/usr/bin/dpkg", "-i", &file]]
        } else {
            vec![
                vec!["/usr/bin/dnf", "install", "-y", &file],
                vec!["/usr/bin/zypper", "--non-interactive", "install", "--allow-unsigned-rpm", &file],
                vec!["/usr/bin/rpm", "-U", "--replacepkgs", &file],
            ]
        };
        let Some(args) = attempts.into_iter().find(|a| a.first().is_some_and(|p| Path::new(p).is_file())) else {
            return Err(Error::Install("no package manager found to install the update".into()));
        };
        let status = Command::new(pkexec)
            .args(&args)
            .stdin(Stdio::null())
            .status()
            .map_err(|e| Error::Install(format!("couldn't start the package manager: {e}")))?;
        match status.code() {
            Some(0) => {}
            // pkexec: the person dismissed the password prompt, or wasn't allowed.
            Some(126 | 127) => return Err(Error::Cancelled),
            _ => return Err(Error::Install(format!("the package manager failed ({status})"))),
        }
        if restart {
            let exe = std::env::current_exe().map_err(|e| Error::Install(e.to_string()))?;
            spawn_detached(Command::new(strip_deleted(&exe)))?;
        }
        Ok(Applied::Installed)
    }

    fn apply_msi(&self, restart: bool) -> Result<Applied, Error> {
        let exe = std::env::current_exe().map_err(|e| Error::Install(e.to_string()))?;
        let script = msi_script(&self.file, restart.then_some(exe.as_path()));
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encode_powershell(&script),
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        spawn_detached(cmd)?;
        Ok(Applied::InstallerStarted)
    }
}

/// The PowerShell that runs the installer once the app has quit, then reopens the app.
/// Windows paths can't contain `"`, and `'` is doubled inside PowerShell's single quotes, so
/// no path can break out of its string.
fn msi_script(msi: &Path, reopen: Option<&Path>) -> String {
    let quote = |p: &Path| p.to_string_lossy().replace('\'', "''");
    // Give the app a moment to quit; msiexec's Restart Manager handles one that is slow to.
    let mut s =
        format!("Start-Sleep -Seconds 2; Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i \"{}\" /passive /norestart' -Wait", quote(msi));
    if let Some(exe) = reopen {
        s.push_str(&format!("; Start-Process -FilePath '{}'", quote(exe)));
    }
    s
}

/// `-EncodedCommand` takes base64 of UTF-16LE.
fn encode_powershell(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn find(paths: &[&'static str]) -> Option<&'static str> {
    paths.iter().copied().find(|p| Path::new(p).is_file())
}

fn spawn_detached(mut cmd: Command) -> Result<(), Error> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().map(drop).map_err(|e| Error::Install(format!("couldn't start the new version: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_script_quotes_every_path() {
        let s = msi_script(
            Path::new(r"C:\Users\O'Neil & Co\AppData\Local\PdfCraft\updates\pdfcraft-1.2.0-windows-x64.msi"),
            Some(Path::new(r"C:\Program Files\PdfCraft\pdfcraft.exe")),
        );
        assert!(
            s.contains(r#"'/i "C:\Users\O''Neil & Co\AppData\Local\PdfCraft\updates\pdfcraft-1.2.0-windows-x64.msi" /passive /norestart'"#),
            "{s}"
        );
        assert!(s.ends_with(r"Start-Process -FilePath 'C:\Program Files\PdfCraft\pdfcraft.exe'"), "{s}");
        assert!(!msi_script(Path::new("a.msi"), None).contains("pdfcraft.exe"));
        // UTF-16LE then base64: "a" is 61 00.
        assert_eq!(encode_powershell("a"), "YQA=");
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_is_replaced_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("pdfcraft.AppImage");
        std::fs::write(&target, b"old").unwrap();
        let kind = InstallKind::AppImage(target.clone());
        prepare_dir(dir.path(), &kind).unwrap();
        let file = download_path(dir.path(), &kind, "pdfcraft-1.2.0-linux-x86_64.AppImage");
        std::fs::write(&file, b"new").unwrap();
        let p = Prepared { kind, file: file.clone(), version: Version::parse("1.2.0").unwrap() };
        assert_eq!(p.apply(false).unwrap(), Applied::Installed);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!file.exists());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        // Applying twice finds the download gone and says so.
        assert!(matches!(p.apply(false), Err(Error::Install(_))));
    }

    #[test]
    fn other_copies_refuse_to_apply() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.zip");
        std::fs::write(&file, b"z").unwrap();
        let p = Prepared { kind: InstallKind::WindowsPortable, file, version: Version::parse("1.0.0").unwrap() };
        assert!(matches!(p.apply(true), Err(Error::Unsupported(_))));
    }

    #[test]
    fn earlier_downloads_are_cleared_but_not_the_users_files() {
        let dir = tempfile::tempdir().unwrap();
        let app_image = dir.path().join("pdfcraft.AppImage");
        std::fs::write(&app_image, b"app").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"mine").unwrap();
        std::fs::write(dir.path().join(".pdfcraft-1.1.0-linux-x86_64.AppImage.update"), b"stale").unwrap();
        prepare_dir(dir.path(), &InstallKind::AppImage(app_image.clone())).unwrap();
        assert!(app_image.exists() && dir.path().join("notes.txt").exists());
        assert!(!dir.path().join(".pdfcraft-1.1.0-linux-x86_64.AppImage.update").exists());
        let updates = dir.path().join("updates");
        std::fs::create_dir(&updates).unwrap();
        std::fs::write(updates.join("old.msi"), b"old").unwrap();
        prepare_dir(&updates, &InstallKind::WindowsMsi).unwrap();
        assert!(!updates.join("old.msi").exists());
    }
}
