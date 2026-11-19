//! The certificate verifier: `tls.set`'s checks 3 to 6 (X-02 §6.2), in that order, the first
//! failure answered. Used for a supplied pair at setup and at the first start, for every
//! stored pair at a start and at `tls.fetch`, and for the pair the instance makes itself.
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_host_control::listen::{ListenOrigin, OriginHost, OriginScheme, Uncovered};
use org_host_control::ops::{TlsRefusal, UtcTimestamp};
use org_host_control::pem;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};
use zeroize::Zeroizing;

use super::{Checked, Fingerprint};

/// Checks the pair `cert` (a PEM chain, leaf first) and `key` (PEM), in this order:
/// 1. every `CERTIFICATE` block and the first key block are framed by the PEM reader
///    `tls.set`'s fingerprint uses, every certificate is one whole X.509 certificate, and the
///    key loads as a signing key (`tls_invalid`);
/// 2. the key's public key is the leaf's (`tls_key_mismatch`);
/// 3. at `now` (Unix seconds; `None` skips this step, as `tls.fetch`'s re-check does) the
///    leaf is within its validity (`tls_not_valid`);
/// 4. the leaf names the host of every `https` origin in `origins` by a subject alternative
///    name: a DNS name (one leading `*.` label matches one label) or an IP address
///    (`tls_name_mismatch`, listing the origins it does not cover). The subject's common name
///    is not read: browsers do not.
///
/// It never quotes the pair in an error.
pub(crate) fn check(
    cert: &str,
    key: &str,
    origins: &[&ListenOrigin],
    now: Option<i64>,
) -> Result<Checked, TlsRefusal> {
    let chain = pem::all_blocks(cert, pem::CERTIFICATE).map_err(invalid)?;
    for der in &chain {
        let (rest, _) = X509Certificate::from_der(der.bytes()).map_err(invalid)?;
        if !rest.is_empty() {
            return Err(TlsRefusal::TlsInvalid);
        }
    }
    let (_, leaf) = X509Certificate::from_der(chain[0].bytes()).map_err(invalid)?;
    // The PEM reader frames the key as the fingerprint does; the typed reader (the web
    // process's) must find the same block, or the two would serve and fingerprint different
    // keys.
    let framed = pem::first_block(key, pem::PRIVATE_KEY).map_err(invalid)?;
    let typed = PrivateKeyDer::from_pem_slice(key.as_bytes()).map_err(invalid)?;
    if typed.secret_der() != framed.bytes() {
        return Err(TlsRefusal::TlsInvalid);
    }
    // Everything step 3 parses, before the key is compared (step 4): the leaf's subject
    // alternative names, the key's form and the chain in the normal form the registry keeps.
    let names = subject_alternative_names(&leaf)?;
    let label = match &typed {
        PrivateKeyDer::Pkcs8(_) => "PRIVATE KEY",
        PrivateKeyDer::Pkcs1(_) => "RSA PRIVATE KEY",
        PrivateKeyDer::Sec1(_) => "EC PRIVATE KEY",
        _ => return Err(TlsRefusal::TlsInvalid),
    };
    let mut chain_hash = Sha256::new();
    let mut normal = String::new();
    for der in &chain {
        let len = u32::try_from(der.bytes().len()).map_err(invalid)?;
        chain_hash.update(len.to_be_bytes());
        chain_hash.update(der.bytes());
        encode(&mut normal, "CERTIFICATE", der.bytes());
    }
    // The registry keeps the chain in this form: a chain that only fits it as written (long
    // lines) does not parse as a pair the host can keep.
    if normal.len() > CHAIN_LIMIT {
        return Err(TlsRefusal::TlsInvalid);
    }
    // Sized up front, so the text is never moved to a larger buffer and left behind.
    let der = typed.secret_der();
    let mut key = Zeroizing::new(String::with_capacity(
        der.len() * 4 / 3 + der.len() / 32 + 128,
    ));
    encode(&mut key, label, der);
    // The host's secrets keep the key in this form, and no larger than an item they hold.
    // (No key the signing provider loads is this large today, RSA ending at 8192 bits; the
    // bound keeps setup from accepting one the first start could not keep.)
    if key.len() > crate::host_secrets::MAX_BYTES {
        return Err(TlsRefusal::TlsInvalid);
    }
    let provider = rustls::crypto::ring::default_provider();
    let signing = provider
        .key_provider
        .load_private_key(typed.clone_key())
        .map_err(invalid)?;
    let certificates: Vec<CertificateDer<'static>> = chain
        .iter()
        .map(|der| CertificateDer::from(der.bytes().to_vec()))
        .collect();
    match rustls::sign::CertifiedKey::new(certificates, Arc::clone(&signing)).keys_match() {
        Ok(()) => {}
        Err(rustls::Error::InconsistentKeys(rustls::InconsistentKeys::KeyMismatch)) => {
            return Err(TlsRefusal::TlsKeyMismatch)
        }
        Err(_) => return Err(TlsRefusal::TlsInvalid),
    }
    let validity = leaf.validity();
    let (not_before, not_after) = (
        validity.not_before.timestamp(),
        validity.not_after.timestamp(),
    );
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
    let mut uncovered: Vec<ListenOrigin> = Vec::new();
    for origin in origins {
        if origin.scheme() == OriginScheme::Https
            && !covers(&names, origin.host())
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
    let leaf = Fingerprint(Sha256::digest(chain[0].bytes()).into());
    let mut pair = Sha256::new();
    pair.update(chain_hash.finalize());
    pair.update(Sha256::digest(typed.secret_der()));
    Ok(Checked {
        chain: normal,
        key,
        pair: pair.finalize().into(),
        leaf,
        not_before,
        not_after,
    })
}

/// The longest chain the registry keeps, in its normal form (`web_tls_pair.cert_pem`).
pub(crate) const CHAIN_LIMIT: usize = 65_536;

/// Every parse failure is `tls_invalid`.
fn invalid<E>(_: E) -> TlsRefusal {
    TlsRefusal::TlsInvalid
}

/// A leaf's subject alternative names that can name an origin's host. A leaf whose extension
/// does not parse (or appears twice) is `tls_invalid`.
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

/// Whether `names` name `host`, as a browser matches them: a DNS name exactly (ASCII case
/// aside) or by a wildcard `*.` in place of the host's first label, never a wildcard over a
/// single remaining label; an IP address by its bytes, never by a DNS name.
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
