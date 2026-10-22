//! Installing a verified download, once the app is ready to quit.

use std::fs::{File, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use crate::install_kind::strip_deleted;
use crate::{AppInfo, Error, InstallKind, Version, verify::Hasher};

/// A downloaded, verified update waiting to be installed.
#[derive(Clone, Debug)]
pub struct Prepared {
    /// How this copy is installed (decides how the update is applied).
    pub kind: InstallKind,
    /// The verified file.
    pub file: PathBuf,
    /// The version it installs.
    pub version: Version,
    download: Arc<VerifiedDownload>,
}

#[derive(Debug)]
struct VerifiedDownload {
    directory: Mutex<Option<TempDir>>,
    kind: InstallKind,
    file: PathBuf,
    version: Version,
    digest: [u8; 32],
    size: u64,
}

impl PartialEq for Prepared {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.file == other.file
            && self.version == other.version
            && self.download.digest == other.download.digest
            && self.download.size == other.download.size
    }
}

impl Eq for Prepared {}

/// What [`Prepared::apply`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// The new version is in place; if a restart was asked for, it is starting. Quit now.
    Installed,
    /// The installer starts as soon as this app quits (Windows) and, if a restart was asked for,
    /// opens the new version when it is done. Quit now.
    InstallerStarted,
}

/// Parent of isolated per-download folders. AppImage staging stays on the target filesystem.
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

/// Validate parent directories without following links or Windows reparse points.
fn regular_parents(path: &Path) -> Result<(), Error> {
    if !path.is_absolute() || path.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        return Err(Error::Install("the update folder must be an absolute normal path".into()));
    }
    for parent in path.ancestors() {
        match std::fs::symlink_metadata(parent) {
            Ok(meta) if meta.is_dir() && !redirected(&meta) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(Error::Install("the update folder passes through a link or special file".into())),
        }
    }
    Ok(())
}

fn redirected(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// Allocate a fresh private folder; never sweep another download or a user's files.
pub(crate) fn staging_dir(base: &Path) -> Result<TempDir, Error> {
    regular_parents(base)?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(base).map_err(|e| Error::Install(format!("can't create {}: {e}", base.display())))?;
    regular_parents(base)?;
    tempfile::Builder::new().prefix(".craft-update-").tempdir_in(base).map_err(|e| Error::Install(format!("can't stage an update: {e}")))
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    if !after.is_file() || redirected(after) || before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.dev() == after.dev() && before.ino() == after.ino()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn verified_file(path: &Path, size: u64, digest: &[u8; 32], _kind: &InstallKind) -> Result<File, Error> {
    regular_parents(path.parent().ok_or_else(|| Error::Install("the update file has no folder".into()))?)?;
    let before = std::fs::symlink_metadata(path).map_err(|e| Error::Install(format!("the update is unavailable: {e}")))?;
    if !before.is_file() || redirected(&before) || before.len() != size {
        return Err(Error::Verify("the prepared update was replaced or changed; download it again".into()));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        // Hold a read-only sharing lease through launch: no writer or rename can replace it.
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(if matches!(_kind, InstallKind::AppImage(_)) { 1 | 4 } else { 1 });
    }
    let mut file = options.open(path).map_err(|e| Error::Install(format!("can't read the prepared update: {e}")))?;
    let opened = file.metadata().map_err(|e| Error::Install(e.to_string()))?;
    if !unchanged(&before, &opened) {
        return Err(Error::Verify("the prepared update changed while opening it".into()));
    }
    let mut hasher = Hasher::default();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| Error::Install(format!("can't recheck the update: {e}")))?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > size {
            return Err(Error::Verify("the prepared update grew after verification".into()));
        }
        hasher.update(buffer.get(..n).ok_or_else(|| Error::Verify("invalid update read length".into()))?);
    }
    let after = std::fs::symlink_metadata(path).map_err(|e| Error::Install(e.to_string()))?;
    if total != size || &hasher.finish() != digest || !unchanged(&opened, &after) {
        return Err(Error::Verify("the prepared update no longer matches its signed checksum; download it again".into()));
    }
    Ok(file)
}

impl Prepared {
    pub(crate) fn verified(directory: TempDir, kind: InstallKind, file: PathBuf, version: Version, digest: [u8; 32], size: u64) -> Self {
        let download = Arc::new(VerifiedDownload {
            directory: Mutex::new(Some(directory)),
            kind: kind.clone(),
            file: file.clone(),
            version: version.clone(),
            digest,
            size,
        });
        Self { kind, file, version, download }
    }

    /// Installs the update. Call it when the app is about to quit, after the person has saved
    /// their work: with `restart`, the new version opens once it is installed.
    ///
    /// Blocks while a Linux package manager runs (it asks for a password); returns at once on
    /// Windows, where the installer waits for this app to quit.
    pub fn apply(&self, restart: bool) -> Result<Applied, Error> {
        let mut directory = self.download.directory.lock().map_err(|_| Error::Install("the prepared update is unavailable".into()))?;
        let stage = directory.as_ref().ok_or_else(|| Error::Install("this prepared update was already applied".into()))?;
        if self.file != self.download.file
            || self.kind != self.download.kind
            || self.version != self.download.version
            || self.file.parent() != Some(stage.path())
        {
            return Err(Error::Verify("the prepared update's destination or metadata changed".into()));
        }
        let file = verified_file(&self.file, self.download.size, &self.download.digest, &self.kind)?;
        let result = match &self.kind {
            InstallKind::AppImage(target) => self.apply_appimage(target, restart, &file),
            InstallKind::WindowsMsi => self.apply_windows(WindowsInstaller::Msi, restart),
            InstallKind::WindowsSetup => self.apply_windows(WindowsInstaller::Setup, restart),
            InstallKind::LinuxSetup => self.apply_setup_script(restart),
            InstallKind::Deb | InstallKind::Rpm => self.apply_package(restart),
            other => Err(Error::Unsupported(format!("{other:?} copies don't update themselves"))),
        };
        drop(file);
        // Taking drops synchronous staging. Only the deferred Windows installers retain it.
        if result.is_ok()
            && let Some(stage) = directory.take()
            && matches!(self.kind, InstallKind::WindowsMsi | InstallKind::WindowsSetup)
        {
            let _ = stage.keep();
        }
        result
    }

    fn apply_appimage(&self, target: &Path, restart: bool, file: &File) -> Result<Applied, Error> {
        regular_parents(target.parent().ok_or_else(|| Error::Install("the AppImage has no folder".into()))?)?;
        let target_meta = std::fs::symlink_metadata(target).map_err(|e| Error::Install(e.to_string()))?;
        if !target_meta.is_file() || redirected(&target_meta) {
            return Err(Error::Install("the AppImage target is a link or special file".into()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o755))
                .map_err(|e| Error::Install(format!("can't make the new AppImage runnable: {e}")))?;
        }
        #[cfg(not(unix))]
        let _ = file;
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

    fn apply_setup_script(&self, restart: bool) -> Result<Applied, Error> {
        let bash = find(&["/usr/bin/bash", "/bin/bash"]).ok_or_else(|| Error::Install("installing needs bash, which isn't installed".into()))?;
        // The setup replaces the program by renaming it, so this copy keeps running until it quits.
        let output = Command::new(bash)
            .arg(&self.file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| Error::Install(format!("couldn't start the setup: {e}")))?;
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            let reason: String = said.lines().map(str::trim).rev().find(|line| !line.is_empty()).unwrap_or("no details").chars().take(300).collect();
            return Err(Error::Install(format!("the setup failed ({}): {reason}", output.status)));
        }
        if restart {
            let exe = std::env::current_exe().map_err(|e| Error::Install(e.to_string()))?;
            spawn_detached(Command::new(strip_deleted(&exe)))?;
        }
        Ok(Applied::Installed)
    }

    fn apply_windows(&self, installer: WindowsInstaller, restart: bool) -> Result<Applied, Error> {
        let exe = std::env::current_exe().map_err(|e| Error::Install(e.to_string()))?;
        let script =
            windows_script(&self.file, installer, &self.download.digest, self.download.size, std::process::id(), restart.then_some(exe.as_path()));
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| Error::Install("Windows did not provide its system directory".into()))?;
        let mut cmd = Command::new(root.join("System32/WindowsPowerShell/v1.0/powershell.exe"));
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

/// What the deferred Windows handoff runs once the app has quit.
#[derive(Clone, Copy)]
enum WindowsInstaller {
    /// Windows Installer: `msiexec /i … /passive /norestart`.
    Msi,
    /// The per-user setup program itself, silently.
    Setup,
}

/// The PowerShell that runs the installer once the app has quit, then reopens the app.
/// Windows paths can't contain `"`, and `'` is doubled inside PowerShell's single quotes, so
/// no path can break out of its string.
fn windows_script(file: &Path, installer: WindowsInstaller, digest: &[u8; 32], size: u64, pid: u32, reopen: Option<&Path>) -> String {
    let quote = |p: &Path| p.to_string_lossy().replace('\'', "''");
    let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    // The read-sharing lease stays open while the installer runs, so nothing can replace it.
    let run = match installer {
        WindowsInstaller::Msi => format!(
            "$u=Start-Process -FilePath (Join-Path $env:SystemRoot 'System32/msiexec.exe') -ArgumentList '/i \"{}\" /passive /norestart' -Wait -PassThru; if($u.ExitCode -notin @(0,1641,3010)){{throw 'Windows Installer did not complete.'}}; ",
            quote(file)
        ),
        WindowsInstaller::Setup => {
            format!(
                "$u=Start-Process -FilePath '{}' -ArgumentList '/S' -Wait -PassThru; if($u.ExitCode -ne 0){{throw 'The setup did not complete.'}}; ",
                quote(file)
            )
        }
    };
    let mut s = format!(
        "$ErrorActionPreference='Stop'; $f=$null; try{{$p=Get-Process -Id {pid} -ErrorAction SilentlyContinue; if($p -and -not $p.WaitForExit(60000)){{throw 'The app did not exit.'}}; $f=[IO.File]::Open('{}',[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read); if($f.Length -ne {size}){{throw 'The update size changed.'}}; $sha=[Security.Cryptography.SHA256]::Create(); try{{$h=[BitConverter]::ToString($sha.ComputeHash($f)).Replace('-','').ToLowerInvariant()}}finally{{$sha.Dispose()}}; if($h -ne '{hash}'){{throw 'The update checksum changed.'}}; {run}",
        quote(file)
    );
    if let Some(exe) = reopen {
        s.push_str(&format!("Start-Process -FilePath '{}'; ", quote(exe)));
    }
    s.push_str("}catch{Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('The update could not be installed. Check for updates and try again.','Craft update') | Out-Null; exit 1}finally{if($f){$f.Dispose()}; ");
    s.push_str(&format!(
        "Remove-Item -LiteralPath '{}' -ErrorAction SilentlyContinue; [IO.Directory]::Delete('{}') }}",
        quote(file),
        quote(file.parent().unwrap_or_else(|| Path::new("")))
    ));
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

    fn prepared(base: &Path, kind: InstallKind, name: &str, bytes: &[u8]) -> Prepared {
        let directory = staging_dir(base).unwrap();
        let file = directory.path().join(name);
        std::fs::write(&file, bytes).unwrap();
        let mut hash = Hasher::default();
        hash.update(bytes);
        Prepared::verified(directory, kind, file, Version::parse("1.2.0").unwrap(), hash.finish(), bytes.len() as u64)
    }

    #[test]
    fn the_windows_script_quotes_every_path() {
        let s = windows_script(
            Path::new(r"C:\Users\O'Neil & Co\AppData\Local\PdfCraft\updates\pdfcraft-1.2.0-windows-x64.msi"),
            WindowsInstaller::Msi,
            &[0xab; 32],
            123,
            456,
            Some(Path::new(r"C:\Program Files\PdfCraft\pdfcraft.exe")),
        );
        assert!(
            s.contains(r#"'/i "C:\Users\O''Neil & Co\AppData\Local\PdfCraft\updates\pdfcraft-1.2.0-windows-x64.msi" /passive /norestart'"#),
            "{s}"
        );
        assert!(s.contains(r"Start-Process -FilePath 'C:\Program Files\PdfCraft\pdfcraft.exe'"), "{s}");
        assert!(s.contains(&"ab".repeat(32)) && s.contains("$f.Length -ne 123") && s.contains("[IO.FileShare]::Read"));
        assert!(s.contains("Get-Process -Id 456") && s.contains("finally") && s.contains("Remove-Item -LiteralPath"));
        assert!(!windows_script(Path::new("a.msi"), WindowsInstaller::Msi, &[0; 32], 1, 1, None).contains("pdfcraft.exe"));
        let setup = windows_script(
            Path::new(r"C:\Users\O'Neil\AppData\Local\Craftworks\updates\craftworks-1.2.0-windows-x64-setup.exe"),
            WindowsInstaller::Setup,
            &[0xcd; 32],
            77,
            88,
            Some(Path::new(r"C:\Users\O'Neil\AppData\Local\Programs\Craftworks\craftworks.exe")),
        );
        assert!(
            setup.contains(r"Start-Process -FilePath 'C:\Users\O''Neil\AppData\Local\Craftworks\updates\craftworks-1.2.0-windows-x64-setup.exe' -ArgumentList '/S' -Wait"),
            "{setup}"
        );
        assert!(setup.contains("$u.ExitCode -ne 0") && !setup.contains("msiexec") && setup.contains(&"cd".repeat(32)));
        assert!(setup.contains("Get-Process -Id 88") && setup.contains("$f.Length -ne 77") && setup.contains("[IO.FileShare]::Read"));
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
        let p = prepared(dir.path(), kind, "pdfcraft-1.2.0-linux-x86_64.AppImage", b"new");
        let file = p.file.clone();
        assert_eq!(p.apply(false).unwrap(), Applied::Installed);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!file.exists());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        // Applying twice finds the download gone and says so.
        assert!(matches!(p.apply(false), Err(Error::Install(_))));
    }

    #[cfg(unix)]
    #[test]
    fn a_linux_setup_script_runs_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("installed");
        let script = format!("#!/usr/bin/env bash\nprintf new > '{}'\n", marker.display());
        let p = prepared(dir.path(), InstallKind::LinuxSetup, "pdfcraft-1.2.0-linux-x86_64-setup.sh", script.as_bytes());
        let stage = p.file.parent().unwrap().to_path_buf();
        assert_eq!(p.apply(false).unwrap(), Applied::Installed);
        assert_eq!(std::fs::read(&marker).unwrap(), b"new");
        // The synchronous setup leaves nothing staged behind, and only runs once.
        assert!(!stage.exists());
        assert!(matches!(p.apply(false), Err(Error::Install(_))));
        let failing = prepared(dir.path(), InstallKind::LinuxSetup, "bad-setup.sh", b"echo 'Craftworks setup: no room' >&2\nexit 3\n");
        match failing.apply(false) {
            Err(Error::Install(message)) => assert!(message.contains("no room"), "{message}"),
            other => panic!("a failed setup was reported as {other:?}"),
        }
    }

    #[test]
    fn other_copies_refuse_to_apply() {
        let dir = tempfile::tempdir().unwrap();
        let p = prepared(dir.path(), InstallKind::WindowsPortable, "x.zip", b"z");
        assert!(matches!(p.apply(true), Err(Error::Unsupported(_))));
    }

    #[test]
    fn independent_downloads_and_existing_user_files_are_never_swept() {
        let dir = tempfile::tempdir().unwrap();
        let app_image = dir.path().join("pdfcraft.AppImage");
        std::fs::write(&app_image, b"app").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"mine").unwrap();
        std::fs::write(dir.path().join(".pdfcraft-1.1.0-linux-x86_64.AppImage.update"), b"stale").unwrap();
        let first = prepared(dir.path(), InstallKind::AppImage(app_image.clone()), "first.AppImage", b"first");
        let second = prepared(dir.path(), InstallKind::AppImage(app_image.clone()), "second.AppImage", b"second");
        assert_ne!(first.file.parent(), second.file.parent());
        assert_eq!(std::fs::read(&first.file).unwrap(), b"first");
        assert!(app_image.exists() && dir.path().join("notes.txt").exists());
        assert!(dir.path().join(".pdfcraft-1.1.0-linux-x86_64.AppImage.update").exists());
        let second_path = second.file.parent().unwrap().to_path_buf();
        drop(second);
        assert!(!second_path.exists());
        assert!(first.file.exists());
        let updates = dir.path().join("updates");
        std::fs::create_dir(&updates).unwrap();
        std::fs::write(updates.join("old.msi"), b"old").unwrap();
        let ready = prepared(&updates, InstallKind::WindowsMsi, "new.msi", b"new");
        assert!(updates.join("old.msi").exists());
        drop(ready);
        assert_eq!(std::fs::read(updates.join("old.msi")).unwrap(), b"old");
    }

    #[test]
    fn a_prepared_download_is_kept_until_the_last_owner_drops() {
        let base = tempfile::tempdir().unwrap();
        let first = prepared(base.path(), InstallKind::WindowsPortable, "x.zip", b"new");
        let path = first.file.parent().unwrap().to_path_buf();
        let second = first.clone();
        drop(first);
        assert!(path.exists());
        drop(second);
        assert!(!path.exists());
    }

    #[test]
    fn changed_payload_and_public_metadata_cannot_be_applied() {
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("App.AppImage");
        std::fs::write(&target, b"old").unwrap();
        let mut first = prepared(base.path(), InstallKind::AppImage(target.clone()), "x.AppImage", b"new");
        std::fs::write(&first.file, b"bad").unwrap();
        assert!(matches!(first.apply(false), Err(Error::Verify(_))));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        std::fs::write(&first.file, b"new").unwrap();
        first.file = target.clone();
        assert!(matches!(first.apply(false), Err(Error::Verify(_))));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    #[cfg(unix)]
    #[test]
    fn linked_staging_and_changed_download_links_are_refused() {
        use std::os::unix::fs::symlink;
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("App.AppImage");
        std::fs::write(&target, b"old").unwrap();
        let linked = base.path().join("linked");
        symlink(base.path(), &linked).unwrap();
        assert!(staging_dir(&linked).is_err());
        let ready = prepared(base.path(), InstallKind::AppImage(target.clone()), "x.AppImage", b"new");
        let unrelated = base.path().join("other");
        std::fs::write(&unrelated, b"new").unwrap();
        std::fs::remove_file(&ready.file).unwrap();
        symlink(&unrelated, &ready.file).unwrap();
        assert!(matches!(ready.apply(false), Err(Error::Verify(_))));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        drop(ready);
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"new");
    }

    #[cfg(windows)]
    #[test]
    fn the_deferred_scripts_parse_in_windows_powershell() {
        let base = tempfile::tempdir().unwrap();
        for (name, installer) in [("x.msi", WindowsInstaller::Msi), ("x-setup.exe", WindowsInstaller::Setup)] {
            let path = base.path().join("script.ps1");
            std::fs::write(
                &path,
                windows_script(
                    &Path::new(r"C:\Users\O'Neil & Co\updates").join(name),
                    installer,
                    &[0xab; 32],
                    123,
                    456,
                    Some(Path::new(r"C:\Program Files\App\app.exe")),
                ),
            )
            .unwrap();
            let system = std::env::var_os("SystemRoot").unwrap();
            let check = format!(
                "$t=$null; $e=$null; [System.Management.Automation.Language.Parser]::ParseFile('{}',[ref]$t,[ref]$e) | Out-Null; if($e.Count){{$e | Out-String | Write-Error; exit 1}}",
                path.to_string_lossy().replace('\'', "''")
            );
            let output = Command::new(PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"))
                .args(["-NoProfile", "-NonInteractive", "-Command", &check])
                .output()
                .unwrap();
            assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
}
