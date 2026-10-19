//! The whole flow against a fake GitHub: check, download, verify, install.
#![allow(clippy::unwrap_used, clippy::panic, missing_docs)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use craft_update::{AppInfo, Error, InstallKind, Source, State, Updater};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct Fake {
    files: Mutex<HashMap<String, Vec<u8>>>,
}

impl Source for Fake {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, Error> {
        let f = self.files.lock().unwrap();
        let body = f.get(url).cloned().ok_or_else(|| Error::Network(format!("404 {url}")))?;
        if body.len() as u64 > limit {
            return Err(Error::Network("too big".into()));
        }
        Ok(body)
    }

    fn download(&self, url: &str, limit: u64, sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        let body = self.get(url, limit)?;
        for chunk in body.chunks(3) {
            sink(chunk)?;
        }
        Ok(())
    }
}

const REPO: &str = "treeforjs/pdfcraft";
const ASSET: &str = "pdfcraft-0.6.0-linux-x86_64.AppImage";

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn app() -> AppInfo {
    AppInfo {
        id: "pdfcraft".into(),
        display_name: "PdfCraft".into(),
        repo: REPO.into(),
        current: "0.5.0".into(),
        public_key: Some(STANDARD.encode(key().verifying_key().to_bytes())),
        prereleases: false,
    }
}

fn dl(name: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/v0.6.0/{name}")
}

/// A fake GitHub with release v0.6.0 holding `payload` as the AppImage; `listed` is what the
/// signed checksum list says it should be.
fn github(payload: &[u8], listed: &[u8], sign_with: &SigningKey) -> Arc<Fake> {
    let hex: String = Sha256::digest(listed).iter().map(|b| format!("{b:02x}")).collect();
    let sums = format!("{hex}  {ASSET}\n");
    let sig = STANDARD.encode(sign_with.sign(sums.as_bytes()).to_bytes());
    let asset = |name: &str, size: usize| format!(r#"{{"name":"{name}","size":{size},"browser_download_url":"{}"}}"#, dl(name));
    let list = format!(
        r#"[{{"tag_name":"v0.6.0","html_url":"https://github.com/{REPO}/releases/tag/v0.6.0","assets":[{},{},{}]}}]"#,
        asset(ASSET, payload.len()),
        asset("SHA256SUMS.txt", sums.len()),
        asset("SHA256SUMS.txt.sig", sig.len())
    );
    let fake = Fake::default();
    {
        let mut f = fake.files.lock().unwrap();
        f.insert(format!("https://api.github.com/repos/{REPO}/releases?per_page=20"), list.into_bytes());
        f.insert(dl(ASSET), payload.to_vec());
        f.insert(dl("SHA256SUMS.txt"), sums.into_bytes());
        f.insert(dl("SHA256SUMS.txt.sig"), sig.into_bytes());
    }
    Arc::new(fake)
}

fn settle(u: &mut Updater) {
    let start = Instant::now();
    while u.busy() || matches!(u.state(), State::Idle) {
        u.poll();
        assert!(start.elapsed() < Duration::from_secs(10), "stuck at {:?}", u.state());
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn appimage_updater(dir: &std::path::Path, source: Arc<Fake>, app: AppInfo) -> (Updater, std::path::PathBuf) {
    let target = dir.join("PdfCraft.AppImage");
    std::fs::write(&target, b"old version").unwrap();
    (Updater::new(app, source).with_install_kind(InstallKind::AppImage(target.clone())), target)
}

#[test]
fn a_signed_update_downloads_verifies_and_installs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut u, target) = appimage_updater(dir.path(), github(b"new version", b"new version", &key()), app());
    u.check();
    settle(&mut u);
    let State::Available(offer) = u.state() else { panic!("{:?}", u.state()) };
    assert_eq!(offer.release.version.to_string(), "0.6.0");
    assert_eq!(offer.install.as_ref().unwrap().name, ASSET);
    u.download();
    settle(&mut u);
    assert!(matches!(u.state(), State::Ready(_)), "{:?}", u.state());
    // Nothing changes until the app quits.
    assert_eq!(std::fs::read(&target).unwrap(), b"old version");
    u.apply(false).unwrap().unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"new version");
    assert_eq!(u.apply(false).unwrap(), None, "only once");
}

#[test]
fn a_tampered_download_is_thrown_away() {
    let dir = tempfile::tempdir().unwrap();
    let (mut u, target) = appimage_updater(dir.path(), github(b"evil version", b"new version", &key()), app());
    u.check();
    settle(&mut u);
    u.download();
    settle(&mut u);
    assert!(matches!(u.state(), State::Failed(Error::Verify(_))), "{:?}", u.state());
    assert_eq!(u.apply(true).unwrap(), None);
    assert_eq!(std::fs::read(&target).unwrap(), b"old version");
    let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
}

#[test]
fn a_list_signed_by_someone_else_downloads_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut u, _) = appimage_updater(dir.path(), github(b"evil", b"evil", &SigningKey::from_bytes(&[9u8; 32])), app());
    u.check();
    settle(&mut u);
    u.download();
    settle(&mut u);
    assert!(matches!(u.state(), State::Failed(Error::Verify(_))), "{:?}", u.state());
}

#[test]
fn without_a_key_or_installer_the_release_page_is_offered() {
    let dir = tempfile::tempdir().unwrap();
    let mut no_key = app();
    no_key.public_key = None;
    let (mut u, _) = appimage_updater(dir.path(), github(b"new", b"new", &key()), no_key);
    u.check();
    settle(&mut u);
    let State::Available(offer) = u.state() else { panic!("{:?}", u.state()) };
    assert!(offer.install.is_err());
    u.download();
    assert!(matches!(u.state(), State::Available(_)), "download refuses without a key");

    let mut u = Updater::new(app(), github(b"new", b"new", &key())).with_install_kind(InstallKind::WindowsPortable);
    u.check();
    settle(&mut u);
    let State::Available(offer) = u.state() else { panic!("{:?}", u.state()) };
    assert!(offer.install.as_ref().unwrap_err().contains("portable"));
}

#[test]
fn the_newest_version_is_up_to_date() {
    let mut current = app();
    current.current = "0.6.0".into();
    let mut u = Updater::new(current, github(b"x", b"x", &key())).with_install_kind(InstallKind::WindowsMsi);
    u.check();
    settle(&mut u);
    assert_eq!(u.state(), &State::UpToDate);
}

#[test]
fn a_cancelled_download_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let (mut u, _) = appimage_updater(dir.path(), github(&[1u8; 3000], &[1u8; 3000], &key()), app());
    u.check();
    settle(&mut u);
    u.download();
    u.cancel();
    settle(&mut u);
    // Either the cancel landed or the tiny download finished first.
    assert!(matches!(u.state(), State::Failed(Error::Cancelled) | State::Ready(_)), "{:?}", u.state());
}

#[test]
fn a_network_failure_is_reported() {
    let mut u = Updater::new(app(), Arc::new(Fake::default())).with_install_kind(InstallKind::WindowsMsi);
    u.check();
    settle(&mut u);
    assert!(matches!(u.state(), State::Failed(Error::Network(_))), "{:?}", u.state());
}
