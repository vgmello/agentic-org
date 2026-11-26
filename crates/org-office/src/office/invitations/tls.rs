//! The HQ's Org-CA-issued TLS identity: loading it, issuing it once, and unwrapping the CA key.

use std::collections::BTreeSet;
use std::fs;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_pki::{
    cert::{
        build_org_ca_cert, cert_fingerprint_hex, issue_office_cert, IssueOfficeCertParams,
        OfficeCertRole, DEFAULT_OFFICE_CERT_VALIDITY_DAYS,
    },
    wrap::{self, WrapBinding, WrapPurpose},
    SigningKey, Zeroizing,
};

use crate::office::{satellite::tls_dir, Office};
use crate::{
    error::OfficeError,
    model::OfficeRole,
    private_fs::{private_dir_synced, write_private_atomic},
};

use super::{
    extract_san_hosts, save_revocations, HqTlsIdentity, RevocationsFile, WrappedHqTlsFile,
};

fn try_load_existing_hq_tls(
    office: &Office,
    secret_bytes: &[u8; 32],
    dir: &std::path::Path,
    requested_hosts: &[String],
    merged_hosts: &mut Vec<String>,
) -> Result<Option<HqTlsIdentity>, OfficeError> {
    let ca_path = dir.join("org-ca.v1.der");
    let hq_cert_path = dir.join("hq-cert.v1.der");
    let hq_key_path = dir.join("hq-key.v1.json");
    if !ca_path.is_file() || !hq_key_path.is_file() {
        return Ok(None);
    }
    let ca_cert_der = fs::read(&ca_path).map_err(OfficeError::storage)?;
    let meta: WrappedHqTlsFile =
        serde_json::from_slice(&fs::read(&hq_key_path).map_err(OfficeError::storage)?)
            .map_err(|_| OfficeError::Invalid("malformed HQ TLS key metadata"))?;
    let hq_cert_der = if let Some(b64) = &meta.cert_der_b64 {
        STANDARD
            .decode(b64)
            .map_err(|_| OfficeError::Invalid("malformed HQ TLS cert base64"))?
    } else if hq_cert_path.is_file() {
        fs::read(&hq_cert_path).map_err(OfficeError::storage)?
    } else {
        Vec::new()
    };
    let covers_all = !hq_cert_der.is_empty()
        && requested_hosts
            .iter()
            .all(|h| meta.san_hosts.iter().any(|existing| existing == h));
    if covers_all {
        let wrapped = STANDARD
            .decode(&meta.wrapped_key_b64)
            .map_err(|_| OfficeError::Invalid("malformed HQ TLS key base64"))?;
        let hq_key_der = wrap::unwrap(
            secret_bytes,
            &WrapBinding {
                organization_id: &office.info.org_id,
                office_id: &office.info.office_id,
                purpose: WrapPurpose::OfficeTlsKey,
            },
            &wrapped,
        )
        .map_err(|_| OfficeError::Invalid("failed unwrapping HQ TLS key"))?;
        let ca_fingerprint_hex = cert_fingerprint_hex(&ca_cert_der);
        return Ok(Some(HqTlsIdentity {
            ca_cert_der,
            ca_fingerprint_hex,
            hq_cert_der,
            hq_key_der,
            epoch: meta.epoch,
        }));
    }
    for existing in meta.san_hosts {
        if !merged_hosts.iter().any(|h| h == &existing) {
            merged_hosts.push(existing);
        }
    }
    Ok(None)
}

pub(super) fn ensure_hq_tls_identity_locked(
    office: &Office,
    san_hosts: &[String],
) -> Result<HqTlsIdentity, OfficeError> {
    if office.info.role != OfficeRole::Hq {
        return Err(OfficeError::Invalid("only HQ holds an Org CA TLS identity"));
    }
    let secrets = office
        .secrets_key
        .as_ref()
        .ok_or(OfficeError::Invalid("office secrets key required"))?;
    let dir = tls_dir(&office.durable, &office.info.office_id);
    private_dir_synced(&office.durable, &dir).map_err(OfficeError::storage)?;

    let ca_path = dir.join("org-ca.v1.der");
    let hq_cert_path = dir.join("hq-cert.v1.der");
    let hq_key_path = dir.join("hq-key.v1.json");
    let rev_path = dir.join("revocations.v1.json");
    let epoch = office.replication.org_checkpoint()?.position.epoch.get();
    let requested_hosts = extract_san_hosts(san_hosts);
    let ca_existed = ca_path.is_file();
    // Fail closed if an established Org CA is missing its revocation store (`TASK-52` H-5);
    // only initialize an empty revocation store during initial Org CA bootstrap.
    if ca_existed && !rev_path.is_file() {
        return Err(OfficeError::Invalid(
            "established Org CA is missing revocations.v1.json; refusing federation",
        ));
    }
    if !ca_existed && !rev_path.is_file() {
        save_revocations(
            office,
            &RevocationsFile {
                v: 1,
                revoked_serials: BTreeSet::new(),
                removed_offices: BTreeSet::new(),
            },
        )?;
    }

    let mut merged_hosts = requested_hosts.clone();
    if let Some(loaded) = try_load_existing_hq_tls(
        office,
        secrets.expose_bytes(),
        &dir,
        &requested_hosts,
        &mut merged_hosts,
    )? {
        return Ok(loaded);
    }

    let ca_key = unwrap_hq_ca_key(office)?;
    let now_unix = (office.monotonic_now_ms() / 1000) as i64;
    let ca_cert_der = if ca_path.is_file() {
        fs::read(&ca_path).map_err(OfficeError::storage)?
    } else {
        let built = build_org_ca_cert(&ca_key, &office.info.org_id, now_unix)
            .map_err(|_| OfficeError::Invalid("failed building Org CA certificate"))?;
        write_private_atomic(&ca_path, &built.cert_der).map_err(OfficeError::storage)?;
        built.cert_der
    };
    let hq_tls_key =
        SigningKey::generate().map_err(|_| OfficeError::Invalid("failed generating HQ TLS key"))?;
    let issued = issue_office_cert(
        &ca_key,
        &ca_cert_der,
        &IssueOfficeCertParams {
            org_id: &office.info.org_id,
            office_id: &office.info.office_id,
            role: OfficeCertRole::Hq { epoch },
            subject_key_der_pkcs8: hq_tls_key.local_der(),
            san_hosts: &merged_hosts,
            now_unix,
            validity_days: DEFAULT_OFFICE_CERT_VALIDITY_DAYS,
        },
    )
    .map_err(|_| OfficeError::Invalid("failed issuing HQ TLS certificate"))?;

    let wrapped = wrap::wrap(
        secrets.expose_bytes(),
        &WrapBinding {
            organization_id: &office.info.org_id,
            office_id: &office.info.office_id,
            purpose: WrapPurpose::OfficeTlsKey,
        },
        hq_tls_key.local_der(),
    )
    .map_err(|_| OfficeError::Invalid("failed wrapping HQ TLS key"))?;
    // Publish cert + wrapped key + SAN metadata together in ONE atomic rename so an
    // interrupted SAN rotation can never pair a new cert with an old key (`TASK-52` P3-#10).
    let key_meta = WrappedHqTlsFile {
        v: 1,
        epoch,
        wrapped_key_b64: STANDARD.encode(&wrapped),
        cert_der_b64: Some(STANDARD.encode(&issued.cert_der)),
        san_hosts: merged_hosts,
    };
    write_private_atomic(
        &hq_key_path,
        &serde_json::to_vec_pretty(&key_meta).map_err(OfficeError::storage)?,
    )
    .map_err(OfficeError::storage)?;
    let _ = write_private_atomic(&hq_cert_path, &issued.cert_der);

    Ok(HqTlsIdentity {
        ca_fingerprint_hex: cert_fingerprint_hex(&ca_cert_der),
        ca_cert_der,
        hq_cert_der: issued.cert_der,
        hq_key_der: Zeroizing::new(hq_tls_key.local_der().to_vec()),
        epoch,
    })
}

pub(super) fn unwrap_hq_ca_key(office: &Office) -> Result<SigningKey, OfficeError> {
    let secrets = office.secrets_key.as_ref().ok_or(OfficeError::Invalid(
        "office secrets key required for Org CA",
    ))?;
    let ca = office.ca()?;
    let raw = wrap::unwrap(
        secrets.expose_bytes(),
        &WrapBinding {
            organization_id: &office.info.org_id,
            office_id: &office.info.office_id,
            purpose: WrapPurpose::OrganizationCa,
        },
        &ca.key.wrapped_ca_key,
    )
    .map_err(|_| OfficeError::Invalid("failed unwrapping Org CA private key"))?;
    SigningKey::from_local_der(&raw).map_err(|_| OfficeError::Invalid("invalid Org CA private key"))
}

impl Office {
    /// Ensures HQ's self-signed Org CA X.509 certificate and HQ TLS server leaf cert/key exist
    /// under `durable/local/offices/<hq_office_id>/tls/` and returns them for federation mTLS.
    pub fn ensure_hq_tls_identity(
        &self,
        san_hosts: &[String],
    ) -> Result<HqTlsIdentity, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        ensure_hq_tls_identity_locked(self, san_hosts)
    }
}
