#[cfg(windows)]
use std::fs::OpenOptions;
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use craft_update::Error;

use super::{Identity, LegacyInstall, LegacyMethod, Removal};
use crate::{APPS, App};

/// An opaque snapshot of the folder selected for retirement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortableTarget {
    directory: PathBuf,
    executable: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

pub(super) fn scan(app: App) -> Vec<LegacyInstall> {
    let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|p| p.is_absolute()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for base in ["Downloads", "Desktop", "Applications", "Apps", "apps", ".local/opt"] {
        let base = home.join(base);
        let Ok(meta) = fs::symlink_metadata(&base) else { continue };
        if !meta.is_dir() || redirected(&meta) {
            continue;
        }
        let Ok(entries) = fs::read_dir(base) else { continue };
        for entry in entries.take(512).flatten() {
            if dedicated_name(app, &entry.file_name().to_string_lossy())
                && let Ok(install) = inspect(app, &entry.path())
            {
                found.push(install);
            }
        }
    }
    found
}

pub(super) fn inspect(app: App, chosen: &Path) -> Result<LegacyInstall, Error> {
    if !chosen.is_absolute() || chosen.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid("Enter the absolute path to the old app folder or executable."));
    }
    regular_ancestors(chosen)?;
    let meta = fs::symlink_metadata(chosen).map_err(io_error)?;
    let directory = if meta.is_file() {
        if !executable_name(app, &chosen.file_name().unwrap_or_default().to_string_lossy()) {
            return Err(invalid("Choose this app's executable or its dedicated app folder."));
        }
        chosen.parent().ok_or_else(|| invalid("The old executable has no app folder."))?
    } else if meta.is_dir() {
        chosen
    } else {
        return Err(invalid("The old app must be in a regular folder."));
    };
    let directory = fs::canonicalize(directory).map_err(io_error)?;
    if !dedicated_name(app, &directory.file_name().unwrap_or_default().to_string_lossy()) {
        return Err(invalid(
            "Choose a dedicated folder named for this app, such as PdfCraft or pdfcraft-0.5.0-linux-x86_64. Shared folders cannot be archived.",
        ));
    }
    if directory.ancestors().any(|p| p.join(".craftworks-owner").exists() || p.file_name().is_some_and(|n| n == ".craftworks-backups")) {
        return Err(invalid("This copy is managed or already archived. Use its existing controls instead."));
    }
    if directory.join(".git").exists() || directory.join("Cargo.toml").exists() {
        return Err(invalid("This is a source checkout. Choose an unpacked release folder instead."));
    }
    let mut executable = None;
    for (count, entry) in fs::read_dir(&directory).map_err(io_error)?.enumerate() {
        if count >= 512 {
            return Err(invalid("This folder contains too many entries to identify as a dedicated app folder."));
        }
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if APPS.iter().any(|other| *other != app && executable_name(*other, &name)) {
            return Err(invalid("This folder contains several apps. Move this app into its own folder before archiving it."));
        }
        if executable_name(app, &name) {
            let meta = fs::symlink_metadata(entry.path()).map_err(io_error)?;
            if !meta.is_file() || redirected(&meta) {
                return Err(invalid("The old executable must be a regular file, not a link."));
            }
            check_binary(&entry.path())?;
            if executable.is_none() {
                executable = Some((entry.path(), meta));
            }
        }
    }
    let (executable, meta) =
        executable.ok_or_else(|| invalid("This folder has no matching app executable. Choose the folder containing the old app."))?;
    Ok(LegacyInstall {
        app,
        version: None,
        description: format!("{} · previous ZIP or app folder", app.name),
        location: Some(directory.clone()),
        method: LegacyMethod::PortableFolder,
        identity: Identity::Portable(PortableTarget { directory, executable, size: meta.len(), modified: meta.modified().ok() }),
    })
}

pub(super) fn archive(install: &LegacyInstall) -> Result<Removal, Error> {
    let target = match &install.identity {
        Identity::Portable(target) => target,
        #[cfg(windows)]
        _ => return Err(invalid("This is not a portable app folder.")),
    };
    let current = inspect(install.app, &target.directory)?;
    if current != *install {
        return Err(invalid("The old app folder changed after it was selected. Locate it again before archiving."));
    }
    // Opening files for write without truncation catches a running executable on Windows
    // before anything moves. Close our handles before renaming the containing directory.
    drop(preflight(&target.directory)?);
    let parent = target.directory.parent().ok_or_else(|| invalid("The app folder has no parent."))?;
    let backups = parent.join(".craftworks-backups");
    match fs::symlink_metadata(&backups) {
        Ok(meta) if !meta.is_dir() || redirected(&meta) => return Err(invalid("The neighbouring backup location is not a regular directory.")),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&backups).map_err(io_error)?,
        Err(error) => return Err(io_error(error)),
    }
    let holder = tempfile::Builder::new().prefix(&format!("{}-", install.app.id)).tempdir_in(&backups).map_err(io_error)?;
    let name = target.directory.file_name().ok_or_else(|| invalid("The app folder has no name."))?;
    let destination = holder.path().join(name);
    // Both paths share a parent volume. Never fall back to a copy/delete operation.
    fs::rename(&target.directory, &destination).map_err(|e| invalid(&format!("Couldn't archive the old app. Close it and retry ({e}).")))?;
    let _kept = holder.keep();
    Ok(Removal { backup: Some(destination), restart_required: false })
}

fn dedicated_name(app: App, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    package_ids(app).any(|id| name == id || name.strip_prefix(id).is_some_and(|tail| tail.starts_with(['-', '_', ' '])))
}

fn executable_name(app: App, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    package_ids(app).any(|id| {
        name == id
            || name == format!("{id}.exe")
            || name == format!("{id}.appimage")
            || name.strip_prefix(id).is_some_and(|tail| tail.starts_with('-') && tail.ends_with(".appimage"))
    })
}

fn package_ids(app: App) -> impl Iterator<Item = &'static str> {
    // VectorCraft was named DrawCraft before October 2026; the old program files are still
    // the same app, and should remain discoverable without matching unrelated products.
    std::iter::once(app.id).chain((app.id == "vectorcraft").then_some("drawcraft"))
}

fn check_binary(path: &Path) -> Result<(), Error> {
    let mut magic = [0u8; 4];
    File::open(path).map_err(io_error)?.read_exact(&mut magic).map_err(|_| invalid("The old executable is incomplete."))?;
    if !magic.starts_with(b"MZ") && magic != *b"\x7fELF" {
        return Err(invalid("The selected file is not a native app executable."));
    }
    Ok(())
}

fn regular_ancestors(path: &Path) -> Result<(), Error> {
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor).map_err(io_error)?;
        if redirected(&meta) {
            return Err(invalid("App folders reached through a symbolic link or junction cannot be archived."));
        }
    }
    Ok(())
}

fn preflight(root: &Path) -> Result<Vec<File>, Error> {
    let mut queue = vec![root.to_path_buf()];
    #[cfg(windows)]
    let mut handles = Vec::new();
    #[cfg(not(windows))]
    let handles = Vec::new();
    let mut count = 0usize;
    while let Some(path) = queue.pop() {
        count += 1;
        if count > 100_000 {
            return Err(invalid("This folder contains too many files to archive safely."));
        }
        let meta = fs::symlink_metadata(&path).map_err(io_error)?;
        if redirected(&meta) || !meta.is_dir() && !meta.is_file() {
            return Err(invalid("The old app folder contains a link or special file. Its contents were left in place."));
        }
        if meta.is_dir() {
            for entry in fs::read_dir(path).map_err(io_error)? {
                queue.push(entry.map_err(io_error)?.path());
            }
        } else {
            #[cfg(windows)]
            handles.push(OpenOptions::new().read(true).write(true).open(path).map_err(|e| invalid(&format!("Close the old app and retry ({e}).")))?);
        }
    }
    Ok(handles)
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

fn invalid(message: &str) -> Error {
    Error::Install(message.to_owned())
}

fn io_error(error: std::io::Error) -> Error {
    invalid(&format!("Couldn't inspect the previous app folder ({error})."))
}

#[cfg(test)]
mod tests;
