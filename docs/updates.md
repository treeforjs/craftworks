# Installers and in-app updates

Every craft app ships real installers from its `release.yml` (pushing to the `release` branch
drafts a GitHub Release; publishing the draft makes it the version apps update to):

- **Windows:** a per-machine `.msi` (x64, x86, arm64; Start menu entry, file associations,
  upgrades in place) and a portable `.zip`.
- **Linux:** an AppImage (with zsync delta-update information), `.deb`, `.rpm`, a Flatpak bundle and
  a tarball, for x86_64 and aarch64.
- **macOS:** a universal `.dmg`.

Apps update themselves with [`craft-update`](../crates/craft-update). This page is what the
maintainer sets up once.

## 1. The update signing key (needed for in-app installs)

The app only installs what is signed with this key. Make it once, on your own machine, and keep
the private half secret:

```sh
openssl genpkey -algorithm ed25519 -out craft-update-key.pem
openssl pkey -in craft-update-key.pem -pubout -outform DER | base64 -w0   # the public key
```

One key can serve every craft app, or make one per app.

- **Private key:** in each app repository, *Settings ▸ Environments ▸ release ▸ Environment
  secrets*, add `UPDATE_SIGNING_KEY` with the whole contents of `craft-update-key.pem`. Store a
  backup offline: if it is lost, already-installed copies can no longer update themselves (people
  would have to download one new version by hand).
- **Public key:** add it as the repository *variable* (not secret) `CRAFT_UPDATE_PUBLIC_KEY`
  in the same place. The release workflow passes it to the build, which embeds it. It is not
  secret.

Until both exist, release builds still check for updates and offer the download page, and the
release still builds unsigned.

## 2. What the release workflow does

After `SHA256SUMS.txt` is written, the draft-release job signs it:

```sh
printf '%s\n' "$UPDATE_SIGNING_KEY" > "$RUNNER_TEMP/key.pem"
openssl pkeyutl -sign -inkey "$RUNNER_TEMP/key.pem" -rawin -in SHA256SUMS.txt | base64 -w0 > SHA256SUMS.txt.sig
rm -f "$RUNNER_TEMP/key.pem"
```

and uploads `SHA256SUMS.txt.sig` with the other files. `craft-update`'s tests verify a
signature made exactly this way.

## 3. Code-signing certificates (separate, recommended)

The update signature protects the update path. Operating systems separately warn about
unsigned installers; those certificates are optional and the workflows already use them when
present (see each app's `docs/releasing.md`):

| Platform | What | Secrets |
| --- | --- | --- |
| Windows | an Authenticode code-signing certificate, either a `.pfx` or Azure Trusted Signing | `WINDOWS_CERTIFICATE` + `WINDOWS_CERTIFICATE_PASSWORD`, or `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` |
| macOS | a Developer ID Application certificate and an app-specific password for notarization | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` |
| Linux | none needed | |

Without the Windows certificate, SmartScreen shows "Windows protected your PC" on first install
(More info ▸ Run anyway); in-app updates after that still work.
