//! Deliberate removal of the installed launcher, with optional managed apps and profile data.

use std::fs::{self, Metadata};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
#[cfg(windows)]
use std::process::{Command, Stdio};

use craft_update::Error;

use crate::{APPS, Manager};

/// Additional, irreversible cleanup selected separately from removing the launcher.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Remove the binaries and versions owned by this Craftworks library.
    pub managed_apps: bool,
    /// Delete the listed standard settings, presets, fonts, caches and recovery folders.
    pub app_data: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    path: PathBuf,
    bytes: Vec<u8>,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}

impl Snapshot {
    fn capture(path: &Path, limit: u64) -> Result<Self, Error> {
        regular_file(path)?;
        let file = fs::File::open(path).map_err(failed)?;
        let before = file.metadata().map_err(failed)?;
        if !before.is_file() || before.len() > limit {
            return Err(invalid("A desktop integration file is not a bounded regular file."));
        }
        let mut bytes = Vec::new();
        let probe = limit.checked_add(1).ok_or_else(|| invalid("The desktop integration file limit is invalid."))?;
        file.take(probe).read_to_end(&mut bytes).map_err(failed)?;
        let after = fs::symlink_metadata(path).map_err(failed)?;
        if redirected(&after)
            || !after.is_file()
            || bytes.len() as u64 > limit
            || after.len() != bytes.len() as u64
            || before.modified().ok() != after.modified().ok()
        {
            return Err(invalid("A desktop integration file changed while it was being inspected."));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if before.dev() != after.dev() || before.ino() != after.ino() {
                return Err(invalid("A desktop integration file was replaced while it was being inspected."));
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            bytes,
            modified: after.modified().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (after.dev(), after.ino())
            },
        })
    }

    fn unchanged(&self) -> bool {
        Self::capture(&self.path, self.bytes.len() as u64).is_ok_and(|current| current == *self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PhotoIntegration {
    desktop: Snapshot,
    icons: Vec<Snapshot>,
    target: PathBuf,
}

impl PhotoIntegration {
    fn scan(data: &Path, manager: &Manager) -> Vec<Self> {
        ["io.github.treeforjs.photocraft", "ai.storyteller.photocraft"]
            .into_iter()
            .filter_map(|id| Self::inspect(data, manager, id).ok().flatten())
            .collect()
    }

    fn inspect(data: &Path, manager: &Manager, id: &str) -> Result<Option<Self>, Error> {
        let desktop = Snapshot::capture(&data.join("applications").join(format!("{id}.desktop")), 65_536)?;
        let text = std::str::from_utf8(&desktop.bytes).map_err(|_| invalid("The desktop entry is not UTF-8."))?;
        let target = match photo_desktop_target(text, id) {
            Some(target) => target,
            None => return Ok(None),
        };
        let Some(app) = APPS.iter().copied().find(|app| app.id == "photocraft") else { return Ok(None) };
        if !manager.owns_appimage(app, &target)? {
            return Ok(None);
        }
        let mut icons = Vec::new();
        for size in ["16x16", "24x24", "32x32", "48x48", "64x64", "128x128", "256x256", "512x512", "scalable"] {
            let extension = if size == "scalable" { "svg" } else { "png" };
            let path = data.join("icons/hicolor").join(size).join("apps").join(format!("{id}.{extension}"));
            // A changed, linked, special or oversized icon is preserved independently.
            if let Ok(snapshot) = Snapshot::capture(&path, 2 << 20) {
                icons.push(snapshot);
            }
        }
        Ok(Some(Self { desktop, icons, target }))
    }

    fn remove_if_unchanged(&self, manager: &Manager) -> Result<(), Error> {
        let Some(app) = APPS.iter().copied().find(|app| app.id == "photocraft") else { return Ok(()) };
        if !self.desktop.unchanged() || !manager.owns_appimage(app, &self.target).unwrap_or(false) {
            return Ok(());
        }
        for icon in &self.icons {
            if !self.desktop.unchanged() {
                return Ok(());
            }
            if icon.unchanged() {
                remove_file_if_present(&icon.path)?;
            }
        }
        if self.desktop.unchanged() {
            remove_file_if_present(&self.desktop.path)?;
        }
        Ok(())
    }
}

fn photo_desktop_target(text: &str, id: &str) -> Option<PathBuf> {
    let mut group = false;
    let mut groups = 0;
    let (mut name, mut icon, mut exec, mut kind) = (None, None, None, None);
    for line in text.lines() {
        if line.starts_with('[') {
            group = line == "[Desktop Entry]";
            groups += usize::from(group);
            continue;
        }
        if group {
            let Some((key, value)) = line.split_once('=') else { continue };
            let field = match key {
                "Name" => &mut name,
                "Icon" => &mut icon,
                "Exec" => &mut exec,
                "Type" => &mut kind,
                _ => continue,
            };
            if field.replace(value).is_some() {
                return None;
            }
        }
    }
    if groups != 1 || name != Some("PhotoCraft") || icon != Some(id) || kind != Some("Application") {
        return None;
    }
    let target = exec?.strip_prefix('"')?.strip_suffix("\" %F")?;
    if target.contains(['"', '`', '$', '\\', '%', '\0', '\n', '\r']) {
        return None;
    }
    let target = PathBuf::from(target);
    safe_absolute(&target).then_some(target)
}

/// A read-only snapshot of the exact installed launcher and recognized cleanup locations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    executable: PathBuf,
    directory: PathBuf,
    library: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    profiles: Vec<PathBuf>,
    launcher_files: Vec<PathBuf>,
    desktop: Option<PathBuf>,
    command_link: Option<PathBuf>,
    photo_integrations: Vec<PhotoIntegration>,
    #[cfg(windows)]
    uninstaller: PathBuf,
    #[cfg(windows)]
    shortcut: PathBuf,
}

impl Plan {
    /// Detect only an installed launcher. Source, build and unpacked portable copies are refused.
    pub fn detect(manager: Option<&Manager>) -> Result<Self, Error> {
        let executable = std::env::current_exe().map_err(failed)?;
        let mut plan = Self::locations(executable, &|name| std::env::var_os(name).map(PathBuf::from))?;
        plan.validate_install()?;
        plan.profiles.retain(|path| path.exists());
        if let Some(manager) = manager
            && let Some(data) = plan.desktop.as_deref().and_then(Path::parent).and_then(Path::parent)
        {
            plan.photo_integrations = PhotoIntegration::scan(data, manager);
        }
        Ok(plan)
    }

    fn locations(executable: PathBuf, env: &impl Fn(&str) -> Option<PathBuf>) -> Result<Self, Error> {
        let get = |name: &str| env(name).filter(|path| safe_absolute(path));
        let home = get(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok_or_else(|| invalid("The user home directory is unavailable."))?;
        let (data, config, cache) = if cfg!(windows) {
            let local = get("LOCALAPPDATA").ok_or_else(|| invalid("LOCALAPPDATA is unavailable."))?;
            let roaming = get("APPDATA").ok_or_else(|| invalid("APPDATA is unavailable."))?;
            (local.clone(), roaming, local)
        } else if cfg!(target_os = "linux") {
            (
                get("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
                get("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
                get("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")),
            )
        } else {
            return Err(invalid("Launcher removal is supported on Windows and Linux."));
        };
        let library = data.join(if cfg!(windows) { "Craftworks" } else { "craftworks" });
        let directory = if cfg!(windows) { registered_directory(&executable)? } else { data.join("craftworks-launcher") };
        let expected = directory.join(if cfg!(windows) { "craftworks.exe" } else { "craftworks" });
        if fs::canonicalize(&executable).map_err(failed)? != fs::canonicalize(&expected).map_err(failed)? {
            return Err(invalid(
                "This is a source, build or portable copy. Use the installed Craftworks launcher to uninstall it; this copy was left in place.",
            ));
        }
        let state = if cfg!(windows) { config.join("Craftworks/data/app.ron") } else { library.join("app.ron") };
        let mut profiles = Vec::new();
        for (upper, lower, display) in [
            ("PdfCraft", "pdfcraft", "PdfCraft"),
            ("VectorCraft", "vectorcraft", "VectorCraft"),
            ("Photocraft", "photocraft", "PhotoCraft"),
            ("PrintCraft", "printcraft", "PrintCraft"),
            ("DrawCraft", "drawcraft", "DrawCraft"),
        ] {
            profiles.push(config.join(if cfg!(windows) { upper } else { lower }));
            profiles.push(cache.join(upper));
            profiles.push(cache.join(display));
            if !cfg!(windows) {
                profiles.push(data.join(lower));
            }
        }
        for id in ["pdfcraft", "vectorcraft", "photocraft"] {
            if !cfg!(windows) {
                profiles.push(data.join(format!("io.github.treeforjs.{id}")));
                profiles.push(data.join(format!("ai.storyteller.{id}")));
            }
        }
        profiles.sort();
        profiles.dedup();
        let mut launcher_files: Vec<_> =
            ["LICENSE-MIT", "LICENSE-APACHE", "README.md", "THIRD-PARTY-NOTICES.txt"].into_iter().map(|name| directory.join(name)).collect();
        launcher_files.push(expected.clone());
        #[cfg(not(windows))]
        launcher_files.push(directory.join("uninstall.sh"));
        Ok(Self {
            executable: expected,
            directory: directory.clone(),
            library,
            state,
            cache: cache.join("Craftworks/updates"),
            profiles,
            launcher_files,
            desktop: (!cfg!(windows)).then(|| data.join("applications/io.github.treeforjs.craftworks.desktop")),
            command_link: (!cfg!(windows)).then(|| home.join(".local/bin/craftworks")),
            photo_integrations: Vec::new(),
            #[cfg(windows)]
            uninstaller: directory.join("Uninstall.exe"),
            #[cfg(windows)]
            shortcut: start_menu_shortcut()?,
        })
    }

    /// Exact filesystem targets. Empty directories are removed only when no unknown files remain.
    pub fn paths(&self, options: Options) -> Vec<PathBuf> {
        let mut paths = self.launcher_files.clone();
        #[cfg(windows)]
        paths.extend([self.uninstaller.clone(), self.shortcut.clone()]);
        paths.extend(self.desktop.iter().cloned());
        paths.extend(self.command_link.iter().cloned());
        paths.push(self.state.clone());
        paths.push(self.cache.clone());
        paths.extend([self.library.join(".craftworks-owner"), self.library.join(".craftworks.lock")]);
        if options.managed_apps {
            paths.extend(APPS.iter().map(|app| self.library.join(app.id)).filter(|path| path.exists()));
            for integration in &self.photo_integrations {
                paths.push(integration.desktop.path.clone());
                paths.extend(integration.icons.iter().map(|icon| icon.path.clone()));
            }
        }
        if options.app_data {
            paths.extend(self.profiles.clone());
        }
        paths
    }

    /// Validate every selected tree before deleting optional apps/data. The launcher stays open
    /// on failure. Call [`Self::finish`] only after the GUI and its persistence thread have exited.
    pub fn prepare(&self, manager: Option<&Manager>, options: Options) -> Result<(), Error> {
        self.validate_install()?;
        let manager = if options.managed_apps {
            let manager = manager.ok_or_else(|| {
                invalid("The managed app library could not be opened safely. Keep managed apps selected off to uninstall only the launcher.")
            })?;
            if fs::canonicalize(manager.root()).map_err(failed)? != fs::canonicalize(&self.library).map_err(failed)? {
                return Err(invalid("The displayed library location changed. Close and reopen Settings."));
            }
            Some(manager)
        } else {
            None
        };
        ensure_apps_closed(options.managed_apps || options.app_data)?;
        validate_tree_if_present(&self.cache)?;
        regular_file_if_present(&self.state)?;
        if options.app_data {
            for path in &self.profiles {
                validate_tree_if_present(path)?;
            }
        }
        if let Some(manager) = manager {
            for app in APPS {
                manager.preflight_uninstall(app)?;
                if !options.app_data && contains_portable_profile(&self.library.join(app.id))? {
                    return Err(invalid(
                        "A managed app has portable settings/recovery data. Keep the app, or explicitly select deletion of app settings and data.",
                    ));
                }
            }
        }
        // All selected targets were checked before the first deletion. Individual filesystem
        // failures can still stop cleanup; the launcher is then retained so the user can retry.
        if let Some(manager) = manager {
            for integration in &self.photo_integrations {
                integration.remove_if_unchanged(manager)?;
            }
            for app in APPS {
                manager.uninstall(app)?;
            }
            if cfg!(target_os = "linux")
                && let Some(data) = self.desktop.as_deref().and_then(Path::parent).and_then(Path::parent)
            {
                manager.sync_desktop_entries_in(data)?;
            }
        }
        if options.app_data {
            for path in &self.profiles {
                remove_tree_if_present(path)?;
            }
        }
        remove_tree_if_present(&self.cache)?;
        Ok(())
    }

    /// Finish after eframe has dropped its storage and joined the last persistence write.
    /// Windows starts the fixed registered NSIS uninstaller after this process exits.
    pub fn finish(self) -> Result<(), Error> {
        self.validate_install()?;
        ensure_apps_closed(false)?;
        remove_file_if_present(&self.state)?;
        #[cfg(windows)]
        if let Some(parent) = self.state.parent() {
            let _ = fs::remove_dir(parent);
            if let Some(profile) = parent.parent() {
                let _ = fs::remove_dir(profile);
            }
        }
        if let Some(parent) = self.cache.parent() {
            let _ = fs::remove_dir(parent);
        }
        clean_empty_library(&self.library)?;
        #[cfg(windows)]
        {
            let script = windows_script(&self, std::process::id());
            use std::os::windows::process::CommandExt;
            let powershell = system_executable("WindowsPowerShell/v1.0/powershell.exe")?;
            Command::new(powershell)
                .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
                .creation_flags(0x0800_0000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(failed)?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            for path in &self.launcher_files {
                remove_file_if_present(path)?;
            }
            if let Some(path) = &self.desktop {
                remove_file_if_present(path)?;
            }
            if let Some(path) = &self.command_link
                && fs::read_link(path).is_ok_and(|target| target == self.executable)
            {
                fs::remove_file(path).map_err(failed)?;
            }
            let _ = fs::remove_dir(&self.directory);
            Ok(())
        }
    }

    fn validate_install(&self) -> Result<(), Error> {
        regular_ancestors(&self.directory)?;
        if self.directory.join("Cargo.toml").exists() || self.directory.join(".git").exists() {
            return Err(invalid("Source checkouts are never removed by the uninstaller."));
        }
        for path in &self.launcher_files {
            regular_file_if_present(path)?;
        }
        regular_file(&self.executable)?;
        #[cfg(windows)]
        {
            regular_file(&self.uninstaller)?;
            if fs::canonicalize(registered_directory(&self.executable)?).map_err(failed)? != fs::canonicalize(&self.directory).map_err(failed)? {
                return Err(invalid("The registered launcher installation changed. Reopen Settings."));
            }
        }
        if let Some(path) = &self.desktop
            && path.exists()
        {
            regular_file(path)?;
            let text = read_small(path)?;
            let escaped = self
                .executable
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('`', "\\`")
                .replace('$', "\\$")
                .replace('%', "%%");
            if !text.lines().any(|line| line == format!("Exec=\"{escaped}\"")) || !text.lines().any(|line| line == "Name=Craftworks") {
                return Err(invalid("The desktop entry was changed and does not belong to this launcher. It was left in place."));
            }
        }
        if let Some(path) = &self.command_link {
            regular_ancestors(path.parent().ok_or_else(|| invalid("The command link has no parent."))?)?;
            if let Ok(meta) = fs::symlink_metadata(path)
                && (!meta.file_type().is_symlink() || fs::read_link(path).map_err(failed)? != self.executable)
            {
                return Err(invalid("The Craftworks command points elsewhere. It was left in place."));
            }
        }
        Ok(())
    }
}

fn invalid(message: &str) -> Error {
    Error::Install(message.into())
}

fn failed(error: io::Error) -> Error {
    invalid(&format!("Couldn't complete removal ({error}). Remaining files were kept; retry after closing apps."))
}

fn safe_absolute(path: &Path) -> bool {
    path.is_absolute() && !path.components().any(|part| matches!(part, Component::ParentDir))
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

fn regular_ancestors(path: &Path) -> Result<(), Error> {
    if !safe_absolute(path) {
        return Err(invalid("A cleanup path is not an absolute normal path."));
    }
    for parent in path.ancestors() {
        match fs::symlink_metadata(parent) {
            Ok(meta) if meta.is_dir() && !redirected(&meta) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(invalid("Cleanup paths must not pass through links, junctions or special files.")),
        }
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<(), Error> {
    regular_ancestors(path.parent().ok_or_else(|| invalid("A cleanup file has no parent."))?)?;
    let meta = fs::symlink_metadata(path).map_err(failed)?;
    if !meta.is_file() || redirected(&meta) {
        return Err(invalid("A cleanup file is a link or special file. It was left in place."));
    }
    Ok(())
}

fn regular_file_if_present(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => regular_file(path),
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => Ok(()),
        Err(error) => Err(failed(error)),
    }
}

fn read_small(path: &Path) -> Result<String, Error> {
    regular_file(path)?;
    let mut text = String::new();
    fs::File::open(path).map_err(failed)?.take(65_537).read_to_string(&mut text).map_err(failed)?;
    if text.len() > 65_536 {
        return Err(invalid("A launcher metadata file is too large."));
    }
    Ok(text)
}

fn tree(path: &Path) -> Result<Vec<PathBuf>, Error> {
    regular_ancestors(path.parent().ok_or_else(|| invalid("A cleanup directory has no parent."))?)?;
    let mut found = Vec::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(next) = pending.pop() {
        if found.len().saturating_add(pending.len()) >= 100_000 {
            return Err(invalid("A cleanup directory has too many files to remove safely."));
        }
        let meta = fs::symlink_metadata(&next).map_err(failed)?;
        if redirected(&meta) || !meta.is_dir() && !meta.is_file() {
            return Err(invalid("A selected data folder contains a link, junction or special file. Nothing in it was removed."));
        }
        if meta.is_dir() {
            for entry in fs::read_dir(&next).map_err(failed)? {
                if found.len().saturating_add(pending.len()) >= 100_000 {
                    return Err(invalid("A cleanup directory has too many files to remove safely."));
                }
                pending.push(entry.map_err(failed)?.path());
            }
        }
        found.push(next);
    }
    Ok(found)
}

fn validate_tree_if_present(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => tree(path).map(|_| ()),
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => Ok(()),
        Err(error) => Err(failed(error)),
    }
}

fn remove_tree_if_present(path: &Path) -> Result<(), Error> {
    validate_tree_if_present(path)?;
    if path.exists() {
        fs::remove_dir_all(path).map_err(failed)?;
    }
    Ok(())
}

fn remove_file_if_present(path: &Path) -> Result<(), Error> {
    regular_file_if_present(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => Ok(()),
        Err(error) => Err(failed(error)),
    }
}

fn contains_portable_profile(path: &Path) -> Result<bool, Error> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(tree(path)?.iter().any(|entry| matches!(entry.file_name().and_then(|name| name.to_str()), Some("PdfCraftData" | "PhotoCraftData"))))
}

fn clean_empty_library(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !redirected(&meta) => {}
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(failed(error)),
    }
    regular_ancestors(path)?;
    for entry in fs::read_dir(path).map_err(failed)? {
        let name = entry.map_err(failed)?.file_name();
        if name != ".craftworks-owner" && name != ".craftworks.lock" {
            return Ok(());
        }
    }
    if read_small(&path.join(".craftworks-owner")).is_ok_and(|text| text == "Craftworks managed applications v1\n") {
        remove_file_if_present(&path.join(".craftworks.lock"))?;
        remove_file_if_present(&path.join(".craftworks-owner"))?;
        fs::remove_dir(path).map_err(failed)?;
    }
    Ok(())
}

fn ensure_apps_closed(include_apps: bool) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        for entry in fs::read_dir("/proc").map_err(failed)? {
            let entry = entry.map_err(failed)?;
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else { continue };
            if pid == std::process::id() {
                continue;
            }
            match fs::read_link(entry.path().join("exe")) {
                Ok(exe) => {
                    let name = exe.file_name().and_then(|name| name.to_str()).unwrap_or_default().trim_end_matches(" (deleted)");
                    if process_blocks_cleanup(name, include_apps) {
                        return Err(invalid("Close the other Craftworks window and all selected Craft apps before uninstalling."));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    use std::os::unix::fs::MetadataExt;
                    let own = fs::metadata("/proc/self").map_err(failed)?.uid();
                    match fs::metadata(entry.path()) {
                        Ok(meta) if meta.uid() != own => {}
                        Ok(_) => match read_small(&entry.path().join("comm")) {
                            Ok(name) if !process_blocks_cleanup(name.trim(), include_apps) => {}
                            Err(Error::Install(_)) if !entry.path().exists() => {}
                            _ => return Err(invalid("A selected Craft app is protected from process inspection. Close it and retry.")),
                        },
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        _ => return Err(invalid("A running process could not be checked safely. Close the Craft apps and retry.")),
                    }
                }
                Err(error) => return Err(failed(error)),
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output =
            Command::new(system_executable("tasklist.exe")?).args(["/FO", "CSV", "/NH"]).creation_flags(0x0800_0000).output().map_err(failed)?;
        if !output.status.success() {
            return Err(invalid("Windows could not establish which apps are running. Close the apps and retry."));
        }
        if windows_processes_block(&String::from_utf8_lossy(&output.stdout), include_apps)? {
            return Err(invalid("Close the other Craftworks window and all selected Craft apps before uninstalling."));
        }
    }
    Ok(())
}

fn process_blocks_cleanup(name: &str, include_apps: bool) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    let name = lower.trim_end_matches(" (deleted)").trim_end_matches(".exe");
    name == "craftworks"
        || include_apps
            && ["pdfcraft", "vectorcraft", "photocraft", "printcraft", "drawcraft"].iter().any(|app| name == *app || name == format!("{app}-cli"))
}

#[cfg(any(windows, test))]
fn windows_processes_block(text: &str, include_apps: bool) -> Result<bool, Error> {
    let names: Vec<_> = text.lines().filter_map(|line| line.strip_prefix('"').and_then(|line| line.split_once('"')).map(|(name, _)| name)).collect();
    let hubs = names.iter().filter(|name| name.eq_ignore_ascii_case("craftworks.exe")).count();
    if hubs == 0 {
        return Err(invalid("Windows did not return a usable process list. No files were removed."));
    }
    Ok(hubs > 1 || names.iter().any(|name| !name.eq_ignore_ascii_case("craftworks.exe") && process_blocks_cleanup(name, include_apps)))
}

#[cfg(windows)]
fn system_executable(name: &str) -> Result<PathBuf, Error> {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|path| safe_absolute(path))
        .ok_or_else(|| invalid("Windows did not provide its system directory."))?;
    let path = root.join("System32").join(name);
    regular_file(&path)?;
    Ok(path)
}

#[cfg(windows)]
fn registered_directory(executable: &Path) -> Result<PathBuf, Error> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
    };
    let user = RegKey::predef(HKEY_CURRENT_USER);
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let read = || -> io::Result<(PathBuf, PathBuf)> {
            let path: String = user.open_subkey_with_flags("Software\\Craftworks", KEY_READ | view)?.get_value("InstallDir")?;
            let installed: String = user
                .open_subkey_with_flags("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Craftworks", KEY_READ | view)?
                .get_value("InstallLocation")?;
            Ok((PathBuf::from(path), PathBuf::from(installed)))
        };
        let (directory, installed) = match read() {
            Ok(paths) => paths,
            Err(error) if error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(87) => continue,
            Err(error) => return Err(failed(error)),
        };
        if safe_absolute(&directory)
            && fs::canonicalize(&directory).map_err(failed)? == fs::canonicalize(installed).map_err(failed)?
            && fs::canonicalize(directory.join("craftworks.exe")).map_err(failed)? == fs::canonicalize(executable).map_err(failed)?
        {
            return Ok(directory);
        }
    }
    Err(invalid("Craftworks has no matching per-user installer record. Unpacked and development copies are not removed."))
}

#[cfg(not(windows))]
fn registered_directory(_: &Path) -> Result<PathBuf, Error> {
    Err(invalid("A Windows installation record is unavailable on this system."))
}

#[cfg(windows)]
fn start_menu_shortcut() -> Result<PathBuf, Error> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let path: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders")
        .map_err(failed)?
        .get_value("Programs")
        .map_err(failed)?;
    let path = PathBuf::from(path);
    if !safe_absolute(&path) {
        return Err(invalid("The Start menu location is unavailable."));
    }
    Ok(path.join("Craftworks.lnk"))
}

#[cfg(windows)]
fn windows_script(plan: &Plan, pid: u32) -> String {
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "''");
    format!(
        "$ErrorActionPreference='Stop'; try{{$p=Get-Process -Id {pid} -ErrorAction SilentlyContinue; if($p -and -not $p.WaitForExit(60000)){{throw 'Craftworks did not exit.'}}; $u=Start-Process -FilePath '{}' -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru; if($u.ExitCode -ne 0 -or (Test-Path -LiteralPath '{}')){{throw 'The registered uninstaller failed.'}}}}catch{{Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('Craftworks could not finish uninstalling. Remaining files were kept. Run the registered uninstaller again from Windows Settings.','Craftworks uninstall') | Out-Null; exit 1}}",
        quote(&plan.uninstaller),
        quote(&plan.executable)
    )
}

#[cfg(test)]
mod tests;
