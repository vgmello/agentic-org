//! The stored pair of an `https` instance: its start, `tls.fetch`, `tls.set` and its renewal.
//!
//! The key is a host secret named `tls.<pair fingerprint>` (add-only); the registry's one row
//! names it and holds the chain and its source. A new pair is installed in three steps: its
//! key is added, the row names it, the previous key is deleted. A stop between them leaves a
//! key no row names, which the next start deletes. One writer at a time.
use std::path::Path;
use std::sync::{Arc, Mutex};

use org_host_control::listen::{ListenFailure, ListenOrigin};
use org_host_control::ops::{TlsRefusal, TlsSetParams};
use org_host_control::{HostOpResult, TlsPrivateKey};
use org_proto::{ApiError, ErrorCode};

use super::{check, hex, make, read_pem_file, Checked, Source};
use crate::host_log::log;
use crate::host_secrets::{HostSecrets, Name, Value};
use crate::hostdb::journal::Attempt;
use crate::hostdb::web_tls::{StoredPair, TlsLookup, TlsStored};
use crate::hostdb::HostDb;

/// Why the host does not start: its stored pair cannot be served, and it is not one the host
/// may make again. The text names the cause and what to do; never key material.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StartRefusal(pub(crate) String);

impl std::fmt::Display for StartRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why the stored pair cannot be served now: `tls.fetch`'s answers and the web start's gate.
#[derive(Debug)]
pub(crate) enum Unservable {
    /// No pair is stored (`tls_missing`).
    Missing,
    /// The stored pair does not cover the origin (`tls_name_mismatch`).
    Failure(ListenFailure),
    /// The stored pair is not valid now (`tls_not_valid`: expired, or not valid yet). Only
    /// the web start's gate and `instance.view` judge it; `tls.fetch` does not.
    NotValid,
    /// The key is gone, or the stored pair or the registry cannot be read: `storage_error`.
    Storage,
}

/// The listener's pair. Made at the start of an `https` instance ([`WebTls::start`]).
pub(crate) struct WebTls {
    db: Arc<HostDb>,
    secrets: Arc<dyn HostSecrets>,
    origin: ListenOrigin,
    /// Held by whatever installs a pair: the start, `tls.set` and the renewal.
    writing: Mutex<()>,
    /// The last stored pair the web start's gate verified with its key: its pair
    /// fingerprint and validity bounds. While the row names that pair, the gate judges only
    /// its validity, without reading the key again.
    verified: Mutex<Option<([u8; 32], i64, i64)>>,
}

fn secret_name(pair: &[u8; 32]) -> Result<Name, String> {
    Name::parse(&format!("tls.{}", hex(pair)))
        .map_err(|_| "the TLS key's name is not a host secret's".to_owned())
}

fn wall_ms() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
}

/// A host secret's bytes as PEM text, or `None` when they are not UTF-8.
fn key_text(value: &Value) -> Option<&str> {
    std::str::from_utf8(value.expose()).ok()
}

impl WebTls {
    /// The start of an `https` instance whose origin is `origin`, at `now` (Unix seconds),
    /// before its web process is started:
    ///
    /// - no stored pair: the pair setup verified, read again from `supplied` (`cert`, `key`)
    ///   and imported when setup's configuration named one, or a new certificate made here;
    /// - a stored pair whose key is gone: one made here is made again (and said so); a supplied
    ///   one refuses the start;
    /// - a stored pair that no longer verifies at `now` (expired, or not the origin's): one
    ///   made here is made again; a supplied one is kept and logged, and the web start's gate
    ///   serves no browser until the administrator replaces it (`tls.set`);
    /// - one made here with fewer than 30 days left: renewed.
    ///
    /// Every start logs the pair's source and fingerprint, and keys no row names are deleted.
    pub(crate) fn start(
        db: Arc<HostDb>,
        secrets: Arc<dyn HostSecrets>,
        origin: ListenOrigin,
        supplied: Option<(&Path, &Path)>,
        now: i64,
    ) -> Result<Arc<Self>, StartRefusal> {
        let tls = Arc::new(Self {
            db,
            secrets,
            origin,
            writing: Mutex::new(()),
            verified: Mutex::new(None),
        });
        let refused = |why: String| StartRefusal(why);
        let _writing = crate::sync::lock(&tls.writing);
        let stored = tls
            .db
            .web_tls_pair()
            .map_err(|e| refused(format!("reading the stored TLS pair: {e:?}")))?;
        let Some(stored) = stored else {
            let (checked, source) = match supplied {
                Some((cert, key)) => (
                    read_supplied(cert, key, &tls.origin, now).map_err(refused)?,
                    Source::Supplied,
                ),
                None => (make(&tls.origin, now).map_err(refused)?, Source::Made),
            };
            tls.install(&checked, source, None).map_err(refused)?;
            log(&format!(
                "the TLS certificate for {}: {}",
                tls.origin,
                checked.summary(source)
            ));
            tls.sweep(&checked.pair());
            return Ok(Arc::clone(&tls));
        };
        let name = secret_name(&stored.pair_fp).map_err(refused)?;
        let key = tls.secrets.load(&name).map_err(|e| {
            refused(format!(
                "reading the TLS key from the host's secrets: {e:?}"
            ))
        })?;
        let verified = key.as_ref().map(|key| {
            key_text(key)
                .ok_or(TlsRefusal::TlsInvalid)
                .and_then(|key| check(&stored.cert_pem, key, &[&tls.origin], Some(now)))
        });
        drop(key);
        let current = match (stored.source, verified) {
            (Source::Supplied, Some(Ok(checked))) => {
                log(&format!(
                    "the TLS certificate for {}: {}",
                    tls.origin,
                    checked.summary(stored.source)
                ));
                if checked.expiring(now) {
                    log(
                        "the supplied TLS certificate ends within 30 days; the instance does \
                         not renew a supplied pair: replace it before then (tls.set), or the \
                         instance does not start",
                    );
                }
                checked
            }
            (Source::Made, Some(Ok(checked))) if !checked.expiring(now) => {
                log(&format!(
                    "the TLS certificate for {}: {}",
                    tls.origin,
                    checked.summary(stored.source)
                ));
                checked
            }
            (Source::Made, verified) => {
                let why = match verified {
                    None => "its key is missing from the host's secrets; made again",
                    Some(Ok(_)) => "fewer than 30 days of its validity remain; renewed",
                    Some(Err(_)) => "it no longer verifies; made again",
                };
                let checked = make(&tls.origin, now).map_err(refused)?;
                tls.install(&checked, Source::Made, Some(&stored))
                    .map_err(refused)?;
                log(&format!(
                    "the TLS certificate for {}: the instance's own certificate: {why}: {}",
                    tls.origin,
                    checked.summary(Source::Made)
                ));
                checked
            }
            (Source::Supplied, None) => {
                return Err(refused(format!(
                    "the supplied TLS key for {} is missing from the host's secrets (the \
                     secrets volume in a container); a supplied key is never made again: \
                     restore the secrets this instance was set up with",
                    tls.origin
                )))
            }
            (Source::Supplied, Some(Err(why))) => {
                // The host starts, so that the administrator can replace the pair
                // (`tls.set`); the browser is not served until a pair that verifies is
                // stored (the web start's gate). Its key stays.
                log(&format!(
                    "the supplied TLS pair for {} (SHA-256 fingerprint {}) cannot be served: \
                     {}; a supplied pair is never replaced by the instance: the browser is not \
                     served until the administrator replaces it (tls.set)",
                    tls.origin,
                    leaf_fingerprint(&stored.cert_pem),
                    refusal_text(&why)
                ));
                tls.sweep(&stored.pair_fp);
                drop(_writing);
                return Ok(tls);
            }
        };
        tls.sweep(&current.pair());
        drop(_writing);
        Ok(tls)
    }

    /// Adds the key of `checked`, makes the registry name it (with `source`), then deletes the
    /// key of `previous` when it is another.
    fn install(
        &self,
        checked: &Checked,
        source: Source,
        previous: Option<&StoredPair>,
    ) -> Result<(), String> {
        let pair = self.stored(checked, source)?;
        self.add_key(checked)?;
        self.db
            .set_web_tls_pair(&pair)
            .map_err(|e| format!("storing the TLS pair: {e:?}"))?;
        self.forget(previous, &pair);
        Ok(())
    }

    fn stored(&self, checked: &Checked, source: Source) -> Result<StoredPair, String> {
        Ok(StoredPair {
            source,
            cert_pem: checked.chain().to_owned(),
            pair_fp: checked.pair(),
            set_at_ms: wall_ms().ok_or("the clock is unavailable")?,
        })
    }

    fn add_key(&self, checked: &Checked) -> Result<(), String> {
        let name = secret_name(&checked.pair())?;
        let value = Value::new(checked.key().as_bytes().to_vec())
            .map_err(|e| format!("keeping the TLS key: {e:?}"))?;
        self.secrets
            .put(&name, &value)
            .map_err(|e| format!("keeping the TLS key in the host's secrets: {e:?}"))
    }

    /// Deletes the key of `previous` once `now` names another.
    fn forget(&self, previous: Option<&StoredPair>, now: &StoredPair) {
        if let Some(previous) = previous.filter(|p| p.pair_fp != now.pair_fp) {
            if let Ok(name) = secret_name(&previous.pair_fp) {
                if self.secrets.delete(&name).is_err() {
                    log("could not delete the previous TLS key; the next start deletes it");
                }
            }
        }
    }

    /// Deletes every TLS key but `keep`'s: what a stop between the steps of an install left.
    fn sweep(&self, keep: &[u8; 32]) {
        let Ok(keep) = secret_name(keep) else {
            return;
        };
        let Ok(names) = self.secrets.list() else {
            log("could not list the host's secrets to delete unused TLS keys");
            return;
        };
        for name in names {
            if name.as_str().starts_with("tls.")
                && name != keep
                && self.secrets.delete(&name).is_err()
            {
                log("could not delete an unused TLS key");
            }
        }
    }

    /// The stored pair, verified for the origin and, at `now` (Unix seconds), for its
    /// validity (`None`: not judged, as `tls.fetch` does not), with its key from the host's
    /// secrets. The caller holds `writing`.
    fn current(&self, now: Option<i64>) -> Result<(Checked, Source), Unservable> {
        self.current_for(&self.origin, now)
    }

    /// [`WebTls::current`] for `origin`.
    fn current_for(
        &self,
        origin: &ListenOrigin,
        now: Option<i64>,
    ) -> Result<(Checked, Source), Unservable> {
        let stored = self
            .db
            .web_tls_pair()
            .map_err(|_| Unservable::Storage)?
            .ok_or(Unservable::Missing)?;
        let name = secret_name(&stored.pair_fp).map_err(|_| Unservable::Storage)?;
        let key = self
            .secrets
            .load(&name)
            .map_err(|_| Unservable::Storage)?
            .ok_or(Unservable::Storage)?;
        let key = key_text(&key).ok_or(Unservable::Storage)?;
        match check(&stored.cert_pem, key, &[origin], now) {
            Ok(checked) => Ok((checked, stored.source)),
            Err(TlsRefusal::TlsNotValid { .. }) => Err(Unservable::NotValid),
            Err(TlsRefusal::TlsNameMismatch { uncovered }) => {
                Err(Unservable::Failure(ListenFailure::TlsNameMismatch {
                    uncovered,
                }))
            }
            Err(_) => Err(Unservable::Storage),
        }
    }

    /// The web start's gate (X-02 §2, pre-spawn) at `now`: a stored pair is present
    /// (`tls_missing`), covers the origin (`tls_name_mismatch`) and is valid now; its key can
    /// be read.
    ///
    /// The pair it verified is remembered by its fingerprint: while the row still names it,
    /// only its validity is judged again, so the gate, asked every few seconds, reads the
    /// registry but not the key. A new pair (installed by `tls.set` or a renewal) is
    /// verified whole.
    pub(crate) fn servable(&self, now: i64) -> Result<(), Unservable> {
        let _writing = crate::sync::lock(&self.writing);
        self.judged(now).map(drop)
    }

    /// [`WebTls::servable`]'s judgement, the stored pair's validity bounds when it can be
    /// served. The caller holds `writing`.
    fn judged(&self, now: i64) -> Result<(i64, i64), Unservable> {
        let stored = self
            .db
            .web_tls_pair()
            .map_err(|_| Unservable::Storage)?
            .ok_or(Unservable::Missing)?;
        let mut verified = crate::sync::lock(&self.verified);
        if let Some((pair, not_before, not_after)) = *verified {
            if pair == stored.pair_fp {
                return if (not_before..=not_after).contains(&now) {
                    Ok((not_before, not_after))
                } else {
                    Err(Unservable::NotValid)
                };
            }
        }
        let (checked, _) = self.current(Some(now))?;
        *verified = Some((checked.pair(), checked.not_before, checked.not_after));
        Ok((checked.not_before, checked.not_after))
    }

    /// The pair, for the instance's own origin (tests).
    #[cfg(test)]
    pub(crate) fn fetch(&self) -> Result<(String, TlsPrivateKey), Unservable> {
        self.fetch_for(&self.origin.clone())
    }

    /// `tls.fetch`'s pair: the chain and the key, re-checked for `origin`, the snapshot's
    /// (X-02 §6.1). Read while no pair is being installed, so the row and its key are one
    /// pair.
    pub(crate) fn fetch_for(
        &self,
        origin: &ListenOrigin,
    ) -> Result<(String, TlsPrivateKey), Unservable> {
        let _writing = crate::sync::lock(&self.writing);
        let (mut checked, _) = self.current_for(origin, None).inspect_err(|_| {
            // A pair that cannot be read (its key lost while the host ran) is verified whole
            // again by the gate, which then keeps the web process waiting.
            *crate::sync::lock(&self.verified) = None;
        })?;
        let key = TlsPrivateKey::new(std::mem::take(&mut *checked.key))
            .map_err(|_| Unservable::Storage)?;
        Ok((std::mem::take(&mut checked.chain), key))
    }

    /// What `instance.view` says of the pair at `now`: served, expiring within 30 days, or
    /// missing (unservable, an expired pair included). Judged as the gate judges it, so a
    /// pair already verified is not read again.
    pub(crate) fn status(&self, now: i64) -> org_proto::gateway::TlsStatus {
        use org_proto::gateway::TlsStatus;
        let _writing = crate::sync::lock(&self.writing);
        match self.judged(now) {
            Ok((_, not_after)) if not_after - now < super::RENEW_WITHIN_DAYS * super::DAY_SECS => {
                TlsStatus::Expiring
            }
            Ok(_) => TlsStatus::Ok,
            Err(_) => TlsStatus::Missing,
        }
    }

    /// `tls.set` (X-02 §6.2) from the administrator `account` at `now` (Unix seconds): the
    /// journal lookup, then the pair's checks in the contract's order, then the pair is
    /// installed as supplied. `Ok(true)` with a result that installed a pair: the caller emits
    /// `tls.renew`. Refusals other than `tls_key_mismatch` are answered and not stored.
    ///
    /// The listen configuration's checks (`origin_invalid`, `hsts_invalid`) pass by
    /// construction: this host's snapshot was validated when it was read, and the scheme is
    /// never changed.
    pub(crate) fn set(
        &self,
        params: &TlsSetParams,
        account: &str,
        now: i64,
    ) -> Result<(HostOpResult<org_proto::Empty, TlsRefusal>, bool), ApiError> {
        let storage =
            |_| ApiError::new(ErrorCode::StorageError, "the TLS pair could not be stored");
        let rejected = |refusal| HostOpResult::Rejected { refusal };
        let Ok(fingerprint) = org_host_control::journal::fingerprint(params) else {
            return Ok((rejected(TlsRefusal::TlsInvalid), false));
        };
        let op = Attempt {
            id: &params.operation_id,
            kind: <TlsSetParams as org_host_control::journal::Journaled>::METHOD,
            fingerprint,
            account,
        };
        let wall =
            wall_ms().ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
        let _writing = crate::sync::lock(&self.writing);
        if let TlsLookup::Replay(stored) = self.db.tls_set_lookup(&op, wall).map_err(journal)? {
            return Ok((replayed(stored)?, false));
        }
        // Coverage of "every https origin among the stored one and the running one": they
        // are this one origin, since the registered start serves only the listener setup
        // recorded, and setup never changes it once the instance is reserved.
        let checked = match check(
            &params.cert,
            params.key.expose(),
            &[&self.origin],
            Some(now),
        ) {
            Ok(checked) => checked,
            Err(TlsRefusal::TlsKeyMismatch) => {
                let stored: TlsStored = HostOpResult::Rejected {
                    refusal: TlsRefusal::TlsKeyMismatch,
                };
                let answer = self
                    .db
                    .tls_set_write(&op, wall, &stored, None)
                    .map_err(journal)?
                    .unwrap_or(stored);
                return Ok((replayed(answer)?, false));
            }
            Err(refusal) => return Ok((rejected(refusal), false)),
        };
        let previous = self.db.web_tls_pair().map_err(storage)?;
        let pair = self
            .stored(&checked, Source::Supplied)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
        self.add_key(&checked).map_err(|_| {
            ApiError::new(ErrorCode::StorageError, "the TLS key could not be stored")
        })?;
        let completed: TlsStored = HostOpResult::Completed {
            result: serde_json::json!({}),
        };
        match self
            .db
            .tls_set_write(&op, wall, &completed, Some(&pair))
            .map_err(journal)?
        {
            None => {
                self.forget(previous.as_ref(), &pair);
                log(&format!(
                    "the TLS certificate for {} was replaced: {}",
                    self.origin,
                    checked.summary(Source::Supplied)
                ));
                Ok((replayed(completed)?, true))
            }
            // Another request with this operation stored its result first; the key added
            // above is this pair's, and is either named by that result or deleted at the next
            // start.
            Some(existing) => Ok((replayed(existing)?, false)),
        }
    }

    /// Renews the instance's own certificate when fewer than 30 days of it remain at `now`,
    /// or when its key is missing or it no longer verifies, as a start does. `Ok(true)`:
    /// renewed; the caller emits `tls.renew`. A supplied pair is never renewed. An error
    /// reading the registry or the secrets changes nothing: it is reported, and the next
    /// look tries again.
    pub(crate) fn renew_if_expiring(&self, now: i64) -> Result<bool, String> {
        let _writing = crate::sync::lock(&self.writing);
        let stored = self
            .db
            .web_tls_pair()
            .map_err(|e| format!("reading the stored TLS pair: {e:?}"))?;
        let Some(stored) = stored.filter(|s| s.source == Source::Made) else {
            return Ok(false);
        };
        let key = self
            .secrets
            .load(&secret_name(&stored.pair_fp)?)
            .map_err(|e| format!("reading the TLS key from the host's secrets: {e:?}"))?;
        if let Some(key) = &key {
            let verified = key_text(key)
                .ok_or(TlsRefusal::TlsInvalid)
                .and_then(|key| check(&stored.cert_pem, key, &[&self.origin], Some(now)));
            if verified.is_ok_and(|checked| !checked.expiring(now)) {
                return Ok(false);
            }
        }
        drop(key);
        let checked = make(&self.origin, now)?;
        self.install(&checked, Source::Made, Some(&stored))?;
        log(&format!(
            "the TLS certificate for {}: the instance's own certificate was renewed: {}",
            self.origin,
            checked.summary(Source::Made)
        ));
        Ok(true)
    }
}

/// The fingerprint of `chain`'s leaf, read without its key, or `unreadable`.
fn leaf_fingerprint(chain: &str) -> String {
    use sha2::Digest;
    org_host_control::pem::first_block(chain, org_host_control::pem::CERTIFICATE)
        .map(|der| super::Fingerprint(sha2::Sha256::digest(der.bytes()).into()).to_string())
        .unwrap_or_else(|_| "unreadable".into())
}

/// A stored `tls.set` result as its answer.
fn replayed(stored: TlsStored) -> Result<HostOpResult<org_proto::Empty, TlsRefusal>, ApiError> {
    match stored {
        HostOpResult::Completed { .. } => Ok(HostOpResult::Completed {
            result: org_proto::Empty {},
        }),
        HostOpResult::Rejected { refusal } => Ok(HostOpResult::Rejected { refusal }),
        _ => Err(ApiError::new(
            ErrorCode::StorageError,
            "the stored result could not be read",
        )),
    }
}

/// A journal error as `tls.set`'s plain error (never stored).
fn journal(error: crate::hostdb::Error) -> ApiError {
    use crate::hostdb::Error;
    match error {
        Error::OperationReused => ApiError::new(
            ErrorCode::OperationReused,
            "the operation ID was used for another operation",
        ),
        Error::Expired => ApiError::new(ErrorCode::Expired, "the operation ID has expired"),
        Error::ClockSkew => {
            ApiError::new(ErrorCode::ClockSkew, "the operation ID is from the future")
        }
        Error::InvalidId => ApiError::new(ErrorCode::InvalidRequest, "invalid operation ID"),
        _ => ApiError::new(ErrorCode::StorageError, "the host registry is unavailable"),
    }
}

/// The supplied pair at `cert` and `key`, read and checked for `origin` at `now`.
pub(crate) fn read_supplied(
    cert: &Path,
    key: &Path,
    origin: &ListenOrigin,
    now: i64,
) -> Result<Checked, String> {
    let cert_text = read_pem_file(cert).map_err(|why| format!("the TLS certificate {why}"))?;
    let key_text = read_pem_file(key).map_err(|why| format!("the TLS key {why}"))?;
    check(&cert_text, &key_text, &[origin], Some(now)).map_err(|why| {
        format!(
            "the TLS pair ({}, {}) is refused: {}",
            cert.display(),
            key.display(),
            refusal_text(&why)
        )
    })
}

/// A refusal in words: its code and what it means.
pub(crate) fn refusal_text(refusal: &TlsRefusal) -> String {
    match refusal {
        TlsRefusal::TlsInvalid => "tls_invalid: the certificate chain or the key does not parse \
             (PEM CERTIFICATE blocks, leaf first, and an unencrypted PRIVATE KEY, RSA PRIVATE KEY \
             or EC PRIVATE KEY block)"
            .into(),
        TlsRefusal::TlsKeyMismatch => "tls_key_mismatch: the key is not the certificate's".into(),
        TlsRefusal::TlsNotValid {
            not_before,
            not_after,
        } => format!(
            "tls_not_valid: the certificate is valid from {} to {}, not now",
            serde_json::to_string(not_before).unwrap_or_default(),
            serde_json::to_string(not_after).unwrap_or_default()
        ),
        TlsRefusal::TlsNameMismatch { uncovered } => format!(
            "tls_name_mismatch: the certificate names no subject alternative name for {}",
            uncovered
                .origins()
                .iter()
                .map(|o| o.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    }
}
