# Signing keys and published releases

You make the signing key once as the publisher. It proves that an update came from you. One
key can sign releases for all four apps and any number of users or computers; people installing
your apps never need the private key or their own key.

Craftworks installs PdfCraft, VectorCraft and PhotoCraft from their **published stable GitHub
Releases**. The release must include a Windows portable zip or Linux AppImage, `SHA256SUMS.txt`
and its Ed25519 signature, `SHA256SUMS.txt.sig`. A draft release or an Actions artifact is not
visible to the launcher.

You can use one free Ed25519 key pair for all four repositories. This is sufficient for
Craftworks download verification on Windows and Linux. A Windows code-signing certificate is
separate and optional; it does not replace the update key.

To try the apps before setting up publishing, you can still run an unpacked Windows portable
zip, or a Linux AppImage/tarball, directly. The signing setup is needed for Craftworks to install
and update those apps automatically. Existing old zip/folder copies do not need keys to launch.

## 1. Make the update key on your own computer

If you already generated a key, reuse it. The helpers below are for creating your first key;
do not create another key for another user, computer or release.

Run **one** of the following from a current Craftworks checkout. The helper requires a new output folder
outside the repository, refuses to overwrite an existing folder and prints only the public key.
It makes no GitHub changes.

**Linux / Bash** (OpenSSL installed):

```sh
bash packaging/signing/new-key.sh "$HOME/.craft-signing-keys"
```

The Linux helper uses folder mode `700`, private-key mode `600` and `umask 077`.

**Windows / PowerShell** (OpenSSL on `PATH`, or the copy supplied with Git for Windows):

```powershell
./packaging/signing/new-key.ps1 -OutputDirectory (Join-Path $env:USERPROFILE 'Craft-signing-keys')
# If OpenSSL cannot be found automatically:
# ./packaging/signing/new-key.ps1 -OutputDirectory (Join-Path $env:USERPROFILE 'Craft-signing-keys') -OpenSsl 'C:/Program Files/Git/usr/bin/openssl.exe'
```

The Windows helper grants the current user and SYSTEM access to the new folder and its files.
Use an ordinary local folder with no junctions or symbolic links in its parents.

The output folder contains:

| File | Use |
| --- | --- |
| `craft-update-key.pem` | Private key. Upload its whole PEM contents only to the GitHub secret below. |
| `craft-update-public-key.txt` | Public key in base64 DER SubjectPublicKeyInfo format. Copy its single line into the GitHub variable below. |
| `craft-update-public.der` | The same public key in binary DER format, useful for signature checks. |

Keep an encrypted offline backup of the private key. Keep using the same key for later releases;
changing it requires distributing a newly built launcher and apps that trust the new public key.
Never commit the private key or paste it into an issue or chat.

The helpers generate an Ed25519 private PEM and export its public key as base64 DER. See
[OpenSSL key generation](https://docs.openssl.org/3.5/man1/openssl-genpkey/) and
[public-key export](https://docs.openssl.org/3.5/man1/openssl-pkey/).

## 2. Put the key in GitHub

Repeat these settings in **all four** repositories: `treeforjs/pdfcraft`, `treeforjs/vectorcraft`,
`treeforjs/photocraft` and `treeforjs/craftworks`.

1. **Settings > Environments > New environment**: create `release` if it does not exist.
   Under **Environment secrets**, add `UPDATE_SIGNING_KEY` with the complete contents of
   `craft-update-key.pem`, including the `BEGIN PRIVATE KEY` and `END PRIVATE KEY` lines.
2. **Settings > Secrets and variables > Actions > Variables > New repository variable**:
   add `CRAFT_UPDATE_PUBLIC_KEY` with the single line from `craft-update-public-key.txt`.
   Use a **repository variable**: Linux build jobs do not enter the `release` environment.
3. If the environment limits deployment branches/tags, allow the `release` **branch** for each
   app and Craftworks. If releasing Craftworks by tag, also allow `craftworks-v*` **tags**. Approve any
   configured environment-review gate when the release run reaches it.

These are two different settings: the private PEM is an **environment secret** and the public
base64 string is a **repository variable**. See
[GitHub environment secrets](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets#creating-secrets-for-an-environment)
and [GitHub variables](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/store-information-in-variables).

With one shared key, `CRAFT_UPDATE_PUBLIC_KEY` is all that Craftworks needs. If its repository
already has `PDFCRAFT_UPDATE_PUBLIC_KEY`, `VECTORCRAFT_UPDATE_PUBLIC_KEY`,
`PHOTOCRAFT_UPDATE_PUBLIC_KEY` or `CRAFTWORKS_UPDATE_PUBLIC_KEY`, set them to the same public key
or remove them: these app-specific variables override the shared fallback. Separate app keys are
also supported; put each corresponding public key in the matching Craftworks variable.

For local Craftworks builds, set the same public variable before building; repository variables
are passed automatically only in GitHub Actions:

```powershell
$env:CRAFT_UPDATE_PUBLIC_KEY = (Get-Content -Raw (Join-Path $env:USERPROFILE 'Craft-signing-keys/craft-update-public-key.txt')).Trim()
```

```sh
export CRAFT_UPDATE_PUBLIC_KEY="$(cat "$HOME/.craft-signing-keys/craft-update-public-key.txt")"
```

A launcher built before this step must be rebuilt and reinstalled. Changing GitHub settings
cannot add a key to an existing executable.

## 3. Build and publish the three app releases

The existing app workflows run on a push to their `release` branch. Their version comes from
`[workspace.package] version` in the root `Cargo.toml`. Inspect it before releasing; for reference,
`main` contained these versions on 20 October 2026:

| Repository | Version | Release tag |
| --- | --- | --- |
| `treeforjs/pdfcraft` | `0.5.0` | `v0.5.0` |
| `treeforjs/vectorcraft` | `0.8.0` | `v0.8.0` |
| `treeforjs/photocraft` | `0.6.0` | `v0.6.0` |

For each app:

1. Open **Actions** and enable workflows if GitHub prompts. On `main`, check the current
   version with `cargo xtask version`. If that version is already
   published, use `cargo xtask version set X.Y.Z` to choose a new one, then commit and review
   the updated `Cargo.toml` and `Cargo.lock`. Choose a normal `X.Y.Z` for the stable channel;
   the launcher does not offer `-rc` or other prereleases.
2. Move the reviewed `main` commit to `release`. If the branch does not exist, create it from
   `main`. In a clean, current clone on `main`, `git push origin main:release` creates it or
   fast-forwards it; if Git rejects a divergent/protected branch, merge through the normal PR
   process. Do not force-push. The push starts the **Release** workflow.
3. If `release` already points at the intended commit, use **Actions > Release > Run workflow**,
   select the **release** branch and leave the optional version override empty. Selecting `main`
   runs a build check that does not create the draft.
4. Wait for every required job to succeed. The workflow creates a **draft** named
   `<App> vX.Y.Z`. Open **Releases**, edit that draft and check that it contains the matching
   Windows `*-portable.zip`, Linux `*.AppImage`, `SHA256SUMS.txt` and `SHA256SUMS.txt.sig`.
   A missing-key warning is not success for Craftworks: do not publish until the signature exists.
5. Download and try the Windows/Linux packages, then choose **Publish release**. Keep it a
   stable release. Repeat for all three apps. The workflow chooses the `vX.Y.Z` tag; pushing
   an app version tag alone does not run these release workflows.

The release workflows build Windows and Linux packages. Their early signing checks reject a
missing or mismatched key before packaging starts. Desktop jobs check out the pinned font assets;
that build input must be available. A queued or failed required job prevents the final
draft; inspect the first run before assuming the signing key is the problem.

Repository-specific packaging details:
[PdfCraft](https://github.com/treeforjs/pdfcraft/blob/main/docs/releasing.md),
[VectorCraft](https://github.com/treeforjs/vectorcraft/blob/main/docs/releasing.md),
[PhotoCraft](https://github.com/treeforjs/photocraft/blob/main/docs/releasing.md).
GitHub documents [publishing a draft release](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository).

## 4. Build and publish Craftworks

1. Use the reviewed `main` commit containing the launcher and its current release workflow.
   Configure the Craftworks keys as above.
2. In **Actions > Craftworks release > Run workflow**, select `main` for a build
   check. A manual run validates keys and uploads build artifacts; it does **not** create a
   release or run the checksum-signing job.
3. The launcher version lives in `apps/craftworks/Cargo.toml` (initially `0.1.0`). For a later
   release, update it and `Cargo.lock`, commit and review the change. From a clean, current
   clone on `main`, push the reviewed commit to `release`, just as for the three apps:

   ```sh
   git push origin main:release
   ```

   If `release` already points at that commit, choose it in **Actions > Craftworks release >
   Run workflow**. The release branch runs the signing preflight, builds the packages and
   drafts `craftworks-v0.1.0` (or the current version), creating its tag at that exact commit.
   A matching `craftworks-vX.Y.Z` tag push is also supported. Existing version tags and published
   releases must never be moved or replaced.
4. Wait for **Craftworks release** to build Windows x64/ARM64 and Linux x86_64/aarch64,
   sign its checksum list and create the draft. Download the draft's installer for each platform,
   install it, and test an app install and launch using the newly published app releases.
5. Publish the draft from **Releases**. Install this new, keyed build in place of the earlier
   preview. For future releases, retain the signing key and increment the application version.

Published release assets must not be replaced. Rebuild with a new version if anything needs
changing. Craftworks updates its managed apps, and an installed launcher runs its own next
signed setup (`-setup.exe` or `-setup.sh`), so keep those asset names. See [launcher.md](launcher.md) for packaging and supported assets.

## Optional operating-system code signing

The Ed25519 key above authenticates downloads. Windows Authenticode separately establishes the
publisher identity used by Windows. A certificate or signing service may reduce operating-system
warnings, but a signature does not guarantee that SmartScreen will show none. The app workflows
already support these optional secrets in their `release` environment:

| Platform | Secrets |
| --- | --- |
| Windows | `WINDOWS_CERTIFICATE` (base64 PFX) + `WINDOWS_CERTIFICATE_PASSWORD`, or `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` |
| Linux | No operating-system signing certificate needed by this workflow. |

The initial Craftworks NSIS setup and executable do not yet have an Authenticode-signing step.
Set up the free update key first; purchasing a certificate is not required to publish working
Windows and Linux packages for Craftworks.
