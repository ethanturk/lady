//! `lady-license` — offline license verification + trial accounting (ADR-0007).
//!
//! # NOT DRM — a commercial speed bump, not a security boundary
//!
//! Per ADR-0007 this is a **client-side** check against an **embedded public
//! key**. A determined attacker can patch the binary; that is accepted. The
//! gate exists to keep honest users on the paid tier — nothing more.
//!
//! Therefore, by deliberate design:
//! - **Never gate a security-sensitive path behind a license check.** License
//!   state must only toggle *feature access*, never authentication, signing,
//!   credential handling, or any safety-relevant code path.
//! - **Never embed a secret that the gate is meant to protect.** The embedded
//!   key is a *public* verifying key; the signing private key stays offline.
//!
//! A license is `base64(payload_json) "." base64(ed25519_signature)`. The
//! signature covers the canonical payload JSON bytes.

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// How long the free trial lasts.
pub const TRIAL_DAYS: i64 = 30;

/// Seconds in a day.
const DAY_SECONDS: i64 = 86_400;

/// The product identifier a license must carry to be valid for this binary.
pub const PRODUCT: &str = "lady";

/// The embedded Ed25519 **public** verifying key (32 bytes).
///
/// DEV PLACEHOLDER (RFC 8032 test-vector key): replaced with the real product
/// key at release time. The matching signing private key is held offline by the
/// business and never appears in this repository (ADR-0007).
pub const EMBEDDED_PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// Errors from license verification.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// The license string is malformed (bad shape / base64 / JSON).
    #[error("malformed license")]
    Format,
    /// The signature did not verify against the public key.
    #[error("invalid license signature")]
    Signature,
    /// The license is for a different product.
    #[error("license is for a different product")]
    WrongProduct,
    /// The license has expired.
    #[error("license has expired")]
    Expired,
}

/// Result alias for license operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The signed contents of a license.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicensePayload {
    /// The product this license is valid for (must equal [`PRODUCT`]).
    pub product: String,
    /// Who the license was issued to (display only).
    pub licensee: String,
    /// Expiry as Unix seconds; `0` means perpetual.
    pub expiry: i64,
}

/// The app's licensing state, surfaced to the UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum LicenseStatus {
    /// In the free trial with `days_left` remaining.
    Trial { days_left: i64 },
    /// Trial elapsed and no valid license — the main UI is gated.
    Expired,
    /// A valid license is active.
    Licensed { licensee: String },
}

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// Verify a `license` string against `public_key`, requiring `expected_product`
/// and a non-expired payload at `now` (Unix seconds). Returns the payload on
/// success.
pub fn verify(
    license: &str,
    public_key: &[u8; 32],
    expected_product: &str,
    now: i64,
) -> Result<LicensePayload> {
    let (payload_b64, sig_b64) = license.split_once('.').ok_or(Error::Format)?;
    let payload_bytes = B64.decode(payload_b64.trim()).map_err(|_| Error::Format)?;
    let sig_bytes = B64.decode(sig_b64.trim()).map_err(|_| Error::Format)?;

    let sig_arr: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| Error::Format)?;
    let signature = Signature::from_bytes(&sig_arr);
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| Error::Signature)?;
    key.verify(&payload_bytes, &signature)
        .map_err(|_| Error::Signature)?;

    let payload: LicensePayload =
        serde_json::from_slice(&payload_bytes).map_err(|_| Error::Format)?;
    if payload.product != expected_product {
        return Err(Error::WrongProduct);
    }
    if payload.expiry != 0 && payload.expiry <= now {
        return Err(Error::Expired);
    }
    Ok(payload)
}

/// Verify against the embedded product key + [`PRODUCT`]. The app entry point.
pub fn verify_embedded(license: &str, now: i64) -> Result<LicensePayload> {
    verify(license, &EMBEDDED_PUBLIC_KEY, PRODUCT, now)
}

/// Days remaining in a trial that started at `started` (Unix seconds), given a
/// `total_days` length, evaluated at `now`. Saturates at 0; can be 0 on the
/// boundary day.
pub fn trial_days_left(started: i64, now: i64, total_days: i64) -> i64 {
    let elapsed_days = (now - started).max(0) / DAY_SECONDS;
    (total_days - elapsed_days).max(0)
}

/// Evaluate the overall [`LicenseStatus`]: a valid license wins; otherwise the
/// trial countdown decides Trial vs Expired.
pub fn evaluate(
    license: Option<&str>,
    public_key: &[u8; 32],
    product: &str,
    trial_started: i64,
    now: i64,
) -> LicenseStatus {
    if let Some(lic) = license {
        if let Ok(payload) = verify(lic, public_key, product, now) {
            return LicenseStatus::Licensed {
                licensee: payload.licensee,
            };
        }
    }
    let left = trial_days_left(trial_started, now, TRIAL_DAYS);
    if left > 0 {
        LicenseStatus::Trial { days_left: left }
    } else {
        LicenseStatus::Expired
    }
}

/// Evaluate using the embedded key + product. The app entry point.
pub fn evaluate_embedded(license: Option<&str>, trial_started: i64, now: i64) -> LicenseStatus {
    evaluate(license, &EMBEDDED_PUBLIC_KEY, PRODUCT, trial_started, now)
}

/// Build a license string by signing `payload` with `signing_key`. Used by the
/// (offline) issuing tool and by tests; the production private key never lives
/// in this repository.
pub fn sign(payload: &LicensePayload, signing_key: &ed25519_dalek::SigningKey) -> String {
    use ed25519_dalek::Signer;
    let payload_bytes =
        serde_json::to_vec(payload).expect("serialize payload to Vec<u8> is infallible");
    let signature = signing_key.sign(&payload_bytes);
    format!(
        "{}.{}",
        B64.encode(&payload_bytes),
        B64.encode(signature.to_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn keypair() -> (SigningKey, [u8; 32]) {
        let sk = SigningKey::generate(&mut OsRng);
        let pk = sk.verifying_key().to_bytes();
        (sk, pk)
    }

    fn payload(expiry: i64) -> LicensePayload {
        LicensePayload {
            product: PRODUCT.to_string(),
            licensee: "Ada Lovelace".to_string(),
            expiry,
        }
    }

    #[test]
    fn valid_license_verifies() {
        let (sk, pk) = keypair();
        let lic = sign(&payload(0), &sk); // perpetual
        let got = verify(&lic, &pk, PRODUCT, 1_700_000_000).expect("verify");
        assert_eq!(got.licensee, "Ada Lovelace");
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let (sk, pk) = keypair();
        let lic = sign(&payload(0), &sk);
        // Flip a character in the payload section before the dot.
        let dot = lic.find('.').unwrap();
        let mut bytes = lic.into_bytes();
        bytes[dot / 2] ^= 0x01;
        let tampered = String::from_utf8(bytes).unwrap();
        let err = verify(&tampered, &pk, PRODUCT, 1_700_000_000).unwrap_err();
        // Corrupting the payload breaks either base64/JSON or the signature.
        assert!(
            matches!(err, Error::Signature | Error::Format),
            "got {err:?}"
        );
    }

    #[test]
    fn wrong_signing_key_is_rejected() {
        let (sk, _pk) = keypair();
        let (_sk2, other_pk) = keypair();
        let lic = sign(&payload(0), &sk);
        assert_eq!(
            verify(&lic, &other_pk, PRODUCT, 1_700_000_000),
            Err(Error::Signature)
        );
    }

    #[test]
    fn expired_license_is_rejected() {
        let (sk, pk) = keypair();
        let lic = sign(&payload(1_000), &sk); // expired long ago
        assert_eq!(
            verify(&lic, &pk, PRODUCT, 1_700_000_000),
            Err(Error::Expired)
        );
    }

    #[test]
    fn wrong_product_is_rejected() {
        let (sk, pk) = keypair();
        let p = LicensePayload {
            product: "other-app".to_string(),
            licensee: "x".to_string(),
            expiry: 0,
        };
        let lic = sign(&p, &sk);
        assert_eq!(
            verify(&lic, &pk, PRODUCT, 1_700_000_000),
            Err(Error::WrongProduct)
        );
    }

    #[test]
    fn trial_countdown_math() {
        let start = 1_700_000_000;
        assert_eq!(trial_days_left(start, start, 30), 30);
        assert_eq!(trial_days_left(start, start + 5 * DAY_SECONDS, 30), 25);
        assert_eq!(trial_days_left(start, start + 30 * DAY_SECONDS, 30), 0);
        assert_eq!(trial_days_left(start, start + 99 * DAY_SECONDS, 30), 0);
    }

    #[test]
    fn evaluate_transitions_trial_expired_licensed() {
        let (sk, pk) = keypair();
        let start = 1_700_000_000;

        // Within trial, no license → Trial.
        assert_eq!(
            evaluate(None, &pk, PRODUCT, start, start + 3 * DAY_SECONDS),
            LicenseStatus::Trial { days_left: 27 }
        );
        // Past trial, no license → Expired.
        assert_eq!(
            evaluate(None, &pk, PRODUCT, start, start + 40 * DAY_SECONDS),
            LicenseStatus::Expired
        );
        // Valid license even past trial → Licensed.
        let lic = sign(&payload(0), &sk);
        assert_eq!(
            evaluate(Some(&lic), &pk, PRODUCT, start, start + 40 * DAY_SECONDS),
            LicenseStatus::Licensed {
                licensee: "Ada Lovelace".to_string()
            }
        );
    }
}
