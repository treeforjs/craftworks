//! Full signed downloads and managed installations from the published app releases.
#![cfg(any(target_os = "linux", target_os = "windows"))]
#![allow(clippy::panic, missing_docs)]

use std::error::Error;
use std::fs;
use std::io;
use std::sync::Arc;

use craft_manager::{APPS, App, Manager};
use craft_update::GitHub;
use tempfile::TempDir;

fn trusted_key(app: App) -> Result<String, io::Error> {
    let variable = format!("{}_UPDATE_PUBLIC_KEY", app.id.to_ascii_uppercase());
    let app_key = std::env::var(&variable).ok();
    let shared_key = std::env::var("CRAFT_UPDATE_PUBLIC_KEY").ok();
    app_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .or_else(|| shared_key.as_deref().map(str::trim).filter(|key| !key.is_empty()))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other(format!("Set {variable} or CRAFT_UPDATE_PUBLIC_KEY to the trusted public release key.")))
}

#[test]
#[ignore = "downloads all three signed published packages; requires network and trusted public keys"]
fn installs_all_published_apps_with_verified_downloads() -> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let temporary_root = fs::canonicalize(temporary.path())?;
    let source = Arc::new(GitHub::new("Craftworks-published-install-test"));
    let mut manager = Manager::new(temporary_root.join("library"), source.clone())?;
    for app in APPS {
        manager = manager.with_public_key(app.id, &trusted_key(app)?)?;
    }
    let library = manager.root().to_path_buf();
    assert!(library.starts_with(&temporary_root));
    let mut activated = Vec::new();

    for app in APPS {
        assert!(manager.installed(app)?.is_none());
        let available = manager.check(app)?;
        assert!(available.blocked.is_none(), "{} is blocked: {:?}", app.name, available.blocked);
        let release = available.release.ok_or_else(|| io::Error::other(format!("{} has no published stable release.", app.name)))?;
        let asset = available.asset.ok_or_else(|| io::Error::other(format!("{} has no package for this platform.", app.name)))?;
        eprintln!("Installing {} {} from {} ({} bytes)", app.name, release.version, asset.name, asset.size);
        let mut last_progress = (0, asset.size);
        let installed = manager.install(app, &release, &mut |done, total| {
            assert_eq!(total, asset.size);
            assert!(done >= last_progress.0 && done <= total);
            last_progress = (done, total);
        })?;
        assert_eq!(last_progress, (asset.size, asset.size));
        assert_eq!(installed.version, release.version.to_string());
        let metadata = fs::symlink_metadata(&installed.executable)?;
        assert!(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() > 0);
        assert!(fs::canonicalize(&installed.executable)?.starts_with(&library));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(metadata.permissions().mode() & 0o111, 0);
        }
        let receipt = manager.installed(app)?.ok_or_else(|| io::Error::other(format!("{} has no active installation receipt.", app.name)))?;
        assert_eq!(receipt, installed);
        activated.push((app, installed));
    }

    drop(manager);
    let reopened = Manager::new(library, source)?;
    for (app, expected) in activated {
        assert_eq!(reopened.installed(app)?, Some(expected));
    }
    drop(reopened);
    temporary.close()?;
    assert!(!temporary_root.exists());
    Ok(())
}
