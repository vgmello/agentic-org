//! The real office: `OfficeConnection` over `org-client`, talking to this machine's host and the HQ office's
//! socket. Phase 1 has one organization per HQ on this Mac, so every call goes to a local socket.
//!
//! Setup: `ensure_running` says `hello` to the host; when nothing is listening it turns the host's login service
//! on (see `login_item`) and waits for it to come up, or says what the owner has to do (allow it in System
//! Settings). `ping` never does any of that. The mock office stays the default until the host is bundled
//! (`AGENTIC_ORG_OFFICE=real` picks this one).

use crate::login_item::{LoginService, LoginState};
use crate::office::{OfficeConnection, SetupError, SetupErrorCode};
use crate::org_command::OrgCommand;
use org_client::{Client, ClientError};
use org_proto::org::{
    MemberAddAgent, MemberAddPerson, MemberMove, MemberOffboard, MemberRemove,
    MemberSetAgentStatus, MemberSetReportsTo, MemberSetRunMode, MemberUpdateAgent,
    MemberUpdatePerson, ProjectAddTeam, ProjectRemoveTeam, TeamAddMember, TeamCreate, TeamDelete,
    TeamMove, TeamRemoveMember, TeamRename, TeamSetLead, TeamSetTitle,
};
use org_proto::{
    ApiError, CommandAccepted, Empty, ErrorCode, HelloResult, OfficeInfo, OfficeRole, OfficeStatus,
    OrgSnapshot, OrgSnapshotCall, Organization,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a host that was just turned on has to start answering.
// Longer than the plist's ThrottleInterval (10 s), so a host that crashed once is back inside it.
const HOST_START_WAIT: Duration = Duration::from_secs(15);
const HOST_START_POLL: Duration = Duration::from_millis(200);

pub struct RealOffice {
    home: PathBuf,
    /// The login service the host runs under: turned on when the host isn't there.
    login: Arc<dyn LoginService>,
    start_wait: Duration,
    host: Mutex<Option<Arc<Client>>>,
    /// Open connections to HQ offices, by office id.
    offices: Mutex<HashMap<String, Arc<Client>>>,
    /// The HQ office the host listed for each organization, so a call doesn't list organizations first; forgotten
    /// when a connection to it fails, and asked for again.
    hq: Mutex<HashMap<String, OfficeInfo>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a failed call means for the app's setup layer: the host can't be reached (not running, gone, or silent).
fn setup_error(e: &ClientError) -> SetupError {
    SetupError {
        code: SetupErrorCode::Unreachable,
        message: failure_words(e).to_string(),
    }
}

fn failure_words(e: &ClientError) -> &'static str {
    match e {
        ClientError::NoOfficeHome => "The app can't find the office folder on this Mac.",
        ClientError::NotRunning(_) => "The office host isn't running.",
        ClientError::VersionMismatch { .. } => {
            "The office host needs a newer version of Agentic Org."
        }
        ClientError::Timeout => "The office didn't answer in time.",
        ClientError::RequestTooLarge => "That request is too large to send.",
        ClientError::OfficeNotReachable | ClientError::SocketPathTooLong(_) => {
            "The app can't reach that office on this Mac."
        }
        ClientError::Protocol(_) => "The office sent an answer the app couldn't read.",
        _ => "The office host isn't answering.",
    }
}

/// What a failed call means for the organization calls: the office's own refusal as it gave it; a request that
/// can't be sent as `invalid_request`; failures a restart can't fix (a bad answer, a wrong version, an office the
/// app can't reach) and a silent office (it may still be working) as `unavailable`, which the shell doesn't restart
/// over and so never sends again; and a lost connection as `unknown`, which the shell reads as "the office isn't
/// there: start it again" (see `with_restart`).
fn api_error(e: ClientError) -> ApiError {
    let code = match &e {
        ClientError::Api(api) => return api.clone(),
        ClientError::RequestTooLarge => ErrorCode::InvalidRequest,
        ClientError::NoOfficeHome
        | ClientError::VersionMismatch { .. }
        | ClientError::OfficeNotReachable
        | ClientError::SocketPathTooLong(_)
        | ClientError::Protocol(_)
        | ClientError::Timeout => ErrorCode::Unavailable,
        _ => ErrorCode::Unknown,
    };
    let message = match &e {
        ClientError::NotRunning(_) | ClientError::Io(_) | ClientError::Disconnected => {
            "The connection to the office was lost."
        }
        _ => failure_words(&e),
    };
    ApiError::new(code, message)
}

/// A connection that has failed is gone: the next call opens a new one. A plain timeout is not that: the answer
/// may just be slow, and the connection is kept.
fn dead(e: &ClientError) -> bool {
    matches!(
        e,
        ClientError::Disconnected | ClientError::Io(_) | ClientError::NotRunning(_)
    )
}

/// Forget `failed` if it is still the cached one (another thread may already have replaced it), and drop it after
/// the lock is released: dropping a client can wait for its reader thread.
fn forget<K: std::hash::Hash + Eq>(
    cache: &Mutex<HashMap<K, Arc<Client>>>,
    key: &K,
    failed: &Arc<Client>,
) {
    let old = {
        let mut cache = lock(cache);
        match cache.get(key) {
            Some(c) if Arc::ptr_eq(c, failed) => cache.remove(key),
            _ => None,
        }
    };
    drop(old);
}

/// Why an office that isn't running (or can't be used) isn't connected to, in words. `None`: try to connect.
fn not_connectable(status: OfficeStatus) -> Option<ApiError> {
    let message = match status {
        OfficeStatus::Unavailable => "That office isn't available right now.",
        OfficeStatus::Starting => "That office is still starting. Try again in a moment.",
        OfficeStatus::Offline => "That office isn't connected right now.",
        OfficeStatus::Pending => "That office hasn't joined yet.",
        OfficeStatus::UpdateNeeded => "That office needs a newer version of Agentic Org.",
        // Active, Stopped and any status this app doesn't know yet: try to connect (the office answers for itself).
        _ => return None,
    };
    Some(ApiError::new(ErrorCode::Unavailable, message))
}

impl RealOffice {
    pub fn new(home: PathBuf, login: Arc<dyn LoginService>) -> Self {
        Self {
            home,
            login,
            start_wait: HOST_START_WAIT,
            host: Mutex::new(None),
            offices: Mutex::new(HashMap::new()),
            hq: Mutex::new(HashMap::new()),
        }
    }

    /// The real office in this user's office home, or `None` when there is no home to look in.
    pub fn from_env(data_dir: &std::path::Path) -> Option<Self> {
        org_proto::location::default_office_home()
            .map(|home| Self::new(home, Arc::from(crate::login_item::system_in(data_dir))))
    }

    /// The host's connection: the cached one, or a new one that has said `hello` (so a host of another API version
    /// is noticed when connecting, not later).
    fn host(&self) -> Result<Arc<Client>, ClientError> {
        let cached = lock(&self.host).as_ref().map(Arc::clone);
        if let Some(client) = cached {
            return Ok(client);
        }
        let client = Arc::new(Client::connect_host_in(&self.home)?);
        client.hello()?;
        let replaced = lock(&self.host).replace(Arc::clone(&client));
        drop(replaced);
        Ok(client)
    }

    /// Forget the host connection, if it is still `failed` (or whatever it is, when `failed` is `None`): another
    /// thread may already have replaced it with a good one. Dropped after the lock is released.
    fn forget_host(&self, failed: Option<&Arc<Client>>) {
        let old = {
            let mut host = lock(&self.host);
            match (host.as_ref(), failed) {
                (Some(current), Some(failed)) if !Arc::ptr_eq(current, failed) => None,
                _ => host.take(),
            }
        };
        drop(old);
    }

    /// Say `hello` to the host. One that gets no answer drops the connection too, so a restart doesn't reuse a
    /// wedged one.
    fn hello(&self) -> Result<HelloResult, ClientError> {
        self.with_host(true, |c| c.hello()).inspect_err(|e| {
            if matches!(e, ClientError::Timeout) {
                self.forget_host(None);
            }
        })
    }

    /// The host isn't there: turn its login service on (once; macOS may ask the owner to approve it) and wait for
    /// the host to answer. What the owner has to do, if anything, comes back as the error.
    fn start_host(&self) -> Result<HelloResult, SetupError> {
        let mut state = self.login.status();
        let mut refused = None;
        if state == LoginState::NotRegistered && self.login.turned_off() {
            return Err(SetupError {
                code: SetupErrorCode::TurnedOff,
                message: "The office is off. Turn it on to start your office on this Mac.".into(),
            });
        }
        if state == LoginState::NotRegistered {
            refused = self.login.register().err();
            state = self.login.status();
        }
        match state {
            LoginState::Enabled => self.wait_for_host(),
            LoginState::RequiresApproval => Err(SetupError {
                code: SetupErrorCode::NeedsApproval,
                message: "Allow Agentic Org in System Settings, under General, Login Items, then try again.".into(),
            }),
            LoginState::NotFound => Err(SetupError {
                code: SetupErrorCode::NotInstalled,
                message: "This copy of Agentic Org doesn't include the office service. Reinstall the app.".into(),
            }),
            LoginState::NotRegistered => Err(SetupError {
                code: SetupErrorCode::NotInstalled,
                message: match refused {
                    Some(reason) => format!("macOS wouldn't turn on the office service: {reason}"),
                    None => "The office service isn't turned on.".into(),
                },
            }),
        }
    }

    fn wait_for_host(&self) -> Result<HelloResult, SetupError> {
        let deadline = Instant::now() + self.start_wait;
        loop {
            match self.hello() {
                Ok(hello) => return Ok(hello),
                // Not listening yet, or accepted and not answering yet: launchd is still starting the host.
                Err(ClientError::NotRunning(_) | ClientError::Io(_) | ClientError::Timeout)
                    if Instant::now() < deadline =>
                {
                    std::thread::sleep(HOST_START_POLL);
                }
                Err(ClientError::NotRunning(_) | ClientError::Io(_) | ClientError::Timeout) => {
                    return Err(SetupError {
                        code: SetupErrorCode::Unreachable,
                        message: "The office service is on, but the host didn't start. Try again in a moment.".into(),
                    })
                }
                Err(e) => return Err(setup_error(&e)),
            }
        }
    }

    /// Run a call on the host. `again`: if the connection had died, forget it and try once on a new one; only for
    /// calls that are safe to send twice. Anything else (creating an organization) is sent once.
    fn with_host<T>(
        &self,
        again: bool,
        f: impl Fn(&Client) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let client = self.host()?;
        match f(&client) {
            Err(e) if dead(&e) => {
                self.forget_host(Some(&client));
                if again {
                    f(&*self.host()?)
                } else {
                    Err(e)
                }
            }
            other => other,
        }
    }

    /// The HQ office of `organization_id` on this machine: remembered, or as the host lists it now.
    fn hq_of(&self, organization_id: &str, fresh: bool) -> Result<OfficeInfo, ApiError> {
        if !fresh {
            if let Some(office) = lock(&self.hq).get(organization_id) {
                return Ok(office.clone());
            }
        }
        let organizations = self
            .with_host(true, |c| c.list_organizations())
            .map_err(api_error)?;
        let organization = organizations
            .into_iter()
            .find(|o| o.id == organization_id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "That organization isn't here."))?;
        let office = organization
            .offices
            .into_iter()
            .find(|o| o.role == OfficeRole::Hq && o.organization_id == organization_id)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::NotFound,
                    "That organization has no HQ office on this Mac.",
                )
            })?;
        lock(&self.hq).insert(organization_id.to_string(), office.clone());
        Ok(office)
    }

    fn connect_office(&self, office: &OfficeInfo) -> Result<Arc<Client>, ClientError> {
        let cached = lock(&self.offices).get(&office.id).map(Arc::clone);
        if let Some(client) = cached {
            return Ok(client);
        }
        let client = Arc::new(Client::connect_office(office, &self.home)?);
        client.hello()?; // a version this app can't speak is noticed here
        let replaced = lock(&self.offices).insert(office.id.clone(), Arc::clone(&client));
        drop(replaced);
        Ok(client)
    }

    /// Run a call on the organization's HQ office. If the connection had died (or the office is not where it was),
    /// the connection and the remembered office are forgotten, the host is asked again where the HQ office is, and
    /// the call is sent once more: reads are safe to send twice, and so are org commands, which carry a `command_id`.
    /// A timeout is not retried: the answer may only be slow, and the command may have been applied.
    fn with_hq<T>(
        &self,
        organization_id: &str,
        f: impl Fn(&Client) -> Result<T, ClientError>,
    ) -> Result<T, ApiError> {
        let remembered = lock(&self.hq).contains_key(organization_id);
        let mut office = self.hq_of(organization_id, false)?;
        // What the host said earlier may be old: before refusing over its status, ask it again (an office that was
        // starting is up now). What it said just now is not asked twice.
        if remembered && not_connectable(office.status).is_some() {
            office = self.hq_of(organization_id, true)?;
        }
        if let Some(refusal) = not_connectable(office.status) {
            return Err(refusal);
        }
        match self.call_office(&office, &f) {
            Err(e) if dead(&e) || matches!(e, ClientError::OfficeNotReachable) => {
                lock(&self.hq).remove(organization_id);
                let latest = self.hq_of(organization_id, true)?;
                if let Some(refusal) = not_connectable(latest.status) {
                    return Err(refusal);
                }
                self.call_office(&latest, &f).map_err(api_error)
            }
            other => other.map_err(api_error),
        }
    }

    /// One attempt on the connection to `office`; a connection that fails is forgotten.
    fn call_office<T>(
        &self,
        office: &OfficeInfo,
        f: &impl Fn(&Client) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let client = self.connect_office(office)?;
        let result = f(&client);
        if matches!(&result, Err(e) if dead(e)) {
            forget(&self.offices, &office.id, &client);
        }
        result
    }
}

impl OfficeConnection for RealOffice {
    fn ensure_running(&self) -> Result<HelloResult, SetupError> {
        match self.hello() {
            Err(ClientError::NotRunning(_)) => self.start_host(),
            other => other.map_err(|e| setup_error(&e)),
        }
    }

    fn ping(&self) -> Result<(), SetupError> {
        self.hello().map(|_| ()).map_err(|e| setup_error(&e))
    }

    fn create_organization(&self, name: &str) -> Result<Organization, ApiError> {
        // Never run this through `AppState::with_restart` (it re-runs calls that fail as `unknown`): creating is not
        // safe to send twice, as the host would answer `already_exists` for what it just made. Make sure the connection
        // is alive first (a call that is safe to repeat), then send this once.
        self.with_host(true, |c| c.hello()).map_err(api_error)?;
        let sent = std::cell::Cell::new(false);
        self.with_host(false, |c| {
            sent.set(true);
            c.create_organization(name)
        })
        .map_err(|e| match e {
            // The host may have created it: say so, and the owner can look in Open organization. (Not when the
            // connection could not be made at all: then nothing was sent.)
            ClientError::Timeout
            | ClientError::Disconnected
            | ClientError::Io(_)
            | ClientError::Protocol(_)
                if sent.get() =>
            {
                ApiError::new(
                    ErrorCode::Unknown,
                    "The office didn't confirm. The organization may have been created: look in Open organization.",
                )
            }
            other => api_error(other),
        })
    }

    fn list_organizations(&self) -> Result<Vec<Organization>, ApiError> {
        self.with_host(true, |c| c.list_organizations())
            .map_err(api_error)
    }

    fn org_snapshot(&self, organization_id: &str) -> Result<OrgSnapshot, ApiError> {
        self.with_hq(organization_id, |c| c.call::<OrgSnapshotCall>(&Empty {}))
    }

    fn org_command(
        &self,
        organization_id: &str,
        command: &OrgCommand,
    ) -> Result<CommandAccepted, ApiError> {
        self.with_hq(organization_id, |c| {
            send(c, command).map_err(|e| match e {
                // A silent office may have applied it: sending the same command_id again gets the same answer.
                ClientError::Timeout => ClientError::Api(ApiError::new(
                    ErrorCode::Unavailable,
                    "The office didn't answer in time. The change may or may not have been applied: reload to see.",
                )),
                other => other,
            })
        })
    }

    fn serves_sample_data(&self) -> bool {
        false
    }
}

/// Each command goes out under its own method with its own params, as the contract names them.
fn send(c: &Client, command: &OrgCommand) -> Result<CommandAccepted, ClientError> {
    match command {
        OrgCommand::TeamCreate(p) => c.call::<TeamCreate>(p),
        OrgCommand::TeamRename(p) => c.call::<TeamRename>(p),
        OrgCommand::TeamMove(p) => c.call::<TeamMove>(p),
        OrgCommand::TeamSetLead(p) => c.call::<TeamSetLead>(p),
        OrgCommand::TeamDelete(p) => c.call::<TeamDelete>(p),
        OrgCommand::TeamAddMember(p) => c.call::<TeamAddMember>(p),
        OrgCommand::TeamRemoveMember(p) => c.call::<TeamRemoveMember>(p),
        OrgCommand::TeamSetTitle(p) => c.call::<TeamSetTitle>(p),
        OrgCommand::MemberAddPerson(p) => c.call::<MemberAddPerson>(p),
        OrgCommand::MemberUpdatePerson(p) => c.call::<MemberUpdatePerson>(p),
        OrgCommand::MemberRemove(p) => c.call::<MemberRemove>(p),
        OrgCommand::MemberSetReportsTo(p) => c.call::<MemberSetReportsTo>(p),
        OrgCommand::MemberMove(p) => c.call::<MemberMove>(p),
        OrgCommand::MemberAddAgent(p) => c.call::<MemberAddAgent>(p),
        OrgCommand::MemberUpdateAgent(p) => c.call::<MemberUpdateAgent>(p),
        OrgCommand::MemberSetAgentStatus(p) => c.call::<MemberSetAgentStatus>(p),
        OrgCommand::MemberSetRunMode(p) => c.call::<MemberSetRunMode>(p),
        OrgCommand::MemberOffboard(p) => c.call::<MemberOffboard>(p),
        OrgCommand::ProjectAddTeam(p) => c.call::<ProjectAddTeam>(p),
        OrgCommand::ProjectRemoveTeam(p) => c.call::<ProjectRemoveTeam>(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login_item::Unsupported;
    use crate::org_mock::empty_org;
    use org_proto::frame::{read_message, write_message};
    use org_proto::location::{host_socket_path, office_socket_path};
    use org_proto::{
        CommandStatus, Hello, ListOrganizationsResult, OfficeInfo, OrganizationList, Position,
        Request, Response,
    };
    use std::io::BufReader;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const HQ: &str = "hq1";

    fn hq(status: OfficeStatus) -> OfficeInfo {
        OfficeInfo {
            id: HQ.into(),
            organization_id: "org1".into(),
            name: "HQ".into(),
            role: OfficeRole::Hq,
            host: "This Mac".into(),
            endpoint: None,
            status,
        }
    }

    /// Answers each request, or closes the connection (`None`).
    type Answer = dyn Fn(&Request) -> Option<Response> + Send + Sync;

    fn serve(listener: UnixListener, answer: Arc<Answer>) {
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let answer = Arc::clone(&answer);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer: UnixStream = stream;
                    while let Ok(Some(request)) = read_message::<Request>(&mut reader) {
                        let Some(response) = answer(&request) else {
                            break;
                        };
                        if write_message(&mut writer, &response).is_err() {
                            break;
                        }
                    }
                });
            }
        });
    }

    /// A host and an HQ office on Unix sockets in a short temporary home (socket paths are short on macOS).
    struct Fake {
        home: tempfile::TempDir,
        /// Every method the HQ office was asked, in order.
        asked: Arc<Mutex<Vec<String>>>,
        /// How many requests the host has answered or refused to.
        host_requests: Arc<AtomicUsize>,
    }

    /// `host_hangs_up_on_request`: the host closes the connection on its Nth request (0: never).
    fn fake(status: OfficeStatus, host_hangs_up_on_request: usize) -> Fake {
        fake_with(host_hangs_up_on_request, vec![hq(status)])
    }

    /// Like `fake`, with the offices the host lists for the organization (the HQ office's socket is `hq1`'s).
    fn fake_with(host_hangs_up_on_request: usize, offices: Vec<OfficeInfo>) -> Fake {
        let home = tempfile::Builder::new().prefix("ao").tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("run")).unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let host_requests = Arc::new(AtomicUsize::new(0));
        let organization = Organization {
            id: "org1".into(),
            name: "Acme".into(),
            created_at: "2026-11-08T10:00:00.000Z".into(),
            offices,
        };

        let host = UnixListener::bind(host_socket_path(home.path())).unwrap();
        let count = Arc::clone(&host_requests);
        serve(
            host,
            Arc::new(move |r| {
                let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                if host_hangs_up_on_request == n {
                    return None;
                }
                Some(match r.method.as_str() {
                    "hello" => Response::ok::<Hello>(
                        r.id,
                        &HelloResult {
                            api_version: org_proto::API_VERSION,
                            server_version: "test".into(),
                            host: "This Mac".into(),
                            office: None,
                        },
                    )
                    .unwrap(),
                    "organization.list" => Response::ok::<OrganizationList>(
                        r.id,
                        &ListOrganizationsResult {
                            organizations: vec![organization.clone()],
                        },
                    )
                    .unwrap(),
                    "organization.create" if r.params["name"] == "Taken" => {
                        Response::error(r.id, ApiError::new(ErrorCode::AlreadyExists, "Taken."))
                    }
                    "organization.create" => {
                        Response::ok::<org_proto::OrganizationCreate>(r.id, &organization).unwrap()
                    }
                    _ => Response::error(r.id, ApiError::new(ErrorCode::UnknownMethod, "no")),
                })
            }),
        );

        let office = UnixListener::bind(office_socket_path(home.path(), HQ).unwrap()).unwrap();
        let log = Arc::clone(&asked);
        serve(
            office,
            Arc::new(move |r| {
                if r.method == "hello" {
                    return Some(
                        Response::ok::<Hello>(
                            r.id,
                            &HelloResult {
                                api_version: org_proto::API_VERSION,
                                server_version: "test".into(),
                                host: "This Mac".into(),
                                office: None,
                            },
                        )
                        .unwrap(),
                    );
                }
                log.lock().unwrap().push(r.method.clone());
                Some(match r.method.as_str() {
                    "org.snapshot" => Response::ok::<OrgSnapshotCall>(r.id, &empty_org()).unwrap(),
                    _ if r.params["name"] == "Taken" => {
                        Response::error(r.id, ApiError::new(ErrorCode::AlreadyExists, "Taken."))
                    }
                    _ => Response::ok::<TeamCreate>(
                        r.id,
                        &CommandAccepted {
                            status: CommandStatus::Committed,
                            position: Some(Position { epoch: 1, seq: 2 }),
                        },
                    )
                    .unwrap(),
                })
            }),
        );
        Fake {
            home,
            asked,
            host_requests,
        }
    }

    fn real(f: &Fake) -> RealOffice {
        RealOffice::new(f.home.path().to_path_buf(), Arc::new(Unsupported))
    }

    fn command(method: &str, params: serde_json::Value) -> OrgCommand {
        let mut params = params;
        params["command_id"] = serde_json::json!("c1");
        serde_json::from_value(serde_json::json!({"method": method, "params": params})).unwrap()
    }

    #[test]
    fn it_says_hello_and_pings_the_host() {
        let f = fake(OfficeStatus::Active, 0);
        let office = real(&f);
        assert_eq!(office.ensure_running().unwrap().host, "This Mac");
        office.ping().unwrap();
        assert!(!office.serves_sample_data());
    }

    #[test]
    fn a_host_that_is_not_there_is_unreachable_and_says_so() {
        let home = tempfile::Builder::new().prefix("ao").tempdir().unwrap();
        let office = RealOffice::new(home.path().to_path_buf(), Arc::new(Unsupported));
        // A ping only looks: it never starts the host.
        assert_eq!(
            office.ping().unwrap_err().message,
            "The office host isn't running."
        );
        assert_eq!(office.ping().unwrap_err().code, SetupErrorCode::Unreachable);
        // Organization calls report the transport failure as `unknown`, which the shell reads as "start it again".
        assert_eq!(
            office.list_organizations().unwrap_err().code,
            ErrorCode::Unknown
        );
    }

    #[test]
    fn organizations_are_listed_and_created_through_the_host_and_its_refusals_pass_through() {
        let f = fake(OfficeStatus::Active, 0);
        let office = real(&f);
        assert_eq!(office.list_organizations().unwrap()[0].name, "Acme");
        assert_eq!(office.create_organization("Acme 2").unwrap().id, "org1");
        let err = office.create_organization("Taken").unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::AlreadyExists, "Taken.")
        );
    }

    #[test]
    fn the_structure_comes_from_the_hq_office_of_that_organization() {
        let f = fake(OfficeStatus::Active, 0);
        let office = real(&f);
        let snapshot = office.org_snapshot("org1").unwrap();
        assert_eq!(snapshot.members.iter().filter(|m| m.owner).count(), 1);
        assert_eq!(f.asked.lock().unwrap().as_slice(), ["org.snapshot"]);
        let err = office.org_snapshot("nope").unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn an_office_the_host_calls_unavailable_is_not_connected_to() {
        let f = fake(OfficeStatus::Unavailable, 0);
        let err = real(&f).org_snapshot("org1").unwrap_err();
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert!(f.asked.lock().unwrap().is_empty(), "nothing was sent to it");
    }

    #[test]
    fn a_command_is_answered_and_the_offices_refusal_comes_back_as_it_was() {
        let f = fake(OfficeStatus::Active, 0);
        let office = real(&f);
        let ok = office
            .org_command(
                "org1",
                &command(
                    "team.create",
                    serde_json::json!({"team_id": "t", "name": "Ops"}),
                ),
            )
            .unwrap();
        assert_eq!(ok.status, CommandStatus::Committed);
        let err = office
            .org_command(
                "org1",
                &command(
                    "team.create",
                    serde_json::json!({"team_id": "t", "name": "Taken"}),
                ),
            )
            .unwrap_err();
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::AlreadyExists, "Taken.")
        );
    }

    #[test]
    fn every_command_goes_out_under_its_own_contract_method() {
        let f = fake(OfficeStatus::Active, 0);
        let office = real(&f);
        let all = [
            (
                "team.create",
                serde_json::json!({"team_id": "t", "name": "n"}),
            ),
            (
                "team.rename",
                serde_json::json!({"team_id": "t", "expected_version": 1, "name": "n"}),
            ),
            (
                "team.move",
                serde_json::json!({"team_id": "t", "expected_version": 1}),
            ),
            (
                "team.set_lead",
                serde_json::json!({"team_id": "t", "expected_version": 1}),
            ),
            (
                "team.delete",
                serde_json::json!({"team_id": "t", "expected_version": 1}),
            ),
            (
                "team.add_member",
                serde_json::json!({"membership_id": "m", "team_id": "t", "member_id": "p"}),
            ),
            (
                "team.remove_member",
                serde_json::json!({"membership_id": "m", "expected_version": 1}),
            ),
            (
                "team.set_title",
                serde_json::json!({"membership_id": "m", "expected_version": 1}),
            ),
            (
                "member.add_person",
                serde_json::json!({"member_id": "p", "name": "n"}),
            ),
            (
                "member.update_person",
                serde_json::json!({"member_id": "p", "expected_version": 1}),
            ),
            (
                "member.remove",
                serde_json::json!({"member_id": "p", "expected_version": 1}),
            ),
            (
                "member.set_reports_to",
                serde_json::json!({"member_id": "p", "expected_version": 1}),
            ),
            (
                "member.move",
                serde_json::json!({"membership_id": "m", "expected_version": 1, "new_membership_id": "n", "to_team_id": "t"}),
            ),
            (
                "member.add_agent",
                serde_json::json!({"member_id": "a", "name": "A", "office_id": "o", "profile": {"role": "r", "title": "t", "runtime": "codex", "messaging_name": "a", "status": "active"}}),
            ),
            (
                "member.update_agent",
                serde_json::json!({"member_id": "a", "expected_version": 1, "role": "r"}),
            ),
            (
                "member.set_agent_status",
                serde_json::json!({"member_id": "a", "expected_version": 1, "status": "paused"}),
            ),
            (
                "member.set_run_mode",
                serde_json::json!({"member_id": "a", "expected_version": 1}),
            ),
            (
                "member.offboard",
                serde_json::json!({"member_id": "a", "expected_version": 1}),
            ),
            (
                "project.add_team",
                serde_json::json!({"project_team_id": "x", "project_id": "p", "team_id": "t"}),
            ),
            (
                "project.remove_team",
                serde_json::json!({"project_team_id": "x", "expected_version": 1}),
            ),
        ];
        for (method, params) in &all {
            office
                .org_command("org1", &command(method, params.clone()))
                .unwrap();
        }
        let asked = f.asked.lock().unwrap();
        let methods: Vec<&str> = all.iter().map(|(m, _)| *m).collect();
        assert_eq!(
            asked.iter().map(String::as_str).collect::<Vec<_>>(),
            methods
        );
    }

    #[test]
    fn a_connection_that_died_is_replaced_once_and_the_call_goes_through() {
        // The host hangs up on its second request (as if it had restarted); the call opens a new connection.
        // Each new connection says hello first, then the call: ping #1 is requests 1-2, ping #2 is request 3 (dropped),
        // then a new connection (4) and the call again (5).
        let f = fake(OfficeStatus::Active, 3);
        let office = real(&f);
        office.ping().unwrap();
        office.ping().unwrap();
        assert_eq!(f.host_requests.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn creating_an_organization_is_sent_once_even_when_the_connection_dies_and_says_it_may_have_worked(
    ) {
        // Requests: hello on connecting (1), hello to check the connection (2), create (3: the host hangs up).
        let f = fake(OfficeStatus::Active, 3);
        let err = real(&f).create_organization("Acme").unwrap_err();
        assert_eq!(err.code, ErrorCode::Unknown);
        assert!(
            err.message.contains("may have been created"),
            "{}",
            err.message
        );
        assert_eq!(
            f.host_requests.load(Ordering::SeqCst),
            3,
            "create was not sent again"
        );
    }

    #[test]
    fn an_office_that_is_starting_or_offline_is_not_connected_to_and_says_why() {
        for (status, words) in [
            (OfficeStatus::Starting, "still starting"),
            (OfficeStatus::Offline, "isn't connected"),
            (OfficeStatus::Pending, "hasn't joined"),
            (OfficeStatus::UpdateNeeded, "newer version"),
        ] {
            let f = fake(status, 0);
            let err = real(&f).org_snapshot("org1").unwrap_err();
            assert_eq!(err.code, ErrorCode::Unavailable);
            assert!(err.message.contains(words), "{status:?}: {}", err.message);
            assert!(f.asked.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn an_organization_without_an_hq_office_here_is_not_found() {
        let mut other = hq(OfficeStatus::Active);
        other.role = OfficeRole::Satellite;
        let f = fake_with(0, vec![other]);
        assert_eq!(
            real(&f).org_snapshot("org1").unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn an_office_id_that_is_not_a_plain_id_is_never_turned_into_a_path() {
        let mut hostile = hq(OfficeStatus::Active);
        hostile.id = "../../etc/x".into();
        let f = fake_with(0, vec![hostile]);
        let err = real(&f).org_snapshot("org1").unwrap_err();
        assert_eq!(err.code, ErrorCode::Unavailable);
    }

    #[test]
    fn an_office_that_is_gone_is_tried_again_once_and_then_reported_as_lost() {
        let mut gone = hq(OfficeStatus::Active);
        gone.id = "gone".into(); // no socket for it
        let f = fake_with(0, vec![gone]);
        let err = real(&f).org_snapshot("org1").unwrap_err();
        assert_eq!(err.code, ErrorCode::Unknown);
        // hello on connecting, the first list, and the one list that asks again: no loop.
        assert_eq!(f.host_requests.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn failures_that_a_restart_cannot_fix_are_unavailable_and_a_lost_connection_is_unknown() {
        let code = |e: ClientError| api_error(e).code;
        assert_eq!(
            code(ClientError::RequestTooLarge),
            ErrorCode::InvalidRequest
        );
        assert_eq!(code(ClientError::Timeout), ErrorCode::Unavailable);
        assert_eq!(
            code(ClientError::Protocol("x".into())),
            ErrorCode::Unavailable
        );
        assert_eq!(
            code(ClientError::OfficeNotReachable),
            ErrorCode::Unavailable
        );
        assert_eq!(
            code(ClientError::VersionMismatch {
                client: 1,
                server: 2
            }),
            ErrorCode::Unavailable
        );
        assert_eq!(code(ClientError::Disconnected), ErrorCode::Unknown);
        assert_eq!(
            code(ClientError::Io(std::io::Error::other("x"))),
            ErrorCode::Unknown
        );
        // Only a lost connection is worth a new one; a timeout, an answer or a refusal is not.
        assert!(dead(&ClientError::Disconnected) && !dead(&ClientError::Timeout));
        assert!(!dead(&ClientError::Api(ApiError::new(
            ErrorCode::Conflict,
            "c"
        ))));
    }

    #[test]
    fn the_hq_office_is_remembered_and_not_asked_for_on_every_call() {
        let f = fake(OfficeStatus::Active, 0);
        let office = real(&f);
        office.org_snapshot("org1").unwrap();
        let after_first = f.host_requests.load(Ordering::SeqCst); // hello on connecting, then the list
        assert_eq!(after_first, 2);
        office.org_snapshot("org1").unwrap();
        assert_eq!(f.host_requests.load(Ordering::SeqCst), after_first);
    }

    #[test]
    fn a_status_the_host_just_gave_is_not_asked_again_but_a_remembered_one_is() {
        let f = fake(OfficeStatus::Starting, 0);
        let office = real(&f);
        office.org_snapshot("org1").unwrap_err();
        // hello, list. (Nothing is remembered about an office that can't be used... it was, so the next call asks once more.)
        assert_eq!(f.host_requests.load(Ordering::SeqCst), 2);
        office.org_snapshot("org1").unwrap_err();
        assert_eq!(f.host_requests.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn an_office_listed_under_another_organization_is_not_used_as_its_hq() {
        let mut stray = hq(OfficeStatus::Active);
        stray.organization_id = "org2".into();
        let f = fake_with(0, vec![stray]);
        assert_eq!(
            real(&f).org_snapshot("org1").unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(f.asked.lock().unwrap().is_empty());
    }

    // --- Starting the host through its login service ---

    use crate::login_item::FakeLogin;

    fn without_host() -> tempfile::TempDir {
        let home = tempfile::Builder::new().prefix("ao").tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("run")).unwrap();
        home
    }

    fn office_with(home: &tempfile::TempDir, login: Arc<FakeLogin>, wait_ms: u64) -> RealOffice {
        let mut office = RealOffice::new(home.path().to_path_buf(), login);
        office.start_wait = Duration::from_millis(wait_ms);
        office
    }

    fn setup(result: Result<HelloResult, SetupError>) -> (SetupErrorCode, String) {
        let e = result.unwrap_err();
        (e.code, e.message)
    }

    /// A host that appears when the login service is registered, as launchd would start it.
    fn host_appears_on_register(home: &std::path::Path, login: &mut FakeLogin) {
        let path = host_socket_path(home);
        login.on_register = Box::new(move || {
            let listener = UnixListener::bind(&path).unwrap();
            serve(
                listener,
                Arc::new(|r| {
                    Some(
                        Response::ok::<Hello>(
                            r.id,
                            &HelloResult {
                                api_version: org_proto::API_VERSION,
                                server_version: "test".into(),
                                host: "This Mac".into(),
                                office: None,
                            },
                        )
                        .unwrap(),
                    )
                }),
            );
        });
    }

    #[test]
    fn a_host_that_is_already_there_does_not_touch_the_login_service() {
        let f = fake(OfficeStatus::Active, 0);
        let login = Arc::new(FakeLogin::new(&[LoginState::NotRegistered]));
        let office = RealOffice::new(f.home.path().to_path_buf(), login.clone());
        office.ensure_running().unwrap();
        assert_eq!(login.register_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_missing_host_is_started_by_turning_the_login_service_on() {
        let home = without_host();
        let mut login = FakeLogin::new(&[LoginState::NotRegistered, LoginState::Enabled]);
        host_appears_on_register(home.path(), &mut login);
        let login = Arc::new(login);
        let hello = office_with(&home, login.clone(), 3000)
            .ensure_running()
            .unwrap();
        assert_eq!(hello.host, "This Mac");
        assert_eq!(login.register_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_login_service_waiting_for_approval_says_so_without_registering_again() {
        let home = without_host();
        let login = Arc::new(FakeLogin::new(&[LoginState::RequiresApproval]));
        let result = office_with(&home, login.clone(), 50).ensure_running();
        let (code, message) = setup(result);
        assert_eq!(code, SetupErrorCode::NeedsApproval);
        assert!(message.contains("Login Items"), "{message}");
        assert_eq!(login.register_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn registering_that_leaves_the_service_waiting_for_approval_asks_for_approval() {
        let home = without_host();
        let mut login = FakeLogin::new(&[LoginState::NotRegistered, LoginState::RequiresApproval]);
        login.register_result = Err("Operation not permitted".into());
        let login = Arc::new(login);
        let (code, _) = setup(office_with(&home, login.clone(), 50).ensure_running());
        assert_eq!(code, SetupErrorCode::NeedsApproval);
        assert_eq!(login.register_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_build_without_the_service_is_not_installed_and_nothing_is_registered() {
        let home = without_host();
        let login = Arc::new(FakeLogin::new(&[LoginState::NotFound]));
        let (code, message) = setup(office_with(&home, login.clone(), 50).ensure_running());
        assert_eq!(code, SetupErrorCode::NotInstalled);
        assert!(message.contains("Reinstall"), "{message}");
        assert_eq!(login.register_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn macos_refusing_to_register_is_not_installed_with_its_reason() {
        let home = without_host();
        let mut login = FakeLogin::new(&[LoginState::NotRegistered]);
        login.register_result = Err("Disk is full".into());
        let (code, message) = setup(office_with(&home, Arc::new(login), 50).ensure_running());
        assert_eq!(code, SetupErrorCode::NotInstalled);
        assert!(message.contains("Disk is full"), "{message}");
    }

    #[test]
    fn a_service_that_is_on_but_whose_host_never_answers_is_unreachable_after_the_wait() {
        let home = without_host();
        let login = Arc::new(FakeLogin::new(&[LoginState::Enabled]));
        let started = Instant::now();
        let (code, _) = setup(office_with(&home, login.clone(), 400).ensure_running());
        assert_eq!(code, SetupErrorCode::Unreachable);
        assert!(started.elapsed() >= Duration::from_millis(400));
        assert_eq!(
            login.register_calls.load(Ordering::SeqCst),
            0,
            "already on: nothing to register"
        );
    }

    #[test]
    fn a_ping_never_turns_anything_on() {
        let home = without_host();
        let login = Arc::new(FakeLogin::new(&[LoginState::NotRegistered]));
        let err = office_with(&home, login.clone(), 50).ping().unwrap_err();
        assert_eq!(err.code, SetupErrorCode::Unreachable);
        assert_eq!(login.register_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_service_the_owner_turned_off_is_not_turned_on_again_by_the_app() {
        let home = without_host();
        let mut login = FakeLogin::new(&[LoginState::NotRegistered]);
        login.off = true;
        let login = Arc::new(login);
        let (code, message) = setup(office_with(&home, login.clone(), 50).ensure_running());
        assert_eq!(code, SetupErrorCode::TurnedOff);
        assert!(message.contains("Turn it on"), "{message}");
        assert_eq!(login.register_calls.load(Ordering::SeqCst), 0);
    }
}
