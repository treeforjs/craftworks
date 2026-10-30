use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use craft_update::{Arch, Error, Os, Version};

use crate::App;
use crate::manager::{check_cancel, io_error, metadata};

const MAX_ENTRIES: usize = 20_000;
const MAX_UNPACKED: u64 = 4 << 30;
const MAX_FILE: u64 = 2 << 30;

pub(crate) fn asset_name(app: App, version: &Version, os: Os, arch: Arch) -> Option<String> {
    let suffix = match (os, arch) {
        (Os::Windows, Arch::X86_64) => "windows-x64-portable.zip",
        (Os::Windows, Arch::X86) => "windows-x86-portable.zip",
        (Os::Windows, Arch::Aarch64) => "windows-arm64-portable.zip",
        (Os::Linux, Arch::X86_64) => "linux-x86_64.AppImage",
        (Os::Linux, Arch::Aarch64) => "linux-aarch64.AppImage",
        _ => return None,
    };
    Some(format!("{}-{version}-{suffix}", app.id))
}

pub(crate) fn prepare_appimage(download: &Path, executable: &Path) -> Result<(), Error> {
    // An AppImage is an ELF executable. Do not mark a signed HTML error page executable.
    let mut magic = [0u8; 11];
    File::open(download).map_err(io_error)?.read_exact(&mut magic).map_err(|_| Error::Invalid("The AppImage download is incomplete.".into()))?;
    if !magic.starts_with(b"\x7fELF") || magic.get(8..11) != Some(b"AI\x02".as_slice()) {
        return Err(Error::Invalid("The release package is not a type 2 AppImage.".into()));
    }
    fs::rename(download, executable).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).map_err(io_error)?;
    }
    Ok(())
}

pub(crate) fn extract_zip(download: &Path, destination: &Path, app: App, name: &str, cancelled: &AtomicBool) -> Result<(), Error> {
    let package_root = name.strip_suffix(".zip").ok_or_else(|| Error::Invalid("The Windows package is not a ZIP file.".into()))?;
    let file = File::open(download).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    if archive.len() > MAX_ENTRIES {
        return Err(Error::Invalid("The ZIP package has too many files.".into()));
    }
    let mut unpacked = 0u64;
    let mut seen = HashSet::new();
    for index in 0..archive.len() {
        check_cancel(cancelled)?;
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        let directory = entry.is_dir();
        let entry_name = entry.name().map_err(zip_error)?;
        let name = entry_name.trim_end_matches('/');
        if name.contains('\\') || name.len() > 4096 || name.is_empty() || name.split('/').any(|part| !safe_part(part)) {
            return Err(Error::Invalid("The ZIP package contains an unsafe path.".into()));
        }
        let Some((root, relative)) = name.split_once('/') else {
            if directory && name == package_root {
                continue;
            }
            return Err(Error::Invalid("The ZIP package does not have the expected app directory.".into()));
        };
        if root != package_root || relative.is_empty() || !seen.insert(relative.to_ascii_lowercase()) {
            return Err(Error::Invalid("The ZIP package contains duplicate files or an unexpected app directory.".into()));
        }
        if let Some(mode) = entry.unix_mode() {
            let file_type = mode & 0o170000;
            if file_type != 0 && file_type != 0o100000 && file_type != 0o040000 || file_type == 0o040000 && !directory {
                return Err(Error::Invalid("The ZIP package contains a link or special file.".into()));
            }
        }
        let output = destination.join(relative);
        if directory {
            fs::create_dir_all(&output).map_err(io_error)?;
            continue;
        }
        let size = entry.size();
        unpacked = unpacked.checked_add(size).ok_or_else(|| Error::Invalid("The ZIP package expands beyond its limit.".into()))?;
        if size > MAX_FILE || unpacked > MAX_UNPACKED || size > entry.compressed_size().max(1).saturating_mul(1000) {
            return Err(Error::Invalid("The ZIP package expands beyond its safe limits.".into()));
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(output).map_err(io_error)?;
        let mut actual = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            check_cancel(cancelled)?;
            let read = entry.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                break;
            }
            actual = actual.checked_add(read as u64).ok_or_else(|| Error::Invalid("A ZIP entry is too large.".into()))?;
            if actual > size {
                return Err(Error::Invalid("A ZIP entry exceeds its declared size.".into()));
            }
            file.write_all(buffer.get(..read).unwrap_or_default()).map_err(io_error)?;
        }
        if actual != size {
            return Err(Error::Invalid("A ZIP entry is incomplete.".into()));
        }
        file.sync_all().map_err(io_error)?;
    }
    let executable = destination.join(format!("{}.exe", app.id));
    if !metadata(&executable)?.is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink() && meta.len() > 0) {
        return Err(Error::Invalid("The ZIP package has no app executable.".into()));
    }
    for name in ["portable.txt".to_owned(), format!("{}.portable", app.name)] {
        let path = destination.join(name);
        if let Some(meta) = metadata(&path)? {
            if !meta.is_file() {
                return Err(Error::Invalid("The portable settings marker is not a regular file.".into()));
            }
            fs::remove_file(path).map_err(io_error)?;
        }
    }
    Ok(())
}

fn safe_part(part: &str) -> bool {
    if part.is_empty()
        || matches!(part, "." | "..")
        || part.ends_with(['.', ' '])
        || part.chars().any(|ch| ch.is_control() || matches!(ch, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    let stem = part.split('.').next().unwrap_or_default().trim_end_matches(' ').to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") {
        return false;
    }
    if let Some(number) = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT"))
        && (number.len() == 1 && number.bytes().any(|b| matches!(b, b'1'..=b'9')) || matches!(number, "¹" | "²" | "³"))
    {
        return false;
    }
    true
}

fn zip_error(error: zip::result::ZipError) -> Error {
    Error::Invalid(format!("The Windows ZIP package could not be read ({error})."))
}

#[cfg(test)]
mod tests;
