//! The real office: `OfficeConnection` over `org-client`, talking to this machine's host and the HQ office's
//! socket. Phase 1 has one organization per HQ on this Mac, so every call goes to a local socket.
//!
//! Setup (installing the host, registering the login item, starting it) is not here yet: `ensure_running` only
//! connects and says `hello`, and reports a host that isn't there as unreachable. The mock office stays the default
//! until the host is bundled (`AGENTIC_ORG_OFFICE=real` picks this one).

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
    ApiError, CommandAccepted, Empty, ErrorCode, HelloResult, OfficeRole, OfficeStatus,
    OrgSnapshot, OrgSnapshotCall, Organization,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct RealOffice {
    home: PathBuf,
    host: Mutex<Option<Arc<Client>>>,
    /// Open connections to HQ offices, by office id.
    offices: Mutex<HashMap<String, Arc<Client>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a failed call means for the app's setup layer: the host can't be reached (not running, gone, or silent).
fn setup_error(e: &ClientError) -> SetupError {
    let message = match e {
        ClientError::NoOfficeHome => "The app can't find the office folder on this Mac.",
        ClientError::NotRunning(_) => "The office host isn't running.",
        ClientError::VersionMismatch { .. } => {
            "The office host needs a newer version of Agentic Org."
        }
        ClientError::Timeout => "The office host didn't answer in time.",
        _ => "The office host isn't answering.",
    };
    SetupError {
        code: SetupErrorCode::Unreachable,
        message: message.to_string(),
    }
}

/// What a failed call means for the organization calls: the office's own refusal as it gave it, and any transport
/// failure as `unknown`, which the shell reads as "the office isn't there: start it again" (see `with_restart`).
fn api_error(e: ClientError) -> ApiError {
    match e {
        ClientError::Api(api) => api,
        other => ApiError::new(ErrorCode::Unknown, setup_error(&other).message),
    }
}

/// A connection that has failed is gone: the next call opens a new one.
fn dead(e: &ClientError) -> bool {
    matches!(
        e,
        ClientError::Disconnected
            | ClientError::Io(_)
            | ClientError::NotRunning(_)
            | ClientError::Timeout
    )
}

impl RealOffice {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            host: Mutex::new(None),
            offices: Mutex::new(HashMap::new()),
        }
    }

    /// The real office in this user's office home, or `None` when there is no home to look in.
    pub fn from_env() -> Option<Self> {
        org_proto::location::default_office_home().map(Self::new)
    }

    fn host(&self) -> Result<Arc<Client>, ClientError> {
        if let Some(client) = lock(&self.host).as_ref() {
            return Ok(Arc::clone(client));
        }
        let client = Arc::new(Client::connect_host_in(&self.home)?);
        *lock(&self.host) = Some(Arc::clone(&client));
        Ok(client)
    }

    /// Run `f` on the host; if the connection had died, forget it and try once on a new one.
    fn with_host<T>(
        &self,
        f: impl Fn(&Client) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let first = self.host().and_then(|c| f(&c));
        match first {
            Err(e) if dead(&e) => {
                *lock(&self.host) = None;
                self.host().and_then(|c| f(&c))
            }
            other => other,
        }
    }

    /// The HQ office of `organization_id` on this machine, as the host lists it.
    fn hq_of(&self, organization_id: &str) -> Result<org_proto::OfficeInfo, ApiError> {
        let organizations = self
            .with_host(|c| c.list_organizations())
            .map_err(api_error)?;
        let organization = organizations
            .into_iter()
            .find(|o| o.id == organization_id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "That organization isn't here."))?;
        organization
            .offices
            .into_iter()
            .find(|o| o.role == OfficeRole::Hq)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::NotFound,
                    "That organization has no HQ office on this Mac.",
                )
            })
    }

    /// Run `f` on the HQ office's connection, opening it when needed and once more if it had died.
    fn with_hq<T>(
        &self,
        organization_id: &str,
        f: impl Fn(&Client) -> Result<T, ClientError>,
    ) -> Result<T, ApiError> {
        let office = self.hq_of(organization_id)?;
        if office.status == OfficeStatus::Unavailable {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "That office isn't available right now.",
            ));
        }
        let connect = || -> Result<Arc<Client>, ClientError> {
            if let Some(c) = lock(&self.offices).get(&office.id) {
                return Ok(Arc::clone(c));
            }
            let client = Arc::new(Client::connect_office(&office, &self.home)?);
            lock(&self.offices).insert(office.id.clone(), Arc::clone(&client));
            Ok(client)
        };
        let first = connect().and_then(|c| f(&c));
        let result = match first {
            Err(e) if dead(&e) => {
                lock(&self.offices).remove(&office.id);
                connect().and_then(|c| f(&c))
            }
            other => other,
        };
        result.map_err(api_error)
    }
}

impl OfficeConnection for RealOffice {
    fn ensure_running(&self) -> Result<HelloResult, SetupError> {
        self.with_host(|c| c.hello()).map_err(|e| setup_error(&e))
    }

    fn ping(&self) -> Result<(), SetupError> {
        self.with_host(|c| c.hello())
            .map(|_| ())
            .map_err(|e| setup_error(&e))
    }

    fn create_organization(&self, name: &str) -> Result<Organization, ApiError> {
        self.with_host(|c| c.create_organization(name))
            .map_err(api_error)
    }

    fn list_organizations(&self) -> Result<Vec<Organization>, ApiError> {
        self.with_host(|c| c.list_organizations())
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
        self.with_hq(organization_id, |c| send(c, command))
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
        let home = tempfile::Builder::new().prefix("ao").tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("run")).unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let host_requests = Arc::new(AtomicUsize::new(0));
        let organization = Organization {
            id: "org1".into(),
            name: "Acme".into(),
            created_at: "2026-11-08T10:00:00.000Z".into(),
            offices: vec![hq(status)],
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
        RealOffice::new(f.home.path().to_path_buf())
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
        let office = RealOffice::new(home.path().to_path_buf());
        let err = office.ensure_running().unwrap_err();
        assert_eq!(err.code, SetupErrorCode::Unreachable);
        assert_eq!(err.message, "The office host isn't running.");
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
        let f = fake(OfficeStatus::Active, 2);
        let office = real(&f);
        office.ping().unwrap(); // request 1
        office.ping().unwrap(); // request 2 is dropped, then retried on a new connection
        assert_eq!(f.host_requests.load(Ordering::SeqCst), 3);
    }
}
