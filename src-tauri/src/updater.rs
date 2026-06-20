//! In-app auto-update (PH6-008).
//!
//! Wraps the Tauri updater plugin. Updates are **explicit user action only** —
//! the UI calls [`check_for_updates`], shows the user what's available, and only
//! [`install_update`] (behind a button) downloads + applies it. There is no
//! silent or forced update.
//!
//! Trust: the updater verifies a minisign signature against the PUBLIC key
//! embedded in `tauri.conf.json` (`plugins.updater.pubkey`). The matching
//! private key signs release manifests in CI and is never committed. The
//! `#[cfg(test)]` module below exercises that exact verification with committed
//! fixtures: a valid signature verifies, a tampered manifest is rejected.

use tauri_plugin_updater::UpdaterExt;

/// What the UI needs to render the "update available" prompt.
#[derive(serde::Serialize)]
pub struct UpdateInfo {
    /// Whether a newer version is available at the configured endpoint.
    pub available: bool,
    /// The available version (when `available`).
    pub version: Option<String>,
    /// Release notes for the available version, if any.
    pub notes: Option<String>,
    /// The currently-running version.
    pub current: String,
}

/// Check the signed update endpoint for a newer version. Read-only — never
/// downloads or installs.
#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateInfo {
            available: true,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            current,
        }),
        Ok(None) => Ok(UpdateInfo {
            available: false,
            version: None,
            notes: None,
            current,
        }),
        Err(e) => Err(e.to_string()),
    }
}

/// Download + apply the available update, then relaunch. Only ever invoked by an
/// explicit user click in the UI (PH6-008). The plugin verifies the artifact's
/// signature against the committed public key before applying — a tampered or
/// unsigned artifact is rejected.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available.".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    // Relaunch into the freshly-installed version. `restart` diverges.
    app.restart();
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use minisign_verify::{PublicKey, Signature};

    /// The committed updater PUBLIC key. This is the raw minisign key line; it is
    /// the same key carried in `tauri.conf.json` (`plugins.updater.pubkey`),
    /// which stores the whole `.pub` file base64-encoded. Only the public key is
    /// ever committed — the private key signs manifests in CI.
    const PUBKEY_B64: &str = "RWTQM20+Q2OlHRL3xsW8Gmd1qesgjJmDiuvS8mFToFbICoKqftNXUVvf";

    fn fixture(name: &str) -> Vec<u8> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let bytes = std::fs::read(p).expect("read fixture");
        // Manifests are signed with LF newlines. Windows checkouts may still carry
        // CRLF when core.autocrlf is on and .gitattributes has not been applied
        // yet — normalize so verification matches what minisign signed.
        if name.ends_with(".json") && bytes.contains(&b'\r') {
            return String::from_utf8(bytes)
                .expect("fixture utf-8")
                .replace("\r\n", "\n")
                .into_bytes();
        }
        bytes
    }

    /// Decode a Tauri `.sig` (base64 of a minisign signature file) into a
    /// `Signature`.
    fn load_sig() -> Signature {
        let sig_b64 = String::from_utf8(fixture("latest.json.sig")).unwrap();
        let sig_text = base64::engine::general_purpose::STANDARD
            .decode(sig_b64.trim())
            .expect("base64 decode .sig");
        Signature::decode(&String::from_utf8(sig_text).unwrap()).expect("decode minisign sig")
    }

    #[test]
    fn config_pubkey_matches_committed_key() {
        // The tauri.conf.json pubkey is this key as a base64-encoded .pub file.
        let conf = include_str!("../tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(conf).unwrap();
        let conf_b64 = conf["plugins"]["updater"]["pubkey"].as_str().unwrap();
        let pub_file = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(conf_b64.trim())
                .unwrap(),
        )
        .unwrap();
        // Second line of the .pub file is the raw key — must equal our const.
        let raw = pub_file.lines().nth(1).unwrap().trim();
        assert_eq!(raw, PUBKEY_B64, "test pubkey must match tauri.conf.json");
    }

    #[test]
    fn valid_manifest_signature_verifies() {
        let pk = PublicKey::from_base64(PUBKEY_B64).unwrap();
        let manifest = fixture("latest.json");
        pk.verify(&manifest, &load_sig(), false)
            .expect("a correctly-signed manifest must verify against the committed key");
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let pk = PublicKey::from_base64(PUBKEY_B64).unwrap();
        let mut manifest = fixture("latest.json");
        manifest[0] ^= 0xff; // flip one byte
        assert!(
            pk.verify(&manifest, &load_sig(), false).is_err(),
            "a tampered manifest must NOT verify"
        );
    }
}
