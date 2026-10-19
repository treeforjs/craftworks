//! The whole flow on a background thread; the app polls it once per frame.

use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use crate::install::{download_dir, download_path, prepare_dir};
use crate::release::MAX_ASSET_BYTES;
use crate::verify::{self, Hasher, MAX_LIST_BYTES, Sums};
use crate::{AppInfo, Applied, Arch, Asset, Error, InstallKind, Prepared, Release, Source, Version, newest};

/// A newer release, and whether this copy can install it itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// The release.
    pub release: Release,
    /// How this copy is installed.
    pub kind: InstallKind,
    /// The download that updates this copy, or why there is none (people read it; the app then
    /// offers the release page instead).
    pub install: Result<Asset, String>,
}

/// Where the updater is.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum State {
    /// Nothing has happened yet.
    #[default]
    Idle,
    /// Asking for the newest release.
    Checking,
    /// The running version is the newest.
    UpToDate,
    /// A newer release exists.
    Available(Offer),
    /// Downloading and verifying it: `received` of `total` bytes.
    Downloading {
        /// The release being downloaded.
        offer: Offer,
        /// Bytes so far.
        received: u64,
        /// Bytes in all (0 if unknown).
        total: u64,
    },
    /// Downloaded and verified: it installs when the app quits, or on restart.
    Ready(Offer),
    /// The last step failed.
    Failed(Error),
}

enum Msg {
    Checked(Result<Option<Offer>, Error>),
    Progress(u64),
    Downloaded(Result<Prepared, Error>),
}

/// Checks, downloads and installs updates without ever blocking the UI thread.
pub struct Updater {
    app: AppInfo,
    source: Arc<dyn Source>,
    state: State,
    rx: Option<Receiver<Msg>>,
    cancel: Arc<AtomicBool>,
    prepared: Option<Prepared>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    kind: Option<InstallKind>,
}

impl Updater {
    /// An updater for `app` that asks `source` (normally [`crate::GitHub`]).
    pub fn new(app: AppInfo, source: Arc<dyn Source>) -> Updater {
        Updater { app, source, state: State::Idle, rx: None, cancel: Arc::default(), prepared: None, wake: None, kind: None }
    }

    /// Called from the background thread whenever there is news (apps pass
    /// `move || ctx.request_repaint()`).
    pub fn on_change(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(Arc::new(wake));
    }

    /// Uses `kind` instead of detecting how this copy was installed (tests, previews).
    pub fn with_install_kind(mut self, kind: InstallKind) -> Updater {
        self.kind = Some(kind);
        self
    }

    /// The app this updater serves.
    pub fn app(&self) -> &AppInfo {
        &self.app
    }

    /// Where it is.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Whether a check or download is running.
    pub fn busy(&self) -> bool {
        matches!(self.state, State::Checking | State::Downloading { .. })
    }

    /// The verified download waiting to be installed, if any.
    pub fn prepared(&self) -> Option<&Prepared> {
        self.prepared.as_ref()
    }

    /// Starts looking for a newer release (does nothing while busy or once one is downloaded).
    pub fn check(&mut self) {
        if self.busy() || self.prepared.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let (app, source, kind) = (self.app.clone(), self.source.clone(), self.kind.clone());
        self.spawn(tx, move |tx| {
            let kind = kind.unwrap_or_else(|| InstallKind::detect(&app));
            let _ = tx.send(Msg::Checked(find_offer(&app, source.as_ref(), kind)));
        });
        self.rx = Some(rx);
        self.state = State::Checking;
    }

    /// Starts downloading the offered release, if this copy can install it.
    pub fn download(&mut self) {
        let State::Available(offer) = &self.state else { return };
        let Ok(asset) = offer.install.clone() else { return };
        let Some(key) = self.app.key() else { return };
        let offer = offer.clone();
        let (tx, rx) = channel();
        self.cancel = Arc::default();
        let (app, source, cancel) = (self.app.clone(), self.source.clone(), self.cancel.clone());
        let (release, kind) = (offer.release.clone(), offer.kind.clone());
        self.spawn(tx, move |tx| {
            let progress = tx.clone();
            let r = fetch(&app, source.as_ref(), &release, &asset, kind, &key, &cancel, &mut |n| {
                let _ = progress.send(Msg::Progress(n));
            });
            let _ = tx.send(Msg::Downloaded(r));
        });
        self.rx = Some(rx);
        self.state = State::Downloading { total: offer.install.as_ref().map_or(0, |a| a.size), offer, received: 0 };
    }

    /// Stops a running download.
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Forgets the outcome of the last check (after the person dismisses it).
    pub fn dismiss(&mut self) {
        if !self.busy() && self.prepared.is_none() {
            self.state = State::Idle;
        }
    }

    /// Picks up news from the background thread. Call once per frame; returns whether the
    /// state changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Some(rx) = &self.rx {
            let msg = match rx.try_recv() {
                Ok(m) => m,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.rx = None;
                    if self.busy() {
                        self.state = State::Failed(Error::Install("the update stopped unexpectedly".into()));
                        changed = true;
                    }
                    break;
                }
            };
            changed = true;
            match msg {
                Msg::Checked(Ok(Some(offer))) => self.state = State::Available(offer),
                Msg::Checked(Ok(None)) => self.state = State::UpToDate,
                Msg::Checked(Err(e)) | Msg::Downloaded(Err(e)) => self.state = State::Failed(e),
                Msg::Progress(n) => {
                    if let State::Downloading { received, .. } = &mut self.state {
                        *received = n;
                    }
                }
                Msg::Downloaded(Ok(p)) => {
                    if let State::Downloading { offer, .. } = std::mem::take(&mut self.state) {
                        self.state = State::Ready(offer);
                    }
                    self.prepared = Some(p);
                }
            }
        }
        changed
    }

    /// Installs the verified download, if there is one. Call it as the app quits, after unsaved
    /// work is handled; `restart` opens the new version afterwards. `Ok(None)` when there is
    /// nothing to install.
    pub fn apply(&mut self, restart: bool) -> Result<Option<Applied>, Error> {
        let Some(p) = self.prepared.take() else { return Ok(None) };
        p.apply(restart).map(Some)
    }

    fn spawn(&self, tx: Sender<Msg>, job: impl FnOnce(&Sender<Msg>) + Send + 'static) {
        let wake = self.wake.clone();
        // If the thread can't start, `tx` is dropped with the closure and `poll` reports it.
        let _ = std::thread::Builder::new().name("craft-update".into()).spawn(move || {
            // A bug in the update code must never take the app down with it.
            let waker = WakeOnDrop(wake);
            if catch_unwind(AssertUnwindSafe(|| job(&tx))).is_err() {
                let _ = tx.send(Msg::Checked(Err(Error::Install("the update stopped unexpectedly".into()))));
            }
            drop(waker);
        });
    }
}

struct WakeOnDrop(Option<Arc<dyn Fn() + Send + Sync>>);

impl Drop for WakeOnDrop {
    fn drop(&mut self) {
        if let Some(w) = &self.0 {
            w();
        }
    }
}

fn find_offer(app: &AppInfo, source: &dyn Source, kind: InstallKind) -> Result<Option<Offer>, Error> {
    let current = Version::parse(&app.current).ok_or_else(|| Error::Invalid(format!("this version ({}) isn't a version", app.current)))?;
    let url = format!("https://api.github.com/repos/{}/releases?per_page=20", app.repo);
    let body = source.get(&url, 4 << 20)?;
    let text = String::from_utf8(body).map_err(|_| Error::Invalid("not text".into()))?;
    let Some(release) = newest(&text, app)? else { return Ok(None) };
    if release.version <= current {
        return Ok(None);
    }
    let install = match (kind.why_not(app), kind.asset_name(app, &release.version, Arch::current())) {
        (Some(why), _) => Err(why),
        (None, None) => Err(format!("{} has no download for this system.", app.display_name)),
        (None, Some(name)) => match release.asset(&name) {
            None => Err(format!("This release has no {name} yet.")),
            Some(_) if app.key().is_none() => Err("This build can't verify downloads, so it doesn't install them itself.".into()),
            Some(_) if release.asset(verify::SUMS).is_none() || release.asset(verify::SIGNATURE).is_none() => {
                Err("This release isn't signed, so it can't be installed from inside the app.".into())
            }
            Some(a) => Ok(a.clone()),
        },
    };
    Ok(Some(Offer { release, kind, install }))
}

#[allow(clippy::too_many_arguments)]
fn fetch(
    app: &AppInfo,
    source: &dyn Source,
    release: &Release,
    asset: &Asset,
    kind: InstallKind,
    key: &[u8; 32],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<Prepared, Error> {
    // The signed list first: nothing large is downloaded for a release that doesn't verify.
    let list_url = release.asset(verify::SUMS).ok_or_else(|| Error::Verify("no checksum list".into()))?.url.clone();
    let sig_url = release.asset(verify::SIGNATURE).ok_or_else(|| Error::Verify("no signature".into()))?.url.clone();
    let list = source.get(&list_url, MAX_LIST_BYTES)?;
    let sig = source.get(&sig_url, MAX_LIST_BYTES)?;
    let sig = String::from_utf8(sig).map_err(|_| Error::Verify("the signature isn't text".into()))?;
    let sums = Sums::verified(&list, &sig, key)?;
    if sums.get(&asset.name).is_none() {
        return Err(Error::Verify(format!("{} isn't in the signed checksum list", asset.name)));
    }

    let dir = download_dir(app, &kind)?;
    prepare_dir(&dir, &kind)?;
    let path = download_path(&dir, &kind, &asset.name);
    let result = (|| {
        let mut file = std::fs::File::create(&path).map_err(|e| Error::Install(format!("can't write {}: {e}", path.display())))?;
        let mut hasher = Hasher::default();
        let mut received: u64 = 0;
        let limit = asset.size.clamp(1, MAX_ASSET_BYTES);
        source.download(&asset.url, limit, &mut |chunk| {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            hasher.update(chunk);
            file.write_all(chunk).map_err(|e| Error::Install(format!("can't write the download: {e}")))?;
            received = received.saturating_add(chunk.len() as u64);
            progress(received);
            Ok(())
        })?;
        file.sync_all().map_err(|e| Error::Install(format!("can't write the download: {e}")))?;
        verify::check(&sums, &asset.name, &hasher.finish())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(Prepared { kind, file: path, version: release.version.clone() })
}
