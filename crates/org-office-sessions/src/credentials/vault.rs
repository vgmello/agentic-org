//! Wrapped, owner-only credential vault for provider CLI enrollments and usage credentials
//! (`ADR-0032`, `ENROLL-4..6`).

use super::*;

/// Persists wrapped provider credentials under `<durable>/local/offices/<office_id>/auth/<connection_id>/credential.v1.json`.
#[derive(Clone)]
pub struct CredentialVault {
    pub(super) durable_root: PathBuf,
    pub(super) organization_id: String,
    pub(super) office_id: String,
    pub(super) office_secrets_key: Option<Arc<Zeroizing<[u8; 32]>>>,
}

impl fmt::Debug for CredentialVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialVault")
            .field("organization_id", &self.organization_id)
            .field("office_id", &self.office_id)
            .field("has_key", &self.office_secrets_key.is_some())
            .finish()
    }
}

impl CredentialVault {
    pub fn new(
        durable_root: PathBuf,
        organization_id: impl Into<String>,
        office_id: impl Into<String>,
        office_secrets_key: Option<[u8; 32]>,
    ) -> Self {
        Self {
            durable_root,
            organization_id: organization_id.into(),
            office_id: office_id.into(),
            office_secrets_key: office_secrets_key.map(|k| Arc::new(Zeroizing::new(k))),
        }
    }

    pub fn credential_path(&self, connection_id: &str) -> Result<PathBuf, EnrollmentVaultError> {
        if !is_safe_id(connection_id) {
            return Err(EnrollmentVaultError::InvalidInput(
                "invalid connection_id".to_owned(),
            ));
        }
        Ok(self
            .durable_root
            .join("local")
            .join("offices")
            .join(&self.office_id)
            .join("auth")
            .join(connection_id)
            .join("credential.v1.json"))
    }

    fn wrapping_key(&self) -> Result<Zeroizing<[u8; 32]>, EnrollmentVaultError> {
        let Some(master) = &self.office_secrets_key else {
            return Err(EnrollmentVaultError::MissingSecretsKey);
        };
        Ok(derive_wrapping_key(master, KeyClass::ProviderCredential)?)
    }

    fn binding<'a>(&'a self, connection_id: &'a str) -> ProviderCredentialBinding<'a> {
        ProviderCredentialBinding {
            organization_id: &self.organization_id,
            office_id: &self.office_id,
            connection_id,
        }
    }

    /// Encrypts and atomically writes `secret` with mode `0600` under `KeyClass::ProviderCredential`.
    pub fn store_wrapped(
        &self,
        connection_id: &str,
        provider: &str,
        secret: &[u8],
        expires_at: Option<String>,
        account_hint: Option<String>,
    ) -> Result<ProviderCredentialView, EnrollmentVaultError> {
        let key = self.wrapping_key()?;
        let path = self.credential_path(connection_id)?;
        let parent = path
            .parent()
            .ok_or_else(|| EnrollmentVaultError::Io("credential path has no parent".into()))?;
        ensure_owner_dirs(parent)?;

        let wrapped = wrap_provider_credential(&key, &self.binding(connection_id), secret)?;
        let enrolled_at = rfc3339_now();
        let record = CredentialMetadata {
            version: CREDENTIAL_VERSION,
            organization_id: self.organization_id.clone(),
            office_id: self.office_id.clone(),
            connection_id: connection_id.to_owned(),
            provider: provider.to_owned(),
            enrolled_at: enrolled_at.clone(),
            expires_at: expires_at.clone(),
            account_hint: account_hint.clone(),
            bound_origin: None,
            wrapped_b64: BASE64.encode(&wrapped),
        };
        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|_| EnrollmentVaultError::Io("could not encode credential metadata".into()))?;
        write_private_atomic(parent, &path, &bytes)?;
        Ok(ProviderCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            enrolled: true,
            enrolled_at: Some(enrolled_at),
            expires_at,
            account_hint,
        })
    }

    /// Unwraps and returns the raw credential in a zeroizing buffer.
    pub fn load_unwrapped(
        &self,
        connection_id: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, EnrollmentVaultError> {
        Ok(self
            .load_unwrapped_with_meta(connection_id)?
            .map(|c| c.secret))
    }

    /// Unwraps and returns the enrolled credential together with expiry & account metadata.
    pub fn load_unwrapped_with_meta(
        &self,
        connection_id: &str,
    ) -> Result<Option<UnwrappedCredential>, EnrollmentVaultError> {
        let path = self.credential_path(connection_id)?;
        self.load_unwrapped_from_path(&path, connection_id, connection_id)
    }

    /// Returns the office-local path for a connection's dedicated usage-API credential (`TASK-79`).
    pub fn usage_credential_path(
        &self,
        connection_id: &str,
    ) -> Result<PathBuf, EnrollmentVaultError> {
        if !is_safe_id(connection_id) {
            return Err(EnrollmentVaultError::InvalidInput(
                "invalid connection_id".to_owned(),
            ));
        }
        Ok(self
            .durable_root
            .join("local")
            .join("offices")
            .join(&self.office_id)
            .join("auth")
            .join(connection_id)
            .join("usage-credential.v1.json"))
    }

    /// Encrypts and atomically stores a private usage-API credential under origin- and
    /// domain-separated AAD (`{connection_id}:usage:{bound_origin}`, mode `0600`, outside org Git).
    pub fn store_wrapped_usage(
        &self,
        connection_id: &str,
        provider: &str,
        bound_origin: &str,
        secret: &[u8],
        expires_at: Option<String>,
        account_hint: Option<String>,
    ) -> Result<SubscriptionUsageCredentialView, EnrollmentVaultError> {
        let key = self.wrapping_key()?;
        let path = self.usage_credential_path(connection_id)?;
        let parent = path.parent().ok_or_else(|| {
            EnrollmentVaultError::Io("usage credential path has no parent".into())
        })?;
        ensure_owner_dirs(parent)?;

        let aad_conn = format!("{connection_id}:usage:{bound_origin}");
        let wrapped = wrap_provider_credential(&key, &self.binding(&aad_conn), secret)?;
        let configured_at = rfc3339_now();
        let record = CredentialMetadata {
            version: CREDENTIAL_VERSION,
            organization_id: self.organization_id.clone(),
            office_id: self.office_id.clone(),
            connection_id: format!("{connection_id}:usage"),
            provider: provider.to_owned(),
            enrolled_at: configured_at.clone(),
            expires_at: expires_at.clone(),
            account_hint: account_hint.clone(),
            bound_origin: Some(bound_origin.to_owned()),
            wrapped_b64: BASE64.encode(&wrapped),
        };
        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|_| EnrollmentVaultError::Io("could not encode usage credential".into()))?;
        write_private_atomic(parent, &path, &bytes)?;
        Ok(SubscriptionUsageCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            configured: true,
            configured_at: Some(configured_at),
            expires_at,
            account_hint,
        })
    }

    /// Unwraps and returns the dedicated usage-API credential + metadata only when its bound
    /// destination origin matches `expected_origin` (`TASK-79`).
    pub fn load_unwrapped_usage_with_meta(
        &self,
        connection_id: &str,
        expected_origin: &str,
    ) -> Result<Option<UnwrappedCredential>, EnrollmentVaultError> {
        let path = self.usage_credential_path(connection_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(&path)
            .map_err(|_| EnrollmentVaultError::Io("could not read usage credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed usage credential file".into()))?;
        let expected_conn = format!("{connection_id}:usage");
        if record.version != CREDENTIAL_VERSION
            || record.organization_id != self.organization_id
            || record.office_id != self.office_id
            || record.connection_id != expected_conn
        {
            return Err(EnrollmentVaultError::Crypto(
                "usage credential binding metadata mismatch".into(),
            ));
        }
        if record.bound_origin.as_deref() != Some(expected_origin) {
            return Err(EnrollmentVaultError::Conflict(
                "usage credential is bound to a different endpoint origin; re-configure credential"
                    .into(),
            ));
        }
        let key = self.wrapping_key()?;
        let aad_conn = format!("{connection_id}:usage:{expected_origin}");
        let ciphertext = BASE64
            .decode(&record.wrapped_b64)
            .map_err(|_| EnrollmentVaultError::Crypto("invalid credential base64".into()))?;
        let plaintext = unwrap_provider_credential(&key, &self.binding(&aad_conn), &ciphertext)?;
        Ok(Some(UnwrappedCredential {
            secret: plaintext,
            enrolled_at: record.enrolled_at,
            expires_at: record.expires_at,
            account_hint: record.account_hint,
            bound_origin: record.bound_origin,
        }))
    }

    /// Returns non-secret status metadata for a connection's dedicated usage-API credential.
    pub fn usage_status(
        &self,
        connection_id: &str,
    ) -> Result<SubscriptionUsageCredentialView, EnrollmentVaultError> {
        let path = self.usage_credential_path(connection_id)?;
        if !path.exists() {
            return Ok(SubscriptionUsageCredentialView {
                connection_id: connection_id.to_owned(),
                office_id: self.office_id.clone(),
                configured: false,
                configured_at: None,
                expires_at: None,
                account_hint: None,
            });
        }
        let raw = fs::read(&path)
            .map_err(|_| EnrollmentVaultError::Io("could not read usage credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed usage credential file".into()))?;
        let expected_conn = format!("{connection_id}:usage");
        if record.version != CREDENTIAL_VERSION
            || record.organization_id != self.organization_id
            || record.office_id != self.office_id
            || record.connection_id != expected_conn
        {
            return Err(EnrollmentVaultError::Crypto(
                "usage credential binding metadata mismatch".into(),
            ));
        }
        Ok(SubscriptionUsageCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            configured: true,
            configured_at: Some(record.enrolled_at),
            expires_at: record.expires_at,
            account_hint: record.account_hint,
        })
    }

    /// Revokes and removes a connection's dedicated usage-API credential durably (`TASK-79`).
    pub fn revoke_usage(
        &self,
        connection_id: &str,
    ) -> Result<SubscriptionUsageCredentialView, EnrollmentVaultError> {
        let path = self.usage_credential_path(connection_id)?;
        remove_file_durably(&path)?;
        self.usage_status(connection_id)
    }

    fn load_unwrapped_from_path(
        &self,
        path: &Path,
        expected_connection_id: &str,
        aad_connection_id: &str,
    ) -> Result<Option<UnwrappedCredential>, EnrollmentVaultError> {
        if !path.exists() {
            return Ok(None);
        }
        let key = self.wrapping_key()?;
        let raw = fs::read(path)
            .map_err(|_| EnrollmentVaultError::Io("could not read credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed credential file".into()))?;
        if record.version != CREDENTIAL_VERSION
            || record.organization_id != self.organization_id
            || record.office_id != self.office_id
            || record.connection_id != expected_connection_id
        {
            return Err(EnrollmentVaultError::Crypto(
                "credential binding metadata mismatch".into(),
            ));
        }
        let ciphertext = BASE64
            .decode(&record.wrapped_b64)
            .map_err(|_| EnrollmentVaultError::Crypto("invalid credential base64".into()))?;
        let plaintext =
            unwrap_provider_credential(&key, &self.binding(aad_connection_id), &ciphertext)?;
        Ok(Some(UnwrappedCredential {
            secret: plaintext,
            enrolled_at: record.enrolled_at,
            expires_at: record.expires_at,
            account_hint: record.account_hint,
            bound_origin: record.bound_origin,
        }))
    }

    /// Returns non-secret status metadata for `connection_id` (`ENROLL-3`, `ENROLL-5`).
    pub fn status(
        &self,
        connection_id: &str,
    ) -> Result<ProviderCredentialView, EnrollmentVaultError> {
        let path = self.credential_path(connection_id)?;
        if !path.exists() {
            return Ok(ProviderCredentialView {
                connection_id: connection_id.to_owned(),
                office_id: self.office_id.clone(),
                enrolled: false,
                enrolled_at: None,
                expires_at: None,
                account_hint: None,
            });
        }
        let raw = fs::read(&path)
            .map_err(|_| EnrollmentVaultError::Io("could not read credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed credential file".into()))?;
        Ok(ProviderCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            enrolled: true,
            enrolled_at: Some(record.enrolled_at),
            expires_at: record.expires_at,
            account_hint: record.account_hint,
        })
    }

    /// Revokes and removes the stored wrapped credential (`ENROLL-5`).
    pub fn revoke(
        &self,
        connection_id: &str,
    ) -> Result<ProviderCredentialView, EnrollmentVaultError> {
        let path = self.credential_path(connection_id)?;
        remove_file_durably(&path)?;
        self.status(connection_id)
    }
}

pub(super) fn remove_file_durably(path: &Path) -> Result<(), EnrollmentVaultError> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(EnrollmentVaultError::Io(
                "could not remove credential".into(),
            ))
        }
    }
    if let Some(parent) = path.parent() {
        if parent.exists() {
            sync_dir(parent)?;
        }
    }
    Ok(())
}

pub(super) fn sync_dir(dir: &Path) -> Result<(), EnrollmentVaultError> {
    let handle = fs::File::open(dir)
        .map_err(|_| EnrollmentVaultError::Io("could not open credential directory".into()))?;
    org_platform::fs::full_sync(&handle)
        .map_err(|_| EnrollmentVaultError::Io("could not sync credential directory".into()))?;
    Ok(())
}

pub(super) fn sync_ancestor_dirs(dir: &Path, depth: usize) -> Result<(), EnrollmentVaultError> {
    let mut cur = Some(dir);
    for _ in 0..depth {
        let Some(d) = cur else {
            break;
        };
        if d.as_os_str().is_empty() || d.parent().is_none() {
            break;
        }
        sync_dir(d)?;
        cur = d.parent();
    }
    Ok(())
}

pub(super) fn write_private_atomic(
    parent: &Path,
    dest: &Path,
    bytes: &[u8],
) -> Result<(), EnrollmentVaultError> {
    let tmp = parent.join(format!(".credential.{}.tmp", random_hex(8)?));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|_| EnrollmentVaultError::Io("could not create credential temp file".into()))?;
    if file.write_all(bytes).is_err() || org_platform::fs::full_sync(&file).is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(EnrollmentVaultError::Io(
            "could not write credential temp file".into(),
        ));
    }
    drop(file);
    fs::rename(&tmp, dest).map_err(|_| {
        let _ = fs::remove_file(&tmp);
        EnrollmentVaultError::Io("could not commit credential file".into())
    })?;
    // Sync <conn_id>, auth, <office_id>, offices, and local so retries after any
    // earlier ancestor sync failure always persist the directory chain durably.
    sync_ancestor_dirs(parent, 5)
}

pub(super) fn ensure_owner_dirs(path: &Path) -> Result<(), EnrollmentVaultError> {
    if path.exists() {
        return Ok(());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        let prev = current.clone();
        current.push(component);
        if !current.exists() {
            match org_platform::fs::create_owner_only_dir(&current) {
                Ok(()) => {
                    if !prev.as_os_str().is_empty() {
                        sync_dir(&prev)?;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => {
                    return Err(EnrollmentVaultError::Io(
                        "could not create owner-only directory".into(),
                    ))
                }
            }
        }
    }
    Ok(())
}
