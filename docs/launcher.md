# Craftworks launcher

Craftworks is the single place to install, update and open PdfCraft, VectorCraft and PhotoCraft
on Windows and Linux. Each app keeps its own release schedule; the launcher checks the latest
published stable GitHub Release in `treeforjs/pdfcraft`, `treeforjs/vectorcraft` and
`treeforjs/photocraft`.

## Current availability

Craftworks needs a published, signed GitHub Release for each app it installs. A repository,
an Actions artifact or a draft release alone is not an installable release. Existing unpacked
copies can still be run directly while publishing is being set up. Craftworks shows missing
release assets without inventing a download or installing an unsigned package.

The new launcher can be built from source. To distribute an installer that installs the apps,
maintainers must complete the [step-by-step signing and publishing guide](updates.md). Release assets
and repository secrets could not be tested from an unauthenticated checkout.

## Install and use

On Windows, run `craftworks-<version>-windows-x64-setup.exe`, or the `arm64` installer on an
ARM PC. It installs into `%LOCALAPPDATA%\Programs\Craftworks` by default, adds a Start menu
entry and appears in Windows Installed apps. No administrator access is needed. The portable
zip can also be unpacked anywhere; app downloads still use your normal user data folder.

On Linux, unpack the matching `craftworks-<version>-linux-x86_64.tar.gz` or `aarch64` tarball,
then run:

```sh
cd craftworks-<version>-linux-<arch>
bash ./install.sh
```

The installer adds an application menu entry and `~/.local/bin/craftworks`. The launcher lives
under `$XDG_DATA_HOME/craftworks-launcher` (normally `~/.local/share/craftworks-launcher`).
Run its `uninstall.sh` to remove the launcher. Linux releases are built on Ubuntu 22.04 and
need a desktop with glibc 2.35 or newer and the usual X11 or Wayland/OpenGL libraries.

Open Craftworks, check for releases, then choose Install, Update or Open for each app. App
downloads are installed for the current user. Existing MSI, distribution-package, Flatpak or
manually unpacked installations are separate from the copies managed by Craftworks.

### Retire an old ZIP or app folder

Under each app, choose **Locate old ZIP copy**, paste the absolute path to the old app folder
or its executable, and review the detected location. You can also drop the folder into this
dialog. The app must be inside a dedicated folder named for it, such as `PdfCraft` or
`pdfcraft-0.5.0-linux-x86_64`; shared folders and source checkouts are refused.
Earlier `DrawCraft` folders and executables are recognized as VectorCraft copies too.

Choose **Archive old copy** to move that entire folder into a uniquely named folder under
`.craftworks-backups` beside its original location. Nothing in the old folder is deleted:
embedded portable settings, documents and resources all move with it. The launcher shows the
backup path. Restore the old copy by moving its archived folder back to the original location.
Settings in this backup are not automatically imported into a new managed installation, and
old shortcuts may need updating or removing.

Craftworks also looks for dedicated app folders directly inside Downloads, Desktop,
Applications, Apps and `.local/opt`. Other locations can be supplied manually. On Windows it
recognizes the apps' previous registered MSI packages and offers their official Windows
Installer removal separately. That removal preserves normal user settings and documents.
Linux distribution-package and Flatpak removal remain with their package managers.

Retiring an old copy is optional and requires confirmation. It never runs automatically when
Craftworks is installed. Install and test the replacement first when possible; if no verified
replacement is available, the confirmation explains that the old location will stop working.

Managed app files live under `%LOCALAPPDATA%\Craftworks` on Windows or
`$XDG_DATA_HOME/craftworks` on Linux. Each successful update activates a new version directory
only after verification and extraction succeed. A failed or cancelled install leaves the
previous active version in place. Older versions are retained until the app is removed;
selecting an older version in the interface is not supported yet.

Linux app launches require the AppImage runtime to work on the host (including FUSE where
that published AppImage requires it). The launcher does not currently extract AppImages as a
fallback.

Use Craftworks to update apps installed by Craftworks. In particular, the Windows apps'
existing in-app updater assumes an MSI when no portable marker is present; it does not yet
recognize the launcher's managed copies. A hub install removes the portable markers before
first launch so PdfCraft and PhotoCraft keep their settings in stable per-user folders across
updates. It does not import the settings from an existing portable copy.

Update Craftworks itself by downloading its newer release and running the installer again.
Automatic replacement of the launcher is not part of this first version. Uninstalling the
launcher retains managed apps and user preferences; remove unwanted apps from Craftworks
first.

## Run or package from source

Install the current stable Rust toolchain. Linux builds also need `libxkbcommon-dev`,
`libwayland-dev`, `libx11-dev`, `libxrandr-dev`, `libxi-dev` and `libgl1-mesa-dev` (Ubuntu names).

```sh
cargo run -p craftworks
cargo run -p craftworks -- --version
```

The launcher follows the existing Craft design system. Generate Light, Dark, narrow-window,
offline and missing-key previews without accessing the network:

```sh
cargo run -p craftworks --example launcher_gallery -- target/launcher-gallery
```

The offline and missing-key previews use clearly labelled test fixtures. They are not
published app versions. The committed [Light](images/launcher-light.png) and
[Dark](images/launcher-dark.png) screenshots show the current unpublished-release state.

A development build with no trusted keys can inspect release availability but cannot install
apps. Keys are embedded at compile time, so configure them before building a distributable.

Windows packaging uses PowerShell, an MSVC Rust toolchain and NSIS:

```powershell
./packaging/windows/package.ps1 -Arch x64 -MakeNsis 'C:/Program Files (x86)/NSIS/makensis.exe'
# On ARM Windows, use -Arch arm64.
```

Linux packaging uses Bash, Python 3 and the desktop build dependencies:

```sh
bash packaging/linux/package.sh
```

The scripts write to `dist/release`. They include both open-source licenses. The Windows build
links the C runtime statically. The NSIS installer is per user; the Linux installer uses the
user's own data and application folders. Neither installer downloads app code during setup.

## Trusted release keys

Craftworks reuses `craft-update` verification. It first verifies the Ed25519 signature of
`SHA256SUMS.txt`, then checks the selected download's SHA-256. HTTPS transport or GitHub asset
metadata alone does not authorize an install. Missing keys, missing signatures and mismatching
hashes stop installation.

The signing key belongs to the publisher. Make it once; any number of users and computers can
install the signed releases, and users never need the private key. The key authenticates
updates on both Windows and Linux; a Windows signing certificate is separate and optional.

Use the existing app release workflows and the key setup in [`updates.md`](updates.md). For
each app, its `release` environment needs an `UPDATE_SIGNING_KEY` secret, and its repository
needs the corresponding `CRAFT_UPDATE_PUBLIC_KEY` variable. These are not Windows code-signing
certificates: they authenticate the release's checksum list.

In the **Craftworks repository**, set the following Actions repository variables. They are
public keys, not secrets:

| Variable | Trusts releases of |
| --- | --- |
| `PDFCRAFT_UPDATE_PUBLIC_KEY` | PdfCraft |
| `VECTORCRAFT_UPDATE_PUBLIC_KEY` | VectorCraft |
| `PHOTOCRAFT_UPDATE_PUBLIC_KEY` | PhotoCraft |
| `CRAFTWORKS_UPDATE_PUBLIC_KEY` | Craftworks release checksum lists |

Each variable accepts a base64 32-byte Ed25519 public key or a base64 DER SubjectPublicKeyInfo.
If all four use one signing key, set just `CRAFT_UPDATE_PUBLIC_KEY`: it is the fallback for any
unset app-specific variable. Never embed a private key. Changing a repository variable does
not change already-built launchers; rebuild and redistribute them when a trusted key changes.

For local builds, export the same environment variables before `cargo build`. The release
workflow validates that all four public keys are present and correctly encoded. Craftworks'
`release` environment also needs `UPDATE_SIGNING_KEY`, matching its own public key, to draft a
release with signed checksums. Operating-system code signing is separate; the initial NSIS
packaging does not sign the executable or setup file with Authenticode.
If the `release` environment restricts allowed branches or tags, allow the `release` branch
and the `craftworks-v*` release tags.

## First release checklist

See [`updates.md`](updates.md) for Linux and Windows key-generation helpers, exact GitHub
settings and branch/publishing steps. The helpers keep private keys outside the checkout and
make no GitHub changes. Release from the reviewed commit containing the current launcher workflow.

1. Configure the app signing keys, then run each app's existing release workflow on its
   `release` branch. Those workflows create **draft** releases; a draft is not visible to
   Craftworks. Verify the Windows portable zip, Linux AppImage, `SHA256SUMS.txt` and
   `SHA256SUMS.txt.sig`, then publish each stable release.
2. Configure the Craftworks repository's matching public keys and its release signing secret.
3. Bump `apps/craftworks/Cargo.toml` and update `Cargo.lock`. Run the CI checks and test a real
   signed app install and update on both Windows and Linux.
4. Push the reviewed commit to the `release` branch, or push a matching version tag such as
   `craftworks-v0.1.0`.
   `launcher-release.yml` builds Windows x64 and ARM64 and Linux x86_64 and aarch64, signs the
   checksum list, and creates a draft Craftworks release. A manual workflow dispatch on `release`
   also creates a draft; selecting `main` only builds and checks artifacts. Signing preflight
   rejects missing or mismatched private/public keys before the package builds start.
5. Download the draft installers, check installation, app launches and updates on both
   platforms, then publish the draft. Published version assets must not be replaced.

The launcher uses the existing, common app asset names:

| Platform | Asset |
| --- | --- |
| Windows | `<app>-<version>-windows-<arch>-portable.zip` (`x64`, `x86`, `arm64`) |
| Linux | `<app>-<version>-linux-<arch>.AppImage` (`x86_64`, `aarch64`) |
| Verification | `SHA256SUMS.txt` and `SHA256SUMS.txt.sig` |

Windows archives contain one `<app>-<version>-windows-<arch>-portable` directory with the
executable and its resources. PdfCraft's `models` folder must stay beside its executable. The
launcher uses the verified portable package instead of invoking a system MSI or requiring
administrator access. Linux apps use their published AppImages; distribution packages and
Flatpak remain available independently through each app's release page.
