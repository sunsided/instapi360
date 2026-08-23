//! Request signing.
//!
//! Reverse-engineering status (see project plan, Steps 2–3):
//! `studio_worker.dll` computes the signature with **HMAC-SHA256**
//! (OpenSSL `HMAC_Init_ex/Update/Final` + `EVP_sha256`) over an ordered
//! query-style param string. A verbatim template found in the binary:
//!
//! ```text
//! %1?share_type=pc&client_key=%2&nonce_str=%3&timestamp=%4&signature=%5&...
//! ```
//!
//! i.e. `signature = HMAC_SHA256(sign_key, canonical_params)`, hex-encoded.
//!
//! Two things still need confirming against a live capture before signed
//! endpoints will succeed: (1) the exact **key** (`ClientConfig::sign_key`),
//! and (2) the exact **param set + ordering** that goes into the canonical
//! string. This module centralizes both so a single edit finalizes signing.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::config::ClientConfig;
use crate::error::{Error, Result};

type HmacSha256 = Hmac<Sha256>;

/// Parameters that vary per request and feed the canonical string.
#[derive(Debug, Clone)]
pub struct SignParams {
    pub client_key: String,
    pub nonce: String,
    pub timestamp: String,
}

/// A computed signature plus the nonce/timestamp that produced it, so the
/// caller can attach all three to the request consistently.
#[derive(Debug, Clone)]
pub struct Signature {
    pub signature: String,
    pub nonce: String,
    pub timestamp: String,
}

/// Builds request signatures from the static [`ClientConfig`] material.
#[derive(Debug, Clone)]
pub struct Signer {
    app_key: String,
    sign_key: String,
    can_sign: bool,
}

impl Signer {
    pub fn new(cfg: &ClientConfig) -> Self {
        Signer {
            app_key: cfg.app_key.clone(),
            sign_key: cfg.sign_key.clone(),
            can_sign: cfg.can_sign(),
        }
    }

    /// Build the canonical string that gets HMAC'd.
    ///
    /// Param ordering mirrors the template recovered from the binary. This is
    /// the single place to adjust once capture pins down the full field list.
    fn canonical(&self, p: &SignParams) -> String {
        // NOTE: keep keys in the exact recovered order; do NOT sort.
        format!(
            "share_type=pc&client_key={}&nonce_str={}&timestamp={}",
            p.client_key, p.nonce, p.timestamp
        )
    }

    /// Compute `HMAC_SHA256(sign_key, canonical)` as lowercase hex.
    ///
    /// `nonce` and `timestamp` are caller-supplied so they can be reproduced in
    /// tests against captured fixtures; production code uses [`Self::sign_now`].
    pub fn sign(&self, nonce: &str, timestamp: &str) -> Result<Signature> {
        if !self.can_sign {
            return Err(Error::Signing(
                "app_key/sign_key not configured — recover them via capture (plan Steps 2–3)"
                    .into(),
            ));
        }
        let params = SignParams {
            client_key: self.app_key.clone(),
            nonce: nonce.to_string(),
            timestamp: timestamp.to_string(),
        };
        let canonical = self.canonical(&params);
        let mut mac = HmacSha256::new_from_slice(self.sign_key.as_bytes())
            .map_err(|e| Error::Signing(format!("bad hmac key: {e}")))?;
        mac.update(canonical.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        Ok(Signature {
            signature: sig,
            nonce: nonce.to_string(),
            timestamp: timestamp.to_string(),
        })
    }

    /// Whether this signer has the material needed to produce a signature.
    pub fn is_ready(&self) -> bool {
        self.can_sign
    }
}

/// A random-ish nonce string. Uses the timestamp plus a process-local counter;
/// nonces only need to be unique per request, not cryptographically strong.
/// Retained for the share-link endpoints, which do use the HMAC scheme.
#[allow(dead_code)]
pub fn make_nonce(seed: u64) -> String {
    // 16 hex chars derived from the seed via a splitmix step.
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    format!("{x:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(app_key: &str, sign_key: &str) -> ClientConfig {
        let mut c = ClientConfig::windows(crate::config::Region::Global);
        c.app_key = app_key.to_string();
        c.sign_key = sign_key.to_string();
        c
    }

    #[test]
    fn unconfigured_signer_refuses() {
        let signer = Signer::new(&ClientConfig::windows(crate::config::Region::Global));
        assert!(!signer.is_ready());
        assert!(signer.sign("abc", "123").is_err());
    }

    #[test]
    fn hmac_is_deterministic() {
        let signer = Signer::new(&cfg_with("APPKEY", "secret"));
        let a = signer.sign("nonce1", "1700000000").unwrap();
        let b = signer.sign("nonce1", "1700000000").unwrap();
        assert_eq!(a.signature, b.signature);
        // Different nonce -> different signature.
        let c = signer.sign("nonce2", "1700000000").unwrap();
        assert_ne!(a.signature, c.signature);
    }

    // Placeholder for the byte-for-byte fixture test (plan Verification #3):
    // once a real request is captured, assert the recovered algorithm
    // reproduces its `signature` exactly.
    #[test]
    #[ignore = "needs captured request fixture (plan Step 2)"]
    fn reproduces_captured_signature() {
        // let signer = Signer::new(&cfg_with(REAL_APP_KEY, REAL_SIGN_KEY));
        // let s = signer.sign(CAPTURED_NONCE, CAPTURED_TS).unwrap();
        // assert_eq!(s.signature, CAPTURED_SIGNATURE);
    }
}
