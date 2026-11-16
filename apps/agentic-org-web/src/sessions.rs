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

use crate::{
    selected::{OfficeAccess, SessionAuthority},
    service::ApiReply,
};
use hmac::{Hmac, Mac};
use hyper::{http::request::Parts, StatusCode};
use org_host_control::{Password, SessionHandle};
use org_proto::{
    gateway::{AccountView, GatewayErrorCode, Milliseconds, OrganizationLink, SessionView},
    selected_tasks::OfficeScope,
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
fn denied() -> ApiReply {
    ApiReply::error(
        StatusCode::UNAUTHORIZED,
        GatewayErrorCode::NotAuthorized,
        "Session ended. Sign in again.",
    )
}
fn unavailable() -> ApiReply {
    ApiReply::error(
        StatusCode::SERVICE_UNAVAILABLE,
        GatewayErrorCode::Unavailable,
        "Session service unavailable.",
    )
}
fn not_found() -> ApiReply {
    ApiReply::error(
        StatusCode::NOT_FOUND,
        GatewayErrorCode::NotFound,
        "Not found.",
    )
}
fn session_failure(error: control::Failure) -> ApiReply {
    match error {
        control::Failure::Denied => denied(),
        control::Failure::NotFound | control::Failure::Unavailable => unavailable(),
    }
}
fn office_failure(error: control::Failure) -> ApiReply {
    match error {
        control::Failure::Denied => denied(),
        control::Failure::NotFound => not_found(),
        control::Failure::Unavailable => unavailable(),
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
        Zeroizing::new(format!(
            "{}={value}; HttpOnly; SameSite=Strict; Path=/api/v1{}",
            self.cookie_name,
            if self.secure { "; Secure" } else { "" }
        ))
    }
    pub fn clear_cookie(&self) -> Zeroizing<String> {
        Zeroizing::new(format!(
            "{}=; Max-Age=0; HttpOnly; SameSite=Strict; Path=/api/v1{}",
            self.cookie_name,
            if self.secure { "; Secure" } else { "" }
        ))
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
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        sessions.retain(|_, s| s.live() && self.host.live());
        if sessions.len() >= MAX_SESSIONS || sessions.contains_key(&key) {
            drop(sessions);
            let _ = self.host.end(&session.handle);
            return Err(unavailable());
        }
        sessions.insert(key, Arc::new(session));
        Ok(result)
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
impl SessionAuthority for Arc<BrowserSessions> {
    type Ticket = Ticket;
    fn authenticate(&self, request: &Parts) -> Result<Ticket, ApiReply> {
        if !self.host.live() {
            return Err(denied());
        }
        let mut proofs = request.headers.get_all("X-Org-Session-Proof").iter();
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
    fn office(
        &self,
        ticket: Ticket,
        scope: &OfficeScope,
        _timeout: Duration,
    ) -> Result<OfficeAccess, ApiReply> {
        self.current(&ticket)?;
        if scope.instance_id != self.instance {
            return Err(not_found());
        }
        // Re-read protected host links, not the browser's cached display view.
        // Refuse an unlinked organization/office before opening any channel;
        // malformed scope must not close the shared host actor or bind a cached
        // channel to the caller's invented organization.
        let view = self.view(&ticket.session)?;
        if !view
            .organizations
            .iter()
            .any(|link| link.org_id == scope.org_id && link.office_ids.contains(&scope.office_id))
        {
            return Err(not_found());
        }
        let access = self
            .host
            .office(&ticket.session.handle, scope)
            .map_err(office_failure)?;
        self.current(&ticket)?;
        if access.principal.grant.claims.account != ticket.session.account.id {
            return Err(denied());
        }
        *ticket.session.touched.lock().map_err(|_| unavailable())? = Instant::now();
        Ok(access)
    }
}
