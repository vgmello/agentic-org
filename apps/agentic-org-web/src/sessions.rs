//! Browser authority stays in memory and is issued only by the live host.
//! No organization profile, task command or imported document can create it.
mod control;
mod http;
mod ingress;
#[cfg(test)]
mod tests;
pub use control::HostControl;
pub use http::SessionApi;
pub use ingress::{read_secret_body, SecretBody};

use crate::{blocking::Watch, service::ApiReply};
use hmac::{Hmac, Mac};
use hyper::{http::request::Parts, StatusCode};
use org_host_control::{Password, SessionHandle};
use org_proto::gateway::{
    AccountView, GatewayError, GatewayErrorCode, Milliseconds, OrganizationLink, ReadResponse,
    SessionReason, SessionView, SESSION_PROOF_HEADER,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ops::Bound::{Excluded, Unbounded},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const ABSOLUTE: Duration = Duration::from_secs(12 * 3600);
const IDLE: Duration = Duration::from_secs(30 * 60);
/// The browser map holds at most the host's gateway budget, never more, so the
/// host's local reserve stays free for local sign-in.
const MAX_SESSIONS: usize = org_host_control::ops::GATEWAY_SESSIONS;
/// One sign-in's single window, from its start, in which expiry cleanup
/// (before account.verify) and capacity probes (after it) may start host
/// calls. Both phases share it, and neither starts a call after it closes, so
/// at most one cleanup/probe call is still in flight at the close. On a
/// healthy host a sign-in therefore takes at most these 2 s + account.verify
/// (password hashing, under 1 s) + the new session's inspect + that one call
/// (+ the new session's end when admission fails; milliseconds each): about
/// 3 s against the default 10 s `backend_timeout`, leaving about 7 s for pool
/// queueing. A
/// degraded host can exceed it (each call keeps its 5 s deadline); the
/// abandonment check before insertion then ends the new host session instead
/// of binding it, and an abandonment after that check is undone when the reply
/// cannot be delivered.
const CAPACITY_RECHECK: Duration = Duration::from_secs(2);
/// Host ends queued behind one cleanup thread. Ends beyond it wait in the
/// overflow list (see [`Cleanup`]) rather than being dropped.
const CLEANUP_QUEUE: usize = 64;
type ProofMac = Hmac<Sha256>;
pub struct BrowserSessions {
    host: Arc<HostControl>,
    instance: String,
    cookie_name: String,
    secure: bool,
    proof_key: Zeroizing<[u8; 32]>,
    /// Shared with a sign-in's delivery rollback, which may run after the call.
    sessions: Arc<Mutex<BTreeMap<[u8; 32], Arc<Session>>>>,
    capacity_cursor: Mutex<Option<[u8; 32]>>,
    cleanup: Cleanup,
}
/// Host ends owed for sessions whose browser binding is already gone. Each
/// owed end is attempted once unless control EOF made it unnecessary: the
/// bounded queue feeds the cleanup thread, and an end that does not fit waits
/// in `overflow` (one entry per host handle, so no larger than the host's
/// per-connection session budget) for that thread or the next sign-in.
#[derive(Clone)]
struct Cleanup {
    queue: SyncSender<Arc<SessionHandle>>,
    overflow: Arc<Overflow>,
}
impl Cleanup {
    fn end_later(&self, handle: Arc<SessionHandle>) {
        match self.queue.try_send(handle) {
            Ok(()) => {}
            Err(TrySendError::Full(handle)) => self.overflow.defer(handle),
            // The caller holds a sender, so the receiver is gone only because
            // the cleanup thread saw the control connection no longer live;
            // that EOF ends every host session on this connection.
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}
#[derive(Default)]
struct Overflow(Mutex<Vec<Arc<SessionHandle>>>);
impl Overflow {
    fn owed(&self) -> std::sync::MutexGuard<'_, Vec<Arc<SessionHandle>>> {
        // Entries are plain handles: a panic elsewhere cannot corrupt them.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn defer(&self, handle: Arc<SessionHandle>) {
        let mut owed = self.owed();
        if !owed.iter().any(|known| Arc::ptr_eq(known, &handle)) {
            owed.push(handle);
        }
    }
    fn take(&self) -> Option<Arc<SessionHandle>> {
        self.owed().pop()
    }
    fn is_empty(&self) -> bool {
        self.owed().is_empty()
    }
}
struct Session {
    handle: Arc<SessionHandle>,
    proof_mac: [u8; 32],
    account: AccountView,
    admin: bool,
    organizations: Vec<OrganizationLink>,
    created: Instant,
    touched: Mutex<Instant>,
}
/// An authenticated request retains the original binding. Refresh/logout remove
/// the binding, so a queued ticket fails `current()`.
pub struct Ticket {
    key: [u8; 32],
    session: Arc<Session>,
}
/// The host end of a sign-out whose browser pairs are already invalid. It runs
/// exactly once: [`SignOut::end`] asks the host directly; dropping it unexecuted
/// (a refused body, a refused pool admission) owes the end to [`Cleanup`]
/// instead, which never drops it while the control connection is live.
pub struct SignOut {
    handle: Option<Arc<SessionHandle>>,
    host: Arc<HostControl>,
    cleanup: Cleanup,
}
impl SignOut {
    pub fn end(mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        match self.host.end(&handle) {
            // Ended, or the host had already ended or forgotten it.
            Ok(()) | Err(control::Failure::Denied | control::Failure::NotFound) => {}
            // Retry once on the cleanup thread.
            Err(control::Failure::Unavailable) => self.cleanup.end_later(handle),
        }
    }
}
impl Drop for SignOut {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.cleanup.end_later(handle);
        }
    }
}
pub struct Established {
    pub view: SessionView,
    pub proof: org_proto::gateway::SessionProof,
    pub cookie: Zeroizing<String>,
}
/// The single protected-auth failure shape (web-api-v1 §4): 401
/// `not_authorized` with `session_reason: session_ended`. Unknown login, wrong
/// password, a missing/mismatched pair and an ended session are indistinguishable.
fn denied() -> ApiReply {
    let mut error = GatewayError::new(
        GatewayErrorCode::NotAuthorized,
        "Session ended. Sign in again.",
    );
    error.session_reason = Some(SessionReason::SessionEnded);
    ApiReply::json(
        StatusCode::UNAUTHORIZED,
        serde_json::to_value(ReadResponse::<serde_json::Value>::Error(error))
            .expect("gateway error serializes"),
    )
}
fn unavailable() -> ApiReply {
    ApiReply::error(
        StatusCode::SERVICE_UNAVAILABLE,
        GatewayErrorCode::Unavailable,
        "Session service unavailable.",
    )
}
fn random() -> Result<[u8; 32], ApiReply> {
    let mut bytes = [0; 32];
    tokio_rustls::rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| unavailable())?;
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(bytes.len() * 2));
    use std::fmt::Write;
    for b in bytes {
        write!(out, "{b:02x}").expect("String write");
    }
    out
}
fn datetime(ms: u64) -> Result<String, ApiReply> {
    if ms > 253_402_300_799_999 {
        return Err(unavailable());
    }
    Ok(time::format_ms(ms))
}
mod time;
/// The single host-cleanup worker. It ends when the registry, pending delivery
/// rollbacks and unexecuted sign-outs (the only senders) drop or the host
/// control connection is no longer live. Whenever the queue is empty it ends one deferred overflow
/// entry, so overflow drains without waiting for another sign-in.
fn end_expired(host: &HostControl, queued: &Receiver<Arc<SessionHandle>>, overflow: &Overflow) {
    while host.live() {
        let wait = if overflow.is_empty() {
            Duration::from_millis(100)
        } else {
            Duration::ZERO
        };
        match queued.recv_timeout(wait) {
            Ok(handle) => {
                let _ = host.end(&handle);
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(handle) = overflow.take() {
                    let _ = host.end(&handle);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}
impl BrowserSessions {
    pub fn new(
        host: Arc<HostControl>,
        instance: String,
        secure: bool,
    ) -> Result<Arc<Self>, ApiReply> {
        if host.instance() != instance {
            return Err(unavailable());
        }
        let suffix = hex(&Sha256::digest(instance.as_bytes()));
        let (queue, queued) = mpsc::sync_channel(CLEANUP_QUEUE);
        let cleanup = Cleanup {
            queue,
            overflow: Arc::default(),
        };
        // The worker holds the overflow list but no sender, so dropping the
        // registry (and any pending rollbacks) still disconnects its queue.
        let (ending, owed) = (Arc::clone(&host), Arc::clone(&cleanup.overflow));
        thread::Builder::new()
            .name("gateway-session-cleanup".into())
            .spawn(move || end_expired(&ending, &queued, &owed))
            .map_err(|_| unavailable())?;
        Ok(Arc::new(Self {
            host,
            cleanup,
            instance,
            cookie_name: format!("org_session_{}", &suffix[..16]),
            secure,
            proof_key: Zeroizing::new(random()?),
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            capacity_cursor: Mutex::new(None),
        }))
    }
    fn mac(&self, proof: &[u8]) -> ProofMac {
        let mut mac = ProofMac::new_from_slice(&self.proof_key[..]).expect("fixed HMAC key");
        mac.update(b"Agentic Org browser proof v1\0");
        mac.update(proof);
        mac
    }
    fn cookie(&self, value: &str) -> Zeroizing<String> {
        set_cookie(&self.cookie_name, value, self.secure, false)
    }
    pub fn clear_cookie(&self) -> Zeroizing<String> {
        set_cookie(&self.cookie_name, "", self.secure, true)
    }
    pub fn sign_in(&self, login: String, password: Password) -> Result<Established, ApiReply> {
        self.sign_in_watched(login, password, &Watch::never())
    }
    pub fn sign_in_watched(
        &self,
        login: String,
        password: Password,
        watch: &Watch<'_>,
    ) -> Result<Established, ApiReply> {
        self.sign_in_with(login, password, watch, Ok)
    }
    /// `watch` reports that the HTTP waiter gave up: the issued host session is
    /// then ended rather than bound to a cookie nobody will receive. `respond`
    /// builds the reply; a failure there binds nothing and ends the host session.
    pub fn sign_in_with<R>(
        &self,
        login: String,
        password: Password,
        watch: &Watch<'_>,
        respond: impl FnOnce(Established) -> Result<R, ApiReply>,
    ) -> Result<R, ApiReply> {
        // Browser expiry can precede native expiry (the host clock renews on
        // every authorized host call). Release those host slots before
        // account.verify asks for another one on the same connection.
        let window = Instant::now() + CAPACITY_RECHECK;
        self.reap_before_sign_in(window, watch)?;
        let result = self.host.verify(&login, &password).map_err(|e| {
            if e == control::Failure::Denied {
                denied()
            } else {
                unavailable()
            }
        })?;
        let session = Session {
            handle: Arc::new(result.session),
            proof_mac: [0; 32],
            account: AccountView {
                id: result.account,
                login,
            },
            admin: result.admin,
            organizations: result
                .orgs
                .into_iter()
                .map(|o| OrganizationLink {
                    org_id: o.org_id,
                    member_id: o.member_id,
                    office_ids: o.office_ids,
                })
                .collect(),
            created: Instant::now(),
            touched: Mutex::new(Instant::now()),
        };
        self.establish(session, window, watch, respond)
    }
    fn prepare_established(
        &self,
        mut session: Session,
    ) -> Result<([u8; 32], Session, Established), ApiReply> {
        let cookie = hex(&Zeroizing::new(random()?)[..]);
        let proof = hex(&Zeroizing::new(random()?)[..]);
        session.proof_mac = self.mac(proof.as_bytes()).finalize().into_bytes().into();
        let view = self.view(&session)?;
        let key = Sha256::digest(cookie.as_bytes()).into();
        let result = Established {
            view,
            proof: org_proto::gateway::SessionProof::parse(proof.to_string())
                .map_err(|_| unavailable())?,
            cookie: self.cookie(&cookie),
        };
        Ok((key, session, result))
    }
    fn establish<R>(
        &self,
        session: Session,
        window: Instant,
        watch: &Watch<'_>,
        respond: impl FnOnce(Established) -> Result<R, ApiReply>,
    ) -> Result<R, ApiReply> {
        // Keep cleanup ownership until the newly issued host session has a
        // browser binding, including when preparation's inspection fails.
        let handle = Arc::clone(&session.handle);
        let admission = (|| {
            let (key, session, result) = self.prepare_established(session)?;
            // Build the reply before binding: a construction failure binds nothing.
            let reply = respond(result)?;
            self.reclaim_capacity_until(window)?;
            let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
            if sessions.len() >= MAX_SESSIONS || sessions.contains_key(&key) {
                return Err(unavailable());
            }
            // Checked under the map lock, last: an abandoned request binds nothing.
            if watch.abandoned() {
                return Err(unavailable());
            }
            sessions.insert(key, Arc::new(session));
            // The waiter can still give up after this check. If the reply then
            // cannot be delivered, nobody holds the new pair: evict it by its
            // host handle and owe the host end to the cleanup thread.
            let (map, cleanup, bound) = (
                Arc::clone(&self.sessions),
                self.cleanup.clone(),
                Arc::clone(&handle),
            );
            watch.on_undelivered(move || {
                if let Ok(mut sessions) = map.lock() {
                    sessions.retain(|_, current| !Arc::ptr_eq(&current.handle, &bound));
                }
                cleanup.end_later(bound);
            });
            Ok(reply)
        })();
        if admission.is_err() {
            // Nothing binds the new host session: a transient end failure owes
            // the end to the cleanup thread rather than orphaning the slot.
            if let Err(control::Failure::Unavailable) = self.host.end(&handle) {
                self.cleanup.end_later(handle);
            }
        }
        admission
    }
    /// Runs only on sign-in's blocking worker. End owed and expired authority
    /// directly, starting calls only before `window` closes, each with the full
    /// native deadline. Remove a binding only once the host confirms it ended:
    /// entries beyond the window or a failed call remain available for the
    /// next pass, rather than overflowing the asynchronous cleanup queue.
    fn reap_before_sign_in(&self, window: Instant, watch: &Watch<'_>) -> Result<(), ApiReply> {
        // Ends already owed to the cleanup thread first: their bindings are gone.
        while Instant::now() < window {
            let Some(handle) = self.cleanup.overflow.take() else {
                break;
            };
            if watch.abandoned() {
                self.cleanup.overflow.defer(handle);
                return Err(unavailable());
            }
            if let Err(control::Failure::Unavailable) = self.host.end(&handle) {
                self.cleanup.overflow.defer(handle);
                return Err(unavailable());
            }
        }
        for _ in 0..MAX_SESSIONS {
            if Instant::now() >= window {
                break;
            }
            let expired = self
                .sessions
                .lock()
                .map_err(|_| unavailable())?
                .values()
                .find(|session| !session.live())
                .cloned();
            let Some(session) = expired else {
                break;
            };
            if watch.abandoned() {
                return Err(unavailable());
            }
            self.end_bound(&session)?;
        }
        Ok(())
    }
    /// Ends a browser-expired binding's host session and removes the binding
    /// once the host confirms the end; a transient failure keeps it for retry.
    fn end_bound(&self, session: &Session) -> Result<(), ApiReply> {
        match self.host.end(&session.handle) {
            Ok(()) | Err(control::Failure::Denied | control::Failure::NotFound) => {
                self.sessions
                    .lock()
                    .map_err(|_| unavailable())?
                    .retain(|_, current| !Arc::ptr_eq(&current.handle, &session.handle));
                Ok(())
            }
            Err(control::Failure::Unavailable) => Err(unavailable()),
        }
    }
    #[cfg(test)]
    fn reclaim_capacity(&self) -> Result<(), ApiReply> {
        self.reclaim_capacity_until(Instant::now() + CAPACITY_RECHECK)
    }
    fn reclaim_capacity_until(&self, window: Instant) -> Result<(), ApiReply> {
        // Only sign-in's blocking worker probes the host, never authenticate/reap.
        // The window bounds when a probe may start, not how long it may take:
        // each probe keeps the normal per-call deadline, because truncating an
        // in-flight call would close the shared control connection and end every
        // session. The scan therefore ends at most one call deadline after it.
        for _ in 0..MAX_SESSIONS {
            if Instant::now() >= window {
                break;
            }
            let session = {
                // Reserve the next probe across concurrent scans. Continue after
                // the last key even if its binding was removed or rotated; wrap
                // at the end so bounded retries eventually cover the whole map.
                let mut cursor = self.capacity_cursor.lock().map_err(|_| unavailable())?;
                let sessions = self.sessions.lock().map_err(|_| unavailable())?;
                if sessions.len() < MAX_SESSIONS {
                    return Ok(());
                }
                let (key, session) = cursor
                    .and_then(|key| sessions.range((Excluded(key), Unbounded)).next())
                    .or_else(|| sessions.first_key_value())
                    .expect("capacity requires a nonempty map");
                *cursor = Some(*key);
                Arc::clone(session)
            };
            if !session.live() {
                // An inspection would authorize, and so renew, the host idle
                // clock of a pair the browser can no longer use: end it instead.
                self.end_bound(&session)?;
            } else {
                match self.host.inspect(&session.handle) {
                    Err(control::Failure::Denied) => {
                        self.session_failure(&session, control::Failure::Denied);
                    }
                    Err(_) => return Err(unavailable()),
                    Ok(_) => {}
                }
            }
            if self.sessions.lock().map_err(|_| unavailable())?.len() < MAX_SESSIONS {
                break;
            }
        }
        Ok(())
    }
    fn reap(&self) -> Result<(), ApiReply> {
        // The host owns expiry and revocation (its own reap stays authoritative),
        // and control EOF ends every host session on this connection.
        let host_live = self.host.live();
        let mut expired = Vec::new();
        self.sessions
            .lock()
            .map_err(|_| unavailable())?
            .retain(|_, session| {
                let keep = host_live && session.live();
                if !keep {
                    expired.push(Arc::clone(session));
                }
                keep
            });
        // Browser expiry ends the host session too (§4), so an expired pair
        // does not hold a host slot until the host's own idle timeout. This
        // runs on the async request path, so it never calls the host: the end
        // is owed to the cleanup thread (queued, or deferred when the queue is
        // full). After control EOF the host has already ended every session on
        // this connection.
        if host_live {
            for session in expired {
                self.cleanup.end_later(Arc::clone(&session.handle));
            }
        }
        Ok(())
    }
    fn session_failure(&self, session: &Session, error: control::Failure) -> ApiReply {
        match error {
            control::Failure::Denied => {
                let Ok(mut sessions) = self.sessions.lock() else {
                    return unavailable();
                };
                // The host has ended this authority. Evict its binding even if a
                // concurrent refresh rotated the pair while this call was in flight.
                sessions.retain(|_, current| !Arc::ptr_eq(&current.handle, &session.handle));
                denied()
            }
            control::Failure::NotFound | control::Failure::Unavailable => unavailable(),
        }
    }
    fn view(&self, session: &Session) -> Result<SessionView, ApiReply> {
        let status = self
            .host
            .inspect(&session.handle)
            .map_err(|error| self.session_failure(session, error))?;
        if status.account != session.account.id {
            return Err(denied());
        }
        Ok(SessionView {
            instance_id: self.instance.clone(),
            account: AccountView {
                id: status.account,
                login: status.login,
            },
            admin: status.admin,
            organizations: status
                .orgs
                .into_iter()
                .map(|o| OrganizationLink {
                    org_id: o.org_id,
                    member_id: o.member_id,
                    office_ids: o.office_ids,
                })
                .collect(),
            server_time_ms: Milliseconds::new(status.server_time_ms).map_err(|_| unavailable())?,
            absolute_expires_at: datetime(status.absolute_expires_at_ms)?,
            idle_expires_at: datetime(status.idle_expires_at_ms)?,
        })
    }

    fn current(&self, ticket: &Ticket) -> Result<(), ApiReply> {
        self.reap()?;
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        if !self.host.live()
            || !ticket.session.live()
            || !sessions
                .get(&ticket.key)
                .is_some_and(|s| Arc::ptr_eq(s, &ticket.session))
        {
            return Err(denied());
        }
        Ok(())
    }
    pub fn get(&self, ticket: Ticket) -> Result<SessionView, ApiReply> {
        self.current(&ticket)?;
        self.host
            .renew(&ticket.session.handle)
            .map_err(|error| self.session_failure(&ticket.session, error))?;
        *ticket.session.touched.lock().map_err(|_| unavailable())? = Instant::now();
        self.view(&ticket.session)
    }
    /// Sign-out succeeds once the browser pair is invalid locally; the host end
    /// that follows cannot keep a signed-out pair usable (web-api-v1 §4).
    pub fn sign_out(&self, ticket: Ticket) -> Result<(), ApiReply> {
        self.current(&ticket)?;
        self.begin_sign_out(ticket)?.end();
        Ok(())
    }
    /// Invalidates every browser pair bound to the authenticated ticket's host
    /// session now, including one a concurrent refresh rotated after the ticket
    /// was issued (a refresh that has not swapped yet then finds its ticket
    /// gone). The returned [`SignOut`] owns the host end that follows.
    pub fn begin_sign_out(&self, ticket: Ticket) -> Result<SignOut, ApiReply> {
        let handle = Arc::clone(&ticket.session.handle);
        self.sessions
            .lock()
            .map_err(|_| unavailable())?
            .retain(|_, current| !Arc::ptr_eq(&current.handle, &handle));
        Ok(SignOut {
            handle: Some(handle),
            host: Arc::clone(&self.host),
            cleanup: self.cleanup.clone(),
        })
    }
    pub fn refresh(&self, ticket: Ticket) -> Result<Established, ApiReply> {
        self.refresh_watched(ticket, &Watch::never())
    }
    pub fn refresh_watched(
        &self,
        ticket: Ticket,
        watch: &Watch<'_>,
    ) -> Result<Established, ApiReply> {
        self.refresh_with(ticket, watch, Ok)
    }
    /// `respond` builds the reply; a failure there leaves the old pair bound.
    /// `watch` reports that the HTTP waiter gave up: the browser then never
    /// receives the new pair and keeps the old one bound instead.
    pub fn refresh_with<R>(
        &self,
        ticket: Ticket,
        watch: &Watch<'_>,
        respond: impl FnOnce(Established) -> Result<R, ApiReply>,
    ) -> Result<R, ApiReply> {
        self.current(&ticket)?;
        self.host
            .renew(&ticket.session.handle)
            .map_err(|error| self.session_failure(&ticket.session, error))?;
        // Share the same secret holder; rotating the browser pair never mints
        // another host session or extends its absolute lifetime.
        let session = Session {
            handle: Arc::clone(&ticket.session.handle),
            proof_mac: [0; 32],
            account: ticket.session.account.clone(),
            admin: ticket.session.admin,
            organizations: ticket.session.organizations.clone(),
            created: ticket.session.created,
            touched: Mutex::new(Instant::now()),
        };
        let (key, session, result) = self.prepare_established(session)?;
        // Build the reply before swapping: a construction failure keeps the old pair.
        let reply = respond(result)?;
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        if !sessions
            .get(&ticket.key)
            .is_some_and(|s| Arc::ptr_eq(s, &ticket.session))
        {
            return Err(denied());
        }
        if sessions.contains_key(&key) {
            return Err(unavailable());
        }
        // Checked under the map lock, last: an abandoned request keeps the old pair.
        if watch.abandoned() {
            return Err(unavailable());
        }
        let rotated = Arc::new(session);
        sessions.remove(&ticket.key);
        sessions.insert(key, Arc::clone(&rotated));
        // The waiter can still give up after this check. Then the browser never
        // receives the new pair and still holds the old one, so the rotation did
        // not happen from its side (§4 keeps exactly one valid pair): evict the
        // undelivered pair and rebind the old one. Both share one host handle,
        // which renewal extends but never replaces, so the old pair maps to the
        // same live host session. Only while the new pair is still bound: a
        // sign-out or host denial that evicted it in between stays final.
        let (map, old_key, old) = (
            Arc::clone(&self.sessions),
            ticket.key,
            Arc::clone(&ticket.session),
        );
        watch.on_undelivered(move || {
            if let Ok(mut sessions) = map.lock() {
                if sessions
                    .get(&key)
                    .is_some_and(|current| Arc::ptr_eq(current, &rotated))
                {
                    sessions.remove(&key);
                    sessions.insert(old_key, old);
                }
            }
        });
        Ok(reply)
    }
}
impl Session {
    fn live(&self) -> bool {
        self.created.elapsed() < ABSOLUTE && self.touched.lock().is_ok_and(|at| at.elapsed() < IDLE)
    }
}
impl BrowserSessions {
    /// Requires both the exact single cookie and the exact single proof header.
    /// Nothing is ever recovered from the cookie alone.
    pub fn authenticate(&self, request: &Parts) -> Result<Ticket, ApiReply> {
        self.reap()?;
        if !self.host.live() {
            return Err(denied());
        }
        let mut proofs = request.headers.get_all(SESSION_PROOF_HEADER).iter();
        let proof = proofs
            .next()
            .filter(|p| {
                p.as_bytes().len() == 64
                    && p.as_bytes()
                        .iter()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            })
            .ok_or_else(denied)?;
        if proofs.next().is_some() {
            return Err(denied());
        }
        // The transport bounds the whole header block (HttpLimits::header_bytes),
        // so this allocation-free byte scan stays linear in admitted bytes. Items
        // that cannot be this instance's cookie (no `=`, another name, non-ASCII
        // content) are skipped rather than refused; another site's or app's
        // loopback cookies must not end this session. Our name is exact: any
        // duplicate or malformed value of it is refused.
        let mut value: Option<&[u8]> = None;
        for header in request.headers.get_all(hyper::header::COOKIE) {
            for item in header.as_bytes().split(|b| *b == b';') {
                let item = item.trim_ascii();
                let Some(v) = item
                    .iter()
                    .position(|b| *b == b'=')
                    .filter(|at| item[..*at] == *self.cookie_name.as_bytes())
                    .map(|at| &item[at + 1..])
                else {
                    continue;
                };
                if value.replace(v).is_some() {
                    return Err(denied());
                }
            }
        }
        let value = value
            .filter(|v| {
                v.len() == 64
                    && v.iter()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            })
            .ok_or_else(denied)?;
        let key = Sha256::digest(value).into();
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let session = sessions.get(&key).filter(|s| s.live()).ok_or_else(denied)?;
        self.mac(proof.as_bytes())
            .verify_slice(&session.proof_mac)
            .map_err(|_| denied())?;
        Ok(Ticket {
            key,
            session: Arc::clone(session),
        })
    }
}

/// `Set-Cookie` for the browser pair's cookie half (web-api-v1 §4): HttpOnly,
/// SameSite=Strict, Path=/api/v1, never a Domain, and Secure whenever the
/// configured origin is HTTPS. `clear` expires the cookie with the same scope.
fn set_cookie(name: &str, value: &str, secure: bool, clear: bool) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{name}={value};{} HttpOnly; SameSite=Strict; Path=/api/v1{}",
        if clear { " Max-Age=0;" } else { "" },
        if secure { "; Secure" } else { "" }
    ))
}
