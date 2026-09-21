use cullflow_core::license::{self, LicensePayload, LicenseTier};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct StoredActivation {
    key: String,
    machine_fingerprint: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LicenseStatus {
    /// No key has ever been activated on this machine.
    NotActivated { machine_fingerprint: String },
    /// A key is activated, cryptographically valid, not expired, and bound
    /// to this exact machine.
    Active {
        machine_fingerprint: String,
        license_id: String,
        customer_email: String,
        tier: LicenseTier,
    },
    /// A key was activated but no longer checks out - could be a tampered
    /// local file, an expired key, or (most commonly) the activation file
    /// was copied over from a different machine.
    Invalid {
        machine_fingerprint: String,
        reason: String,
    },
}

fn license_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join("license.json")
}

/// Verifies `key` cryptographically and, if valid, binds it to this
/// machine's fingerprint and stores that pairing locally. Activating a
/// second key overwrites the first.
pub fn activate(config_dir: &Path, key: &str) -> Result<LicenseStatus, String> {
    let payload = license::verify_license(key).map_err(|e| e.to_string())?;
    let fingerprint = license::machine_fingerprint();

    let stored = StoredActivation {
        key: key.to_string(),
        machine_fingerprint: fingerprint.clone(),
    };
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
    std::fs::write(license_file_path(config_dir), json).map_err(|e| e.to_string())?;

    Ok(status_from_payload(fingerprint, payload))
}

/// Loads and re-validates whatever activation is stored locally, if any.
/// Re-checking on every call (rather than trusting the stored file blindly)
/// means an expired key or a copied-over activation file from another
/// machine gets caught immediately, not just at the next `activate` call.
pub fn current_status(config_dir: &Path) -> LicenseStatus {
    let current_fingerprint = license::machine_fingerprint();

    let Ok(json) = std::fs::read_to_string(license_file_path(config_dir)) else {
        return LicenseStatus::NotActivated {
            machine_fingerprint: current_fingerprint,
        };
    };
    let Ok(stored) = serde_json::from_str::<StoredActivation>(&json) else {
        return LicenseStatus::Invalid {
            machine_fingerprint: current_fingerprint,
            reason: "local license file is corrupted".to_string(),
        };
    };

    if stored.machine_fingerprint != current_fingerprint {
        return LicenseStatus::Invalid {
            machine_fingerprint: current_fingerprint,
            reason: "this license was activated on a different machine".to_string(),
        };
    }

    match license::verify_license(&stored.key) {
        Ok(payload) => status_from_payload(current_fingerprint, payload),
        Err(e) => LicenseStatus::Invalid {
            machine_fingerprint: current_fingerprint,
            reason: e.to_string(),
        },
    }
}

pub fn is_active(config_dir: &Path) -> bool {
    matches!(current_status(config_dir), LicenseStatus::Active { .. })
}

fn status_from_payload(machine_fingerprint: String, payload: LicensePayload) -> LicenseStatus {
    LicenseStatus::Active {
        machine_fingerprint,
        license_id: payload.license_id,
        customer_email: payload.customer_email,
        tier: payload.tier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cullflow_core::license::sign_license;
    use ed25519_dalek::SigningKey;
    use tempfile::tempdir;

    // These tests activate against CullFlow's real embedded public key by
    // signing with a throwaway keypair and swapping in via env would need
    // the module to accept an override, which it intentionally doesn't (the
    // point is there's exactly one trusted key). So this suite instead
    // covers the storage/status logic using `license::LICENSE_PUBLIC_KEY_B64`
    // through the crate's own test keypair helper indirectly isn't
    // possible - these tests exercise the not-activated and corrupted-file
    // paths, which don't depend on a real signature.

    #[test]
    fn no_file_means_not_activated() {
        let dir = tempdir().unwrap();
        assert!(matches!(
            current_status(dir.path()),
            LicenseStatus::NotActivated { .. }
        ));
        assert!(!is_active(dir.path()));
    }

    #[test]
    fn corrupted_file_is_invalid_not_a_panic() {
        let dir = tempdir().unwrap();
        std::fs::write(license_file_path(dir.path()), "not json").unwrap();
        assert!(matches!(
            current_status(dir.path()),
            LicenseStatus::Invalid { .. }
        ));
    }

    #[test]
    fn garbage_key_in_activate_is_rejected() {
        let dir = tempdir().unwrap();
        assert!(activate(dir.path(), "not-a-real-key").is_err());
        assert!(!is_active(dir.path()));
    }

    #[test]
    fn mismatched_machine_fingerprint_is_invalid() {
        let dir = tempdir().unwrap();
        // A syntactically well-formed but unsigned-by-our-key license won't
        // verify against the embedded public key either, so simulate the
        // "copied from another machine" path directly at the storage layer.
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let payload = LicensePayload {
            license_id: "lic_x".into(),
            customer_email: "a@b.com".into(),
            tier: LicenseTier::Founding,
            issued_at: Utc::now(),
            expires_at: None,
        };
        let key = sign_license(&payload, &signing_key);
        let stored = StoredActivation {
            key,
            machine_fingerprint: "not-this-machine".to_string(),
        };
        std::fs::write(
            license_file_path(dir.path()),
            serde_json::to_string(&stored).unwrap(),
        )
        .unwrap();

        assert!(matches!(
            current_status(dir.path()),
            LicenseStatus::Invalid { .. }
        ));
    }
}
