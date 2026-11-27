//! Synthetic self-signed TLS pair generation for tests only.

use rcgen::{CertificateParams, KeyPair};

/// A pair made by rcgen, outside the host: `names` as its subject alternative names (an IP
/// literal is an IP address), valid from `from` to `to` (Unix seconds).
pub fn pair_for(names: &[&str], from: i64, to: i64) -> (String, String) {
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("generate key");
    let mut params =
        CertificateParams::new(names.iter().map(|n| (*n).to_string()).collect::<Vec<_>>())
            .expect("certificate params");
    params.not_before = time::OffsetDateTime::from_unix_timestamp(from).expect("not_before");
    params.not_after = time::OffsetDateTime::from_unix_timestamp(to).expect("not_after");
    let cert = params.self_signed(&key).expect("self-signed cert");
    (cert.pem(), key.serialize_pem())
}
