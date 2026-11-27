//! The stored pair of an `https` instance: its start, `tls.fetch`, and its renewal.
//!
//! The key is a host secret named `tls.<pair fingerprint>` (add-only); the registry's one row
//! names it and holds the chain and its source. A new pair is installed in three steps: its
//! key is added, the row names it, the previous key is deleted. A stop between them leaves a
//! key no row names, which the next start deletes. One writer at a time.

use std::path::Path;
use std::sync::{Arc, Mutex};

use org_host_control::listen::{ListenFailure, ListenOrigin};
use org_host_control::ops::TlsRefusal;
use org_host_control::TlsPrivateKey;

use super::{
    check, hex, leaf_fingerprint, make, read_supplied, refusal_text, Checked, Fingerprint, Source,
};
use crate::host_log::log;
use crate::host_secrets::{HostSecrets, Name, Value};
use crate::hostdb::web_tls::StoredPair;
use crate::hostdb::WebTlsStore;

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
    pub(super) db: Arc<dyn WebTlsStore>,
    pub(super) secrets: Arc<dyn HostSecrets>,
    pub(super) origin: ListenOrigin,
    /// Held by whatever installs a pair: the start, `tls.set` and the renewal.
    pub(super) writing: Mutex<()>,
    /// Cached verification summary `(pair_fp, not_before, not_after)` for the gate.
    pub(super) verified: Mutex<Option<([u8; 32], i64, i64)>>,
}

pub(super) fn secret_name(pair: &[u8; 32]) -> Result<Name, String> {
    Name::parse(&format!("tls.{}", hex(pair)))
        .map_err(|_| "the TLS key's name is not a host secret's".to_owned())
}

pub(super) fn wall_ms() -> Option<i64> {
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
    /// before its web process is started.
    pub(crate) fn start(
        db: Arc<dyn WebTlsStore>,
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
        let _writing = crate::sync::lock(&tls.writing);
        let stored = tls
            .db
            .web_tls_pair()
            .map_err(|e| StartRefusal(format!("reading the stored TLS pair: {e:?}")))?;
        match stored {
            None => tls.initialize_first_pair(supplied, now)?,
            Some(stored) => tls.reconcile_stored_pair(&stored, now)?,
        }
        drop(_writing);
        Ok(tls)
    }

    fn initialize_first_pair(
        &self,
        supplied: Option<(&Path, &Path)>,
        now: i64,
    ) -> Result<(), StartRefusal> {
        let (checked, source) = match supplied {
            Some((cert, key)) => (
                read_supplied(cert, key, &self.origin, now).map_err(StartRefusal)?,
                Source::Supplied,
            ),
            None => (make(&self.origin, now).map_err(StartRefusal)?, Source::Made),
        };
        self.install(&checked, source, None).map_err(StartRefusal)?;
        log(&format!(
            "the TLS certificate for {}: {}",
            self.origin,
            checked.summary(source)
        ));
        self.sweep(&checked.pair());
        Ok(())
    }

    fn reconcile_stored_pair(&self, stored: &StoredPair, now: i64) -> Result<(), StartRefusal> {
        let name = secret_name(&stored.pair_fp).map_err(StartRefusal)?;
        let key = self.secrets.load(&name).map_err(|e| {
            StartRefusal(format!(
                "reading the TLS key from the host's secrets: {e:?}"
            ))
        })?;
        let verified = key.as_ref().map(|key| {
            key_text(key)
                .ok_or(TlsRefusal::TlsInvalid)
                .and_then(|key| check(&stored.cert_pem, key, &[&self.origin], Some(now)))
        });
        drop(key);
        let keep_fp = match (stored.source, verified) {
            (Source::Supplied, Some(Ok(checked))) => {
                log(&format!(
                    "the TLS certificate for {}: {}",
                    self.origin,
                    checked.summary(stored.source)
                ));
                if checked.expiring(now) {
                    log(
                        "the supplied TLS certificate ends within 30 days; the instance does \
                         not renew a supplied pair: replace it before then (tls.set), or the \
                         instance does not start",
                    );
                }
                checked.pair()
            }
            (Source::Made, Some(Ok(checked))) if !checked.expiring(now) => {
                log(&format!(
                    "the TLS certificate for {}: {}",
                    self.origin,
                    checked.summary(stored.source)
                ));
                checked.pair()
            }
            (Source::Made, verified) => {
                let why = match verified {
                    None => "its key is missing from the host's secrets; made again",
                    Some(Ok(_)) => "fewer than 30 days of its validity remain; renewed",
                    Some(Err(_)) => "it no longer verifies; made again",
                };
                let checked = make(&self.origin, now).map_err(StartRefusal)?;
                self.install(&checked, Source::Made, Some(stored))
                    .map_err(StartRefusal)?;
                log(&format!(
                    "the TLS certificate for {}: the instance's own certificate: {why}: {}",
                    self.origin,
                    checked.summary(Source::Made)
                ));
                checked.pair()
            }
            (Source::Supplied, None) => {
                return Err(StartRefusal(format!(
                    "the supplied TLS key for {} is missing from the host's secrets (the \
                     secrets volume in a container); a supplied key is never made again: \
                     restore the secrets this instance was set up with",
                    self.origin
                )));
            }
            (Source::Supplied, Some(Err(why))) => {
                log(&format!(
                    "the supplied TLS pair for {} (SHA-256 fingerprint {}) cannot be served: \
                     {}; a supplied pair is never replaced by the instance: the browser is not \
                     served until the administrator replaces it (tls.set)",
                    self.origin,
                    leaf_fingerprint(&stored.cert_pem),
                    refusal_text(&why)
                ));
                stored.pair_fp
            }
        };
        self.sweep(&keep_fp);
        Ok(())
    }

    /// Installs a verified pair on a plain-HTTP instance that is opting in to HTTPS
    /// (ADR-0033 / TASK-62).
    pub(crate) fn install_for_upgrade(
        db: &dyn WebTlsStore,
        secrets: &dyn HostSecrets,
        checked: &Checked,
        source: Source,
    ) -> Result<Fingerprint, String> {
        let pair = StoredPair {
            source,
            cert_pem: checked.chain().to_owned(),
            pair_fp: checked.pair(),
            set_at_ms: wall_ms().ok_or("the clock is unavailable")?,
        };
        let previous = db
            .web_tls_pair()
            .map_err(|e| format!("reading the stored TLS pair: {e:?}"))?;
        let name = secret_name(&checked.pair())?;
        let value = Value::new(checked.key().as_bytes().to_vec())
            .map_err(|e| format!("keeping the TLS key: {e:?}"))?;
        secrets
            .put(&name, &value)
            .map_err(|e| format!("keeping the TLS key in the host's secrets: {e:?}"))?;
        if let Err(e) = db.set_web_tls_pair(&pair) {
            if previous.as_ref().is_none_or(|p| p.pair_fp != pair.pair_fp) {
                let _ = secrets.delete(&name);
            }
            return Err(format!("storing the TLS pair: {e:?}"));
        }
        if let Some(prev) = previous.filter(|p| p.pair_fp != pair.pair_fp) {
            if let Ok(prev_name) = secret_name(&prev.pair_fp) {
                let _ = secrets.delete(&prev_name);
            }
        }
        Ok(checked.fingerprint())
    }

    /// Generates and installs a fresh self-signed certificate on a live HTTPS host under the
    /// write lock, updating the gate cache so the caller can announce `tls.renew`.
    pub(crate) fn install_made(&self, now_secs: i64) -> Result<Fingerprint, String> {
        let _guard = crate::sync::lock(&self.writing);
        let checked = make(&self.origin, now_secs)?;
        let previous = self
            .db
            .web_tls_pair()
            .map_err(|e| format!("reading the stored TLS pair: {e:?}"))?;
        self.install(&checked, Source::Made, previous.as_ref())?;
        let fingerprint = checked.fingerprint();
        *crate::sync::lock(&self.verified) =
            Some((checked.pair(), checked.not_before(), checked.not_after()));
        Ok(fingerprint)
    }

    /// The leaf SHA-256 fingerprint of the currently stored pair, if readable.
    pub(crate) fn stored_fingerprint(&self) -> Option<String> {
        let stored = self.db.web_tls_pair().ok()??;
        Some(leaf_fingerprint(&stored.cert_pem))
    }

    fn install(
        &self,
        checked: &Checked,
        source: Source,
        previous: Option<&StoredPair>,
    ) -> Result<(), String> {
        let pair = self.stored(checked, source)?;
        self.add_key(checked)?;
        if let Err(e) = self.db.set_web_tls_pair(&pair) {
            self.drop_unnamed(checked);
            return Err(format!("storing the TLS pair: {e:?}"));
        }
        self.forget(previous, &pair);
        Ok(())
    }

    pub(super) fn stored(&self, checked: &Checked, source: Source) -> Result<StoredPair, String> {
        Ok(StoredPair {
            source,
            cert_pem: checked.chain().to_owned(),
            pair_fp: checked.pair(),
            set_at_ms: wall_ms().ok_or("the clock is unavailable")?,
        })
    }

    pub(super) fn add_key(&self, checked: &Checked) -> Result<(), String> {
        let name = secret_name(&checked.pair())?;
        let value = Value::new(checked.key().as_bytes().to_vec())
            .map_err(|e| format!("keeping the TLS key: {e:?}"))?;
        self.secrets
            .put(&name, &value)
            .map_err(|e| format!("keeping the TLS key in the host's secrets: {e:?}"))
    }

    pub(super) fn drop_unnamed(&self, checked: &Checked) {
        match self.db.web_tls_pair() {
            Ok(Some(row)) if row.pair_fp != checked.pair() => {}
            Ok(None) => {}
            _ => return,
        }
        if let Ok(name) = secret_name(&checked.pair()) {
            if self.secrets.delete(&name).is_err() {
                log("could not delete an unused TLS key; the next start deletes it");
            }
        }
    }

    pub(super) fn forget(&self, previous: Option<&StoredPair>, now: &StoredPair) {
        if let Some(previous) = previous.filter(|p| p.pair_fp != now.pair_fp) {
            if let Ok(name) = secret_name(&previous.pair_fp) {
                if self.secrets.delete(&name).is_err() {
                    log("could not delete the previous TLS key; the next start deletes it");
                }
            }
        }
    }

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

    fn current(&self, now: Option<i64>) -> Result<(Checked, Source), Unservable> {
        self.current_for(&self.origin, now)
    }

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

    /// The web start's gate (X-02 §2, pre-spawn) at `now`.
    pub(crate) fn servable(&self, now: i64) -> Result<(), Unservable> {
        let _writing = crate::sync::lock(&self.writing);
        self.judged(now).map(drop)
    }

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
        let checked = match self.current(Some(now)) {
            Ok((checked, _)) => checked,
            Err(Unservable::NotValid) => {
                if let Ok((checked, _)) = self.current(None) {
                    *verified = Some((checked.pair(), checked.not_before(), checked.not_after()));
                }
                return Err(Unservable::NotValid);
            }
            Err(other) => return Err(other),
        };
        *verified = Some((checked.pair(), checked.not_before(), checked.not_after()));
        Ok((checked.not_before(), checked.not_after()))
    }

    /// The pair, for the instance's own origin (tests).
    #[cfg(any(test, all(feature = "test-support", debug_assertions)))]
    pub(crate) fn fetch(&self) -> Result<(String, TlsPrivateKey), Unservable> {
        self.fetch_for(&self.origin.clone())
    }

    /// `tls.fetch`'s pair: the chain and the key, re-checked for `origin`.
    pub(crate) fn fetch_for(
        &self,
        origin: &ListenOrigin,
    ) -> Result<(String, TlsPrivateKey), Unservable> {
        let _writing = crate::sync::lock(&self.writing);
        let (checked, _) = self.current_for(origin, None).inspect_err(|_| {
            *crate::sync::lock(&self.verified) = None;
        })?;
        let (chain, mut key_pem) = checked.into_parts();
        let key =
            TlsPrivateKey::new(std::mem::take(&mut *key_pem)).map_err(|_| Unservable::Storage)?;
        Ok((chain, key))
    }

    /// What `instance.view` says of the pair at `now`.
    pub(crate) fn status(&self, now: i64) -> org_proto::web::TlsStatus {
        use org_proto::web::TlsStatus;
        let _writing = crate::sync::lock(&self.writing);
        match self.judged(now) {
            Ok((_, not_after)) if not_after - now < super::RENEW_WITHIN_DAYS * super::DAY_SECS => {
                TlsStatus::Expiring
            }
            Ok(_) => TlsStatus::Ok,
            Err(_) => TlsStatus::Missing,
        }
    }

    /// Renews the instance's own certificate when fewer than 30 days of it remain at `now`,
    /// or when its key is missing or it no longer verifies.
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
