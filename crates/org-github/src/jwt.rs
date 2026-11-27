//! Pure-Rust RS256 GitHub App JWT signer built on `ring::signature::RsaKeyPair`.
//!
//! Accepts both PKCS#8 (`PRIVATE KEY`) and PKCS#1 (`RSA PRIVATE KEY`, GitHub's default download
//! format) RSA private keys (`>= 2048` bits), zeroes temporary PEM/DER buffers via
//! `zeroize::Zeroizing`, and never exposes key material or signed JWTs in `Debug` output.

use std::fmt;
use std::sync::Arc;

use base64::{
    engine::general_purpose::{STANDARD as BASE64_STD, URL_SAFE_NO_PAD as BASE64_URL},
    Engine as _,
};
use ring::rand::SystemRandom;
use ring::signature::{
    RsaKeyPair, UnparsedPublicKey, RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_SHA256,
};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::error::GitHubError;

/// Maximum GitHub App private key PEM bytes accepted (matches `org_pki::wrap::MAX_KEY_BYTES`).
pub const MAX_PRIVATE_KEY_PEM_BYTES: usize = 4096;

/// Minimum RSA modulus size in bytes (2048 bits).
const MIN_RSA_MODULUS_BYTES: usize = 256;

/// Clock-skew backdate applied to JWT `iat` per GitHub App specification (60 seconds).
const JWT_IAT_SKEW_SECS: u64 = 60;

/// Maximum JWT lifetime after `now` per GitHub App specification (540s -> `exp - iat = 600s <= 10m`).
const JWT_EXP_LIFETIME_SECS: u64 = 540;

const PKCS8_LABEL: &str = "PRIVATE KEY";
const PKCS1_LABEL: &str = "RSA PRIVATE KEY";

/// Loaded RSA private key for signing GitHub App RS256 JWTs.
#[derive(Clone)]
pub struct GitHubAppPrivateKey {
    pair: Arc<RsaKeyPair>,
}

impl fmt::Debug for GitHubAppPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubAppPrivateKey")
            .field("modulus_bits", &(self.pair.public().modulus_len() * 8))
            .finish_non_exhaustive()
    }
}

enum PemKind {
    Pkcs8,
    Pkcs1,
}

impl GitHubAppPrivateKey {
    /// Parses a PKCS#8 or PKCS#1 RSA private key PEM (at least 2048 bits) while zeroizing all
    /// intermediate DER buffers on drop.
    pub fn from_pem(pem_bytes: &[u8]) -> Result<Self, GitHubError> {
        if pem_bytes.is_empty() || pem_bytes.len() > MAX_PRIVATE_KEY_PEM_BYTES {
            return Err(GitHubError::InvalidPrivateKey(
                "private key PEM must be 1..=4096 bytes".into(),
            ));
        }
        let pem_text = std::str::from_utf8(pem_bytes).map_err(|_| {
            GitHubError::InvalidPrivateKey("private key PEM must be valid UTF-8".into())
        })?;
        let (kind, der) = decode_pem_der(pem_text)?;
        let pair = match kind {
            PemKind::Pkcs8 => RsaKeyPair::from_pkcs8(&der),
            PemKind::Pkcs1 => RsaKeyPair::from_der(&der),
        }
        .map_err(|_| {
            GitHubError::InvalidPrivateKey(
                "private key is not a valid unencrypted RSA PKCS#1 or PKCS#8 key".into(),
            )
        })?;
        if pair.public().modulus_len() < MIN_RSA_MODULUS_BYTES {
            return Err(GitHubError::InvalidPrivateKey(
                "GitHub App RSA key must be at least 2048 bits".into(),
            ));
        }
        Ok(Self {
            pair: Arc::new(pair),
        })
    }

    /// Raw RSA public key bytes (DER `RSAPublicKey`) for verifying RS256 signatures.
    pub fn public_key_der(&self) -> &[u8] {
        self.pair.public().as_ref()
    }

    /// Generates a short-lived RS256 GitHub App JWT (`iat = now - 60s`, `exp = now + 540s`,
    /// `iss = app_id`) inside a zeroizing buffer.
    pub fn sign_app_jwt(
        &self,
        app_id: u64,
        now_epoch_secs: u64,
    ) -> Result<Zeroizing<String>, GitHubError> {
        if app_id == 0 {
            return Err(GitHubError::InvalidConfig("app_id must be positive".into()));
        }
        #[derive(Serialize)]
        struct JwtHeader<'a> {
            alg: &'a str,
            typ: &'a str,
        }
        #[derive(Serialize)]
        struct JwtClaims {
            iat: u64,
            exp: u64,
            iss: String,
        }
        let header_json = serde_json::to_vec(&JwtHeader {
            alg: "RS256",
            typ: "JWT",
        })
        .map_err(|_| GitHubError::SigningFailed("failed encoding JWT header".into()))?;
        let claims_json = serde_json::to_vec(&JwtClaims {
            iat: now_epoch_secs.saturating_sub(JWT_IAT_SKEW_SECS),
            exp: now_epoch_secs.saturating_add(JWT_EXP_LIFETIME_SECS),
            iss: app_id.to_string(),
        })
        .map_err(|_| GitHubError::SigningFailed("failed encoding JWT claims".into()))?;

        let signing_input = format!(
            "{}.{}",
            BASE64_URL.encode(&header_json),
            BASE64_URL.encode(&claims_json)
        );
        let rng = SystemRandom::new();
        let mut sig = vec![0u8; self.pair.public().modulus_len()];
        self.pair
            .sign(&RSA_PKCS1_SHA256, &rng, signing_input.as_bytes(), &mut sig)
            .map_err(|_| GitHubError::SigningFailed("RSA-SHA256 signing failed".into()))?;

        let mut token = Zeroizing::new(String::with_capacity(
            signing_input.len() + 1 + (sig.len() * 4 / 3) + 4,
        ));
        token.push_str(&signing_input);
        token.push('.');
        token.push_str(&BASE64_URL.encode(&sig));
        Ok(token)
    }
}

/// Cryptographically verifies an RS256 GitHub App JWT against `public_key_der` and returns the
/// decoded JSON claims object.
pub fn verify_app_jwt_rs256(
    jwt: &str,
    public_key_der: &[u8],
) -> Result<serde_json::Value, GitHubError> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(GitHubError::SigningFailed(
            "JWT must have exactly three dot-separated segments".into(),
        ));
    }
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let signature = BASE64_URL
        .decode(parts[2])
        .map_err(|_| GitHubError::SigningFailed("JWT signature is not valid base64url".into()))?;
    let verifier = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public_key_der);
    verifier
        .verify(signing_input.as_bytes(), &signature)
        .map_err(|_| {
            GitHubError::SigningFailed("JWT RS256 signature verification failed".into())
        })?;

    let header_bytes = BASE64_URL
        .decode(parts[0])
        .map_err(|_| GitHubError::SigningFailed("JWT header is not valid base64url".into()))?;
    let header: serde_json::Value = serde_json::from_slice(&header_bytes)
        .map_err(|_| GitHubError::SigningFailed("JWT header is not valid JSON".into()))?;
    if header.get("alg").and_then(serde_json::Value::as_str) != Some("RS256") {
        return Err(GitHubError::SigningFailed("JWT alg must be RS256".into()));
    }
    let payload_bytes = BASE64_URL
        .decode(parts[1])
        .map_err(|_| GitHubError::SigningFailed("JWT payload is not valid base64url".into()))?;
    serde_json::from_slice(&payload_bytes)
        .map_err(|_| GitHubError::SigningFailed("JWT payload is not valid JSON".into()))
}

fn strip_pem_block<'a>(trimmed: &'a str, label: &str) -> Result<Option<&'a str>, GitHubError> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let Some(rest) = trimmed.strip_prefix(&begin) else {
        return Ok(None);
    };
    let inner = rest
        .strip_suffix(&end)
        .ok_or_else(|| GitHubError::InvalidPrivateKey(format!("missing `{label}` PEM footer")))?;
    Ok(Some(inner))
}

fn decode_pem_der(pem: &str) -> Result<(PemKind, Zeroizing<Vec<u8>>), GitHubError> {
    let trimmed = pem.trim();
    let (kind, body) = if let Some(inner) = strip_pem_block(trimmed, PKCS8_LABEL)? {
        (PemKind::Pkcs8, inner)
    } else if let Some(inner) = strip_pem_block(trimmed, PKCS1_LABEL)? {
        (PemKind::Pkcs1, inner)
    } else {
        return Err(GitHubError::InvalidPrivateKey(
            "PEM must be an unencrypted PKCS#8 (`PRIVATE KEY`) or PKCS#1 (`RSA PRIVATE KEY`) block"
                .into(),
        ));
    };
    if body.contains("-----") || body.contains("Proc-Type:") || body.contains("DEK-Info:") {
        return Err(GitHubError::InvalidPrivateKey(
            "encrypted or multi-block PEM keys are not supported".into(),
        ));
    }
    let mut compact = Zeroizing::new(String::with_capacity(body.len()));
    for ch in body.chars() {
        if matches!(ch, '\r' | '\n' | ' ' | '\t') {
            continue;
        }
        if !ch.is_ascii() {
            return Err(GitHubError::InvalidPrivateKey(
                "PEM body contains non-ASCII characters".into(),
            ));
        }
        compact.push(ch);
    }
    if compact.is_empty() {
        return Err(GitHubError::InvalidPrivateKey("PEM body is empty".into()));
    }
    let der = BASE64_STD
        .decode(compact.as_bytes())
        .map_err(|_| GitHubError::InvalidPrivateKey("PEM body is not valid base64".into()))?;
    Ok((kind, Zeroizing::new(der)))
}
