# Mobile (iOS / Android)

Lady builds as an iOS and Android app via [Tauri 2's mobile
support](https://tauri.app/develop/). The UI is **adaptive**: a stacked,
top-to-bottom flow on phones (narrow, `< 768px`) and the full side-by-side pane
layout on tablets and foldables (`>= 768px`).

> **Scope.** Full Git functionality on small phone screens is *not* a goal.
> Phones get a degraded-but-usable layout; tablets/foldables get the full
> experience. Several backend operations are desktop-only and **error at
> runtime on mobile** — see [Limitations](#limitations).

---

## Prerequisites

Install the Tauri CLI and the platform toolchains.

```sh
cargo install tauri-cli --version "^2"   # provides `cargo tauri …`
```

### iOS

- **Xcode** (full install, not just Command Line Tools) + an iOS Simulator
  runtime, from the Mac App Store.
- Rust targets:

  ```sh
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
  ```

- A signing team for on-device builds. Tauri reads it from the
  `TAURI_APPLE_DEVELOPMENT_TEAM` environment variable (your 10-character Apple
  Team ID); the Simulator does not require it.

### Android

- **Android Studio**, then via its SDK Manager: the **Android SDK**, **NDK**,
  and **SDK Command-line Tools**.
- Environment: `ANDROID_HOME` (SDK path) and `NDK_HOME` (the chosen NDK
  revision under `$ANDROID_HOME/ndk/<version>`); a JDK 17+ on `PATH`.
- Rust targets:

  ```sh
  rustup target add aarch64-linux-android armv7-linux-androideabi \
      i686-linux-android x86_64-linux-android
  ```

---

## Build & run

The native projects under `src-tauri/gen/apple` and `src-tauri/gen/android` are
**generated** (and git-ignored). Generate them once per checkout:

```sh
cargo tauri ios init        # creates src-tauri/gen/apple
cargo tauri android init    # creates src-tauri/gen/android
```

Then run with hot-reload, or build a package:

```sh
cargo tauri ios dev         # Simulator (or `--host` for a tethered device)
cargo tauri android dev     # emulator or attached device

cargo tauri ios build       # .ipa / app archive
cargo tauri android build   # .apk / .aab
```

`tauri ios/android dev` sets `TAURI_DEV_HOST` to your machine's LAN IP so a
physical device can reach the Vite dev server and its HMR socket;
[`ui/vite.config.ts`](../ui/vite.config.ts) honours it. Desktop dev
(`cargo tauri dev`) is unaffected.

---

## How the platform split works

- **Entry point.** `run()` in [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs)
  is annotated `#[cfg_attr(mobile, tauri::mobile_entry_point)]` so the mobile
  runtime can call it.
- **Updater.** The desktop-only `tauri-plugin-updater` is gated three ways so it
  never compiles or is referenced on mobile:
  - the dependency lives under
    `[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]`
    in `Cargo.toml` (Cargo can't evaluate Tauri's build-time `desktop` cfg in a
    target table, so the real `target_os` predicate is used);
  - `mod updater;`, the plugin registration, and the two updater commands are
    behind `#[cfg(desktop)]`;
  - the `updater:default` permission moved out of `capabilities/default.json`
    into `capabilities/desktop.json`, which is scoped to
    `"platforms": ["macOS", "windows", "linux"]`.
- **Bundle config.** `tauri.conf.json` sets `bundle.android.minSdkVersion = 24`
  and `bundle.iOS.minimumSystemVersion = "13.0"`. The existing
  `dev.lady.client` identifier is valid for both stores.

---

## Release pipeline (CI)

[`.github/workflows/release.yml`](../.github/workflows/release.yml) builds mobile
alongside the desktop bundles on a `v*` tag push (or a manual `workflow_dispatch`
dry run). Both mobile jobs run after the shared `gate` job and **degrade
gracefully** — with no secrets they still build and publish nothing, matching the
fork-friendly desktop legs.

The native projects are **generated fresh each run** (`tauri android/ios init`) —
nothing under `src-tauri/gen/` is committed. The job syncs `tauri.conf.json`'s
`version` from the tag so `versionName`/`versionCode` (Android) and the IPA
version (iOS) track the release.

### `android` job — signed APK → Firebase App Distribution

Ubuntu runner: Android SDK + NDK + `cargo-ndk` → `tauri android init` → inject
release signing into the generated Gradle project (per Tauri's *Sign Android*
guide; errors loudly if the template shape changes) → `tauri android build
--apk`. The universal APK is always uploaded as a workflow artifact; with
Firebase secrets it also ships to App Distribution (real releases only, not dry
runs). Without signing secrets it builds an **unsigned** APK and skips upload.

| Secret | Purpose |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | base64 of the release keystore (`.jks`) |
| `ANDROID_KEYSTORE_PASSWORD` | keystore (store) password |
| `ANDROID_KEY_ALIAS` | signing key alias |
| `ANDROID_KEY_PASSWORD` | signing key password |
| `FIREBASE_ANDROID_APP_ID` | Firebase Android app id (`1:123:android:abcd`) |
| `FIREBASE_SERVICE_ACCOUNT` | Firebase service-account JSON (App Distribution Admin) |
| `FIREBASE_TESTER_GROUPS` *(var)* | comma-separated tester groups (default `testers`) |

### `ios` job — signed IPA → TestFlight

macOS runner: `tauri ios init`, then one of two paths. **Without** Apple secrets
it runs a cheap unsigned cross-compile check (`cargo build --target
aarch64-apple-ios`) so mobile breakage is caught on every tag, and publishes
nothing. **With** them it imports the cert + provisioning profile into a
temporary keychain, runs `tauri ios build --export-method app-store-connect`,
uploads the IPA artifact, and — given an App Store Connect API key, on a real
release — pushes to TestFlight via `xcrun altool`.

| Secret | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | base64 of the Apple Distribution `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password |
| `APPLE_TEAM_ID` | 10-char Apple Developer Team ID |
| `IOS_PROVISIONING_PROFILE_BASE64` | base64 of the App Store `.mobileprovision` |
| `ASC_KEY_ID` / `ASC_ISSUER_ID` | App Store Connect API key id + issuer |
| `ASC_PRIVATE_KEY` | App Store Connect API private key (`.p8`) |

> **Validate on a real run.** Because this environment has no Xcode/Android SDK,
> the signing/distribution paths were authored but not executed end-to-end. The
> no-secret build/compile paths are the safe baseline; run a `workflow_dispatch`
> dry run with your secrets set to confirm signing before tagging a release. If
> a future Tauri version reshapes the generated Gradle file, the Android signing
> step fails loudly rather than shipping an unsigned APK.

---

## Limitations

These work on desktop but **fail at runtime on iOS/Android** (no system `git`,
no process spawning on iOS). This is accepted under the "not fully functional on
phones" scope and is not worked around:

- **Process-based Git ops** — clone / fetch / pull / push, custom commands, and
  external diff/merge tools shell out to the system `git` binary.
- **Open / reveal** — `open_url`, `open_path`, `reveal_path`.
- **Auto-update** — desktop-only by construction (see above).

### Known follow-up risks

- **Hosting token store.** Tokens are kept in the OS keyring via
  `lady-hosting`'s `KeyringStore`. `keyring` Android support should be verified
  at `init`/build time; a mobile fallback may be needed.
- **Touch affordances.** `GraphView` multi-select needs Cmd/Shift-click — there
  is no touch equivalent yet; single tap-to-select still works.
