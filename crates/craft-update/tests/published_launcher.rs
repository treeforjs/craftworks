//! Read-only check of the published launcher's real update feed.
#![cfg(all(not(target_arch = "wasm32"), feature = "github"))]
#![allow(clippy::panic, clippy::unwrap_used, missing_docs)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use craft_update::{AppInfo, GitHub, InstallKind, State, Updater, Version};

#[test]
#[ignore = "requires the public Craftworks release feed"]
fn discovers_a_newer_published_launcher_without_downloading() {
    let repository = "treeforjs/craftworks";
    let current = "0.1.0";
    let mut updater = Updater::new(
        AppInfo {
            id: "craftworks".into(),
            display_name: "Craftworks".into(),
            repo: repository.into(),
            current: current.into(),
            public_key: None,
            prereleases: false,
        },
        Arc::new(GitHub::new("Craftworks-published-launcher-test")),
    )
    .with_install_kind(InstallKind::Other);
    updater.check();

    let started = Instant::now();
    loop {
        updater.poll();
        match updater.state() {
            State::Available(offer) => {
                let release = &offer.release;
                assert!(release.version > Version::parse(current).unwrap(), "offered version is not newer than {current}");
                assert!(!release.version.is_prerelease(), "a prerelease was offered on the stable channel");
                assert_eq!(release.tag, format!("craftworks-v{}", release.version));
                assert_eq!(release.page, format!("https://github.com/{repository}/releases/tag/{}", release.tag));
                for suffix in ["linux-x86_64-setup.sh", "linux-aarch64-setup.sh", "windows-x64-setup.exe", "windows-arm64-setup.exe"] {
                    let name = format!("craftworks-{}-{suffix}", release.version);
                    let asset = release.asset(&name).unwrap_or_else(|| panic!("published launcher release is missing {name}"));
                    assert!(asset.size > 0, "{name} is empty");
                    assert_eq!(asset.url, format!("https://github.com/{repository}/releases/download/{}/{name}", release.tag));
                }
                for name in ["SHA256SUMS.txt", "SHA256SUMS.txt.sig"] {
                    assert!(release.asset(name).is_some(), "published launcher release is missing {name}");
                }
                assert_eq!(offer.kind, InstallKind::Other);
                assert!(offer.install.is_err(), "a source installation with no public key must only offer the release page");
                assert!(updater.prepared().is_none(), "checking unexpectedly prepared an installation");
                println!("Discovered Craftworks {} at {} without downloading or applying an update.", release.version, release.page);
                return;
            }
            State::Failed(error) => panic!("published launcher update check failed: {error}"),
            State::UpToDate => panic!("the public release feed did not offer a Craftworks version newer than {current}"),
            State::Idle | State::Checking => {}
            state => panic!("checking the public release feed entered unexpected state {state:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(30), "published launcher check timed out in state {:?}", updater.state());
        std::thread::sleep(Duration::from_millis(10));
    }
}
