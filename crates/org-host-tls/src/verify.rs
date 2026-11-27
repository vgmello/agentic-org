//! The certificate verifier: `tls.set`'s checks 3 to 6 (X-02 §6.2), in that order, the first
//! failure answered. Used for a supplied pair at setup and at the first start, for every
//! stored pair at a start and at `tls.fetch`, and for the pair the instance makes itself.

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_host_control::listen::{ListenOrigin, OriginHost, OriginScheme, Uncovered};
use org_host_control::ops::{TlsRefusal, UtcTimestamp};
use org_host_control::pem::{self, Der};
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};
use zeroize::Zeroizing;

use super::{Checked, Fingerprint};

/// The longest chain the registry keeps, in its normal form (`web_tls_pair.cert_pem`).
pub const CHAIN_LIMIT: usize = 65_536;

/// Upper bound on normalized PEM key bytes matching `org_host::host_secrets::MAX_BYTES`.
pub const MAX_KEY_BYTES: usize = 16 * 1024;

struct ParsedChain {
    ders: Vec<Der>,
    normal_pem: String,
    chain_digest: [u8; 32],
    leaf: Fingerprint,
    names: Vec<Name>,
    not_before: i64,
    not_after: i64,
}

fn parse_and_normalize_chain(cert: &str) -> Result<ParsedChain, TlsRefusal> {
    let chain = pem::all_blocks(cert, pem::CERTIFICATE).map_err(invalid)?;
    for der in &chain {
        let (rest, _) = X509Certificate::from_der(der.bytes()).map_err(invalid)?;
        if !rest.is_empty() {
            return Err(TlsRefusal::TlsInvalid);
        }
    }
    let (_, leaf_cert) = X509Certificate::from_der(chain[0].bytes()).map_err(invalid)?;
    let names = subject_alternative_names(&leaf_cert)?;
    let validity = leaf_cert.validity();
    let (not_before, not_after) = (
        validity.not_before.timestamp(),
        validity.not_after.timestamp(),
    );
    let mut chain_hash = Sha256::new();
    let mut normal_pem = String::new();
    for der in &chain {
        let len = u32::try_from(der.bytes().len()).map_err(invalid)?;
        chain_hash.update(len.to_be_bytes());
        chain_hash.update(der.bytes());
        encode(&mut normal_pem, "CERTIFICATE", der.bytes());
    }
    if normal_pem.len() > CHAIN_LIMIT {
        return Err(TlsRefusal::TlsInvalid);
    }
    let leaf = Fingerprint(Sha256::digest(chain[0].bytes()).into());
    Ok(ParsedChain {
        ders: chain,
        normal_pem,
        chain_digest: chain_hash.finalize().into(),
        leaf,
        names,
        not_before,
        not_after,
    })
}

fn parse_and_normalize_key(
    key_pem: &str,
) -> Result<(PrivateKeyDer<'static>, Zeroizing<String>), TlsRefusal> {
    // The PEM reader frames the key as the fingerprint does; the typed reader (the web
    // process's) must find the same block, or the two would serve and fingerprint different
    // keys.
    let framed = pem::first_block(key_pem, pem::PRIVATE_KEY).map_err(invalid)?;
    let typed = PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).map_err(invalid)?;
    if typed.secret_der() != framed.bytes() {
        return Err(TlsRefusal::TlsInvalid);
    }
    let label = match &typed {
        PrivateKeyDer::Pkcs8(_) => "PRIVATE KEY",
        PrivateKeyDer::Pkcs1(_) => "RSA PRIVATE KEY",
        PrivateKeyDer::Sec1(_) => "EC PRIVATE KEY",
        _ => return Err(TlsRefusal::TlsInvalid),
    };
    // Sized up front, so the text is never moved to a larger buffer and left behind.
    let der = typed.secret_der();
    let mut normal = Zeroizing::new(String::with_capacity(
        der.len() * 4 / 3 + der.len() / 32 + 128,
    ));
    encode(&mut normal, label, der);
    if normal.len() > MAX_KEY_BYTES {
        return Err(TlsRefusal::TlsInvalid);
    }
    Ok((typed, normal))
}

fn verify_key_matches_leaf(ders: &[Der], typed: &PrivateKeyDer<'static>) -> Result<(), TlsRefusal> {
    let provider = rustls::crypto::ring::default_provider();
    let signing = provider
        .key_provider
        .load_private_key(typed.clone_key())
        .map_err(invalid)?;
    let certificates: Vec<CertificateDer<'static>> = ders
        .iter()
        .map(|der| CertificateDer::from(der.bytes().to_vec()))
        .collect();
    match rustls::sign::CertifiedKey::new(certificates, Arc::clone(&signing)).keys_match() {
        Ok(()) => Ok(()),
        Err(rustls::Error::InconsistentKeys(rustls::InconsistentKeys::KeyMismatch)) => {
            Err(TlsRefusal::TlsKeyMismatch)
        }
        Err(_) => Err(TlsRefusal::TlsInvalid),
    }
}

fn check_validity_window(
    not_before: i64,
    not_after: i64,
    now: Option<i64>,
) -> Result<(), TlsRefusal> {
    if let Some(now) = now {
        if now < not_before || now > not_after {
            let stamp = |secs| {
                super::utc(secs)
                    .and_then(|text| UtcTimestamp::parse(&text).ok())
                    .ok_or(TlsRefusal::TlsInvalid)
            };
            return Err(TlsRefusal::TlsNotValid {
                not_before: stamp(not_before)?,
                not_after: stamp(not_after)?,
            });
        }
    }
    Ok(())
}

fn check_origin_coverage(names: &[Name], origins: &[&ListenOrigin]) -> Result<(), TlsRefusal> {
    let mut uncovered: Vec<ListenOrigin> = Vec::new();
    for origin in origins {
        if origin.scheme() == OriginScheme::Https
            && !covers(names, origin.host())
            && !uncovered.contains(origin)
        {
            uncovered.push((*origin).clone());
        }
    }
    if !uncovered.is_empty() {
        return Err(TlsRefusal::TlsNameMismatch {
            uncovered: Uncovered::new(uncovered).map_err(invalid)?,
        });
    }
    Ok(())
}

/// Checks the pair `cert` (a PEM chain, leaf first) and `key` (PEM), in contract order:
/// 1. PEM framing + X.509/key parse (`tls_invalid`),
/// 2. key matches leaf (`tls_key_mismatch`),
/// 3. validity at `now` (`tls_not_valid`),
/// 4. SAN coverage of every `https` origin (`tls_name_mismatch`).
///
/// Never quotes certificate or key bytes in an error.
pub fn check(
    cert: &str,
    key: &str,
    origins: &[&ListenOrigin],
    now: Option<i64>,
) -> Result<Checked, TlsRefusal> {
    let chain = parse_and_normalize_chain(cert)?;
    let (typed, key_normal) = parse_and_normalize_key(key)?;
    verify_key_matches_leaf(&chain.ders, &typed)?;
    check_validity_window(chain.not_before, chain.not_after, now)?;
    check_origin_coverage(&chain.names, origins)?;
    let mut pair = Sha256::new();
    pair.update(chain.chain_digest);
    pair.update(Sha256::digest(typed.secret_der()));
    Ok(Checked {
        chain: chain.normal_pem,
        key: key_normal,
        pair: pair.finalize().into(),
        leaf: chain.leaf,
        not_before: chain.not_before,
        not_after: chain.not_after,
    })
}

fn invalid<E>(_: E) -> TlsRefusal {
    TlsRefusal::TlsInvalid
}

enum Name {
    Dns(String),
    Ip(Vec<u8>),
}

fn subject_alternative_names(leaf: &X509Certificate<'_>) -> Result<Vec<Name>, TlsRefusal> {
    let Some(extension) = leaf
        .subject_alternative_name()
        .map_err(|_| TlsRefusal::TlsInvalid)?
    else {
        return Ok(Vec::new());
    };
    Ok(extension
        .value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::DNSName(dns) => Some(Name::Dns(dns.to_ascii_lowercase())),
            GeneralName::IPAddress(ip) => Some(Name::Ip(ip.to_vec())),
            _ => None,
        })
        .collect())
}

fn covers(names: &[Name], host: &OriginHost) -> bool {
    names.iter().any(|name| match (name, host) {
        (Name::Dns(pattern), OriginHost::Domain(domain)) => dns_matches(pattern, domain),
        (Name::Ip(bytes), OriginHost::Ipv4(addr)) => bytes[..] == addr.octets(),
        (Name::Ip(bytes), OriginHost::Ipv6(addr)) => bytes[..] == addr.octets(),
        _ => false,
    })
}

fn dns_matches(pattern: &str, domain: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(parent) => {
            parent.contains('.')
                && domain
                    .split_once('.')
                    .is_some_and(|(first, rest)| !first.is_empty() && rest == parent)
        }
        None => pattern == domain,
    }
}

/// Appends `der` as one PEM block with `label`, 64 characters a line.
fn encode(out: &mut String, label: &str, der: &[u8]) {
    out.push_str("-----BEGIN ");
    out.push_str(label);
    out.push_str("-----\n");
    for chunk in der.chunks(48) {
        STANDARD.encode_string(chunk, out);
        out.push('\n');
    }
    out.push_str("-----END ");
    out.push_str(label);
    out.push_str("-----\n");
}
