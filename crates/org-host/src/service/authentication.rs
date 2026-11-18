//! Live host-account sessions. The registry remains authoritative on every call;
//! handles are random, connection-bound, hashed in memory, and never persisted.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use org_host_control::{
    ops::{self, AccountVerifyResult, LinkedOrg},
    Password, SessionHandle,
};
use org_proto::{ApiError, ErrorCode};
use sha2::{Digest, Sha256};

use super::listener::Connection;
use crate::accounts::identity::{AccountId, LoginKey};
use crate::host_secrets::HostSecrets;
use crate::hostdb::{
    accounts::Account,
    journal::Verifier,
    lockout::{Outcome, Socket},
    HostDb,
};

const MAX_HASHES: usize = 2;
const MAX_SESSIONS: usize = 1024;
const MAX_CONNECTION_SESSIONS: usize = 32;
const ABSOLUTE: Duration = Duration::from_secs(12 * 3600);
const IDLE: Duration = Duration::from_secs(30 * 60);

pub(crate) struct Authentication {
    db: Arc<HostDb>,
    // Retain custody lifetime (including the debug store's exclusive lock).
    _custody: Arc<dyn HostSecrets>,
    dummy: Verifier,
    hashing: AtomicUsize,
    sessions: Mutex<BTreeMap<[u8; 32], Session>>,
}

/// Monotonic expiry also runs when no client makes another request. Removing a
/// session drops its issued channel grants through the actual transfer service.
pub(crate) struct Lifecycle {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Lifecycle {
    pub(crate) fn start(authentication: &Arc<Authentication>) -> std::io::Result<Self> {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let weak = Arc::downgrade(authentication);
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
            thread: Some(thread),
        })
    }
}
impl Drop for Lifecycle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}
struct Session {
    account: AccountId,
    password_changed_at_ms: i64,
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
    _slot: crate::supervisor::channels::ChannelSlot,
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
fn wall_ms() -> Result<u64, ApiError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or_else(unavailable)
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
    .map_err(storage)
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
    pub(crate) fn provisioning_storage(&self) -> (Arc<HostDb>, Arc<dyn HostSecrets>) {
        (Arc::clone(&self.db), Arc::clone(&self._custody))
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
                            || session.channels.values().any(|channel| {
                                !account
                                    .links()
                                    .iter()
                                    .any(|link| link.org_id() == channel.organization)
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
    }
    pub(crate) fn writers(
        &self,
        instance: &str,
        root: &std::path::Path,
    ) -> std::io::Result<Arc<crate::writer::Writers>> {
        crate::writer::Writers::open(
            Arc::clone(&self._custody),
            Arc::clone(&self.db),
            instance,
            root,
        )
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
    }
    pub(crate) fn open(
        home: &std::path::Path,
        custody: Arc<dyn HostSecrets>,
        instance: &str,
    ) -> Result<Self, ApiError> {
        let db = Arc::new(HostDb::open(home, &*custody).map_err(|e| match e {
            crate::hostdb::Error::Outdated => {
                ApiError::new(ErrorCode::Unavailable, crate::hostdb::sessions::OUTDATED)
            }
            e => storage(e),
        })?);
        Self::new(db, custody, instance)
    }
    fn new(
        db: Arc<HostDb>,
        custody: Arc<dyn HostSecrets>,
        instance: &str,
    ) -> Result<Self, ApiError> {
        db.verify_instance(instance).map_err(storage)?;
        // Frozen acknowledgement deletion must finish before any office or
        // gateway can become ready. Missing custody without intent is an error.
        db.recovery_reconcile(&*custody).map_err(storage)?;
        let dummy_password = random_handle()?;
        let dummy = hash_password(
            &Password::new(dummy_password.expose().to_owned()).map_err(|_| unavailable())?,
        )?;
        Ok(Self {
            db,
            _custody: custody,
            dummy,
            hashing: AtomicUsize::new(0),
            sessions: Mutex::new(BTreeMap::new()),
        })
    }
    pub(crate) fn verify(
        &self,
        connection: &Connection,
        login: &str,
        password: &Password,
    ) -> Result<AccountVerifyResult, ApiError> {
        // Hard ingress limits precede normalization or Argon2 work.
        if login.len() > 4096 || password.expose().len() > 4096 {
            return Err(denied());
        }
        let key = LoginKey::new(login).map_err(|_| denied())?;
        let permit = self
            .hashing
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_HASHES).then_some(n + 1)
            })
            .map_err(|_| unavailable())?;
        let _ = permit;
        let _permit = HashPermit(&self.hashing);
        let now = wall_ms()?;
        let socket = if connection.channel_kind() == org_office_launch::ChannelKind::Web {
            Socket::Control
        } else {
            Socket::Local
        };
        if self
            .db
            .login_lockout(socket, key.as_str(), now)
            .map_err(storage)?
            .is_locked()
            || self
                .db
                .login_lockout(socket, "", now)
                .map_err(storage)?
                .is_locked()
        {
            return Err(denied());
        }
        let credentials = self.db.account_credentials(&key).map_err(storage)?;
        let verifier = credentials.as_ref().map_or(&self.dummy, |c| c.verifier());
        let parsed = PasswordHash::new(verifier.expose()).map_err(|_| unavailable())?;
        let matches = argon()?
            .verify_password(password.expose().as_bytes(), &parsed)
            .is_ok();
        let current = match &credentials {
            Some(c) if matches => self
                .db
                .account_authority(
                    &AccountId::parse(c.account().account_id()).map_err(|_| unavailable())?,
                )
                .map_err(storage)?,
            _ => None,
        };
        let allowed = current.as_ref().is_some_and(|a| {
            a.revoked_at_ms().is_none()
                && credentials.as_ref().is_some_and(|c| {
                    a.password_changed_at_ms() == c.account().password_changed_at_ms()
                })
        });
        let outcome = if allowed {
            Outcome::Success
        } else {
            Outcome::Failure
        };
        let state = self
            .db
            .record_login(socket, key.as_str(), now, outcome)
            .map_err(storage)?;
        // Empty key is the socket-wide budget; normalized login can never be empty.
        if !allowed || state.is_locked() {
            self.db
                .record_login(socket, "", now, Outcome::Failure)
                .map_err(storage)?;
            return Err(denied());
        }
        let account = current.ok_or_else(denied)?;
        self.issue(connection, account)
    }
    fn issue(
        &self,
        connection: &Connection,
        account: Account,
    ) -> Result<AccountVerifyResult, ApiError> {
        self.reap();
        let handle = random_handle()?;
        let key = digest(&handle);
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let now = Instant::now();
        let alive = connection.liveness();
        if alive
            .upgrade()
            .is_none_or(|closed| closed.load(Ordering::SeqCst))
        {
            return Err(denied());
        }
        if sessions.len() >= MAX_SESSIONS
            || sessions
                .values()
                .filter(|s| s.connection.ptr_eq(&alive))
                .count()
                >= MAX_CONNECTION_SESSIONS
        {
            return Err(unavailable());
        }
        let result = AccountVerifyResult {
            session: handle,
            account: account.account_id().into(),
            admin: account.admin(),
            orgs: account
                .links()
                .iter()
                .map(|l| LinkedOrg {
                    org_id: l.org_id().into(),
                    employee_id: l.member_id().into(),
                    office_ids: vec![],
                })
                .collect(),
        };
        let session = Session {
            account: AccountId::parse(account.account_id()).map_err(|_| unavailable())?,
            password_changed_at_ms: account.password_changed_at_ms(),
            id: crate::ids::generate().map_err(|_| unavailable())?,
            connection: alive,
            created: now,
            touched: now,
            channels: BTreeMap::new(),
            authority_live: Arc::new(AtomicBool::new(true)),
        };
        if sessions.contains_key(&key) {
            return Err(unavailable());
        }
        sessions.insert(key, session);
        Ok(result)
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
        if account.revoked_at_ms().is_some()
            || account.password_changed_at_ms() != session.password_changed_at_ms
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

    pub(super) fn open_channel(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
        endpoint: Arc<crate::supervisor::channels::Endpoint>,
        organization: &str,
    ) -> Result<
        (
            String,
            std::os::unix::net::UnixStream,
            crate::supervisor::channels::ChannelSlot,
        ),
        ApiError,
    > {
        let account = self.authorize(connection, handle)?;
        if !account
            .links()
            .iter()
            .any(|link| link.org_id() == organization)
        {
            return Err(denied());
        }
        // Authorization is checked before contacting the worker and repeated
        // after its descriptor receipt, because logout may race that handoff.
        let (id, socket, slot) = endpoint
            .open(connection.channel_kind())
            .map_err(|_| unavailable())?;
        self.authorize(connection, handle)?;
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let session = sessions.get_mut(&digest(handle)).ok_or_else(denied)?;
        session
            .channels
            .retain(|_, channel| channel._slot.is_live());
        if !session.live(Instant::now()) || !slot.is_live() || session.channels.len() >= 64 {
            return Err(unavailable());
        }
        session.channels.insert(
            id.clone(),
            Channel {
                endpoint: Arc::downgrade(&endpoint),
                organization: organization.into(),
                _slot: slot.clone(),
            },
        );
        Ok((id, socket, slot))
    }

    pub(super) fn grant(
        &self,
        connection: &Connection,
        handle: &SessionHandle,
        office: &str,
        channel: &str,
    ) -> Result<org_host_control::ops::SessionGrantResult, ApiError> {
        use hmac::{Hmac, Mac};
        if !connection.channel_received(channel) {
            return Err(denied());
        }
        let account = self.authorize(connection, handle)?;
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let session = sessions
            .get(&digest(handle))
            .filter(|s| s.live(Instant::now()))
            .ok_or_else(denied)?;
        let bound = session.channels.get(channel).ok_or_else(denied)?;
        let endpoint = bound.endpoint.upgrade().ok_or_else(unavailable)?;
        if endpoint.office != office || !endpoint.is_live() || !bound._slot.is_live() {
            return Err(denied());
        }
        let link = account
            .links()
            .iter()
            .find(|link| link.org_id() == bound.organization)
            .ok_or_else(denied)?;
        let remaining = ABSOLUTE
            .checked_sub(Instant::now().duration_since(session.created))
            .ok_or_else(denied)?;
        let ttl = u64::try_from(remaining.min(Duration::from_secs(300)).as_millis())
            .map_err(|_| unavailable())?;
        if ttl == 0 {
            return Err(denied());
        }
        let exp = wall_ms()?.checked_add(ttl).ok_or_else(unavailable)?;
        let claims = org_proto::GrantClaims {
            account: account.account_id().into(),
            member: link.member_id().into(),
            org_id: bound.organization.clone(),
            office_id: office.into(),
            channel_id: channel.into(),
            session_id: session.id.clone(),
            exp,
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(endpoint.key.expose_bytes())
            .map_err(|_| unavailable())?;
        mac.update(&org_proto::grant_mac_input(&claims).ok_or_else(unavailable)?);
        let mut bytes = mac.finalize().into_bytes();
        let mut tag: [u8; 32] = bytes[..].try_into().map_err(|_| unavailable())?;
        let signature = org_proto::GrantMac::from_bytes(tag);
        crate::secrets::zero(&mut tag);
        bytes.fill(0);
        Ok(org_host_control::ops::SessionGrantResult {
            grant: org_proto::Grant {
                claims,
                mac: signature,
            },
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
