# craft-update

In-app updates for the craft apps, the same way in every app: look for a newer release on
GitHub, download the file that matches how this copy was installed, verify it, and install it
when the app quits (or restart into it right away).

| Installed with | Updates by |
| --- | --- |
| Windows installer (`.msi`) | running the new `.msi` after the app quits (`msiexec /passive`), then reopening the app |
| Windows setup program (`-setup.exe`, per user) | running the new setup silently (`/S`) after the app quits, then reopening the app |
| AppImage | replacing the AppImage file in place (atomic rename), then reopening it |
| Linux setup script (`-setup.sh`, per user) | running the new setup script with `bash`, then reopening the app |
| `.deb` / `.rpm` | the system package manager through `pkexec` (asks for a password) |
| Windows portable zip, Flatpak, tarball, macOS, source builds | not in place: the app offers the release page |

The two setup kinds are never detected: an app installed by its own setup program or script
says so with `Updater::with_install_kind(InstallKind::WindowsSetup)` (or `LinuxSetup`).

## Trust

Every release carries `SHA256SUMS.txt` (the release workflows already write it) and
`SHA256SUMS.txt.sig`, an Ed25519 signature of that list. The app carries the public key. The
updater downloads and checks the signed list first, downloads the installer only if its name is
in that list, hashes it while it arrives, and throws it away unless the hash matches. Only
links inside the app's own repository's release are followed, only versions newer than the
running one are offered, and because the version is part of each signed file name, an old
signed release can't be replayed as a new one.

Each download owns a fresh private staging folder. Starting, failing or cancelling another
download never sweeps that folder. Dropping the final prepared-update owner removes its
staging. Applying an update rechecks its size and signed checksum; changed payloads and
changed prepared metadata are refused. AppImage staging stays on the same filesystem for
atomic replacement. The Windows handoff waits for the app to exit, opens the MSI or setup
program against writes, rechecks the checksum, and removes its own staged file after the
installer exits.
An interrupted process may leave a private staging folder; a later download leaves it alone.

`Prepared` values come from `Updater::prepared`; they cannot be constructed from arbitrary
paths. Clones share staging ownership and only one successful application is allowed.

An app built without a public key (or a release without a signature) still says a new version
exists and links to its page; it just never installs anything itself.

## Use

```toml
craft-update = { git = "https://github.com/treeforjs/craftworks", tag = "craft-update-v0.1.0" }
```

```rust
let app = craft_update::AppInfo {
    id: "pdfcraft".into(),
    display_name: "PdfCraft".into(),
    repo: "treeforjs/pdfcraft".into(),
    current: env!("CARGO_PKG_VERSION").into(),
    public_key: option_env!("CRAFT_UPDATE_PUBLIC_KEY").map(Into::into),
    prereleases: false,
};
let mut updater = craft_update::Updater::new(app, Arc::new(craft_update::GitHub::new("PdfCraft/1.2.0")));
updater.on_change(move || ctx.request_repaint());

// AutoCheck defaults off. After the user opts in and startup finishes: check when due.
if settings.updates.due(craft_update::AutoCheck::now()) { /* after START_DELAY_SECS */ updater.check(); }

// Every frame: pick up news and draw `updater.state()`.
updater.poll();

// On quit, after unsaved work is handled.
updater.apply(restart)?;
```

The updater never blocks the UI thread: checks and downloads run on a background thread under
`catch_unwind`, and the automatic check waits until the app has been up for a few seconds and
runs at most once a day ([`AutoCheck`](src/schedule.rs)). `AutoCheck::default()` and settings
without `enabled` leave background checks off. Explicit saved choices are preserved. Callers
can still request a manual check with `Updater::check()`; Craftworks' launcher separately
checks its catalogue on launch. A release request sends the caller's generic app/version
User-Agent and normal connection information to GitHub, with no document or account payload.

## Signing releases

See [`docs/updates.md`](../../docs/updates.md) for the one-time key setup and the release
workflow step.
