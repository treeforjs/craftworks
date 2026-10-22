use super::*;

fn fixture(root: &Path) -> Plan {
    let directory = root.join("launcher");
    let executable = directory.join(if cfg!(windows) { "craftworks.exe" } else { "craftworks" });
    fs::create_dir_all(&directory).unwrap();
    fs::write(&executable, b"fixture, never executed").unwrap();
    let library = root.join("library");
    fs::create_dir_all(&library).unwrap();
    let state = library.join("app.ron");
    fs::write(&state, b"window state").unwrap();
    let license = directory.join("LICENSE-MIT");
    fs::write(&license, b"license").unwrap();
    Plan {
        executable: executable.clone(),
        directory: directory.clone(),
        library,
        state,
        cache: root.join("cache/updates"),
        profiles: vec![root.join("config/photocraft")],
        launcher_files: vec![executable, license],
        desktop: None,
        command_link: None,
        photo_integrations: Vec::new(),
        #[cfg(windows)]
        uninstaller: directory.join("Uninstall.exe"),
        #[cfg(windows)]
        shortcut: root.join("Start Menu/Craftworks.lnk"),
    }
}

#[test]
fn optional_data_and_managed_apps_are_excluded_by_default() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path());
    fs::create_dir_all(plan.library.join("photocraft")).unwrap();
    let default = plan.paths(Options::default());
    assert!(!default.contains(&plan.profiles[0]));
    assert!(!default.contains(&plan.library.join("photocraft")));
    let all = plan.paths(Options { managed_apps: true, app_data: true });
    assert!(all.contains(&plan.profiles[0]));
    assert!(all.contains(&plan.library.join("photocraft")));
    assert!(default.contains(&plan.state));
    assert!(default.contains(&plan.executable));
}

#[test]
fn source_checkout_guard_precedes_any_delete_or_uninstaller_launch() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path());
    fs::write(plan.directory.join("Cargo.toml"), b"source").unwrap();
    let executable = plan.executable.clone();
    let state = plan.state.clone();
    assert!(plan.finish().is_err());
    assert!(executable.exists());
    assert!(state.exists());
}

#[test]
fn metadata_reads_are_bounded_and_special_file_trees_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("metadata");
    fs::write(&file, vec![b'x'; 65_537]).unwrap();
    assert!(read_small(&file).is_err());
    assert!(regular_file(root.path()).is_err());
    assert!(!safe_absolute(Path::new("relative")));
    assert!(!safe_absolute(&root.path().join("../elsewhere")));
}

#[test]
fn portable_profiles_are_detected_before_binaries_can_be_removed() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("photocraft");
    fs::create_dir_all(folder.join("versions/1/PhotoCraftData/Recovery")).unwrap();
    fs::write(folder.join("versions/1/PhotoCraftData/Recovery/unsaved.pcraft"), b"unsaved work").unwrap();
    assert!(contains_portable_profile(&folder).unwrap());
    assert!(folder.join("versions/1/PhotoCraftData/Recovery/unsaved.pcraft").is_file());
}

#[test]
fn photo_desktop_parser_requires_exact_unambiguous_name_icon_and_quoted_target() {
    let id = "io.github.treeforjs.photocraft";
    let path = if cfg!(windows) {
        "C:/managed/versions/00000000000000000001/photocraft.AppImage"
    } else {
        "/managed/versions/00000000000000000001/photocraft.AppImage"
    };
    let entry = format!("[Desktop Entry]\nType=Application\nName=PhotoCraft\nIcon={id}\nExec=\"{path}\" %F\n");
    assert_eq!(photo_desktop_target(&entry, id), Some(PathBuf::from(path)));
    for changed in [
        entry.replace("Name=PhotoCraft", "Name=Standalone editor"),
        entry.replace(&format!("Icon={id}"), "Icon=another-icon"),
        entry.replace(" %F", " --other"),
        format!("{entry}Exec=\"{path}\" %F\n"),
        entry.replace("[Desktop Entry]", "[Desktop Action edit]"),
    ] {
        assert!(photo_desktop_target(&changed, id).is_none(), "{changed}");
    }
}

#[test]
fn integration_snapshots_preserve_files_changed_after_preview() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("icon.png");
    fs::write(&path, b"managed icon").unwrap();
    let snapshot = Snapshot::capture(&path, 1024).unwrap();
    assert!(snapshot.unchanged());
    fs::write(&path, b"replacement from a different install").unwrap();
    assert!(!snapshot.unchanged());
    assert_eq!(fs::read(&path).unwrap(), b"replacement from a different install");
}

#[cfg(target_os = "linux")]
#[test]
fn managed_photo_integration_removal_preserves_standalone_changed_and_linked_entries() {
    use craft_update::Source;
    use std::sync::Arc;
    struct Offline;
    impl Source for Offline {
        fn get(&self, _: &str, _: u64) -> Result<Vec<u8>, Error> {
            Err(Error::Network("offline fixture".into()))
        }
        fn download(&self, _: &str, _: u64, _: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
            Err(Error::Network("offline fixture".into()))
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let manager = Manager::new(data.join("craftworks"), Arc::new(Offline)).unwrap();
    let app = manager.root().join("photocraft");
    fs::create_dir_all(app.join("versions/00000000000000000001")).unwrap();
    fs::create_dir(app.join("receipts")).unwrap();
    fs::write(app.join(".craftworks-owner"), b"Craftworks managed app v1: photocraft\n").unwrap();
    fs::write(app.join("receipts/00000000000000000001.json"), br#"{"schema":1,"app":"photocraft","version":"0.8.0","slot":1}"#).unwrap();
    let image = app.join("versions/00000000000000000001/photocraft.AppImage");
    fs::write(&image, b"fixture: never executed").unwrap();
    fs::create_dir(data.join("applications")).unwrap();
    let current = "io.github.treeforjs.photocraft";
    let legacy = "ai.storyteller.photocraft";
    let entry =
        |target: &Path, id: &str| format!("[Desktop Entry]\nType=Application\nName=PhotoCraft\nIcon={id}\nExec=\"{}\" %F\n", target.display());
    let desktop = data.join("applications").join(format!("{legacy}.desktop"));
    fs::write(&desktop, entry(&image, legacy)).unwrap();
    let icon = data.join("icons/hicolor/256x256/apps").join(format!("{legacy}.png"));
    fs::create_dir_all(icon.parent().unwrap()).unwrap();
    fs::write(&icon, b"managed icon").unwrap();
    let standalone = dir.path().join("standalone.AppImage");
    fs::write(&standalone, b"standalone").unwrap();
    let other_entry = data.join("applications").join(format!("{current}.desktop"));
    fs::write(&other_entry, entry(&standalone, current)).unwrap();
    let sentinel = data.join("icons/hicolor/256x256/apps/other-app.png");
    fs::write(&sentinel, b"external sentinel").unwrap();
    let captured = PhotoIntegration::scan(&data, &manager);
    assert_eq!(captured.len(), 1);
    let mut preview = fixture(dir.path());
    preview.photo_integrations = captured.clone();
    assert!(!preview.paths(Options::default()).contains(&desktop));
    assert!(preview.paths(Options { managed_apps: true, app_data: false }).contains(&desktop));
    let owned = captured.first().unwrap();
    fs::write(&desktop, entry(&standalone, legacy)).unwrap();
    owned.remove_if_unchanged(&manager).unwrap();
    assert!(icon.exists());
    assert!(desktop.exists());
    fs::write(&desktop, entry(&image, legacy)).unwrap();
    let captured = PhotoIntegration::scan(&data, &manager);
    fs::write(&icon, b"changed after preview").unwrap();
    captured.first().unwrap().remove_if_unchanged(&manager).unwrap();
    assert!(!desktop.exists());
    assert_eq!(fs::read(&icon).unwrap(), b"changed after preview");
    assert!(other_entry.exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"external sentinel");
    fs::write(&desktop, entry(&image, legacy)).unwrap();
    let captured = PhotoIntegration::scan(&data, &manager);
    captured.first().unwrap().remove_if_unchanged(&manager).unwrap();
    assert!(!desktop.exists());
    assert!(!icon.exists());
    // A linked entry never proves ownership and cannot authorize icon removal.
    std::os::unix::fs::symlink(&other_entry, &desktop).unwrap();
    assert!(PhotoIntegration::scan(&data, &manager).is_empty());
    assert!(other_entry.exists());
}

#[test]
fn process_names_cover_current_legacy_and_cli_apps_without_blocking_other_agents() {
    for name in ["pdfcraft", "VectorCraft.exe", "photocraft-cli", "printcraft", "drawcraft-cli.exe", "vectorcraft (deleted)"] {
        assert!(process_blocks_cleanup(name, true), "{name}");
        assert!(!process_blocks_cleanup(name, false), "launcher-only cleanup: {name}");
    }
    for name in ["ssh-agent", "gnome-keyring-daemon", "not-vectorcraft", "photocraft-notes.txt"] {
        assert!(!process_blocks_cleanup(name, true), "{name}");
    }
    assert!(process_blocks_cleanup("craftworks.exe", false));
    let own = "\"craftworks.exe\",\"1\",\"Console\",\"1\",\"1 K\"\n";
    assert!(!windows_processes_block(own, true).unwrap());
    assert!(windows_processes_block(&format!("{own}\"drawcraft-cli.exe\",\"2\"\n"), true).unwrap());
    assert!(!windows_processes_block(&format!("{own}\"drawcraft-cli.exe\",\"2\"\n"), false).unwrap());
    assert!(windows_processes_block(&format!("{own}{own}"), false).unwrap());
    assert!(windows_processes_block("unusable process list", true).is_err());
}

#[cfg(unix)]
#[test]
fn selected_data_links_and_symlinked_parents_are_never_traversed() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), b"documents").unwrap();
    let selected = root.path().join("selected");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("keep-on-error"), b"original").unwrap();
    symlink(outside.path(), selected.join("redirected")).unwrap();
    assert!(remove_tree_if_present(&selected).is_err());
    assert!(selected.join("keep-on-error").exists());
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"documents");
    symlink(outside.path(), root.path().join("parent")).unwrap();
    assert!(remove_file_if_present(&root.path().join("parent/keep")).is_err());
    assert!(outside.path().join("keep").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn launcher_cleanup_does_not_need_a_healthy_managed_library() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path());
    fs::write(plan.library.join(".craftworks-owner"), b"damaged ownership").unwrap();
    fs::write(plan.library.join("unknown"), b"keep").unwrap();
    plan.prepare(None, Options::default()).unwrap();
    let executable = plan.executable.clone();
    let library = plan.library.clone();
    plan.finish().unwrap();
    assert!(!executable.exists());
    assert_eq!(fs::read(library.join("unknown")).unwrap(), b"keep");
    assert_eq!(fs::read(library.join(".craftworks-owner")).unwrap(), b"damaged ownership");
}

#[cfg(target_os = "linux")]
#[test]
fn final_cleanup_preserves_unknown_files_profiles_backups_and_managed_apps() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path());
    let unknown = plan.directory.join("my-document.txt");
    fs::write(&unknown, b"work").unwrap();
    let managed = plan.library.join("photocraft");
    fs::create_dir(&managed).unwrap();
    let backup = root.path().join("craftworks-launcher-backups/previous");
    fs::create_dir_all(&backup).unwrap();
    let state = plan.state.clone();
    let executable = plan.executable.clone();
    // Simulate eframe's final write. finish() is called only after storage has exited.
    fs::write(&state, b"final saved window state").unwrap();
    plan.finish().unwrap();
    assert!(!executable.exists());
    assert!(!state.exists());
    assert!(unknown.exists());
    assert!(managed.exists());
    assert!(backup.exists());
}

#[cfg(windows)]
#[test]
fn deferred_windows_cleanup_quotes_literal_paths_and_waits_for_exit() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixture(root.path());
    plan.uninstaller = PathBuf::from("C:\\Users\\O'Neil & Co\\Craftworks\\Uninstall.exe");
    plan.state = PathBuf::from("C:\\Users\\O'Neil & Co\\Craftworks\\data\\app.ron");
    let script = windows_script(&plan, 1234);
    assert!(script.contains("Get-Process -Id 1234"));
    assert!(script.contains("WaitForExit(60000)"));
    assert!(script.contains("O''Neil & Co"));
    assert!(script.contains("-ArgumentList '/S'"));
    assert!(script.contains("-WindowStyle Hidden"));
    assert!(script.contains("Test-Path -LiteralPath"));
    assert!(!script.contains("-Recurse"));
    assert!(script.contains("MessageBox"));
    use std::io::Write;
    let parser = "$source=[Console]::In.ReadToEnd(); $errors=$null; [System.Management.Automation.Language.Parser]::ParseInput($source,[ref]$null,[ref]$errors) | Out-Null; if($errors){$errors | Out-String | Write-Output; exit 1}";
    let mut child = Command::new(system_executable("WindowsPowerShell/v1.0/powershell.exe").unwrap())
        .args(["-NoProfile", "-NonInteractive", "-Command", parser])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
