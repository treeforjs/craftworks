use std::collections::BTreeMap;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use craft_update::{AppInfo, Arch, Asset, Error, Os, Release, Source, Version, newest, verify};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::{APPS, App, Availability, Installed, package};

const ROOT_OWNER: &str = "Craftworks managed applications v1\n";
const OWNER_FILE: &str = ".craftworks-owner";
const LOCK_FILE: &str = ".craftworks.lock";
const MAX_ASSET_BYTES: u64 = 2 << 30;
const MAX_RECEIPTS: usize = 10_000;

/// A cloneable manager for the current user's application library.
///
/// Create and use this on a background thread: network requests and package extraction block.
/// Only keys embedded in the calling app should be passed to [`Self::with_public_key`].
#[derive(Clone)]
pub struct Manager {
    root: PathBuf,
    source: Arc<dyn Source>,
    keys: BTreeMap<String, [u8; 32]>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    app: String,
    version: String,
    slot: u64,
}

struct MutationLock(File);

impl Drop for MutationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

impl Manager {
    /// Opens a private library at an absolute path, or initializes an empty directory.
    /// Refuses nonempty directories belonging to another application and symbolic links.
    pub fn new(root: PathBuf, source: Arc<dyn Source>) -> Result<Self, Error> {
        if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Error::Install("The Craftworks library needs an absolute directory path.".into()));
        }
        if let Some(meta) = metadata(&root)? {
            directory(&root, &meta)?;
        } else {
            fs::create_dir_all(&root).map_err(io_error)?;
            directory(&root, &fs::symlink_metadata(&root).map_err(io_error)?)?;
        }
        let root = fs::canonicalize(root).map_err(io_error)?;
        let manager = Self { root, source, keys: BTreeMap::new() };
        let _lock = manager.lock(false)?;
        let marker = manager.root.join(OWNER_FILE);
        if metadata(&marker)?.is_some() {
            check_marker(&manager.root, ROOT_OWNER)?;
        } else {
            for entry in fs::read_dir(&manager.root).map_err(io_error)? {
                if entry.map_err(io_error)?.file_name() != LOCK_FILE {
                    return Err(Error::Install(
                        "This directory contains files that Craftworks does not own. Choose an empty library directory.".into(),
                    ));
                }
            }
            write_new(&marker, ROOT_OWNER.as_bytes())?;
        }
        Ok(manager)
    }

    /// The default per-user directory; no administrator privileges are needed.
    pub fn default_root() -> Result<PathBuf, Error> {
        if cfg!(target_os = "windows") {
            return absolute_env("LOCALAPPDATA")
                .map(|p| p.join("Craftworks"))
                .ok_or_else(|| Error::Install("Windows did not provide a local application data directory.".into()));
        }
        if cfg!(target_os = "linux") {
            return absolute_env("XDG_DATA_HOME")
                .or_else(|| absolute_env("HOME").map(|p| p.join(".local/share")))
                .map(|p| p.join("craftworks"))
                .ok_or_else(|| Error::Install("Set HOME or XDG_DATA_HOME to a user data directory.".into()));
        }
        Err(Error::Unsupported("Craftworks installs apps on Windows and Linux.".into()))
    }

    /// Sets an embedded, trusted Ed25519 public key for one catalog app.
    /// Accepts base64 of 32 raw bytes or the OpenSSL public-key DER representation.
    pub fn with_public_key(mut self, app_id: &str, key: &str) -> Result<Self, Error> {
        if !APPS.iter().any(|app| app.id == app_id) {
            return Err(Error::Invalid("That app is not in the Craftworks catalog.".into()));
        }
        self.keys.insert(app_id.to_owned(), verify::decode_public_key(key)?);
        Ok(self)
    }

    /// The absolute, canonical library directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the active owned installation. Missing or redirected executables are errors.
    pub fn installed(&self, app: App) -> Result<Option<Installed>, Error> {
        validate_app(app)?;
        self.validate_root()?;
        let Some(app_dir) = self.existing_app(app)? else { return Ok(None) };
        let Some((_, receipt)) = self.latest_receipt(app, &app_dir)? else { return Ok(None) };
        let executable = executable_path(app, &app_dir, receipt.slot)?;
        validate_regular_tree(&app_dir, &executable)?;
        let meta = metadata(&executable)?
            .ok_or_else(|| Error::Install(format!("{}'s installed executable is missing. Install it again to repair it.", app.name)))?;
        if !meta.is_file() || is_link(&meta) {
            return Err(Error::Install(format!("{}'s installed executable is not a regular file.", app.name)));
        }
        Ok(Some(Installed { version: receipt.version, executable }))
    }

    /// Checks GitHub's published, stable releases. An absent key never permits installation.
    pub fn check(&self, app: App) -> Result<Availability, Error> {
        validate_app(app)?;
        let info = app_info(app);
        let url = format!("https://api.github.com/repos/{}/releases?per_page=100", app.repo);
        let bytes = self.source.get(&url, 4 << 20)?;
        if bytes.len() > 4 << 20 {
            return Err(Error::Invalid("The release list is too large.".into()));
        }
        let json = std::str::from_utf8(&bytes).map_err(|_| Error::Invalid("The release list isn't text.".into()))?;
        let Some(release) = newest(json, &info)? else {
            return Ok(Availability { release: None, asset: None, blocked: Some(format!("{} has no published releases yet.", app.name)) });
        };
        let name = package::asset_name(app, &release.version, Os::current(), Arch::current());
        let asset = name.as_deref().and_then(|n| release.asset(n)).cloned();
        let blocked = if name.is_none() {
            Some("This operating system or processor is not supported yet.".into())
        } else if asset.is_none() {
            Some("This release has no package for your operating system and processor.".into())
        } else if !self.keys.contains_key(app.id) {
            Some(format!("{}'s trusted release key has not been configured in this Craftworks build.", app.name))
        } else if release.asset(verify::SUMS).is_none() || release.asset(verify::SIGNATURE).is_none() {
            Some("This release has no signed checksums. A signed release is required to install.".into())
        } else {
            None
        };
        Ok(Availability { release: Some(release), asset, blocked })
    }

    /// Verifies, downloads and activates a release, preserving the previous installation on failure.
    pub fn install(&self, app: App, release: &Release, progress: &mut dyn FnMut(u64, u64)) -> Result<Installed, Error> {
        self.install_with_cancel(app, release, progress, &AtomicBool::new(false))
    }

    /// Installs like [`Self::install`], checking cancellation between requests, chunks and ZIP entries.
    /// Cancellation before activation leaves the current installation untouched.
    pub fn install_with_cancel(
        &self,
        app: App,
        release: &Release,
        progress: &mut dyn FnMut(u64, u64),
        cancelled: &AtomicBool,
    ) -> Result<Installed, Error> {
        validate_app(app)?;
        self.validate_root()?;
        let _lock = self.lock(true)?;
        check_cancel(cancelled)?;
        let key = self.keys.get(app.id).ok_or_else(|| Error::Verify(format!("{} has no trusted release key in this Craftworks build.", app.name)))?;
        validate_release(release)?;
        let name = package::asset_name(app, &release.version, Os::current(), Arch::current())
            .ok_or_else(|| Error::Unsupported("This operating system or processor is not supported yet.".into()))?;
        let asset = trusted_asset(app, release, &name)?;
        let list = trusted_asset(app, release, verify::SUMS)?;
        let signature = trusted_asset(app, release, verify::SIGNATURE)?;
        let list = self.get_small(list, cancelled)?;
        let signature = self.get_small(signature, cancelled)?;
        let signature = std::str::from_utf8(&signature).map_err(|_| Error::Verify("The checksum signature isn't text.".into()))?;
        let sums = verify::Sums::verified(&list, signature, key)?;
        if sums.get(&asset.name).is_none() {
            return Err(Error::Verify(format!("{} is missing from the signed checksum list.", asset.name)));
        }
        check_cancel(cancelled)?;
        let stage = tempfile::Builder::new().prefix(".craftworks-stage-").tempdir_in(&self.root).map_err(io_error)?;
        let download = stage.path().join("download");
        let mut file = File::create(&download).map_err(io_error)?;
        let mut hasher = verify::Hasher::default();
        let mut received = 0u64;
        progress(0, asset.size);
        self.source.download(&asset.url, asset.size, &mut |bytes| {
            check_cancel(cancelled)?;
            received = received.checked_add(bytes.len() as u64).ok_or_else(|| Error::Invalid("The package is too large.".into()))?;
            if received > asset.size || received > MAX_ASSET_BYTES {
                return Err(Error::Invalid("The package is larger than its published size.".into()));
            }
            file.write_all(bytes).map_err(io_error)?;
            hasher.update(bytes);
            progress(received, asset.size);
            Ok(())
        })?;
        if received != asset.size {
            return Err(Error::Verify("The package download ended before its published size.".into()));
        }
        verify::check(&sums, &asset.name, &hasher.finish())?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        check_cancel(cancelled)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(io_error)?;
        if Os::current() == Os::Windows {
            package::extract_zip(&download, &payload, app, &asset.name, cancelled)?;
        } else {
            let executable = payload.join(format!("{}.AppImage", app.id));
            package::prepare_appimage(&download, &executable)?;
        }
        check_cancel(cancelled)?;
        let app_dir = self.prepare_app(app)?;
        let slot = self.next_slot(app, &app_dir)?;
        let destination = app_dir.join("versions").join(format!("{slot:020}"));
        if metadata(&destination)?.is_some() {
            return Err(Error::Install("The installation directory already exists. Try again.".into()));
        }
        fs::rename(&payload, &destination).map_err(io_error)?;
        let receipt = Receipt { schema: 1, app: app.id.into(), version: release.version.to_string(), slot };
        let activated = (|| {
            check_cancel(cancelled)?;
            let bytes = serde_json::to_vec(&receipt).map_err(|e| Error::Install(e.to_string()))?;
            let receipts = app_dir.join("receipts");
            let mut pending = tempfile::Builder::new().prefix(".pending-").tempfile_in(&receipts).map_err(io_error)?;
            pending.write_all(&bytes).map_err(io_error)?;
            pending.as_file().sync_all().map_err(io_error)?;
            // This filename has never existed. Rename never replaces the active receipt,
            // which also works atomically on Windows without touching a running executable.
            pending.persist_noclobber(receipts.join(format!("{slot:020}.json"))).map_err(|e| io_error(e.error))?;
            Ok(())
        })();
        if let Err(error) = activated {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        Ok(Installed { version: receipt.version, executable: executable_path(app, &app_dir, slot)? })
    }

    /// Removes only this manager's owned binaries. User documents and app settings are separate.
    /// Running programs can prevent deletion on Windows; close the app and retry in that case.
    pub fn uninstall(&self, app: App) -> Result<(), Error> {
        validate_app(app)?;
        self.validate_root()?;
        let _lock = self.lock(true)?;
        let Some(app_dir) = self.owned_app(app)? else { return Ok(()) };
        // Check every descendant before deletion. Never traverse a symlink or junction,
        // including one added after the package was installed.
        validate_owned_tree(&app_dir)?;
        #[cfg(target_os = "windows")]
        preflight_removal(&app_dir)?;
        fs::remove_dir_all(&app_dir).map_err(|e| Error::Install(format!("Couldn't remove {}. Close the app and try again ({e}).", app.name)))
    }

    /// Launches the active owned executable directly, without a shell or administrator prompt.
    pub fn launch(&self, app: App) -> Result<Child, Error> {
        self.validate_root()?;
        let _lock = self.lock(true)?;
        let installed = self.installed(app)?.ok_or_else(|| Error::Install(format!("{} is not installed through Craftworks.", app.name)))?;
        let directory = installed.executable.parent().ok_or_else(|| Error::Install("The installed executable has no directory.".into()))?;
        Command::new(&installed.executable)
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Install(format!("Couldn't open {} ({e}).", app.name)))
    }

    fn get_small(&self, asset: &Asset, cancelled: &AtomicBool) -> Result<Vec<u8>, Error> {
        check_cancel(cancelled)?;
        if asset.size > verify::MAX_LIST_BYTES {
            return Err(Error::Verify("The signed checksum data is too large.".into()));
        }
        let bytes = self.source.get(&asset.url, verify::MAX_LIST_BYTES)?;
        if bytes.len() as u64 > verify::MAX_LIST_BYTES {
            return Err(Error::Verify("The signed checksum data is too large.".into()));
        }
        check_cancel(cancelled)?;
        Ok(bytes)
    }

    fn validate_root(&self) -> Result<(), Error> {
        directory(&self.root, &fs::symlink_metadata(&self.root).map_err(io_error)?)?;
        check_marker(&self.root, ROOT_OWNER)
    }

    fn lock(&self, require_owner: bool) -> Result<MutationLock, Error> {
        if require_owner {
            self.validate_root()?;
        }
        let path = self.root.join(LOCK_FILE);
        if let Some(meta) = metadata(&path)?
            && (!meta.is_file() || is_link(&meta))
        {
            return Err(Error::Install("The Craftworks installation lock is not a regular file.".into()));
        }
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(io_error)?;
        file.try_lock_exclusive()
            .map_err(|_| Error::Install("Another Craftworks installation or removal is in progress. Try again when it finishes.".into()))?;
        Ok(MutationLock(file))
    }

    fn existing_app(&self, app: App) -> Result<Option<PathBuf>, Error> {
        let Some(path) = self.owned_app(app)? else { return Ok(None) };
        for name in ["versions", "receipts"] {
            let child = path.join(name);
            directory(&child, &fs::symlink_metadata(&child).map_err(io_error)?)?;
        }
        Ok(Some(path))
    }

    fn owned_app(&self, app: App) -> Result<Option<PathBuf>, Error> {
        let path = self.root.join(app.id);
        let Some(meta) = metadata(&path)? else { return Ok(None) };
        directory(&path, &meta)?;
        check_marker(&path, &app_marker(app))?;
        Ok(Some(path))
    }

    fn prepare_app(&self, app: App) -> Result<PathBuf, Error> {
        if let Some(path) = self.owned_app(app)? {
            // Ownership permits repairing absent structure, but never following a replaced
            // directory. Existing receipts and version directories are left intact.
            for name in ["versions", "receipts"] {
                let child = path.join(name);
                if let Some(meta) = metadata(&child)? {
                    directory(&child, &meta)?;
                } else {
                    fs::create_dir(child).map_err(io_error)?;
                }
            }
            return Ok(path);
        }
        let path = self.root.join(app.id);
        let skeleton = tempfile::Builder::new().prefix(".craftworks-app-").tempdir_in(&self.root).map_err(io_error)?;
        write_new(&skeleton.path().join(OWNER_FILE), app_marker(app).as_bytes())?;
        fs::create_dir(skeleton.path().join("versions")).map_err(io_error)?;
        fs::create_dir(skeleton.path().join("receipts")).map_err(io_error)?;
        // Publish complete structure in one rename. A crash before this point leaves only
        // an ignored staging directory, never a half-initialized catalog app.
        fs::rename(skeleton.path(), &path).map_err(io_error)?;
        Ok(path)
    }

    fn latest_receipt(&self, app: App, app_dir: &Path) -> Result<Option<(u64, Receipt)>, Error> {
        let mut latest = None;
        let mut count = 0;
        for entry in fs::read_dir(app_dir.join("receipts")).map_err(io_error)? {
            count += 1;
            if count > MAX_RECEIPTS {
                return Err(Error::Install("The app has too many installation receipts.".into()));
            }
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| Error::Install("An installation receipt has an invalid name.".into()))?;
            if name.starts_with(".pending-") {
                continue;
            }
            let slot = receipt_slot(name)?;
            if latest.as_ref().is_none_or(|(previous, _)| slot > *previous) {
                latest = Some((slot, entry.path()));
            }
        }
        // Only the highest slot is active. Old receipts are retained for history and must
        // not affect the current app merely because read_dir enumerated them first.
        let Some((slot, path)) = latest else { return Ok(None) };
        let bytes = read_small(&path, 16 * 1024)?;
        let receipt: Receipt = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Install("An app installation receipt is damaged. Remove the app, then install it again.".into()))?;
        if receipt.schema != 1 || receipt.app != app.id || receipt.slot != slot || Version::parse(&receipt.version).is_none() {
            return Err(Error::Install("An app installation receipt is invalid. Remove the app, then install it again.".into()));
        }
        Ok(Some((slot, receipt)))
    }

    fn next_slot(&self, app: App, app_dir: &Path) -> Result<u64, Error> {
        let mut maximum = self.latest_receipt(app, app_dir)?.map_or(0, |(slot, _)| slot);
        let mut count = 0;
        for entry in fs::read_dir(app_dir.join("versions")).map_err(io_error)? {
            count += 1;
            if count > MAX_RECEIPTS {
                return Err(Error::Install("The app has too many retained installations.".into()));
            }
            let entry = entry.map_err(io_error)?;
            directory(&entry.path(), &fs::symlink_metadata(entry.path()).map_err(io_error)?)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| Error::Install("An installation directory has an invalid name.".into()))?;
            maximum = maximum.max(parse_slot(name)?);
        }
        maximum.checked_add(1).ok_or_else(|| Error::Install("No more installation slots are available.".into()))
    }
}

fn validate_app(app: App) -> Result<(), Error> {
    if APPS.contains(&app) { Ok(()) } else { Err(Error::Invalid("That app is not in the Craftworks catalog.".into())) }
}

fn validate_release(release: &Release) -> Result<(), Error> {
    if release.tag.is_empty()
        || release.tag.len() > 160
        || !release.tag.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
        || Version::parse(&release.tag).as_ref() != Some(&release.version)
        || release.version.is_prerelease()
    {
        return Err(Error::Invalid("This is not a stable, versioned app release.".into()));
    }
    Ok(())
}

fn trusted_asset<'a>(app: App, release: &'a Release, name: &str) -> Result<&'a Asset, Error> {
    let asset = release.asset(name).ok_or_else(|| Error::Verify(format!("The release is missing {name}. A signed package is required.")))?;
    let expected = format!("https://github.com/{}/releases/download/{}/{name}", app.repo, release.tag);
    if asset.url != expected || asset.size == 0 || asset.size > MAX_ASSET_BYTES {
        return Err(Error::Invalid(format!("The release asset {name} has an invalid URL or size.")));
    }
    Ok(asset)
}

fn app_info(app: App) -> AppInfo {
    AppInfo { id: app.id.into(), display_name: app.name.into(), repo: app.repo.into(), current: "0.0.0".into(), public_key: None, prereleases: false }
}

fn app_marker(app: App) -> String {
    format!("Craftworks managed app v1: {}\n", app.id)
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

fn executable_path(app: App, app_dir: &Path, slot: u64) -> Result<PathBuf, Error> {
    let extension = match Os::current() {
        Os::Windows => "exe",
        Os::Linux => "AppImage",
        _ => return Err(Error::Unsupported("Craftworks installs apps on Windows and Linux.".into())),
    };
    Ok(app_dir.join("versions").join(format!("{slot:020}")).join(format!("{}.{extension}", app.id)))
}

fn parse_slot(name: &str) -> Result<u64, Error> {
    if name.len() != 20 || !name.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Install("An installation directory has an invalid name.".into()));
    }
    name.parse::<u64>().ok().filter(|slot| *slot > 0).ok_or_else(|| Error::Install("An installation directory has an invalid number.".into()))
}

fn receipt_slot(name: &str) -> Result<u64, Error> {
    let slot = name.strip_suffix(".json").ok_or_else(|| Error::Install("An installation receipt has an invalid name.".into()))?;
    parse_slot(slot)
}

pub(crate) fn metadata(path: &Path) -> Result<Option<Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(e)),
    }
}

fn directory(path: &Path, meta: &Metadata) -> Result<(), Error> {
    if !meta.is_dir() || is_link(meta) {
        return Err(Error::Install(format!("{} must be a regular directory, not a link.", path.display())));
    }
    Ok(())
}

fn is_link(meta: &Metadata) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        // Reject every reparse point, including junctions, not only symbolic links.
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(target_os = "windows"))]
    {
        meta.file_type().is_symlink()
    }
}

fn check_marker(directory: &Path, expected: &str) -> Result<(), Error> {
    let marker = directory.join(OWNER_FILE);
    if read_small(&marker, 1024)?.as_slice() != expected.as_bytes() {
        return Err(Error::Install(format!("Craftworks does not own {}. Its files will not be changed.", directory.display())));
    }
    Ok(())
}

fn read_small(path: &Path, limit: u64) -> Result<Vec<u8>, Error> {
    let meta = fs::symlink_metadata(path).map_err(io_error)?;
    if !meta.is_file() || is_link(&meta) || meta.len() > limit {
        return Err(Error::Install("An installation metadata file is invalid or redirected.".into()));
    }
    let mut bytes = Vec::new();
    File::open(path).map_err(io_error)?.take(limit + 1).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 > limit {
        return Err(Error::Install("An installation metadata file is too large.".into()));
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn validate_regular_tree(base: &Path, path: &Path) -> Result<(), Error> {
    let relative = path.strip_prefix(base).map_err(|_| Error::Install("An installation points outside the library.".into()))?;
    let mut current = base.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else { return Err(Error::Install("An installed path is invalid.".into())) };
        current.push(name);
        let Some(meta) = metadata(&current)? else {
            return Err(Error::Install("The installed app is missing files. Install it again to repair it.".into()));
        };
        if is_link(&meta) {
            return Err(Error::Install("The installed app points to a symbolic link or junction.".into()));
        }
        if components.peek().is_some() && !meta.is_dir() {
            return Err(Error::Install("The installed app has an invalid directory.".into()));
        }
    }
    Ok(())
}

fn validate_owned_tree(root: &Path) -> Result<(), Error> {
    let mut remaining = vec![root.to_path_buf()];
    let mut count = 0usize;
    while let Some(path) = remaining.pop() {
        count += 1;
        if count > 1_000_000 {
            return Err(Error::Install("The installation has too many files to remove safely.".into()));
        }
        let meta = fs::symlink_metadata(&path).map_err(io_error)?;
        if is_link(&meta) || !meta.is_file() && !meta.is_dir() {
            return Err(Error::Install("The app contains a symbolic link, junction or special file. Its files were not removed.".into()));
        }
        if meta.is_dir() {
            for entry in fs::read_dir(&path).map_err(io_error)? {
                remaining.push(entry.map_err(io_error)?.path());
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn preflight_removal(root: &Path) -> Result<(), Error> {
    let mut remaining = vec![root.to_path_buf()];
    while let Some(path) = remaining.pop() {
        let meta = fs::symlink_metadata(&path).map_err(io_error)?;
        if is_link(&meta) {
            return Err(Error::Install("The app contains a symbolic link or junction. Its files were not removed.".into()));
        }
        if meta.is_dir() {
            for entry in fs::read_dir(path).map_err(io_error)? {
                remaining.push(entry.map_err(io_error)?.path());
            }
        } else {
            // Windows refuses a write handle to a running image or a file opened without
            // sharing. Probe every file before removing any receipt or ownership marker.
            // No bytes are written and the file is never truncated.
            OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| Error::Install(format!("The app has files in use. Close it and try removing it again ({e}).")))?;
        }
    }
    Ok(())
}

pub(crate) fn check_cancel(cancelled: &AtomicBool) -> Result<(), Error> {
    if cancelled.load(Ordering::Relaxed) { Err(Error::Cancelled) } else { Ok(()) }
}

pub(crate) fn io_error(error: std::io::Error) -> Error {
    Error::Install(format!("Couldn't access the Craftworks library ({error})."))
}

#[cfg(test)]
mod tests;
