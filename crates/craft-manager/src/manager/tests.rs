use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::atomic::AtomicUsize;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use zip::write::SimpleFileOptions;

use super::*;

#[derive(Default)]
struct Fake {
    objects: BTreeMap<String, Vec<u8>>,
    downloads: AtomicUsize,
}

impl Source for Fake {
    fn get(&self, url: &str, _limit: u64) -> Result<Vec<u8>, Error> {
        self.objects.get(url).cloned().ok_or_else(|| Error::Network(format!("Fixture not found: {url}")))
    }

    fn download(&self, url: &str, _limit: u64, sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        self.downloads.fetch_add(1, Ordering::Relaxed);
        let bytes = self.objects.get(url).ok_or_else(|| Error::Network("Fixture not found".into()))?;
        for chunk in bytes.chunks(11) {
            sink(chunk)?;
        }
        Ok(())
    }
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[19; 32])
}

fn app() -> App {
    *APPS.first().unwrap()
}

fn native_package(app: App, version: &Version) -> (String, Vec<u8>) {
    let name = package::asset_name(app, version, Os::current(), Arch::current()).unwrap();
    if Os::current() == Os::Windows {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let root = name.strip_suffix(".zip").unwrap();
        for (path, bytes) in [
            (format!("{root}/{}.exe", app.id), b"signed program".as_slice()),
            (format!("{root}/portable.txt"), b""),
            (format!("{root}/models/test.dat"), b"model"),
        ] {
            zip.start_file(path, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        (name, zip.finish().unwrap().into_inner())
    } else {
        let mut bytes = b"\x7fELF\x02\x01\x01\x00AI\x02".to_vec();
        bytes.extend_from_slice(version.to_string().as_bytes());
        (name, bytes)
    }
}

fn repository(versions: &[&str]) -> (Fake, Vec<Release>) {
    let mut fake = Fake::default();
    let mut raw_releases = Vec::new();
    let mut releases = Vec::new();
    for version in versions {
        let version = Version::parse(version).unwrap();
        let tag = format!("v{version}");
        let base = format!("https://github.com/{}/releases/download/{tag}/", app().repo);
        let (name, bytes) = native_package(app(), &version);
        let mut hash = verify::Hasher::default();
        hash.update(&bytes);
        let hex = hash.finish().iter().map(|b| format!("{b:02x}")).collect::<String>();
        let sums = format!("{hex}  {name}\n").into_bytes();
        let signature = STANDARD.encode(key().sign(&sums).to_bytes()).into_bytes();
        let mut assets = Vec::new();
        for (name, data) in [(name, bytes), (verify::SUMS.into(), sums), (verify::SIGNATURE.into(), signature)] {
            let url = format!("{base}{name}");
            assets.push(Asset { name, url: url.clone(), size: data.len() as u64 });
            fake.objects.insert(url, data);
        }
        raw_releases.push(serde_json::json!({
            "tag_name": tag, "html_url": format!("https://github.com/{}/releases/tag/{tag}", app().repo),
            "assets": assets.iter().map(|a| serde_json::json!({"name": a.name, "browser_download_url": a.url, "size": a.size})).collect::<Vec<_>>()
        }));
        releases.push(Release {
            version,
            tag: tag.clone(),
            page: format!("https://github.com/{}/releases/tag/{tag}", app().repo),
            notes: String::new(),
            assets,
        });
    }
    fake.objects.insert(format!("https://api.github.com/repos/{}/releases?per_page=100", app().repo), serde_json::to_vec(&raw_releases).unwrap());
    (fake, releases)
}

fn trusted(root: &Path, source: Arc<Fake>) -> Manager {
    Manager::new(root.to_path_buf(), source).unwrap().with_public_key(app().id, &STANDARD.encode(key().verifying_key().to_bytes())).unwrap()
}

#[test]
fn empty_releases_and_missing_keys_have_clear_states() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, _) = repository(&[]);
    let manager = trusted(&dir.path().join("empty"), Arc::new(fake));
    let state = manager.check(app()).unwrap();
    assert!(state.release.is_none());
    assert!(state.blocked.unwrap().contains("no published releases"));

    let (fake, releases) = repository(&["1.0.0"]);
    let fake = Arc::new(fake);
    let manager = Manager::new(dir.path().join("no-key"), fake.clone()).unwrap();
    assert!(manager.check(app()).unwrap().blocked.unwrap().contains("trusted release key"));
    assert!(matches!(manager.install(app(), &releases[0], &mut |_, _| {}), Err(Error::Verify(_))));
    assert_eq!(fake.downloads.load(Ordering::Relaxed), 0);
}

#[test]
fn signed_install_update_and_tamper_preserve_the_active_version() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    let (mut fake, releases) = repository(&["1.0.0", "2.0.0", "3.0.0"]);
    let tampered_url = releases[2].assets.first().unwrap().url.clone();
    fake.objects.get_mut(&tampered_url).unwrap()[0] ^= 1;
    let manager = trusted(&root, Arc::new(fake));
    let mut progress = Vec::new();
    let first = manager.install(app(), &releases[0], &mut |done, total| progress.push((done, total))).unwrap();
    assert_eq!(manager.installed(app()).unwrap(), Some(first.clone()));
    assert_eq!(progress.last().unwrap().0, progress.last().unwrap().1);
    assert!(!first.executable.parent().unwrap().join("portable.txt").exists());
    if Os::current() == Os::Windows {
        assert_eq!(fs::read(first.executable.parent().unwrap().join("models/test.dat")).unwrap(), b"model");
    }
    let second = manager.install(app(), &releases[1], &mut |_, _| {}).unwrap();
    assert_eq!(second.version, "2.0.0");
    assert!(first.executable.exists());
    assert!(matches!(manager.install(app(), &releases[2], &mut |_, _| {}), Err(Error::Verify(_))));
    assert_eq!(manager.installed(app()).unwrap(), Some(second));
    fs::write(root.join("unrelated.txt"), b"keep me").unwrap();
    manager.uninstall(app()).unwrap();
    assert!(manager.installed(app()).unwrap().is_none());
    assert_eq!(fs::read(root.join("unrelated.txt")).unwrap(), b"keep me");
    assert!(root.join(OWNER_FILE).exists());
}

#[test]
fn bad_signatures_are_rejected_before_the_binary_download() {
    let dir = tempfile::tempdir().unwrap();
    let (mut fake, releases) = repository(&["1.0.0"]);
    let signature = releases[0].asset(verify::SIGNATURE).unwrap();
    fake.objects.insert(signature.url.clone(), STANDARD.encode([0; 64]).into_bytes());
    let fake = Arc::new(fake);
    let manager = trusted(&dir.path().join("library"), fake.clone());
    assert!(matches!(manager.install(app(), &releases[0], &mut |_, _| {}), Err(Error::Verify(_))));
    assert_eq!(fake.downloads.load(Ordering::Relaxed), 0);
    assert!(manager.installed(app()).unwrap().is_none());
}

#[test]
fn an_unsigned_release_is_never_installed() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, mut releases) = repository(&["1.0.0"]);
    releases[0].assets.retain(|asset| asset.name != verify::SIGNATURE);
    let fake = Arc::new(fake);
    let manager = trusted(&dir.path().join("library"), fake.clone());
    assert!(matches!(manager.install(app(), &releases[0], &mut |_, _| {}), Err(Error::Verify(_))));
    assert_eq!(fake.downloads.load(Ordering::Relaxed), 0);
}

#[test]
fn cancellation_before_activation_retains_the_previous_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0", "2.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    let first = manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    let cancelled = AtomicBool::new(false);
    let result = manager.install_with_cancel(
        app(),
        &releases[1],
        &mut |done, _| {
            if done > 0 {
                cancelled.store(true, Ordering::Relaxed);
            }
        },
        &cancelled,
    );
    assert_eq!(result, Err(Error::Cancelled));
    assert_eq!(manager.installed(app()).unwrap(), Some(first));
    assert!(!fs::read_dir(manager.root()).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".craftworks-stage-")));
}

#[test]
fn missing_executables_are_reported_and_can_be_reinstalled() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    let first = manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    fs::remove_file(first.executable).unwrap();
    assert!(manager.installed(app()).is_err());
    let repaired = manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    assert_eq!(manager.installed(app()).unwrap(), Some(repaired));
}

#[test]
fn unowned_directories_forged_apps_and_outside_receipts_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("existing.txt"), b"untouched").unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    assert!(Manager::new(dir.path().to_path_buf(), Arc::new(Fake::default())).is_err());
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    let forged = App { id: "../outside", ..app() };
    assert!(manager.installed(forged).is_err());
    assert!(manager.uninstall(forged).is_err());
    fs::create_dir(manager.root().join(app().id)).unwrap();
    fs::write(manager.root().join(app().id).join("unrelated.txt"), b"untouched").unwrap();
    assert!(manager.install(app(), &releases[0], &mut |_, _| {}).is_err());
    assert!(manager.uninstall(app()).is_err());
    assert!(manager.root().join(app().id).join("unrelated.txt").exists());
}

#[test]
fn receipts_never_control_executable_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    let receipt = manager.root().join(app().id).join("receipts/00000000000000000001.json");
    fs::write(receipt, br#"{"schema":1,"app":"pdfcraft","version":"1.0.0","slot":1,"executable":"../../outside.exe"}"#).unwrap();
    assert!(manager.installed(app()).is_err());
    assert!(manager.launch(app()).is_err());
}

#[test]
fn incomplete_owned_structure_can_be_removed_or_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    let app_dir = manager.root().join(app().id);
    fs::create_dir(&app_dir).unwrap();
    fs::write(app_dir.join(OWNER_FILE), app_marker(app())).unwrap();
    assert!(manager.installed(app()).is_err());
    manager.uninstall(app()).unwrap();
    assert!(!app_dir.exists());

    fs::create_dir(&app_dir).unwrap();
    fs::write(app_dir.join(OWNER_FILE), app_marker(app())).unwrap();
    let repaired = manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    assert_eq!(manager.installed(app()).unwrap(), Some(repaired));
    assert!(!fs::read_dir(manager.root()).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".craftworks-app-")));
}

#[test]
fn damaged_receipts_explain_the_recovery_and_allow_removal() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    fs::write(manager.root().join(app().id).join("receipts/00000000000000000001.json"), b"damaged JSON").unwrap();
    let error = manager.installed(app()).unwrap_err();
    assert!(error.to_string().contains("Remove the app, then install it again"));
    manager.uninstall(app()).unwrap();
    manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    assert!(manager.installed(app()).unwrap().is_some());
}

#[test]
fn damaged_historical_receipts_do_not_block_the_active_installation() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0", "2.0.0", "3.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    let second = manager.install(app(), &releases[1], &mut |_, _| {}).unwrap();
    fs::write(manager.root().join(app().id).join("receipts/00000000000000000001.json"), b"damaged old receipt").unwrap();
    assert_eq!(manager.installed(app()).unwrap(), Some(second));
    let third = manager.install(app(), &releases[2], &mut |_, _| {}).unwrap();
    assert_eq!(manager.installed(app()).unwrap(), Some(third));
    manager.uninstall(app()).unwrap();
    assert!(manager.installed(app()).unwrap().is_none());
}

#[test]
fn an_interprocess_lock_prevents_concurrent_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    let lock = OpenOptions::new().read(true).write(true).open(manager.root().join(LOCK_FILE)).unwrap();
    lock.try_lock_exclusive().unwrap();
    assert!(manager.clone().install(app(), &releases[0], &mut |_, _| {}).is_err());
    assert!(manager.uninstall(app()).is_err());
    FileExt::unlock(&lock).unwrap();
    manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
}

#[test]
fn externally_supplied_release_urls_are_revalidated() {
    let dir = tempfile::tempdir().unwrap();
    let (fake, mut releases) = repository(&["1.0.0"]);
    releases[0].assets.first_mut().unwrap().url = "https://example.com/foreign.zip".into();
    let fake = Arc::new(fake);
    let manager = trusted(&dir.path().join("library"), fake.clone());
    assert!(matches!(manager.install(app(), &releases[0], &mut |_, _| {}), Err(Error::Invalid(_))));
    assert_eq!(fake.downloads.load(Ordering::Relaxed), 0);
}

#[cfg(target_os = "windows")]
#[test]
fn a_locked_windows_executable_preserves_receipts_on_uninstall_failure() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    let installed = manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    let locked = OpenOptions::new().read(true).share_mode(0).open(&installed.executable).unwrap();
    assert!(manager.uninstall(app()).is_err());
    assert!(manager.root().join(app().id).join(OWNER_FILE).is_file());
    assert!(manager.root().join(app().id).join("receipts/00000000000000000001.json").is_file());
    assert_eq!(manager.installed(app()).unwrap(), Some(installed));
    drop(locked);
    manager.uninstall(app()).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_installations_are_never_launched_or_removed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let (fake, releases) = repository(&["1.0.0"]);
    let manager = trusted(&dir.path().join("library"), Arc::new(fake));
    let installed = manager.install(app(), &releases[0], &mut |_, _| {}).unwrap();
    let outside = dir.path().join("outside");
    fs::write(&outside, b"unrelated").unwrap();
    fs::remove_file(&installed.executable).unwrap();
    symlink(&outside, &installed.executable).unwrap();
    assert!(manager.installed(app()).is_err());
    assert!(manager.launch(app()).is_err());
    assert!(manager.uninstall(app()).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"unrelated");
}
