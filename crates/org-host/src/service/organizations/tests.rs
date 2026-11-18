//! Every test drives the module's one entry, [`dispatch`], against the real host database,
//! custody, writer authority and key store in a temporary directory. Only the office worker
//! is replaced, through the creation's own `Hq` contract.
use super::*;
use crate::broadcast::{Broadcast, Subscriber};
use crate::host_secrets::HostSecrets;
use crate::hostdb::HostDb;
use crate::provisioning::Hq;
use crate::writer::Writers;
use org_host_control::browser::{BrowserOrigin, Digest32, SessionSignInParams};
use org_host_control::organization::{
    CreateParams, ListParams, ListResult, OperationLookupParams, OrganizationView,
};
use org_host_control::{HostOpResult, Password};
use org_proto::selected_bootstrap::PlanV1;
use std::collections::BTreeMap;
use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const PASSWORD: &str = "synthetic test password";

#[derive(Clone, Copy, PartialEq)]
enum Worker {
    Creates,
    Fails,
    /// The worker created the office and the host adopted it, then the host stopped
    /// before it recorded the link.
    AdoptsThenFails,
    Blocks,
}
/// The office side of a create: records what it was asked and answers as told.
struct FakeHq {
    broadcast: Broadcast,
    worker: Mutex<Worker>,
    changed: Condvar,
    calls: AtomicUsize,
    created: Mutex<BTreeMap<String, org_proto::Organization>>,
}
impl FakeHq {
    fn new(worker: Worker) -> Arc<Self> {
        Arc::new(Self {
            broadcast: Broadcast::default(),
            worker: Mutex::new(worker),
            changed: Condvar::new(),
            calls: AtomicUsize::new(0),
            created: Mutex::default(),
        })
    }
    fn set(&self, worker: Worker) {
        *self.worker.lock().unwrap() = worker;
        self.changed.notify_all();
    }
}
impl Hq for FakeHq {
    fn provision_hq(
        &self,
        first: org_office_launch::CreateLine,
        _: org_proto::RecoveryCode,
    ) -> Result<(), ApiError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let mut worker = self.worker.lock().unwrap();
        while *worker == Worker::Blocks {
            worker = self.changed.wait(worker).unwrap();
        }
        if *worker == Worker::Fails {
            return Err(ApiError::new(ErrorCode::Unavailable, "worker failed"));
        }
        let bootstrap = first.hq_creation.unwrap().bootstrap.unwrap();
        assert_eq!(bootstrap.creator.employee_id, first.owner_member_id);
        let office = serde_json::from_value(serde_json::json!({
            "id": first.office.office_id, "organization_id": first.org_id, "name": "HQ",
            "role": "hq", "host": "This Mac", "endpoint": "/private/office.sock",
            "status": "offline",
        }))
        .unwrap();
        self.created.lock().unwrap().insert(
            first.org_id.clone(),
            org_proto::Organization {
                id: first.org_id,
                name: first.org_name,
                created_at: "2026-11-08T02:10:00.000Z".into(),
                offices: vec![office],
            },
        );
        if *worker == Worker::AdoptsThenFails {
            return Err(ApiError::new(ErrorCode::Unavailable, "host stopped"));
        }
        Ok(())
    }
    fn organization(&self, id: &str) -> Option<org_proto::Organization> {
        self.created.lock().unwrap().get(id).cloned()
    }
    fn broadcast(&self) -> &Broadcast {
        &self.broadcast
    }
}

/// What a subscribed web connection would be sent, in order.
#[derive(Default)]
struct Events(Mutex<Vec<String>>);
impl Subscriber for Events {
    fn send_event(&self, name: &str, data: Value) {
        self.0.lock().unwrap().push(format!("{name} {data}"));
    }
}

struct Host {
    _root: tempfile::TempDir,
    _state: tempfile::TempDir,
    authentication: Authentication,
    db: Arc<HostDb>,
    custody: Arc<dyn HostSecrets>,
    provisioning: Arc<Provisioning>,
    hq: Arc<FakeHq>,
    events: Arc<Events>,
    parts: Parts,
}
/// What a second host process over the same instance is built from.
struct Parts {
    keys: Arc<crate::keys::memory::MemoryKeys>,
    writers: Arc<Writers>,
    instance: String,
    root: std::path::PathBuf,
    office_home: std::path::PathBuf,
}
impl Parts {
    fn provisioning(&self, db: &Arc<HostDb>, custody: &Arc<dyn HostSecrets>) -> Arc<Provisioning> {
        Arc::new(Provisioning::new(
            Arc::clone(db),
            Arc::clone(custody),
            self.keys.clone(),
            Arc::clone(&self.writers),
            &self.instance,
            &self.root,
            &self.office_home,
        ))
    }
}
fn subscribe(hq: &FakeHq) -> Arc<Events> {
    let events = Arc::new(Events::default());
    let subscriber: Arc<dyn Subscriber> = events.clone();
    hq.broadcast.subscribe(Arc::downgrade(&subscriber), || true);
    events
}
fn host(worker: Worker) -> Host {
    let instance = crate::ids::uuid_v7().unwrap();
    let (root, authentication, _) = crate::service::authentication::tests::fixture_for(&instance);
    let (db, custody) = authentication.provisioning_storage();
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        state.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let state_root = state.path().canonicalize().unwrap();
    let office_home = state_root.join("office-state");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&office_home)
        .unwrap();
    // The supervisor's catalog makes this folder when the host starts.
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(office_home.join(org_office_launch::OFFICES_DIR_NAME))
        .unwrap();
    let parts = Parts {
        keys: Arc::new(crate::keys::memory::MemoryKeys::default()),
        writers: Writers::open(custody.clone(), db.clone(), &instance, &state_root).unwrap(),
        instance,
        root: state_root,
        office_home,
    };
    let provisioning = parts.provisioning(&db, &custody);
    let hq = FakeHq::new(worker);
    let events = subscribe(&hq);
    provisioning.start(hq.clone());
    Host {
        _root: root,
        _state: state,
        authentication,
        db,
        custody,
        provisioning,
        hq,
        events,
        parts,
    }
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
fn origin(port: u16) -> BrowserOrigin {
    BrowserOrigin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port,
    }
}
impl Host {
    /// Signs `login` in as a browser and returns its pair.
    fn sign_in(&self, login: &str, seed: u8) -> BrowserAuth {
        let params = SessionSignInParams {
            login: login.into(),
            password: Password::new(PASSWORD.into()).unwrap(),
            client_address: "127.0.0.1".parse().unwrap(),
            cookie_digest: Digest32([seed; 32]),
            proof_digest: Digest32([seed ^ 0xff; 32]),
            origin: origin(8080),
        };
        self.authentication
            .sign_in_browser(&params, now(), &mut Vec::new())
            .unwrap();
        BrowserAuth {
            cookie_digest: params.cookie_digest,
            proof_digest: params.proof_digest,
            origin: params.origin,
        }
    }
    fn seed(&self, login: &str, admin: bool) -> String {
        let id = crate::ids::generate().unwrap();
        let verifier =
            crate::service::authentication::hash_password(&Password::new(PASSWORD.into()).unwrap())
                .unwrap();
        self.db
            .seed_account(&id, login, &verifier, admin, now())
            .unwrap();
        id
    }
    fn call<C: Call>(&self, web: bool, params: &C::Params) -> Result<C::Result, ApiError> {
        let request = Request::new::<C>(7, params).unwrap();
        let response = dispatch(
            &request,
            web,
            Some(&self.authentication),
            Some(&self.provisioning),
            &mut |_, _| {},
        )
        .expect("one of this module's methods");
        assert_eq!(response.id, 7);
        response.into_result::<C>().map_err(|e| match e {
            org_proto::CallError::Api(error) => error,
            other => panic!("{other:?}"),
        })
    }
    fn create(
        &self,
        auth: &BrowserAuth,
        op: &str,
        plan: &PlanV1,
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        self.call::<Create>(
            true,
            &CreateParams {
                auth: auth.clone(),
                operation_id: op.into(),
                plan: plan.clone(),
            },
        )
    }
    fn lookup(
        &self,
        auth: &BrowserAuth,
        op: &str,
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        self.call::<OperationLookup>(
            true,
            &OperationLookupParams {
                auth: auth.clone(),
                operation_id: op.into(),
            },
        )
    }
    fn list(&self, auth: &BrowserAuth) -> ListResult {
        self.call::<List>(true, &ListParams { auth: auth.clone() })
            .unwrap()
    }
    /// Polls until the create completes.
    fn completed(&self, auth: &BrowserAuth, op: &str) -> OrganizationView {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.lookup(auth, op).unwrap() {
                HostOpResult::Completed { result } => return result,
                HostOpResult::Accepted => assert!(Instant::now() < deadline, "never completed"),
                other => panic!("{other:?}"),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    /// Waits until the worker has been asked `calls` times: the creation runs on its own
    /// thread, after the answer.
    fn asked(&self, calls: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.hq.calls.load(Ordering::Acquire) < calls {
            assert!(Instant::now() < deadline, "the worker was never asked");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn idle(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.provisioning.driving_now() != 0 {
            assert!(Instant::now() < deadline, "a creation never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn custody_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .custody
            .list()
            .unwrap()
            .iter()
            .map(|n| n.as_str().to_owned())
            .collect();
        names.sort();
        names
    }
    fn reserved(&self) -> usize {
        self.db.provisioning_reserved().unwrap().len()
    }
}
fn plan(name: &str, slug: &str) -> PlanV1 {
    PlanV1 {
        version: 1,
        name: name.into(),
        slug: slug.into(),
        repository_remote_url: "https://example.test/organization.git".into(),
        state_branch: "org/state".into(),
    }
}
fn op() -> String {
    crate::ids::generate().unwrap()
}
fn code(result: Result<impl std::fmt::Debug, ApiError>) -> ErrorCode {
    result.expect_err("a refusal").code
}

#[test]
fn another_method_is_not_this_modules_and_every_call_is_refused_off_the_web_connection() {
    let host = host(Worker::Creates);
    let auth = host.sign_in("Owner", 1);
    let other = Request::new::<org_proto::Hello>(1, &Default::default()).unwrap();
    assert!(dispatch(
        &other,
        true,
        Some(&host.authentication),
        Some(&host.provisioning),
        &mut |_, _| {}
    )
    .is_none());
    let before = host.custody_names();
    let create = CreateParams {
        auth: auth.clone(),
        operation_id: op(),
        plan: plan("Example", "example"),
    };
    assert_eq!(
        code(host.call::<Create>(false, &create)),
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        code(host.call::<List>(false, &ListParams { auth: auth.clone() })),
        ErrorCode::NotAuthorized
    );
    let lookup = OperationLookupParams {
        auth: auth.clone(),
        operation_id: create.operation_id.clone(),
    };
    assert_eq!(
        code(host.call::<OperationLookup>(false, &lookup)),
        ErrorCode::NotAuthorized
    );
    // Without a registry, and without provisioning, nothing is created either.
    let request = Request::new::<Create>(3, &create).unwrap();
    let no_registry = dispatch(
        &request,
        true,
        None,
        Some(&host.provisioning),
        &mut |_, _| {},
    );
    assert!(no_registry.unwrap().into_result::<Create>().is_err());
    let no_provisioning = dispatch(
        &request,
        true,
        Some(&host.authentication),
        None,
        &mut |_, _| {},
    );
    assert!(no_provisioning.unwrap().into_result::<Create>().is_err());
    assert_eq!(host.reserved(), 0);
    assert_eq!(host.custody_names(), before);
    assert_eq!(host.hq.calls.load(Ordering::Acquire), 0);
    assert!(host.list(&auth).creating.is_empty());
}

#[test]
fn only_a_live_administrator_session_from_its_own_origin_creates() {
    let host = host(Worker::Creates);
    let before = host.custody_names();
    host.seed("Member", false);
    let member = host.sign_in("Member", 2);
    let example = plan("Example", "example");
    assert_eq!(
        code(host.create(&member, &op(), &example)),
        ErrorCode::NotAuthorized
    );
    // A member still lists (nothing) and looks up (nothing of its own).
    assert_eq!(
        host.list(&member),
        ListResult {
            organizations: vec![],
            creating: vec![]
        }
    );
    assert_eq!(code(host.lookup(&member, &op())), ErrorCode::NotFound);

    let admin = host.sign_in("Owner", 1);
    let mut elsewhere = admin.clone();
    elsewhere.origin = origin(9090);
    assert_eq!(
        code(host.create(&elsewhere, &op(), &example)),
        ErrorCode::NotAuthorized
    );
    let mut forged = admin.clone();
    forged.proof_digest = Digest32([9; 32]);
    assert_eq!(
        code(host.create(&forged, &op(), &example)),
        ErrorCode::NotAuthorized
    );

    let id = host
        .db
        .account_credentials(&crate::accounts::identity::LoginKey::new("Owner").unwrap())
        .unwrap()
        .unwrap();
    let id = crate::accounts::identity::AccountId::parse(id.account().account_id()).unwrap();
    host.db.change_authority_fixture(&id, true).unwrap();
    let mut pushed = Vec::new();
    let request = Request::new::<Create>(
        7,
        &CreateParams {
            auth: admin.clone(),
            operation_id: op(),
            plan: example,
        },
    )
    .unwrap();
    let refused = dispatch(
        &request,
        true,
        Some(&host.authentication),
        Some(&host.provisioning),
        &mut |name, _| pushed.push(name),
    )
    .unwrap();
    assert!(refused.into_result::<Create>().is_err());
    assert_eq!(
        pushed,
        [SESSION_ENDED],
        "a revoked account's session ends, pushed before the reply"
    );
    assert_eq!(host.reserved(), 0);
    assert_eq!(host.custody_names(), before);
    assert_eq!(host.hq.calls.load(Ordering::Acquire), 0);
}

#[test]
fn an_invalid_plan_or_a_taken_name_or_slug_is_refused_before_any_custody_write() {
    let host = host(Worker::Blocks);
    let auth = host.sign_in("Owner", 1);
    let clean = host.custody_names();
    let mut local = plan("Example", "example");
    local.repository_remote_url = "file:///tmp/organization".into();
    let refused = host.create(&auth, &op(), &local).unwrap_err();
    assert_eq!(
        (
            refused.code,
            refused.field.as_deref(),
            refused.rule.as_deref()
        ),
        (
            ErrorCode::InvalidRequest,
            Some("repository_remote_url"),
            Some("remote_required")
        )
    );
    assert_eq!(host.custody_names(), clean);

    let first = op();
    assert_eq!(
        host.create(&auth, &first, &plan("Example", "example"))
            .unwrap(),
        HostOpResult::Accepted
    );
    // The creation is now waiting in the worker: what it stores is stored.
    host.asked(1);
    let staged = host.custody_names();
    assert!(staged.contains(&format!("recovery.{first}")));
    for (candidate, field) in [
        (plan("EXAMPLE", "another"), "name"),
        (plan("example", "example"), "name"),
        (plan("Another", "example"), "slug"),
    ] {
        let refused = host.create(&auth, &op(), &candidate).unwrap_err();
        assert_eq!(refused.code, ErrorCode::AlreadyExists);
        assert_eq!(refused.field.as_deref(), Some(field));
        assert_eq!(host.custody_names(), staged, "a duplicate stores no code");
    }
    assert_eq!(host.reserved(), 1);
    host.hq.set(Worker::Fails);
    host.idle();
}

#[test]
fn a_reservation_that_fails_to_commit_leaves_no_custody_entry() {
    let host = host(Worker::Creates);
    let auth = host.sign_in("Owner", 1);
    let clean = host.custody_names();
    host.provisioning.fail_next_reservation();
    let failed = op();
    assert_eq!(
        code(host.create(&auth, &failed, &plan("Example", "example"))),
        ErrorCode::Unavailable
    );
    assert_eq!(host.custody_names(), clean, "no orphan recovery code");
    assert_eq!(host.reserved(), 0);
    assert_eq!(host.hq.calls.load(Ordering::Acquire), 0);
    // The same id is then admitted as new: nothing was reserved under it.
    assert_eq!(
        host.create(&auth, &failed, &plan("Example", "example"))
            .unwrap(),
        HostOpResult::Accepted
    );
    host.completed(&auth, &failed);
}

#[test]
fn a_replay_returns_the_same_organization_and_another_plan_or_account_is_operation_reused() {
    let host = host(Worker::Creates);
    let auth = host.sign_in("Owner", 1);
    let id = op();
    let example = plan("Example", "example");
    assert_eq!(
        host.create(&auth, &id, &example).unwrap(),
        HostOpResult::Accepted
    );
    let view = host.completed(&auth, &id);
    assert_eq!(
        (view.name.as_str(), view.slug.as_str()),
        ("Example", "example")
    );
    assert!(org_proto::is_uuid_v7(&view.id) && org_proto::is_uuid_v7(&view.employee_id));
    assert_eq!(view.offices.len(), 1);
    assert_eq!(view.offices[0].role, org_proto::OfficeRole::Hq);
    assert!(org_proto::is_uuid_v7(&view.offices[0].id));
    assert_eq!(
        view.offices[0].endpoint, None,
        "no socket path leaves the host"
    );
    assert_eq!(view.recovery, org_host_control::ops::RecoveryState::Pending);
    // Completed means adopted: the office here is not active.
    assert_eq!(view.offices[0].status, org_proto::OfficeStatus::Offline);
    // A lost response: the same call from a new sign-in is the same operation.
    let again = host.sign_in("Owner", 3);
    assert_eq!(
        host.create(&again, &id, &example).unwrap(),
        HostOpResult::Completed {
            result: view.clone()
        }
    );
    assert_eq!(host.hq.calls.load(Ordering::Acquire), 1);
    assert_eq!(
        code(host.create(&auth, &id, &plan("Example", "other"))),
        ErrorCode::OperationReused
    );
    host.seed("Second", true);
    let second = host.sign_in("Second", 4);
    assert_eq!(
        code(host.create(&second, &id, &example)),
        ErrorCode::OperationReused
    );
    assert_eq!(code(host.lookup(&second, &id)), ErrorCode::NotFound);
    // The account is linked to the creator's person and no response carries the code.
    let account = host
        .authentication
        .browser_account(
            &SessionValidateParams {
                cookie_digest: auth.cookie_digest,
                proof_digest: auth.proof_digest,
                origin: auth.origin.clone(),
            },
            now(),
            &mut Vec::new(),
        )
        .unwrap();
    assert_eq!(account.links().len(), 1);
    assert_eq!(account.links()[0].org_id(), view.id);
    assert_eq!(account.links()[0].member_id(), view.employee_id);
    let reservation = host
        .db
        .provisioning_lookup(account.account_id(), &id)
        .unwrap()
        .unwrap();
    let recovery = crate::provisioning::pending_recovery(&*host.custody, &{
        let mut unlinked = reservation.clone();
        unlinked.linked = false;
        unlinked.key_fingerprint = None;
        unlinked
    })
    .unwrap();
    let wire = serde_json::to_string(&host.lookup(&auth, &id).unwrap()).unwrap();
    assert!(!wire.contains(recovery.expose()));
}

#[test]
fn polls_never_wait_for_a_creation_or_an_admission_and_start_one_creation() {
    let host = Arc::new(host(Worker::Blocks));
    let auth = host.sign_in("Owner", 1);
    let id = op();
    let example = plan("Example", "example");
    let started = Instant::now();
    assert_eq!(
        host.create(&auth, &id, &example).unwrap(),
        HostOpResult::Accepted
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "admission answers at once"
    );
    // Another create's admission is in progress: it holds the gate.
    let admitting = host.provisioning.hold_admission();
    let polls: Vec<_> = (0..8)
        .map(|n| {
            let (host, auth, id, example) =
                (host.clone(), auth.clone(), id.clone(), example.clone());
            std::thread::spawn(move || {
                let started = Instant::now();
                let answer = if n % 2 == 0 {
                    host.lookup(&auth, &id)
                } else {
                    host.create(&auth, &id, &example)
                };
                (answer.unwrap(), started.elapsed())
            })
        })
        .collect();
    for poll in polls {
        let (answer, took) = poll.join().unwrap();
        assert_eq!(answer, HostOpResult::Accepted);
        assert!(took < Duration::from_secs(1), "a poll took {took:?}");
    }
    drop(admitting);
    assert_eq!(
        host.provisioning.drives_started(),
        1,
        "one creation for nine requests"
    );
    host.asked(1);
    assert_eq!(host.hq.calls.load(Ordering::Acquire), 1);
    assert_eq!(
        host.list(&auth).creating,
        [org_host_control::organization::CreatingOrganization {
            operation_id: id.clone(),
            name: "Example".into(),
            slug: "example".into(),
        }]
    );
    assert!(host.list(&auth).organizations.is_empty());
    host.hq.set(Worker::Creates);
    let view = host.completed(&auth, &id);
    host.idle();
    assert_eq!(host.provisioning.drives_started(), 1);
    let listed = host.list(&auth);
    assert!(listed.creating.is_empty());
    assert_eq!(listed.organizations, [view]);
}

#[test]
fn a_failed_creation_keeps_its_reservation_and_the_next_poll_continues_it() {
    let host = host(Worker::Fails);
    let auth = host.sign_in("Owner", 1);
    let id = op();
    assert_eq!(
        host.create(&auth, &id, &plan("Example", "example"))
            .unwrap(),
        HostOpResult::Accepted
    );
    host.idle();
    assert_eq!(host.hq.calls.load(Ordering::Acquire), 1);
    assert_eq!(host.reserved(), 1);
    let account = host.db.provisioning_reserved().unwrap().remove(0);
    assert!(host.custody_names().contains(&format!("recovery.{id}")));
    assert!(host.events.0.lock().unwrap().is_empty());
    host.hq.set(Worker::Creates);
    assert_eq!(
        host.lookup(&auth, &id).unwrap(),
        HostOpResult::Accepted,
        "the poll starts it again"
    );
    let view = host.completed(&auth, &id);
    assert_eq!(
        view.id, account.prepared.organization_id,
        "the same reserved identity"
    );
    assert_eq!(view.employee_id, account.owner_member);
    assert_eq!(host.hq.calls.load(Ordering::Acquire), 2);
    assert_eq!(host.reserved(), 0);
}

#[test]
fn a_create_adopted_before_the_host_stopped_completes_with_the_same_identity() {
    let host = host(Worker::AdoptsThenFails);
    let auth = host.sign_in("Owner", 1);
    let id = op();
    assert_eq!(
        host.create(&auth, &id, &plan("Example", "example"))
            .unwrap(),
        HostOpResult::Accepted
    );
    host.idle();
    // Adopted, with its key, owner and writer stored, and not linked.
    let reserved = host.db.provisioning_reserved().unwrap().remove(0);
    assert!(host
        .hq
        .organization(&reserved.prepared.organization_id)
        .is_some());
    assert!(host.list(&auth).organizations.is_empty());
    assert!(host.events.0.lock().unwrap().is_empty());
    let custody = host.custody_names();
    // The host starts again over the same registry, custody, keys and offices.
    host.hq.set(Worker::Creates);
    let restarted = host.parts.provisioning(&host.db, &host.custody);
    restarted.start(host.hq.clone());
    let deadline = Instant::now() + Duration::from_secs(20);
    while host.reserved() != 0 || restarted.driving_now() != 0 {
        assert!(
            Instant::now() < deadline,
            "the adopted create never completed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let view = host.completed(&auth, &id);
    assert_eq!(view.id, reserved.prepared.organization_id);
    assert_eq!(view.employee_id, reserved.owner_member);
    assert_eq!(view.offices[0].id, reserved.prepared.office_id);
    assert_eq!(
        host.custody_names(),
        custody,
        "no second code, key or device"
    );
    assert_eq!(
        host.events.0.lock().unwrap().len(),
        1,
        "account.changed once"
    );
}

#[test]
fn account_changed_is_queued_before_any_completed_answer() {
    let host = host(Worker::Blocks);
    let auth = host.sign_in("Owner", 1);
    let id = op();
    assert_eq!(
        host.create(&auth, &id, &plan("Example", "example"))
            .unwrap(),
        HostOpResult::Accepted
    );
    let reserved = host.db.provisioning_reserved().unwrap().remove(0);
    assert!(host.events.0.lock().unwrap().is_empty());
    host.hq.set(Worker::Creates);
    let view = host.completed(&auth, &id);
    // Read at the moment the first completed answer arrived: the event is already queued.
    let events = host.events.0.lock().unwrap().clone();
    assert_eq!(
        events,
        [format!(
            "account.changed {{\"account_id\":\"{}\"}}",
            reserved.account
        )]
    );
    assert_eq!(view.id, reserved.prepared.organization_id);
}

#[test]
fn a_restarted_host_finishes_a_reserved_create_with_no_client_poll() {
    let host = host(Worker::Fails);
    let auth = host.sign_in("Owner", 1);
    let id = op();
    assert_eq!(
        host.create(&auth, &id, &plan("Example", "example"))
            .unwrap(),
        HostOpResult::Accepted
    );
    host.idle();
    assert_eq!(host.reserved(), 1);
    // The host stops mid-create. A new process opens the same registry, custody and keys.
    let restarted = host.parts.provisioning(&host.db, &host.custody);
    let hq = FakeHq::new(Worker::Creates);
    let events = subscribe(&hq);
    restarted.start(hq.clone());
    let deadline = Instant::now() + Duration::from_secs(20);
    // The link commits before its event is queued; the creation has ended after both.
    while host.reserved() != 0 || restarted.driving_now() != 0 {
        assert!(
            Instant::now() < deadline,
            "the reserved create was never resumed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(hq.calls.load(Ordering::Acquire), 1);
    assert_eq!(events.0.lock().unwrap().len(), 1, "account.changed");
    // The first request after the restart finds it linked and listed.
    let request = Request::new::<List>(1, &ListParams { auth: auth.clone() }).unwrap();
    let listed = dispatch(
        &request,
        true,
        Some(&host.authentication),
        Some(&restarted),
        &mut |_, _| {},
    )
    .unwrap()
    .into_result::<List>()
    .unwrap();
    assert!(listed.creating.is_empty());
    assert_eq!(listed.organizations.len(), 1);
    assert_eq!(listed.organizations[0].name, "Example");
    assert_eq!(listed.organizations[0].offices.len(), 1);
}

#[test]
fn an_administrator_revoked_after_admission_creates_nothing_more() {
    let host = host(Worker::Blocks);
    let auth = host.sign_in("Owner", 1);
    let id = op();
    assert_eq!(
        host.create(&auth, &id, &plan("Example", "example"))
            .unwrap(),
        HostOpResult::Accepted
    );
    host.hq.set(Worker::Fails);
    host.idle();
    let reserved = host.db.provisioning_reserved().unwrap().remove(0);
    let account = crate::accounts::identity::AccountId::parse(&reserved.account).unwrap();
    host.db.change_authority_fixture(&account, true).unwrap();
    // A restart re-drives every reserved row; this one stops at the authority check.
    let restarted = host.parts.provisioning(&host.db, &host.custody);
    let hq = FakeHq::new(Worker::Creates);
    restarted.start(hq.clone());
    let deadline = Instant::now() + Duration::from_secs(20);
    while restarted.driving_now() != 0 || restarted.drives_started() == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(hq.calls.load(Ordering::Acquire), 0);
    assert_eq!(host.reserved(), 1);
}
