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

use crate::service::ApiReply;
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
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const ABSOLUTE: Duration = Duration::from_secs(12 * 3600);
const IDLE: Duration = Duration::from_secs(30 * 60);
const MAX_SESSIONS: usize = 1024;
type ProofMac = Hmac<Sha256>;
pub struct BrowserSessions {
    host: Arc<HostControl>,
    instance: String,
    cookie_name: String,
    secure: bool,
    proof_key: Zeroizing<[u8; 32]>,
    sessions: Mutex<BTreeMap<[u8; 32], Arc<Session>>>,
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
/// An authenticated request retains the original binding. Refresh/logout revoke
/// it through the registry before a queued request can obtain another grant.
pub struct Ticket {
    key: [u8; 32],
    session: Arc<Session>,
}
pub struct Established {
    pub view: SessionView,
    pub proof: org_proto::gateway::SessionProof,
    pub cookie: Zeroizing<String>,
}
/// The single protected-auth failure shape (gateway-api-v1 §4): 401
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
fn session_failure(error: control::Failure) -> ApiReply {
    match error {
        control::Failure::Denied => denied(),
        control::Failure::NotFound | control::Failure::Unavailable => unavailable(),
    }
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
        Ok(Arc::new(Self {
            host,
            instance,
            cookie_name: format!("org_session_{}", &suffix[..16]),
            secure,
            proof_key: Zeroizing::new(random()?),
            sessions: Mutex::new(BTreeMap::new()),
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
        self.establish(session)
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
    fn establish(&self, session: Session) -> Result<Established, ApiReply> {
        let (key, session, result) = self.prepare_established(session)?;
        self.reap()?;
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        if sessions.len() >= MAX_SESSIONS || sessions.contains_key(&key) {
            drop(sessions);
            let _ = self.host.end(&session.handle);
            return Err(unavailable());
        }
        sessions.insert(key, Arc::new(session));
        Ok(result)
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
        // never holds a host slot until the host's own idle timeout. Best
        // effort, outside the map lock; after control EOF the host has
        // already ended every session on this connection.
        if host_live {
            for session in expired {
                let _ = self.host.end(&session.handle);
            }
        }
        Ok(())
    }
    fn view(&self, session: &Session) -> Result<SessionView, ApiReply> {
        let status = self
            .host
            .inspect(&session.handle)
            .map_err(session_failure)?;
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
            .map_err(session_failure)?;
        *ticket.session.touched.lock().map_err(|_| unavailable())? = Instant::now();
        self.view(&ticket.session)
    }
    pub fn sign_out(&self, ticket: Ticket) -> Result<(), ApiReply> {
        self.current(&ticket)?;
        self.host
            .end(&ticket.session.handle)
            .map_err(session_failure)?;
        self.sessions
            .lock()
            .map_err(|_| unavailable())?
            .remove(&ticket.key);
        Ok(())
    }
    pub fn refresh(&self, ticket: Ticket) -> Result<Established, ApiReply> {
        self.current(&ticket)?;
        self.host
            .renew(&ticket.session.handle)
            .map_err(session_failure)?;
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
        let established = self.prepare_established(session)?;
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        if !sessions
            .get(&ticket.key)
            .is_some_and(|s| Arc::ptr_eq(s, &ticket.session))
        {
            return Err(denied());
        }
        let (key, session, result) = established;
        if sessions.contains_key(&key) {
            return Err(unavailable());
        }
        sessions.remove(&ticket.key);
        sessions.insert(key, Arc::new(session));
        Ok(result)
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
        let mut value = None;
        for header in request.headers.get_all(hyper::header::COOKIE) {
            let header = header.to_str().map_err(|_| denied())?;
            if header.len() > 8192 {
                return Err(denied());
            }
            for item in header.split(';') {
                let Some((name, v)) = item.trim().split_once('=') else {
                    return Err(denied());
                };
                if name == self.cookie_name && value.replace(v).is_some() {
                    return Err(denied());
                }
            }
        }
        let value = value
            .filter(|v| {
                v.len() == 64
                    && v.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or_else(denied)?;
        let key = Sha256::digest(value.as_bytes()).into();
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

/// `Set-Cookie` for the browser pair's cookie half (gateway-api-v1 §4): HttpOnly,
/// SameSite=Strict, Path=/api/v1, never a Domain, and Secure whenever the
/// configured origin is HTTPS. `clear` expires the cookie with the same scope.
fn set_cookie(name: &str, value: &str, secure: bool, clear: bool) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{name}={value};{} HttpOnly; SameSite=Strict; Path=/api/v1{}",
        if clear { " Max-Age=0;" } else { "" },
        if secure { "; Secure" } else { "" }
    ))
}
