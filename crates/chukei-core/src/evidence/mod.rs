//! Signed evidence bundles, ported from kafka-backup-enterprise's
//! migrations evidence pattern:
//!
//! - The artifact is a **self-contained JSON envelope**: the exact bundle
//!   JSON string, an Ed25519 signature over those verbatim bytes, and the
//!   public key — all in one file, verifiable from any language with no
//!   canonicalisation step.
//! - Signing keys are raw 32-byte Ed25519 seeds on disk (generate with
//!   `chukei evidence keygen`, or via openssl as kafka-backup-enterprise's
//!   scripts do). Absent a configured key, an ephemeral demo-grade key is
//!   generated and flagged in the bundle.
//! - Bundle identity: `{kind}--{subject}--{iso8601-utc}` plus a SHA-256
//!   short hash, mirroring the enterprise attempt-key scheme.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::error::{Error, Result};

/// What was measured/simulated, over which input, with what outcome. The
/// signature covers the serialised form of this struct, verbatim. `report`
/// is kind-specific JSON (`replay-projection` → ReplayReport,
/// `savings-ledger` → SavingsReport).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceBundle {
    /// `{kind}--{subject}--{yyyymmddThhmmssZ}`
    pub bundle_id: String,
    pub kind: String,
    pub tool_version: String,
    pub signed_at: DateTime<Utc>,
    /// True when no signing key was configured and an ephemeral key was
    /// used — fine for demos, not for compliance narratives.
    pub ephemeral_key: bool,
    pub corpus: CorpusFacts,
    pub report: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusFacts {
    pub file: String,
    pub rows: usize,
    /// SHA-256 of the corpus bytes — ties the projection to its input.
    pub sha256_hex: String,
}

/// The detached-signature envelope (the single file written to disk).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEvidence {
    /// The exact JSON string that was signed. Verify against these bytes.
    pub bundle_json: String,
    /// URL-safe base64 (no padding) of the 64-byte Ed25519 signature.
    pub signature_b64: String,
    /// URL-safe base64 (no padding) of the 32-byte public key.
    pub public_key_b64: String,
}

pub fn new_bundle_id(kind: &str, subject: &str, at: DateTime<Utc>) -> String {
    let ts = at.format("%Y%m%dT%H%M%SZ");
    format!("{kind}--{subject}--{ts}")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Load a raw 32-byte Ed25519 seed; generate an ephemeral key when absent.
/// Returns (key, ephemeral?).
pub fn load_signing_key_or_default(path: Option<&Path>) -> Result<(SigningKey, bool)> {
    match path {
        Some(p) => {
            let bytes = std::fs::read(p).map_err(|e| {
                Error::Evidence(format!(
                    "failed to read signing key at {}: {e}",
                    p.display()
                ))
            })?;
            let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                Error::Evidence(format!(
                    "signing key at {} must be exactly 32 bytes (got {})",
                    p.display(),
                    bytes.len()
                ))
            })?;
            Ok((SigningKey::from_bytes(&arr), false))
        }
        None => Ok((SigningKey::generate(&mut rand::rngs::OsRng), true)),
    }
}

pub fn generate_key_file(path: &Path) -> Result<String> {
    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    std::fs::write(path, key.to_bytes())?;
    Ok(URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes()))
}

pub fn sign(bundle: &EvidenceBundle, key: &SigningKey) -> Result<SignedEvidence> {
    let bundle_json = serde_json::to_string_pretty(bundle)?;
    let signature = key.sign(bundle_json.as_bytes());
    Ok(SignedEvidence {
        bundle_json,
        signature_b64: URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        public_key_b64: URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes()),
    })
}

/// Verify the envelope and return the parsed bundle. Any tampering with
/// `bundle_json` — even whitespace — fails.
pub fn verify(signed: &SignedEvidence) -> Result<EvidenceBundle> {
    let pk_bytes = URL_SAFE_NO_PAD
        .decode(&signed.public_key_b64)
        .map_err(|e| Error::Evidence(format!("invalid public-key base64: {e}")))?;
    let pk_arr: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Evidence("public key must be 32 bytes".into()))?;
    let vk = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|e| Error::Evidence(format!("invalid public key: {e}")))?;

    let sig_bytes = URL_SAFE_NO_PAD
        .decode(&signed.signature_b64)
        .map_err(|e| Error::Evidence(format!("invalid signature base64: {e}")))?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Evidence("signature must be 64 bytes".into()))?;
    let signature = Signature::from_bytes(&sig_arr);

    vk.verify(signed.bundle_json.as_bytes(), &signature)
        .map_err(|_| Error::Evidence("signature verification failed".into()))?;
    Ok(serde_json::from_str(&signed.bundle_json)?)
}

/// Short content hash for attempt-style file naming (enterprise pattern:
/// `{millis}-{outcome}-{hash}`).
pub fn short_hash(signed: &SignedEvidence) -> String {
    let mut hasher = Sha256::new();
    hasher.update(signed.bundle_json.as_bytes());
    hasher.update(signed.signature_b64.as_bytes());
    let digest = hasher.finalize();
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{read_csv, simulate, ReplayOptions};

    fn bundle() -> EvidenceBundle {
        let csv = "QUERY_TEXT,WAREHOUSE_SIZE,TOTAL_ELAPSED_TIME\nSELECT a FROM t,XS,1000\n";
        let rows = read_csv(csv.as_bytes()).unwrap();
        let report = simulate(
            &rows,
            &crate::config::Config::default(),
            ReplayOptions::default(),
        );
        EvidenceBundle {
            bundle_id: new_bundle_id("chukei-replay", "test", Utc::now()),
            kind: "replay-projection".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            signed_at: Utc::now(),
            ephemeral_key: true,
            corpus: CorpusFacts {
                file: "test.csv".into(),
                rows: rows.len(),
                sha256_hex: sha256_hex(csv.as_bytes()),
            },
            report: serde_json::to_value(report).unwrap(),
        }
    }

    #[test]
    fn sign_verify_roundtrip() {
        let (key, ephemeral) = load_signing_key_or_default(None).unwrap();
        assert!(ephemeral);
        let signed = sign(&bundle(), &key).unwrap();
        let recovered = verify(&signed).unwrap();
        assert_eq!(recovered.kind, "replay-projection");
    }

    #[test]
    fn tampered_bundle_fails_verification() {
        let (key, _) = load_signing_key_or_default(None).unwrap();
        let mut signed = sign(&bundle(), &key).unwrap();
        signed.bundle_json = signed.bundle_json.replace("replay-projection", "x");
        assert!(verify(&signed).is_err());
        // Even pure whitespace tampering fails — verbatim bytes are signed.
        let mut signed2 = sign(&bundle(), &key).unwrap();
        signed2.bundle_json.push(' ');
        assert!(verify(&signed2).is_err());
    }

    #[test]
    fn key_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("signing.key");
        let pub_b64 = generate_key_file(&key_path).unwrap();
        let (key, ephemeral) = load_signing_key_or_default(Some(&key_path)).unwrap();
        assert!(!ephemeral);
        assert_eq!(
            URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes()),
            pub_b64
        );
        let signed = sign(&bundle(), &key).unwrap();
        verify(&signed).unwrap();
    }

    #[test]
    fn wrong_size_key_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("bad.key");
        std::fs::write(&key_path, [0u8; 31]).unwrap();
        assert!(load_signing_key_or_default(Some(&key_path)).is_err());
    }
}
