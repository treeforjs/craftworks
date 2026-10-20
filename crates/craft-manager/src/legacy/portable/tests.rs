use super::*;

fn app() -> App {
    APPS[0]
}

fn folder(root: &Path) -> PathBuf {
    let path = root.join("PdfCraft-0.1.0-portable");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("pdfcraft.exe"), b"MZold binary").unwrap();
    path
}

#[test]
fn archiving_preserves_every_file_and_the_settings_backup() {
    let root = tempfile::tempdir().unwrap();
    let source = folder(root.path());
    fs::create_dir(source.join("PdfCraftData")).unwrap();
    fs::write(source.join("PdfCraftData/settings.json"), b"my settings").unwrap();
    fs::write(source.join("document.pdf"), b"my document").unwrap();
    let install = inspect(app(), &source.join("pdfcraft.exe")).unwrap();
    let result = archive(&install).unwrap();
    assert!(!source.exists());
    let backup = result.backup.unwrap();
    assert!(backup.starts_with(fs::canonicalize(root.path()).unwrap().join(".craftworks-backups")));
    assert_eq!(fs::read(backup.join("PdfCraftData/settings.json")).unwrap(), b"my settings");
    assert_eq!(fs::read(backup.join("document.pdf")).unwrap(), b"my document");
    assert_eq!(fs::read(backup.join("pdfcraft.exe")).unwrap(), b"MZold binary");
    assert!(archive(&install).is_err());
    // The operation is reversible: restoring the directory restores the old copy intact.
    fs::rename(&backup, &source).unwrap();
    assert!(inspect(app(), &source).is_ok());
}

#[test]
fn shared_and_unrelated_folders_are_not_archived() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join("Documents");
    fs::create_dir(&shared).unwrap();
    fs::write(shared.join("pdfcraft.exe"), b"MZold binary").unwrap();
    assert!(inspect(app(), &shared).is_err());
    let source = folder(root.path());
    fs::write(source.join("vectorcraft.exe"), b"MZother app").unwrap();
    assert!(inspect(app(), &source).is_err());
    assert!(source.join("pdfcraft.exe").exists());
    assert!(!root.path().join(".craftworks-backups").exists());
}

#[test]
fn vectorcraft_recognizes_its_old_drawcraft_folder_and_binary() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("DrawCraft-old");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("drawcraft.exe"), b"MZlegacy vector app").unwrap();
    let vector = APPS[1];
    let install = inspect(vector, &source).unwrap();
    assert_eq!(install.app.id, "vectorcraft");
    assert!(inspect(app(), &source).is_err());
    let backup = archive(&install).unwrap().backup.unwrap();
    assert!(backup.join("drawcraft.exe").is_file());
    assert!(!source.exists());
}

#[test]
fn changed_and_forged_targets_are_refused_without_moving_files() {
    let root = tempfile::tempdir().unwrap();
    let source = folder(root.path());
    let install = inspect(app(), &source).unwrap();
    let mut forged = install.clone();
    forged.location = Some(root.path().to_path_buf());
    assert!(archive(&forged).is_err());
    fs::write(source.join("pdfcraft.exe"), b"MZa replacement with a new size").unwrap();
    assert!(archive(&install).is_err());
    assert!(source.exists());
    assert!(!root.path().join(".craftworks-backups").exists());
}

#[test]
fn source_managed_and_already_archived_folders_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let source = folder(root.path());
    fs::write(source.join("Cargo.toml"), b"source checkout").unwrap();
    assert!(inspect(app(), &source).is_err());
    fs::remove_file(source.join("Cargo.toml")).unwrap();
    fs::write(root.path().join(".craftworks-owner"), b"owned").unwrap();
    assert!(inspect(app(), &source).is_err());
    fs::remove_file(root.path().join(".craftworks-owner")).unwrap();
    let backups = root.path().join(".craftworks-backups");
    fs::create_dir(&backups).unwrap();
    let archived = backups.join("PdfCraft");
    fs::rename(&source, &archived).unwrap();
    assert!(inspect(app(), &archived).is_err());
}

#[test]
fn binary_checks_names_and_path_boundaries_do_not_execute_old_code() {
    let root = tempfile::tempdir().unwrap();
    let source = folder(root.path());
    assert!(inspect(app(), Path::new("relative/PdfCraft")).is_err());
    assert!(inspect(app(), &source.join("../PdfCraft-0.1.0-portable")).is_err());
    fs::write(source.join("pdfcraft.exe"), b"<html>download error</html>").unwrap();
    assert!(inspect(app(), &source).is_err());
    fs::remove_file(source.join("pdfcraft.exe")).unwrap();
    fs::write(source.join("pdfcraft"), b"\x7fELFbinary").unwrap();
    assert!(inspect(app(), &source).is_ok());
    assert!(!dedicated_name(app(), "unrelated-folder"));
    assert!(!dedicated_name(app(), "pdfcraftish"));
}

#[cfg(unix)]
#[test]
fn links_in_the_folder_or_backup_location_leave_the_source_untouched() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let source = folder(root.path());
    let install = inspect(app(), &source).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, source.join("linked-data")).unwrap();
    assert!(archive(&install).is_err());
    fs::remove_file(source.join("linked-data")).unwrap();
    symlink(&outside, root.path().join(".craftworks-backups")).unwrap();
    assert!(archive(&install).is_err());
    assert!(source.exists());
    assert!(fs::read_dir(outside).unwrap().next().is_none());
}

#[cfg(windows)]
#[test]
fn locked_old_program_is_not_moved_or_deleted() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let source = folder(root.path());
    let install = inspect(app(), &source).unwrap();
    let _lock = OpenOptions::new().read(true).share_mode(0).open(source.join("pdfcraft.exe")).unwrap();
    assert!(archive(&install).is_err());
    assert!(source.exists());
    assert!(source.join("pdfcraft.exe").exists());
}
