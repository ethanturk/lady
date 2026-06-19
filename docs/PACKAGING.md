# Lady — Packaging, signing & auto-update (PH6-005 … PH6-008)

Lady ships via the **Tauri v2 bundler** (ADR-0001). The bundle config lives in
[src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json) (`bundle.active = true`,
`targets = "all"`). All signing/notarization/updater **secrets come from CI
secrets or the OS keychain — never the repo**. Only the updater PUBLIC key is
committed (in `tauri.conf.json`). When secrets are absent (forks, PRs), the
build still produces **unsigned** artifacts and skips signing gracefully.

Build locally with:

```sh
cargo tauri build            # native targets for the current OS
cargo tauri build --debug    # faster, unsigned
```

## macOS — `.app` + `.dmg`, notarized (PH6-005)

Targets: `app`, `dmg`. Code-signing + notarization are driven entirely by env
vars the bundler reads; set them as CI secrets:

| Env var | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | base64 of the Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarization (app-specific password) |

(or `APPLE_API_KEY` / `APPLE_API_ISSUER` / `APPLE_API_KEY_PATH` for a notarytool
API key instead of Apple ID). With these set, `cargo tauri build` signs with the
Developer ID identity, submits to Apple notarization, and **staples** the ticket
to the `.dmg`/`.app`.

Verify Gatekeeper acceptance:

```sh
spctl --assess --type execute --verbose "Lady.app"      # → "accepted, source=Notarized Developer ID"
xcrun stapler validate "Lady.dmg"                        # → "The validate action worked!"
codesign --verify --deep --strict --verbose=2 "Lady.app"
```

Without the secrets the runner produces an **unsigned** `.app`/`.dmg` (PR builds);
signing/notarization are skipped, not failed.

## Windows — `.msi` / NSIS, Authenticode (PH6-006)

Targets: `nsis` (and/or `msi`). Authenticode signing options:

- **Azure Trusted Signing** (recommended, no cert in CI) — configure
  `bundle.windows.signCommand` / Trusted Signing action, or
- a **PFX cert** stored as a CI secret, signed post-bundle with `signtool`.

The release workflow signs the produced installers only when the cert secret is
present; otherwise it ships unsigned. Verify:

```powershell
signtool verify /pa /v Lady_1.3.0_x64-setup.exe   # → "Successfully verified"
```

## Linux — AppImage + Flatpak (PH6-007)

- **AppImage** comes straight from the Tauri bundler (`appimage` target). It
  bundles the app; the host provides WebKitGTK 2 (`libwebkit2gtk-4.1`) — the CI
  runner installs `libwebkit2gtk-4.1-dev` + `libgtk-3-dev` before building.
- **Flatpak** is built from [flatpak/dev.lady.client.yml](../flatpak/dev.lady.client.yml)
  with the `org.gnome.Platform` runtime (which supplies WebKitGTK + GTK, so the
  runtime deps are correct inside the sandbox). The release workflow stages the
  built binary + `.desktop` + icon next to the manifest, then runs
  `flatpak-builder`.

Linux has no code-signing requirement; integrity is provided by the signed
updater manifest (below). Smoke test: `./Lady_*.AppImage` launches to the repo
manager; `flatpak run dev.lady.client` does the same.

## Auto-update — Tauri updater, signed manifests (PH6-008)

- The app embeds the updater **public key** (`plugins.updater.pubkey`) and polls
  the signed manifest at `plugins.updater.endpoints` (GitHub Releases
  `latest.json`).
- Each release artifact is signed with the updater **private key**
  (`TAURI_SIGNING_PRIVATE_KEY` + `..._PASSWORD` CI secrets) by the bundler when
  `createUpdaterArtifacts = true`. The signature is written into `latest.json`.
- The client verifies the signature against the embedded public key before
  applying anything — a tampered or unsigned manifest/artifact is **rejected**.
  This is exercised offline by the unit tests in
  [src-tauri/src/updater.rs](../src-tauri/src/updater.rs) against committed
  fixtures (`src-tauri/tests/fixtures/latest.json{,.sig}`).
- Updates are **explicit user action only**: Settings → Updates → "Check for
  updates", then "Download & install". There is no silent or forced update.

### Generating updater keys (one-time, kept out of the repo)

```sh
cargo tauri signer generate -w lady_updater.key      # prints the public key
# → put the PUBLIC key in tauri.conf.json plugins.updater.pubkey
# → store the PRIVATE key + password as the CI secrets TAURI_SIGNING_PRIVATE_KEY[_PASSWORD]
```
