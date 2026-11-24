//! Organization CA, HQ, and satellite X.509 certificates and pinned mTLS configuration
//! (`doc-67` §Provisioning a satellite, `TASK-52` Slice 1).
use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PublicKeyData, SanType, SerialNumber, SignatureAlgorithm,
};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{self, UnparsedPublicKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, ConnectionCommon, RootCertStore, ServerConfig, SideData};
use sha2::{Digest as _, Sha256};
use x509_parser::extensions::{ExtendedKeyUsage, GeneralName, ParsedExtension};
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::SigningKey;

const BACKDATED_SECS: i64 = 300;
const DAY_SECS: i64 = 86_400;
const CA_VALIDITY_DAYS: i64 = 3650;
pub const DEFAULT_OFFICE_CERT_VALIDITY_DAYS: i64 = 365;
const EXPORTER_LABEL: &[u8] = b"EXPORTER-agentic-org-enroll-v1";
const POP_DOMAIN: &[u8] = b"agentic-org/satellite-key-pop/v1";

struct P256PublicKey<'a>(&'a [u8; 65]);

impl PublicKeyData for P256PublicKey<'_> {
    fn der_bytes(&self) -> &[u8] {
        self.0
    }

    fn algorithm(&self) -> &'static SignatureAlgorithm {
        &rcgen::PKCS_ECDSA_P256_SHA256
    }
}

fn build_ca_params(
    org_id: &str,
    not_before: i64,
    not_after: i64,
    serial: SerialNumber,
) -> Result<CertificateParams, CertError> {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("Agentic Org CA ({org_id})"));
    params.distinguished_name = dn;
    let uri = format!("urn:agentic-org:ca:{org_id}");
    params.subject_alt_names = vec![SanType::URI(
        uri.try_into()
            .map_err(|_| CertError::InvalidInput("ca uri"))?,
    )];
    params.not_before = unix_to_offset(not_before)?;
    params.not_after = unix_to_offset(not_after)?;
    params.serial_number = Some(serial);
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    Ok(params)
}

fn issuer_from_ca_cert_and_key(
    ca_key: &SigningKey,
    ca_cert_der: &[u8],
    org_id: &str,
) -> Result<Issuer<'static, KeyPair>, CertError> {
    let (rest, parsed) =
        X509Certificate::from_der(ca_cert_der).map_err(|_| CertError::ParseFailed)?;
    if !rest.is_empty() || !parsed.is_ca() {
        return Err(CertError::ParseFailed);
    }
    if parsed.public_key().subject_public_key.as_ref() != ca_key.public_key() {
        return Err(CertError::SignatureInvalid);
    }
    let ca_pair = keypair_from_der(ca_key.local_der())?;
    let ca_params = build_ca_params(
        org_id,
        parsed.validity().not_before.timestamp(),
        parsed.validity().not_after.timestamp(),
        SerialNumber::from_slice(parsed.raw_serial()),
    )?;
    Ok(Issuer::new(ca_params, ca_pair))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeCertRole {
    Hq { epoch: u64 },
    Satellite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgCaCert {
    pub cert_der: Vec<u8>,
    pub cert_pem: String,
    pub fingerprint: [u8; 32],
}

impl OrgCaCert {
    pub fn fingerprint_hex(&self) -> String {
        hex_lower(&self.fingerprint)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuedOfficeCert {
    pub cert_der: Vec<u8>,
    pub cert_pem: String,
    pub serial_hex: String,
    pub not_before_unix: i64,
    pub not_after_unix: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedOfficeCert {
    pub org_id: String,
    pub office_id: String,
    pub role: OfficeCertRole,
    pub serial_hex: String,
    pub not_before_unix: i64,
    pub not_after_unix: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CertError {
    InvalidInput(&'static str),
    SigningFailed,
    ParseFailed,
    SignatureInvalid,
    NotValidAtTime,
    Revoked,
    RoleViolation(&'static str),
    OrgMismatch,
    TlsConfigFailed,
    ExporterFailed,
}

impl fmt::Display for CertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(why) => write!(f, "invalid certificate input: {why}"),
            Self::SigningFailed => f.write_str("certificate signing failed"),
            Self::ParseFailed => f.write_str("certificate parse failed"),
            Self::SignatureInvalid => f.write_str("certificate signature verification failed"),
            Self::NotValidAtTime => f.write_str("certificate is outside its validity window"),
            Self::Revoked => f.write_str("certificate serial is revoked"),
            Self::RoleViolation(why) => write!(f, "certificate role violation: {why}"),
            Self::OrgMismatch => f.write_str("certificate organization does not match"),
            Self::TlsConfigFailed => f.write_str("TLS configuration failed"),
            Self::ExporterFailed => f.write_str("TLS keying material export failed"),
        }
    }
}

impl std::error::Error for CertError {}

pub fn cert_fingerprint_sha256(cert_der: &[u8]) -> [u8; 32] {
    Sha256::digest(cert_der).into()
}

pub fn cert_fingerprint_hex(cert_der: &[u8]) -> String {
    hex_lower(&cert_fingerprint_sha256(cert_der))
}

/// Builds the self-signed Organization CA certificate backed by `ca_key` (P-256 ECDSA).
pub fn build_org_ca_cert(
    ca_key: &SigningKey,
    org_id: &str,
    now_unix: i64,
) -> Result<OrgCaCert, CertError> {
    if org_id.is_empty() || org_id.len() > 128 {
        return Err(CertError::InvalidInput("org_id"));
    }
    let key_pair = keypair_from_der(ca_key.local_der())?;
    let not_before = now_unix - BACKDATED_SECS;
    let not_after = not_before + CA_VALIDITY_DAYS * DAY_SECS;
    let params = build_ca_params(org_id, not_before, not_after, random_serial()?)?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|_| CertError::SigningFailed)?;
    let cert_der = cert.der().to_vec();
    let fingerprint = cert_fingerprint_sha256(&cert_der);
    Ok(OrgCaCert {
        cert_der,
        cert_pem: cert.pem(),
        fingerprint,
    })
}

pub struct IssueOfficeCertParams<'a> {
    pub org_id: &'a str,
    pub office_id: &'a str,
    pub role: OfficeCertRole,
    pub subject_key_der_pkcs8: &'a [u8],
    pub san_hosts: &'a [String],
    pub now_unix: i64,
    pub validity_days: i64,
}

/// Issues a leaf office certificate signed by the Organization CA.
///
/// Satellite certificates carry `ClientAuth` ONLY (never `ServerAuth`) so a stolen or rogue
/// satellite can never impersonate HQ (`doc-67` §Provisioning a satellite).
pub fn issue_office_cert(
    ca_key: &SigningKey,
    ca_cert_der: &[u8],
    p: &IssueOfficeCertParams<'_>,
) -> Result<IssuedOfficeCert, CertError> {
    if p.org_id.is_empty() || p.office_id.is_empty() || p.validity_days <= 0 {
        return Err(CertError::InvalidInput("office cert parameters"));
    }
    let issuer = issuer_from_ca_cert_and_key(ca_key, ca_cert_der, p.org_id)?;
    let subject_pair = keypair_from_der(p.subject_key_der_pkcs8)?;
    let mut params = build_leaf_params(
        p.org_id,
        p.office_id,
        p.role,
        p.san_hosts,
        p.now_unix,
        p.validity_days,
    )?;
    let serial_bytes = random_serial_bytes()?;
    let serial_hex = hex_lower(&serial_bytes);
    params.serial_number = Some(SerialNumber::from_slice(&serial_bytes));
    let not_before = p.now_unix - BACKDATED_SECS;
    let not_after = not_before + p.validity_days * DAY_SECS;
    let cert = params
        .signed_by(&subject_pair, &issuer)
        .map_err(|_| CertError::SigningFailed)?;
    Ok(IssuedOfficeCert {
        cert_der: cert.der().to_vec(),
        cert_pem: cert.pem(),
        serial_hex,
        not_before_unix: not_before,
        not_after_unix: not_after,
    })
}

fn pop_preimage(org_id: &str, office_id: &str, invitation_id: &str, pub_key: &[u8; 65]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        POP_DOMAIN.len() + 12 + org_id.len() + office_id.len() + invitation_id.len() + 65,
    );
    out.extend_from_slice(POP_DOMAIN);
    out.extend_from_slice(&(org_id.len() as u32).to_be_bytes());
    out.extend_from_slice(org_id.as_bytes());
    out.extend_from_slice(&(office_id.len() as u32).to_be_bytes());
    out.extend_from_slice(office_id.as_bytes());
    out.extend_from_slice(&(invitation_id.len() as u32).to_be_bytes());
    out.extend_from_slice(invitation_id.as_bytes());
    out.extend_from_slice(pub_key);
    out
}

/// Signs proof-of-possession of a joining satellite's local TLS private key without exporting it.
pub fn sign_satellite_key_pop(
    subject_key: &SigningKey,
    org_id: &str,
    office_id: &str,
    invitation_id: &str,
) -> Result<Vec<u8>, CertError> {
    if org_id.is_empty() || office_id.is_empty() || invitation_id.is_empty() {
        return Err(CertError::InvalidInput("pop parameters"));
    }
    let preimage = pop_preimage(org_id, office_id, invitation_id, &subject_key.public_key());
    subject_key
        .sign_message_der(&preimage)
        .map_err(|_| CertError::SigningFailed)
}

pub struct IssueSatelliteCertParams<'a> {
    pub org_id: &'a str,
    pub office_id: &'a str,
    pub invitation_id: &'a str,
    pub subject_pub_key_uncompressed: &'a [u8; 65],
    pub pop_signature_der: &'a [u8],
    pub now_unix: i64,
    pub validity_days: i64,
}

/// Verifies proof-of-possession of the satellite's joining public key and signs an HQ-authored
/// `ClientAuth`-only satellite certificate without the satellite's private key ever leaving it.
pub fn issue_satellite_cert_from_pubkey(
    ca_key: &SigningKey,
    ca_cert_der: &[u8],
    p: &IssueSatelliteCertParams<'_>,
) -> Result<IssuedOfficeCert, CertError> {
    if p.org_id.is_empty()
        || p.office_id.is_empty()
        || p.invitation_id.is_empty()
        || p.validity_days <= 0
    {
        return Err(CertError::InvalidInput("satellite cert parameters"));
    }
    let preimage = pop_preimage(
        p.org_id,
        p.office_id,
        p.invitation_id,
        p.subject_pub_key_uncompressed,
    );
    UnparsedPublicKey::new(
        &signature::ECDSA_P256_SHA256_ASN1,
        p.subject_pub_key_uncompressed,
    )
    .verify(&preimage, p.pop_signature_der)
    .map_err(|_| CertError::SignatureInvalid)?;
    let issuer = issuer_from_ca_cert_and_key(ca_key, ca_cert_der, p.org_id)?;
    let mut params = build_leaf_params(
        p.org_id,
        p.office_id,
        OfficeCertRole::Satellite,
        &[],
        p.now_unix,
        p.validity_days,
    )?;
    let serial_bytes = random_serial_bytes()?;
    let serial_hex = hex_lower(&serial_bytes);
    params.serial_number = Some(SerialNumber::from_slice(&serial_bytes));
    let not_before = p.now_unix - BACKDATED_SECS;
    let not_after = not_before + p.validity_days * DAY_SECS;
    let cert = params
        .signed_by(&P256PublicKey(p.subject_pub_key_uncompressed), &issuer)
        .map_err(|_| CertError::SigningFailed)?;
    Ok(IssuedOfficeCert {
        cert_der: cert.der().to_vec(),
        cert_pem: cert.pem(),
        serial_hex,
        not_before_unix: not_before,
        not_after_unix: not_after,
    })
}

/// Verifies a leaf office certificate against the pinned Organization CA, checking signature,
/// non-CA constraint, validity window, revocation list, URI identity binding, and strict EKU.
pub fn verify_office_cert(
    ca_cert_der: &[u8],
    leaf_cert_der: &[u8],
    expected_org_id: &str,
    expected_role: Option<OfficeCertRole>,
    revoked_serials: &BTreeSet<String>,
    now_unix: i64,
) -> Result<VerifiedOfficeCert, CertError> {
    let (ca_rest, ca) =
        X509Certificate::from_der(ca_cert_der).map_err(|_| CertError::ParseFailed)?;
    let (leaf_rest, leaf) =
        X509Certificate::from_der(leaf_cert_der).map_err(|_| CertError::ParseFailed)?;
    if !ca_rest.is_empty() || !leaf_rest.is_empty() {
        return Err(CertError::ParseFailed);
    }
    if leaf.is_ca() || !ca.is_ca() {
        return Err(CertError::RoleViolation(
            "invalid CA/leaf basic constraints",
        ));
    }
    UnparsedPublicKey::new(
        &signature::ECDSA_P256_SHA256_ASN1,
        ca.public_key().subject_public_key.as_ref(),
    )
    .verify(leaf.tbs_certificate.as_ref(), leaf.signature_value.as_ref())
    .map_err(|_| CertError::SignatureInvalid)?;
    let not_before = leaf.validity().not_before.timestamp();
    let not_after = leaf.validity().not_after.timestamp();
    if now_unix < not_before || now_unix > not_after {
        return Err(CertError::NotValidAtTime);
    }
    let serial_hex = hex_lower(leaf.raw_serial());
    if revoked_serials.contains(&serial_hex) {
        return Err(CertError::Revoked);
    }
    let (org_id, office_id, role) = extract_office_identity_san(&leaf)?;
    if org_id != expected_org_id {
        return Err(CertError::OrgMismatch);
    }
    verify_eku_for_role(&leaf, role)?;
    if let Some(wanted) = expected_role {
        if role != wanted {
            return Err(CertError::RoleViolation(
                "certificate role/epoch does not match expected role",
            ));
        }
    }
    Ok(VerifiedOfficeCert {
        org_id,
        office_id,
        role,
        serial_hex,
        not_before_unix: not_before,
        not_after_unix: not_after,
    })
}

/// Builds a rustls `ServerConfig` for HQ's federation listener (ring provider only).
/// When `require_client_cert` is `true`, only certificates chaining to `ca_cert_der` are accepted.
/// When `false` (enrollment handshake), the server presents HQ's cert and authenticates the
/// joining satellite via the TLS-exporter invitation proof.
pub fn build_hq_tls_server_config(
    ca_cert_der: &[u8],
    hq_cert_der: &[u8],
    hq_key_der_pkcs8: &[u8],
    require_client_cert: bool,
) -> Result<Arc<ServerConfig>, CertError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| CertError::TlsConfigFailed)?;
    let builder = if require_client_cert {
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(ca_cert_der.to_vec()))
            .map_err(|_| CertError::TlsConfigFailed)?;
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .map_err(|_| CertError::TlsConfigFailed)?;
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };
    let cert_chain = vec![
        CertificateDer::from(hq_cert_der.to_vec()),
        CertificateDer::from(ca_cert_der.to_vec()),
    ];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(hq_key_der_pkcs8.to_vec()));
    let config = builder
        .with_single_cert(cert_chain, key)
        .map_err(|_| CertError::TlsConfigFailed)?;
    Ok(Arc::new(config))
}

/// Builds a single HQ federation `ServerConfig` that accepts BOTH Org-CA mTLS satellite
/// connections AND unauthenticated-cert enrollment connections (which the application layer
/// restricts strictly to TLS-exporter-authenticated `EnrollRequest` frames).
pub fn build_hq_tls_dual_server_config(
    ca_cert_der: &[u8],
    hq_cert_der: &[u8],
    hq_key_der_pkcs8: &[u8],
) -> Result<Arc<ServerConfig>, CertError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(ca_cert_der.to_vec()))
        .map_err(|_| CertError::TlsConfigFailed)?;
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::clone(&provider),
    )
    .allow_unauthenticated()
    .build()
    .map_err(|_| CertError::TlsConfigFailed)?;
    let cert_chain = vec![
        CertificateDer::from(hq_cert_der.to_vec()),
        CertificateDer::from(ca_cert_der.to_vec()),
    ];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(hq_key_der_pkcs8.to_vec()));
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| CertError::TlsConfigFailed)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(cert_chain, key)
        .map_err(|_| CertError::TlsConfigFailed)?;
    Ok(Arc::new(config))
}

/// Builds a rustls `ClientConfig` for a satellite connecting to HQ, pinned strictly to `ca_cert_der`.
pub fn build_satellite_tls_client_config(
    ca_cert_der: &[u8],
    client_identity: Option<(&[u8], &[u8])>,
) -> Result<Arc<ClientConfig>, CertError> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(ca_cert_der.to_vec()))
        .map_err(|_| CertError::TlsConfigFailed)?;
    let builder =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| CertError::TlsConfigFailed)?
            .with_root_certificates(roots);
    let config = match client_identity {
        Some((cert_der, key_der_pkcs8)) => {
            let chain = vec![
                CertificateDer::from(cert_der.to_vec()),
                CertificateDer::from(ca_cert_der.to_vec()),
            ];
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der_pkcs8.to_vec()));
            builder
                .with_client_auth_cert(chain, key)
                .map_err(|_| CertError::TlsConfigFailed)?
        }
        None => builder.with_no_client_auth(),
    };
    Ok(Arc::new(config))
}

/// Exports 32 bytes of channel-bound TLS keying material for satellite enrollment proofs.
pub fn export_enrollment_binding<S: SideData>(
    conn: &ConnectionCommon<S>,
) -> Result<[u8; 32], CertError> {
    conn.export_keying_material([0u8; 32], EXPORTER_LABEL, None)
        .map_err(|_| CertError::ExporterFailed)
}

fn build_leaf_params(
    org_id: &str,
    office_id: &str,
    role: OfficeCertRole,
    san_hosts: &[String],
    now_unix: i64,
    validity_days: i64,
) -> Result<CertificateParams, CertError> {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("office:{office_id}"));
    params.distinguished_name = dn;
    let uri = match role {
        OfficeCertRole::Hq { epoch } => {
            format!("urn:agentic-org:office:{org_id}:{office_id}:hq:{epoch}")
        }
        OfficeCertRole::Satellite => {
            format!("urn:agentic-org:office:{org_id}:{office_id}:satellite")
        }
    };
    let mut sans = vec![SanType::URI(
        uri.try_into()
            .map_err(|_| CertError::InvalidInput("office uri"))?,
    )];
    if matches!(role, OfficeCertRole::Hq { .. }) {
        for host in san_hosts {
            let trimmed = host.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(ip) = trimmed.parse::<IpAddr>() {
                sans.push(SanType::IpAddress(ip));
            } else {
                sans.push(SanType::DnsName(
                    trimmed
                        .to_owned()
                        .try_into()
                        .map_err(|_| CertError::InvalidInput("san dns host"))?,
                ));
            }
        }
    }
    params.subject_alt_names = sans;
    let not_before = now_unix - BACKDATED_SECS;
    params.not_before = unix_to_offset(not_before)?;
    params.not_after = unix_to_offset(not_before + validity_days * DAY_SECS)?;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = match role {
        OfficeCertRole::Hq { .. } => vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ],
        OfficeCertRole::Satellite => vec![ExtendedKeyUsagePurpose::ClientAuth],
    };
    Ok(params)
}

fn extract_office_identity_san(
    leaf: &X509Certificate<'_>,
) -> Result<(String, String, OfficeCertRole), CertError> {
    let san = leaf
        .subject_alternative_name()
        .map_err(|_| CertError::ParseFailed)?
        .ok_or(CertError::RoleViolation("missing office identity SAN"))?;
    for name in &san.value.general_names {
        let GeneralName::URI(uri) = name else {
            continue;
        };
        let Some(rest) = uri.strip_prefix("urn:agentic-org:office:") else {
            continue;
        };
        let parts: Vec<&str> = rest.split(':').collect();
        match parts.as_slice() {
            [org_id, office_id, "satellite"] if !org_id.is_empty() && !office_id.is_empty() => {
                return Ok((
                    (*org_id).to_owned(),
                    (*office_id).to_owned(),
                    OfficeCertRole::Satellite,
                ));
            }
            [org_id, office_id, "hq", epoch_str] if !org_id.is_empty() && !office_id.is_empty() => {
                let epoch = epoch_str
                    .parse::<u64>()
                    .map_err(|_| CertError::RoleViolation("invalid HQ epoch in SAN"))?;
                return Ok((
                    (*org_id).to_owned(),
                    (*office_id).to_owned(),
                    OfficeCertRole::Hq { epoch },
                ));
            }
            _ => return Err(CertError::RoleViolation("malformed office identity SAN")),
        }
    }
    Err(CertError::RoleViolation("missing office URI SAN"))
}

fn verify_eku_for_role(leaf: &X509Certificate<'_>, role: OfficeCertRole) -> Result<(), CertError> {
    let eku = find_eku(leaf).ok_or(CertError::RoleViolation("missing ExtendedKeyUsage"))?;
    match role {
        OfficeCertRole::Hq { .. } => {
            if !eku.server_auth {
                return Err(CertError::RoleViolation("HQ cert missing serverAuth EKU"));
            }
        }
        OfficeCertRole::Satellite => {
            if eku.server_auth || !eku.client_auth {
                return Err(CertError::RoleViolation(
                    "satellite cert must carry clientAuth only",
                ));
            }
        }
    }
    Ok(())
}

fn find_eku<'a>(leaf: &'a X509Certificate<'a>) -> Option<&'a ExtendedKeyUsage<'a>> {
    for ext in leaf.extensions() {
        if let ParsedExtension::ExtendedKeyUsage(eku) = ext.parsed_extension() {
            return Some(eku);
        }
    }
    None
}

fn keypair_from_der(pkcs8_der: &[u8]) -> Result<KeyPair, CertError> {
    let der = PrivatePkcs8KeyDer::from(pkcs8_der.to_vec());
    KeyPair::from_pkcs8_der_and_sign_algo(&der, &rcgen::PKCS_ECDSA_P256_SHA256)
        .map_err(|_| CertError::ParseFailed)
}

fn unix_to_offset(secs: i64) -> Result<time::OffsetDateTime, CertError> {
    time::OffsetDateTime::from_unix_timestamp(secs)
        .map_err(|_| CertError::InvalidInput("timestamp out of range"))
}

fn random_serial_bytes() -> Result<[u8; 16], CertError> {
    let mut bytes = [0u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| CertError::SigningFailed)?;
    bytes[0] = (bytes[0] & 0x7f) | 0x40;
    Ok(bytes)
}

fn random_serial() -> Result<SerialNumber, CertError> {
    Ok(SerialNumber::from_slice(&random_serial_bytes()?))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
