use std::path::{Path, PathBuf};

use crate::AppInfo;
use crate::Version;

/// The operating system, as release asset names spell it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// Windows.
    Windows,
    /// Linux.
    Linux,
    /// macOS.
    MacOs,
    /// Anything else (FreeBSD, the web).
    Other,
}

impl Os {
    /// The system this was built for.
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Other
        }
    }
}

/// The processor architecture this was built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    /// 64-bit x86.
    X86_64,
    /// 32-bit x86.
    X86,
    /// 64-bit ARM.
    Aarch64,
    /// Anything else.
    Other,
}

impl Arch {
    /// The architecture this was built for.
    pub fn current() -> Arch {
        if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "x86") {
            Arch::X86
        } else if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            Arch::Other
        }
    }

    /// How release asset names spell it on `os` (`x64` on Windows, `x86_64` on Linux).
    fn asset_name(self, os: Os) -> Option<&'static str> {
        match (os, self) {
            (Os::Windows, Arch::X86_64) => Some("x64"),
            (Os::Windows, Arch::X86) => Some("x86"),
            (Os::Windows, Arch::Aarch64) => Some("arm64"),
            (Os::Linux, Arch::X86_64) => Some("x86_64"),
            (Os::Linux, Arch::Aarch64) => Some("aarch64"),
            _ => None,
        }
    }
}

/// How this copy of the app was installed, which decides how it updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// The Windows installer (`.msi`, per machine): updated by running the new `.msi`.
    WindowsMsi,
    /// The Windows portable zip (a `portable.txt` beside the program): not updated in place.
    WindowsPortable,
    /// A Linux AppImage at this path: updated by replacing the file.
    AppImage(PathBuf),
    /// The Debian/Ubuntu package: updated through the package manager (asks for a password).
    Deb,
    /// The Fedora/openSUSE package: updated through the package manager (asks for a password).
    Rpm,
    /// A Flatpak: updated by Flatpak, not by the app.
    Flatpak,
    /// The macOS app: not updated in place yet.
    MacApp,
    /// A tarball or a build from source (`cargo run`): not updated in place.
    Other,
}

/// What [`InstallKind::detect_from`] looks at, gathered by [`InstallKind::detect`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The running program.
    pub exe: PathBuf,
    /// `$APPIMAGE`, set by the AppImage runtime.
    pub appimage: Option<PathBuf>,
    /// Running inside Flatpak (`/.flatpak-info` exists).
    pub flatpak: bool,
    /// A portable marker (`portable.txt`, `<Name>.portable`) sits beside the program.
    pub portable_marker: bool,
    /// dpkg lists the app's package as installed.
    pub dpkg_owned: bool,
    /// rpm lists the app's package as installed.
    pub rpm_owned: bool,
}

impl InstallKind {
    /// Looks at this copy. Cheap file checks only, plus one `rpm -q` on Linux systems with rpm
    /// whose program lives under `/usr`; call it off the UI thread anyway.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn detect(app: &AppInfo) -> InstallKind {
        let exe = std::env::current_exe().map(|p| strip_deleted(&p)).unwrap_or_default();
        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let portable_marker = ["portable.txt".to_owned(), format!("{}.portable", app.display_name)].iter().any(|m| dir.join(m).is_file());
        let os = Os::current();
        let under_usr = exe.starts_with("/usr");
        let dpkg_owned = os == Os::Linux && under_usr && Path::new(&format!("/var/lib/dpkg/info/{}.list", app.id)).is_file();
        let rpm_owned = os == Os::Linux
            && under_usr
            && !dpkg_owned
            && Path::new("/usr/bin/rpm").is_file()
            && std::process::Command::new("/usr/bin/rpm")
                .args(["-q", "--quiet", &app.id])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
        let facts = Facts {
            exe,
            appimage: std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_absolute()),
            flatpak: Path::new("/.flatpak-info").exists(),
            portable_marker,
            dpkg_owned,
            rpm_owned,
        };
        InstallKind::detect_from(os, &facts)
    }

    /// Decides from `facts` (testable without installing anything).
    pub fn detect_from(os: Os, facts: &Facts) -> InstallKind {
        // `cargo run` builds live under a `target` folder (checked as text so Windows paths read
        // the same on every system).
        let exe = facts.exe.to_string_lossy();
        let from_source = exe.contains("/target/") || exe.contains("\\target\\");
        match os {
            Os::Windows if facts.portable_marker => InstallKind::WindowsPortable,
            Os::Windows if from_source || facts.exe.as_os_str().is_empty() => InstallKind::Other,
            Os::Windows => InstallKind::WindowsMsi,
            Os::Linux if facts.flatpak => InstallKind::Flatpak,
            Os::Linux => match &facts.appimage {
                Some(p) => InstallKind::AppImage(p.clone()),
                None if facts.dpkg_owned => InstallKind::Deb,
                None if facts.rpm_owned => InstallKind::Rpm,
                None => InstallKind::Other,
            },
            Os::MacOs => InstallKind::MacApp,
            Os::Other => InstallKind::Other,
        }
    }

    /// The release asset that updates this copy, or `None` when it can't update itself.
    pub fn asset_name(&self, app: &AppInfo, version: &Version, arch: Arch) -> Option<String> {
        let (os, ext) = match self {
            InstallKind::WindowsMsi => (Os::Windows, "msi"),
            InstallKind::AppImage(_) => (Os::Linux, "AppImage"),
            InstallKind::Deb => (Os::Linux, "deb"),
            InstallKind::Rpm => (Os::Linux, "rpm"),
            _ => return None,
        };
        let arch = arch.asset_name(os)?;
        let os = if os == Os::Windows { "windows" } else { "linux" };
        Some(format!("{}-{version}-{os}-{arch}.{ext}", app.id))
    }

    /// Why this copy can't update itself, in words for people; `None` when it can.
    pub fn why_not(&self, app: &AppInfo) -> Option<String> {
        let name = &app.display_name;
        match self {
            InstallKind::WindowsMsi | InstallKind::AppImage(_) | InstallKind::Deb | InstallKind::Rpm => None,
            InstallKind::WindowsPortable => Some(format!("This portable copy of {name} doesn't update itself. Download the new portable zip.")),
            InstallKind::Flatpak => Some(format!("{name} installed with Flatpak updates through Flatpak or your software centre.")),
            InstallKind::MacApp => Some(format!("{name} for macOS doesn't update itself yet. Download the new version.")),
            InstallKind::Other => Some(format!("This copy of {name} wasn't installed with an installer, so it doesn't update itself.")),
        }
    }
}

/// Linux reports a replaced program as `/usr/bin/app (deleted)`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn strip_deleted(p: &Path) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(s) => PathBuf::from(s),
        None => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::tests::app;

    fn facts(exe: &str) -> Facts {
        Facts { exe: exe.into(), ..Facts::default() }
    }

    #[test]
    fn each_install_is_recognised() {
        let w = facts(r"C:\Program Files\PdfCraft\pdfcraft.exe");
        assert_eq!(InstallKind::detect_from(Os::Windows, &w), InstallKind::WindowsMsi);
        let p = Facts { portable_marker: true, ..w.clone() };
        assert_eq!(InstallKind::detect_from(Os::Windows, &p), InstallKind::WindowsPortable);
        assert_eq!(InstallKind::detect_from(Os::Windows, &facts(r"C:\src\pdfcraft\target\release\pdfcraft.exe")), InstallKind::Other);
        let a = Facts { appimage: Some("/home/me/Apps/pdfcraft.AppImage".into()), ..facts("/tmp/.mount_x/usr/bin/pdfcraft") };
        assert_eq!(InstallKind::detect_from(Os::Linux, &a), InstallKind::AppImage("/home/me/Apps/pdfcraft.AppImage".into()));
        let d = Facts { dpkg_owned: true, ..facts("/usr/bin/pdfcraft") };
        assert_eq!(InstallKind::detect_from(Os::Linux, &d), InstallKind::Deb);
        let r = Facts { rpm_owned: true, ..facts("/usr/bin/pdfcraft") };
        assert_eq!(InstallKind::detect_from(Os::Linux, &r), InstallKind::Rpm);
        let f = Facts { flatpak: true, ..d.clone() };
        assert_eq!(InstallKind::detect_from(Os::Linux, &f), InstallKind::Flatpak);
        assert_eq!(InstallKind::detect_from(Os::Linux, &facts("/opt/pdfcraft/pdfcraft")), InstallKind::Other);
        assert_eq!(InstallKind::detect_from(Os::MacOs, &facts("/Applications/PdfCraft.app/Contents/MacOS/pdfcraft")), InstallKind::MacApp);
    }

    #[test]
    fn asset_names_match_the_packaging_scripts() {
        let v = Version::parse("1.2.0").unwrap();
        let app = app();
        let name = |k: InstallKind, a| k.asset_name(&app, &v, a);
        assert_eq!(name(InstallKind::WindowsMsi, Arch::X86_64).as_deref(), Some("pdfcraft-1.2.0-windows-x64.msi"));
        assert_eq!(name(InstallKind::WindowsMsi, Arch::Aarch64).as_deref(), Some("pdfcraft-1.2.0-windows-arm64.msi"));
        assert_eq!(name(InstallKind::WindowsMsi, Arch::X86).as_deref(), Some("pdfcraft-1.2.0-windows-x86.msi"));
        assert_eq!(name(InstallKind::AppImage("/x".into()), Arch::X86_64).as_deref(), Some("pdfcraft-1.2.0-linux-x86_64.AppImage"));
        assert_eq!(name(InstallKind::Deb, Arch::Aarch64).as_deref(), Some("pdfcraft-1.2.0-linux-aarch64.deb"));
        assert_eq!(name(InstallKind::Rpm, Arch::X86_64).as_deref(), Some("pdfcraft-1.2.0-linux-x86_64.rpm"));
        assert_eq!(name(InstallKind::Rpm, Arch::X86), None);
        for k in [InstallKind::WindowsPortable, InstallKind::Flatpak, InstallKind::MacApp, InstallKind::Other] {
            assert_eq!(name(k.clone(), Arch::X86_64), None);
            assert!(k.why_not(&app).is_some());
        }
    }

    #[test]
    fn a_replaced_program_keeps_its_path() {
        assert_eq!(strip_deleted(Path::new("/usr/bin/pdfcraft (deleted)")), PathBuf::from("/usr/bin/pdfcraft"));
        assert_eq!(strip_deleted(Path::new("/usr/bin/pdfcraft")), PathBuf::from("/usr/bin/pdfcraft"));
    }
}
