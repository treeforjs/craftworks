use std::io::Cursor;

use zip::write::SimpleFileOptions;

use super::*;
use crate::APPS;

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn extract(bytes: &[u8]) -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let download = dir.path().join("download.zip");
    let output = dir.path().join("output");
    fs::write(&download, bytes).unwrap();
    fs::create_dir(&output).unwrap();
    extract_zip(&download, &output, *APPS.first().unwrap(), "pdfcraft-1.0.0-windows-x64-portable.zip", &AtomicBool::new(false))
}

#[test]
fn package_names_match_all_supported_release_architectures() {
    let app = *APPS.first().unwrap();
    let version = Version::parse("1.2.3").unwrap();
    for (os, arch, expected) in [
        (Os::Windows, Arch::X86_64, "pdfcraft-1.2.3-windows-x64-portable.zip"),
        (Os::Windows, Arch::Aarch64, "pdfcraft-1.2.3-windows-arm64-portable.zip"),
        (Os::Windows, Arch::X86, "pdfcraft-1.2.3-windows-x86-portable.zip"),
        (Os::Linux, Arch::X86_64, "pdfcraft-1.2.3-linux-x86_64.AppImage"),
        (Os::Linux, Arch::Aarch64, "pdfcraft-1.2.3-linux-aarch64.AppImage"),
    ] {
        assert_eq!(asset_name(app, &version, os, arch).as_deref(), Some(expected));
    }
    assert!(asset_name(app, &version, Os::MacOs, Arch::Aarch64).is_none());
    assert!(asset_name(app, &version, Os::Linux, Arch::X86).is_none());
}

#[test]
fn portable_markers_are_removed_while_models_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let download = dir.path().join("download.zip");
    let output = dir.path().join("output");
    let root = "pdfcraft-1.0.0-windows-x64-portable";
    let bytes = archive(&[
        (&format!("{root}/pdfcraft.exe"), b"program"),
        (&format!("{root}/portable.txt"), b""),
        (&format!("{root}/PdfCraft.portable"), b""),
        (&format!("{root}/models/model.bin"), b"model"),
    ]);
    fs::write(&download, bytes).unwrap();
    fs::create_dir(&output).unwrap();
    extract_zip(&download, &output, *APPS.first().unwrap(), "pdfcraft-1.0.0-windows-x64-portable.zip", &AtomicBool::new(false)).unwrap();
    assert!(output.join("pdfcraft.exe").is_file());
    assert!(!output.join("portable.txt").exists());
    assert!(!output.join("PdfCraft.portable").exists());
    assert_eq!(fs::read(output.join("models/model.bin")).unwrap(), b"model");
}

#[test]
fn path_traversal_absolute_names_and_windows_aliases_are_rejected() {
    for path in [
        "../outside.exe",
        "/outside.exe",
        "C:/outside.exe",
        "pdfcraft-1.0.0-windows-x64-portable/../outside.exe",
        "pdfcraft-1.0.0-windows-x64-portable/models/../../outside.exe",
        "pdfcraft-1.0.0-windows-x64-portable/file:stream",
        "pdfcraft-1.0.0-windows-x64-portable/CON.txt",
        "pdfcraft-1.0.0-windows-x64-portable/NUL .txt",
        "pdfcraft-1.0.0-windows-x64-portable/COM¹.txt",
        "pdfcraft-1.0.0-windows-x64-portable/CONOUT$",
        "pdfcraft-1.0.0-windows-x64-portable/file.",
        "pdfcraft-1.0.0-windows-x64-portable\\outside.exe",
        "other-root/pdfcraft.exe",
    ] {
        assert!(extract(&archive(&[(path, b"hostile")])).is_err(), "{path}");
    }
}

#[test]
fn case_collisions_and_duplicate_files_are_rejected() {
    let root = "pdfcraft-1.0.0-windows-x64-portable";
    assert!(extract(&archive(&[(&format!("{root}/pdfcraft.exe"), b"one"), (&format!("{root}/PDFCRAFT.EXE"), b"two")])).is_err());
    assert!(extract(&archive(&[(&format!("{root}/models/data"), b"one"), (&format!("{root}/models/data/file"), b"two")])).is_err());
}

#[test]
fn symlink_entries_and_zip_bombs_are_rejected() {
    let root = "pdfcraft-1.0.0-windows-x64-portable";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink(format!("{root}/pdfcraft.exe"), "../../outside", SimpleFileOptions::default()).unwrap();
    assert!(extract(&zip.finish().unwrap().into_inner()).is_err());

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(format!("{root}/pdfcraft.exe"), SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
    zip.write_all(&vec![0; 2 << 20]).unwrap();
    assert!(extract(&zip.finish().unwrap().into_inner()).is_err());
}

#[test]
fn a_signed_html_page_is_not_accepted_as_an_appimage() {
    let dir = tempfile::tempdir().unwrap();
    let download = dir.path().join("download");
    fs::write(&download, b"<html>not an executable</html>").unwrap();
    assert!(prepare_appimage(&download, &dir.path().join("app.AppImage")).is_err());
    assert!(!dir.path().join("app.AppImage").exists());
}
