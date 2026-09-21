use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Public half of CullFlow AI's signing keypair, embedded in every build so
/// license keys can be verified fully offline (no server round-trip). The
/// matching private key never ships here - it lives only wherever
/// `cullflow-keygen` is run to issue keys (see that crate's README).
///
/// This is a real dev/test keypair generated for this repository; swap it
/// (and the private key held by whoever runs cullflow-keygen) for a fresh
/// one before any real commercial key is ever issued; a private key that
/// only ever existed in this session is not a production secret, but
/// treat it as burned regardless once this code is public.
pub const LICENSE_PUBLIC_KEY_B64: &str = "6wESXj9l7ilqsImGT8kcF4NlnOcYhgoAZ8gn45toxLY";

const KEY_PREFIX: &str = "CFAI1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicenseTier {
    Founding,
    ProFreelancer,
    StudioSuite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LicensePayload {
    pub license_id: String,
    pub customer_email: String,
    pub tier: LicenseTier,
    pub issued_at: DateTime<Utc>,
    /// `None` means perpetual (the Founding Pass's one-time-purchase model).
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LicenseError {
    #[error("license key is malformed")]
    Malformed,
    #[error("license signature is invalid")]
    InvalidSignature,
    #[error("license expired on {0}")]
    Expired(DateTime<Utc>),
}

/// Signs a payload into the distributable license key string. Only ever
/// called from `cullflow-keygen` with the seller's private key - never at
/// runtime inside the desktop app.
pub fn sign_license(payload: &LicensePayload, signing_key: &SigningKey) -> String {
    let payload_json = serde_json::to_vec(payload).expect("LicensePayload always serializes");
    let signature = signing_key.sign(&payload_json);

    format!(
        "{KEY_PREFIX}.{}.{}",
        URL_SAFE_NO_PAD.encode(payload_json),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

/// Verifies a license key string against CullFlow's embedded public key and
/// returns the payload if the signature checks out and it hasn't expired.
pub fn verify_license(key: &str) -> Result<LicensePayload, LicenseError> {
    verify_license_with_key(key, LICENSE_PUBLIC_KEY_B64)
}

/// Same as `verify_license` but against an explicit public key - lets tests
/// (and `cullflow-keygen`'s own self-check) verify against a throwaway
/// keypair instead of the real embedded one.
pub fn verify_license_with_key(
    key: &str,
    public_key_b64: &str,
) -> Result<LicensePayload, LicenseError> {
    let mut parts = key.split('.');
    let (Some(prefix), Some(payload_b64), Some(sig_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(LicenseError::Malformed);
    };
    if prefix != KEY_PREFIX {
        return Err(LicenseError::Malformed);
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| LicenseError::Malformed)?;
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| LicenseError::Malformed)?;
    let sig_bytes: [u8; 64] = sig_bytes.try_into().map_err(|_| LicenseError::Malformed)?;
    let signature = Signature::from_bytes(&sig_bytes);

    let public_key_bytes = URL_SAFE_NO_PAD
        .decode(public_key_b64)
        .map_err(|_| LicenseError::Malformed)?;
    let public_key_bytes: [u8; 32] = public_key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| LicenseError::Malformed)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key_bytes).map_err(|_| LicenseError::Malformed)?;

    verifying_key
        .verify(&payload_bytes, &signature)
        .map_err(|_| LicenseError::InvalidSignature)?;

    let payload: LicensePayload =
        serde_json::from_slice(&payload_bytes).map_err(|_| LicenseError::Malformed)?;

    if let Some(expires_at) = payload.expires_at {
        if Utc::now() > expires_at {
            return Err(LicenseError::Expired(expires_at));
        }
    }

    Ok(payload)
}

/// A stable-ish per-machine identifier, hashed before use so the raw
/// hardware/OS identifier (which can be sensitive) never leaves this
/// function. Used to bind an activated license to the machine it was
/// activated on - see `crates/cullflow-core/src/license.rs` module docs and
/// the README's licensing section for what this does and doesn't protect
/// against.
pub fn machine_fingerprint() -> String {
    let raw = machine_uid::get().unwrap_or_else(|_| "unknown-machine".to_string());
    let hash = Sha256::digest(raw.as_bytes());
    hex_encode(&hash[..8]) // 16 hex chars - short enough to read/report, plenty unique for this purpose
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn test_keypair() -> (SigningKey, String) {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let public_b64 = URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes());
        (signing_key, public_b64)
    }

    fn sample_payload(expires_at: Option<DateTime<Utc>>) -> LicensePayload {
        LicensePayload {
            license_id: "lic_test".into(),
            customer_email: "editor@example.com".into(),
            tier: LicenseTier::Founding,
            issued_at: Utc::now(),
            expires_at,
        }
    }

    #[test]
    fn valid_signed_key_round_trips() {
        let (signing_key, public_b64) = test_keypair();
        let payload = sample_payload(None);
        let key = sign_license(&payload, &signing_key);

        let verified = verify_license_with_key(&key, &public_b64).unwrap();
        assert_eq!(verified.license_id, payload.license_id);
        assert_eq!(verified.customer_email, payload.customer_email);
    }

    #[test]
    fn tampered_payload_fails_signature_check() {
        let (signing_key, public_b64) = test_keypair();
        let key = sign_license(&sample_payload(None), &signing_key);

        // Flip a character inside the payload segment without re-signing.
        let mut parts: Vec<&str> = key.split('.').collect();
        let tampered_payload: String = parts[1]
            .chars()
            .enumerate()
            .map(|(i, c)| if i == 0 { 'X' } else { c })
            .collect();
        parts[1] = &tampered_payload;
        let tampered_key = parts.join(".");

        assert_eq!(
            verify_license_with_key(&tampered_key, &public_b64),
            Err(LicenseError::InvalidSignature)
        );
    }

    #[test]
    fn wrong_public_key_is_rejected() {
        let (signing_key, _) = test_keypair();
        let (_, other_public_b64) = test_keypair();
        let key = sign_license(&sample_payload(None), &signing_key);

        assert_eq!(
            verify_license_with_key(&key, &other_public_b64),
            Err(LicenseError::InvalidSignature)
        );
    }

    #[test]
    fn expired_key_is_rejected() {
        let (signing_key, public_b64) = test_keypair();
        let payload = sample_payload(Some(Utc::now() - Duration::days(1)));
        let key = sign_license(&payload, &signing_key);

        assert!(matches!(
            verify_license_with_key(&key, &public_b64),
            Err(LicenseError::Expired(_))
        ));
    }

    #[test]
    fn not_yet_expired_key_is_accepted() {
        let (signing_key, public_b64) = test_keypair();
        let payload = sample_payload(Some(Utc::now() + Duration::days(365)));
        let key = sign_license(&payload, &signing_key);

        assert!(verify_license_with_key(&key, &public_b64).is_ok());
    }

    #[test]
    fn malformed_strings_are_rejected_not_panicking() {
        for bad in ["", "not-a-key", "CFAI1.onlyonepart", "CFAI1.a.b.c"] {
            assert_eq!(verify_license(bad), Err(LicenseError::Malformed));
        }
    }

    #[test]
    fn machine_fingerprint_is_stable_across_calls() {
        assert_eq!(machine_fingerprint(), machine_fingerprint());
        assert_eq!(machine_fingerprint().len(), 16);
    }
}
