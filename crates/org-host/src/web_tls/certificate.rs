//! The instance's own certificate (ADR-0017 decision 10): made with rcgen over ring.
use org_host_control::listen::{ListenOrigin, OriginHost};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType, SerialNumber,
};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

use super::{Checked, DAY_SECS, VALIDITY_DAYS};

/// How far before `now` the validity starts, so that a browser whose clock is a little behind
/// the host's accepts it.
const BACKDATED_SECS: i64 = 300;

/// Makes a new ECDSA P-256 key and a self-signed certificate for `origin`: its only subject
/// alternative name is the origin's host (a DNS name, or an IP address for a literal), its
/// subject is the common name `Agentic Org`, it is not a CA, and it is valid for
/// [`VALIDITY_DAYS`] days from five minutes before `now` (Unix seconds), with a random
/// 128-bit serial number. The pair is checked as any other pair ([`super::check`]) before it
/// is returned. An error names the step, never key material.
pub(crate) fn make(origin: &ListenOrigin, now: i64) -> Result<Checked, String> {
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(fail("the key"))?;
    let mut params = CertificateParams::default();
    params.subject_alt_names = vec![match origin.host() {
        OriginHost::Domain(name) => {
            SanType::DnsName(name.as_str().try_into().map_err(fail("the host name"))?)
        }
        OriginHost::Ipv4(addr) => SanType::IpAddress((*addr).into()),
        OriginHost::Ipv6(addr) => SanType::IpAddress((*addr).into()),
    }];
    let mut subject = DistinguishedName::new();
    subject.push(DnType::CommonName, "Agentic Org");
    params.distinguished_name = subject;
    let not_before = now - BACKDATED_SECS;
    let at = |secs| {
        time::OffsetDateTime::from_unix_timestamp(secs)
            .map_err(|_| "making the certificate: the clock is out of range".to_owned())
    };
    params.not_before = at(not_before)?;
    params.not_after = at(not_before + VALIDITY_DAYS * DAY_SECS)?;
    params.serial_number = Some(serial()?);
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let certificate = params.self_signed(&key).map_err(fail("signing"))?;
    let key_pem = Zeroizing::new(key.serialize_pem());
    drop(key);
    super::check(&certificate.pem(), &key_pem, &[origin], Some(now))
        .map_err(|why| format!("the certificate this instance made does not verify: {why:?}"))
}

fn fail(step: &'static str) -> impl Fn(rcgen::Error) -> String {
    move |e| format!("making the certificate: {step}: {e}")
}

/// 16 random bytes, the top bit clear (a positive INTEGER) and never all zero.
fn serial() -> Result<SerialNumber, String> {
    let mut bytes = [0u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "making the certificate: no random serial number".to_owned())?;
    bytes[0] = (bytes[0] & 0x7f) | 0x40;
    Ok(SerialNumber::from_slice(&bytes))
}
