//! Live host-account sessions. The registry remains authoritative on every call;
//! handles are random, connection-bound, hashed in memory, and never persisted.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use org_host_control::{
    ops::{self, LinkedOrg},
    Password, SessionHandle,
};
use org_proto::{ApiError, ErrorCode};
use sha2::{Digest, Sha256};

use crate::accounts::identity::AccountId;
use crate::control_listener::Connection;
use crate::host_secrets::HostSecrets;
use crate::hostdb::{
    accounts::Account, journal::Verifier, HostAccountsStore, HostStore, OperationJournalStore,
    ProvisioningStore, RecoveryStore, WebTlsStore,
};
use crate::links::OneTimeLinks;

mod grants;
mod sign_in;

/// Argon2 runs at most this many at once, native and browser paths together.
const MAX_HASHES: usize = 2;
// The limits below apply only to native sessions (this connection-bound map).
// Browser sessions live in the `BrowserSessions` store: no absolute lifetime,
// no per-connection cap, the 30-day idle rule and the store's host-wide
// `SESSION_LIMIT` (A3 plan B4 step 5).
const MAX_SESSIONS: usize = 1024;
const MAX_CONNECTION_SESSIONS: usize = 32;
const ABSOLUTE: Duration = Duration::from_secs(12 * 3600);
const IDLE: Duration = Duration::from_secs(30 * 60);

pub(crate) struct Authentication {
    db: Arc<dyn HostStore>,
    #[cfg(test)]
    memory_for_test: Option<Arc<crate::hostdb::memory::MemoryHostStore>>,
    // Retain custody lifetime (including the debug store's exclusive lock).
    _custody: Arc<dyn HostSecrets>,
    dummy: Verifier,
    hashing: AtomicUsize,
    sessions: Mutex<BTreeMap<[u8; 32], Session>>,
    web_channels: Mutex<BTreeMap<String, WebChannel>>,
    touch_batcher: touch_batch::TouchBatcher,
}

/// Monotonic expiry also runs when no client makes another request. Removing a
/// session drops its issued channel grants through the actual transfer service.
pub(crate) struct Lifecycle {
    stop: Arc<std::sync::atomic::AtomicBool>,
    authentication: Weak<Authentication>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Lifecycle {
    pub(crate) fn start(authentication: &Arc<Authentication>) -> std::io::Result<Self> {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let weak = Arc::downgrade(authentication);
        let for_drop = Arc::downgrade(authentication);
        let thread = std::thread::Builder::new()
            .name("org-session-lifecycle".into())
            .spawn(move || {
                while !stopping.load(Ordering::SeqCst) {
                    let Some(authentication) = weak.upgrade() else {
                        break;
                    };
                    authentication.reap();
                    drop(authentication);
                    std::thread::park_timeout(Duration::from_millis(100));
                }
            })?;
        Ok(Self {
            stop,
            authentication: for_drop,
            thread: Some(thread),
        })
    }
}
impl Lifecycle {
    /// Stops the expiry and waits for it until `until` at the latest, and says whether it
    /// ended. A sweep waiting for the registry (a create holding it, up to its busy timeout)
    /// is left to end on its own: a stopping host does not wait for it past its bound.
    pub(crate) fn join_by(mut self, until: std::time::Instant) -> bool {
        self.stop.store(true, Ordering::SeqCst);
        let joined = self.thread.take().is_none_or(|thread| {
            thread.thread().unpark();
            crate::sync::join_by(thread, until)
        });
        if let Some(auth) = self.authentication.upgrade() {
            auth.flush_touches(true);
        }
        joined
    }
}
impl Drop for Lifecycle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
        if let Some(auth) = self.authentication.upgrade() {
            auth.flush_touches(true);
        }
    }
}
struct Session {
    account: AccountId,
    password_changed_at_ms: i64,
    local_authless: bool,
    id: String,
    connection: Weak<AtomicBool>,
    created: Instant,
    touched: Instant,
    channels: BTreeMap<String, Channel>,
    authority_live: Arc<AtomicBool>,
}
struct Channel {
    endpoint: Weak<crate::supervisor::channels::Endpoint>,
    organization: String,
    satellite_operator: bool,
    _slot: crate::supervisor::channels::ChannelSlot,
}
struct WebChannel {
    endpoint: Weak<crate::supervisor::channels::Endpoint>,
    organization: String,
    connection: Weak<AtomicBool>,
    webhook_only: bool,
    _slot: crate::supervisor::channels::ChannelSlot,
}
impl WebChannel {
    fn live(&self) -> bool {
        self._slot.is_live()
            && self.endpoint.upgrade().is_some_and(|e| e.is_live())
            && self
                .connection
                .upgrade()
                .is_some_and(|closed| !closed.load(Ordering::SeqCst))
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.authority_live.store(false, Ordering::SeqCst);
        let mut revoked = std::collections::BTreeSet::new();
        for channel in self.channels.values() {
            if let Some(endpoint) = channel.endpoint.upgrade() {
                if revoked.insert(endpoint.office.clone()) {
                    let _ = endpoint.revoke(&self.id);
                }
            }
        }
    }
}
impl Session {
    fn live(&self, at: Instant) -> bool {
        at.duration_since(self.created) < ABSOLUTE
            && at.duration_since(self.touched) < IDLE
            && self
                .connection
                .upgrade()
                .is_some_and(|closed| !closed.load(Ordering::SeqCst))
    }
}
struct HashPermit<'a>(&'a AtomicUsize);
impl Drop for HashPermit<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
fn denied() -> ApiError {
    ApiError::new(ErrorCode::NotAuthorized, "authentication refused")
}
fn unavailable() -> ApiError {
    ApiError::new(ErrorCode::Unavailable, "authentication unavailable")
}
fn storage(_: crate::hostdb::Error) -> ApiError {
    unavailable()
}
pub(super) fn wall_ms() -> Result<u64, ApiError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or_else(unavailable)
}
/// Expired operation journal rows, with their account-create verifiers, go before any request
/// is admitted. Maintenance only: lookup already refuses expired rows and every journaled write
/// prunes again, so a failure is logged and the start goes on.
fn prune_expired_journal(db: &dyn OperationJournalStore) {
    let pruned = wall_ms()
        .ok()
        .and_then(|wall| i64::try_from(wall).ok())
        .ok_or(crate::hostdb::Error::Unsafe)
        .and_then(|wall| db.journal_prune_expired(wall));
    match pruned {
        Ok(outcome) if outcome.more_remaining => {
            crate::host_log::log(&format!(
                "expired operation journal startup prune reached its batch limit after removing \
                 {} entries; remaining expired entries will drain on subsequent journaled writes",
                outcome.removed
            ));
        }
        Ok(_) => {}
        Err(error) => {
            crate::host_log::log(&format!(
                "expired operation journal entries were not pruned at start ({error:?}); \
                 each journaled write still prunes a batch of them"
            ));
        }
    }
}
fn argon() -> Result<Argon2<'static>, ApiError> {
    Ok(Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(65536, 3, 1, Some(32)).map_err(|_| unavailable())?,
    ))
}
/// Shared by actual protected bootstrap/account changes. Passwords are never
/// represented by a fast fingerprint or returned by host operations.
pub(crate) fn hash_password(password: &Password) -> Result<Verifier, ApiError> {
    if password.expose().len() > 4096 {
        return Err(denied());
    }
    let random = crate::secrets::generate_office_secrets_key().map_err(|_| unavailable())?;
    let salt = SaltString::encode_b64(&random.expose_bytes()[..16]).map_err(|_| unavailable())?;
    Verifier::new(
        argon()?
            .hash_password(password.expose().as_bytes(), &salt)
            .map_err(|_| unavailable())?
            .to_string(),
    )
    .map_err(|_| unavailable())
}
fn digest(handle: &SessionHandle) -> [u8; 32] {
    Sha256::digest(handle.expose().as_bytes()).into()
}
fn random_handle() -> Result<SessionHandle, ApiError> {
    let bytes = crate::secrets::generate_office_secrets_key().map_err(|_| unavailable())?;
    SessionHandle::new(
        bytes
            .expose_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
    .map_err(|_| unavailable())
}
impl Authentication {
    #[cfg(any(test, all(feature = "test-support", debug_assertions)))]
    pub(crate) fn provisioning_storage(&self) -> (Arc<dyn HostStore>, Arc<dyn HostSecrets>) {
        (Arc::clone(&self.db), Arc::clone(&self._custody))
    }
    pub(crate) fn custody(&self) -> Arc<dyn HostSecrets> {
        Arc::clone(&self._custody)
    }
    pub(crate) fn accounts_store(&self) -> Arc<dyn HostAccountsStore> {
        Arc::clone(&self.db) as Arc<dyn HostAccountsStore>
    }
    pub(crate) fn journal_store(&self) -> Arc<dyn OperationJournalStore> {
        Arc::clone(&self.db) as Arc<dyn OperationJournalStore>
    }
    pub(crate) fn provisioning_store(&self) -> Arc<dyn ProvisioningStore> {
        Arc::clone(&self.db) as Arc<dyn ProvisioningStore>
    }
    pub(crate) fn recovery_store(&self) -> Arc<dyn RecoveryStore> {
        Arc::clone(&self.db) as Arc<dyn RecoveryStore>
    }
    pub(crate) fn web_tls_store(&self) -> Arc<dyn WebTlsStore> {
        Arc::clone(&self.db) as Arc<dyn WebTlsStore>
    }
    pub(crate) fn links_store(&self) -> Arc<dyn OneTimeLinks + Send + Sync> {
        Arc::clone(&self.db) as Arc<dyn OneTimeLinks + Send + Sync>
    }
    #[cfg(test)]
    pub(crate) fn memory_for_test(&self) -> Arc<crate::hostdb::memory::MemoryHostStore> {
        Arc::clone(
            self.memory_for_test
                .as_ref()
                .expect("in-memory test fixture required"),
        )
    }
    /// The name the organization was created with; empty when no create of this host
    /// made it. A registry that cannot be read is an error, never an empty name: a
    /// caller resolving a name must not be told the account can see no organization.
    fn organization_name(&self, org: &str) -> Result<String, ApiError> {
        Ok(self
            .db
            .provisioning_linked_to(org)
            .map_err(storage)?
            .map(|r| r.prepared.plan.name)
            .unwrap_or_default())
    }
    pub(super) fn inspect(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
    ) -> Result<ops::SessionInspectResult, ApiError> {
        let account = self.authorize(connection, handle)?;
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let session = sessions.get(&digest(handle)).ok_or_else(denied)?;
        let now = Instant::now();
        if !session.live(now) {
            return Err(denied());
        }
        let wall = wall_ms()?;
        let absolute = ABSOLUTE
            .checked_sub(now.duration_since(session.created))
            .ok_or_else(denied)?;
        let idle = IDLE
            .checked_sub(now.duration_since(session.touched))
            .ok_or_else(denied)?
            .min(absolute);
        Ok(ops::SessionInspectResult {
            account: account.account_id().into(),
            login: account.login().into(),
            admin: account.admin(),
            orgs: account
                .links()
                .iter()
                .map(|l| LinkedOrg {
                    org_id: l.org_id().into(),
                    name: String::new(),
                    employee_id: l.member_id().into(),
                    office_ids: vec![],
                })
                .collect(),
            server_time_ms: wall,
            idle_expires_at_ms: wall
                .checked_add(idle.as_millis() as u64)
                .ok_or_else(unavailable)?,
        })
    }
    fn reap(&self) {
        let removed = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return;
            };
            let now = Instant::now();
            let keys: Vec<_> = sessions
                .iter()
                .filter(|(_, session)| {
                    if !session.live(now) {
                        return true;
                    }
                    let account = self.db.account_authority(&session.account).ok().flatten();
                    account.is_none_or(|account| {
                        account.revoked_at_ms().is_some()
                            || account.password_changed_at_ms() != session.password_changed_at_ms
                            || (session.local_authless
                                && self.db.local_authless_account().ok().flatten().as_ref()
                                    != Some(&session.account))
                            || session.channels.values().any(|channel| {
                                let linked = account
                                    .links()
                                    .iter()
                                    .any(|link| link.org_id() == channel.organization);
                                if channel.satellite_operator {
                                    !(linked || account.admin())
                                } else {
                                    !linked
                                }
                            })
                    })
                })
                .map(|(key, _)| *key)
                .collect();
            keys.into_iter()
                .filter_map(|key| sessions.remove(&key))
                .collect::<Vec<_>>()
        };
        drop(removed); // Revoke outside the map lock, even on idle/failed callers.
        if let Ok(mut channels) = self.web_channels.lock() {
            channels.retain(|_, channel| channel.live());
        }
        self.flush_touches(false);
    }
    pub(crate) fn flush_touches(&self, force: bool) {
        if let Ok(ms) = wall_ms().and_then(|v| i64::try_from(v).map_err(|_| unavailable())) {
            self.touch_batcher.flush(&*self.db, ms, force);
        }
    }
    pub(crate) fn revoke_browser_sessions(&self, ended: &[crate::sessions::Ended]) {
        if ended.is_empty() {
            return;
        }
        let endpoints: Vec<Arc<crate::supervisor::channels::Endpoint>> = {
            let Ok(mut channels) = self.web_channels.lock() else {
                return;
            };
            channels.retain(|_, channel| channel.live());
            let mut seen = std::collections::BTreeSet::new();
            channels
                .values()
                .filter_map(|channel| channel.endpoint.upgrade())
                .filter(|endpoint| seen.insert(endpoint.office.clone()))
                .collect()
        };
        for endpoint in endpoints {
            for session in ended {
                let _ = endpoint.revoke(&session.id);
            }
        }
    }
    pub(crate) fn writers(
        &self,
        instance: &str,
        root: &std::path::Path,
    ) -> std::io::Result<Arc<crate::writer::Writers>> {
        crate::writer::Writers::open(self.custody(), self.accounts_store(), instance, root)
    }
    pub(super) fn disconnect(&self, connection: &Connection) {
        let alive = connection.liveness();
        let removed = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return;
            };
            let keys: Vec<_> = sessions
                .iter()
                .filter(|(_, s)| s.connection.ptr_eq(&alive))
                .map(|(key, _)| *key)
                .collect();
            keys.into_iter()
                .filter_map(|key| sessions.remove(&key))
                .collect::<Vec<_>>()
        };
        // Session Drop revokes issued worker grants; never hold the session map
        // while waiting on a bounded transfer or closing a failed worker.
        drop(removed);
        if let Ok(mut channels) = self.web_channels.lock() {
            channels.retain(|_, c| !c.connection.ptr_eq(&alive) && c.live());
        }
    }
    #[cfg(test)]
    pub(crate) fn with_memory_for_test(
        memory: Arc<crate::hostdb::memory::MemoryHostStore>,
        custody: Arc<dyn HostSecrets>,
        instance: &str,
    ) -> Result<Self, ApiError> {
        let db: Arc<dyn HostStore> = Arc::clone(&memory) as Arc<dyn HostStore>;
        let mut auth = Self::with_store(db, custody, instance)?;
        auth.memory_for_test = Some(memory);
        Ok(auth)
    }
    pub(crate) fn with_store(
        db: Arc<dyn HostStore>,
        custody: Arc<dyn HostSecrets>,
        instance: &str,
    ) -> Result<Self, ApiError> {
        db.verify_instance(instance).map_err(storage)?;
        // Frozen acknowledgement deletion must finish before any office or
        // gateway can become ready. Missing custody without intent is an error.
        db.recovery_reconcile(&*custody).map_err(storage)?;
        prune_expired_journal(&*db);
        let dummy_password = random_handle()?;
        let dummy = hash_password(
            &Password::new(dummy_password.expose().to_owned()).map_err(|_| unavailable())?,
        )?;
        Ok(Self {
            db,
            #[cfg(test)]
            memory_for_test: None,
            _custody: custody,
            dummy,
            hashing: AtomicUsize::new(0),
            sessions: Mutex::new(BTreeMap::new()),
            web_channels: Mutex::new(BTreeMap::new()),
            touch_batcher: touch_batch::TouchBatcher::new(),
        })
    }
    pub(crate) fn authorize(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
    ) -> Result<Account, ApiError> {
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let key = digest(handle);
        let session = sessions.get_mut(&key).ok_or_else(denied)?;
        if !session.live(Instant::now()) || !session.connection.ptr_eq(&connection.liveness()) {
            // A wrong connection cannot terminate another connection's session.
            if session.connection.ptr_eq(&connection.liveness()) {
                let expired = sessions.remove(&key);
                drop(sessions);
                drop(expired);
            }
            return Err(denied());
        }
        let account = self
            .db
            .account_authority(&session.account)
            .map_err(storage)?
            .ok_or_else(denied)?;
        let authless_closed = session.local_authless
            && self.db.local_authless_account().map_err(storage)?.as_ref()
                != Some(&session.account);
        if account.revoked_at_ms().is_some()
            || account.password_changed_at_ms() != session.password_changed_at_ms
            || authless_closed
        {
            let removed = sessions.remove(&key);
            drop(sessions);
            drop(removed);
            return Err(denied());
        }
        session.touched = Instant::now();
        Ok(account)
    }
    pub(crate) fn recovery_session_binding(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
    ) -> Result<([u8; 32], Weak<AtomicBool>), ApiError> {
        self.authorize(connection, handle)?;
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let key = digest(handle);
        let session = sessions.get(&key).ok_or_else(denied)?;
        if !session.live(Instant::now()) || !session.connection.ptr_eq(&connection.liveness()) {
            return Err(denied());
        }
        Ok((key, Arc::downgrade(&session.authority_live)))
    }
    pub(crate) fn end(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
    ) -> Result<String, ApiError> {
        self.authorize(connection, handle)?;
        let removed = {
            self.sessions
                .lock()
                .map_err(|_| unavailable())?
                .remove(&digest(handle))
        }
        .ok_or_else(denied)?;
        let id = removed.id.clone();
        drop(removed);
        Ok(id)
    }
}

mod browser;
mod multi_person;
#[cfg(test)]
pub(crate) mod tests;
mod touch_batch;
