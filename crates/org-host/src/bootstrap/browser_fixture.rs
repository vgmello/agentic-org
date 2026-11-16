//! Explicit disposable real-service fixture for the isolated browser lane.
//! This uses public protected bootstrap/provision/adoption calls, not seeded
//! registries. Debug image/custody admission is test evidence, not release trust.
use super::*;
use org_host_control::{ops, provision, HostOpResult, Password};
use org_proto::{
    frame,
    selected_tasks::{adoption, staging},
    Call, HelloParams,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixListener,
    },
    path::PathBuf,
    time::{Duration, Instant},
};

fn answer<C: Call>(wire: &mut BufReader<UnixStream>, id: u64, params: &C::Params) -> C::Result {
    frame::write_message(wire.get_mut(), &Request::new::<C>(id, params).unwrap()).unwrap();
    let response: Response = frame::read_message(wire).unwrap().unwrap();
    assert_eq!(response.id, id);
    response
        .into_result::<C>()
        .unwrap_or_else(|_| panic!("fixture protected call {} failed", C::METHOD))
}
fn copy_public(source: &Path, destination: &Path) {
    let m = fs::symlink_metadata(source).unwrap();
    assert!(!m.file_type().is_symlink());
    if m.is_dir() {
        fs::create_dir(destination).unwrap();
        for entry in source.read_dir().unwrap() {
            let e = entry.unwrap();
            copy_public(&e.path(), &destination.join(e.file_name()));
        }
    } else {
        assert!(m.is_file() && m.nlink() == 1);
        fs::copy(source, destination).unwrap();
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn blob(bytes: &[u8]) -> staging::Blob {
    staging::Blob {
        sha256: hash(bytes),
        byte_length: bytes.len() as u64,
    }
}
// Only synthetic fixture JSON with ASCII keys/strings and exact i64 integers.
// serde_json's sorted Map produces these codec-2 bytes; production admission
// independently checks the complete hash/header/source through the real writer.
// This is not a general codec implementation or a domain effect on the server.
fn fixture_event(org: &str, task: &str, revision: i64, change: serde_json::Value) -> Vec<u8> {
    let mut event = serde_json::json!({"schema_version":2,"resource_profile":"selected-task-resources/1","organization_id":org,"task_kind":"task","task_id":task,"revision":revision,"occurred_at":null,"actor_kind":"unknown","actor_id":null,"operation_id":null,"change":change,"source_metadata":{"fixture_origin":"synthetic_public_import"}});
    let checksum = hash(&serde_json::to_vec(&event).unwrap());
    event
        .as_object_mut()
        .unwrap()
        .insert("payload_sha256".into(), checksum.into());
    serde_json::to_vec(&event).unwrap()
}
pub(super) fn private_json(path: &Path, value: &serde_json::Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    assert!(bytes.len() <= 16_384);
    let stage = path.with_file_name(format!(".fixture-stage-{}", crate::ids::uuid_v7().unwrap()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&stage)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    fs::hard_link(&stage, path).unwrap();
    fs::remove_file(&stage).unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RevokeRequest {
    version: u32,
    request_id: String,
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}
// This file protocol exists only in this cfg(test) disposable fixture. There is
// no HTTP/native product endpoint, session shortcut or caller-selected account.
pub(super) fn revoke_requested(
    request: &Path,
    receipt: &Path,
    db: &HostDb,
    owner: &crate::accounts::identity::AccountId,
) -> bool {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(request)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return false,
        Err(_) => panic!("unsafe private fixture revoke request"),
    };
    let mut before = file.metadata().unwrap();
    let deadline = Instant::now() + Duration::from_millis(500);
    while before.nlink() == 2 {
        assert!(
            Instant::now() < deadline,
            "private fixture publication did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
        before = file.metadata().unwrap();
    }
    assert!(before.is_file() && before.uid() == crate::home::euid());
    assert_eq!(before.permissions().mode() & 0o777, 0o600);
    assert_eq!(before.nlink(), 1);
    assert!(before.len() <= 512);
    let mut bytes = Vec::new();
    (&file).take(513).read_to_end(&mut bytes).unwrap();
    let after = file.metadata().unwrap();
    assert_eq!(
        (before.dev(), before.ino(), before.len()),
        (after.dev(), after.ino(), after.len())
    );
    assert_eq!(bytes.len() as u64, before.len());
    let request: RevokeRequest =
        serde_json::from_slice(&bytes).expect("strict private revoke request");
    assert!(request.version == 1 && uuid(&request.request_id));
    assert!(matches!(fs::symlink_metadata(receipt),Err(e) if e.kind()==io::ErrorKind::NotFound));
    db.change_authority_fixture(owner, true).unwrap();
    assert!(db
        .account_authority(owner)
        .unwrap()
        .unwrap()
        .revoked_at_ms()
        .is_some());
    private_json(
        receipt,
        &serde_json::json!({"version":1,"requestId":request.request_id,"accountId":owner.as_str(),"status":"completed"}),
    );
    true
}
struct Import<'a> {
    root: &'a Path,
    socket: &'a Path,
    instance: &'a str,
    office: &'a provision::Provisioned,
    password: &'a str,
}
impl Import<'_> {
    fn send(
        &self,
        directory: bool,
        generation: i64,
        manifest: &[u8],
        sources: impl IntoIterator<Item = Vec<u8>>,
    ) {
        let stage = crate::ids::generate().unwrap();
        let folder = self.root.join(&stage);
        fs::create_dir(&folder).unwrap();
        let path = folder.join("manifest.json");
        fs::write(&path, manifest).unwrap();
        let source = folder.join("sources");
        fs::create_dir(&source).unwrap();
        for bytes in sources {
            fs::write(source.join(hash(&bytes)), bytes).unwrap();
        }
        let mut args = Vec::new();
        if directory {
            args.push("--directory".into());
        }
        for (k, v) in [
            ("--host-socket", self.socket.to_str().unwrap().to_owned()),
            ("--instance-id", self.instance.to_owned()),
            ("--org-id", self.office.organization_id.clone()),
            ("--office-id", self.office.office_id.clone()),
            ("--login", "Owner".into()),
            ("--stage-id", stage),
            ("--operation-id", crate::ids::generate().unwrap()),
            ("--directory-generation", generation.to_string()),
            ("--manifest", path.to_str().unwrap().into()),
            ("--sources", source.to_str().unwrap().into()),
        ] {
            args.push(k.into());
            args.push(v);
        }
        crate::human::adopt_with_password(&args, || {
            Password::new(self.password.to_owned()).map_err(str::to_owned)
        })
        .unwrap();
    }
}

#[test]
#[ignore = "root-only persistent actual setup/adoption/gateway fixture; explicit binaries/build/ready path required"]
fn selected_browser_fixture_uses_actual_protected_setup_and_adoption_until_explicit_stop() {
    let worker = std::env::var_os("ORG_TEST_OFFICE_BIN").expect("explicit built office");
    let web = std::env::var_os("ORG_TEST_GATEWAY_BIN").expect("explicit built web");
    let ui = std::env::var_os("ORG_TEST_UI_BUILD").expect("explicit actual static build");
    let ready = PathBuf::from(
        std::env::var_os("ORG_SELECTED_FIXTURE_READY").expect("explicit root-owned ready path"),
    );
    let parent = ready.parent().unwrap().canonicalize().unwrap();
    let metadata = fs::symlink_metadata(&parent).unwrap();
    assert!(
        parent.starts_with("/private/tmp")
            && metadata.is_dir()
            && metadata.uid() == crate::home::euid()
            && metadata.permissions().mode() & 0o777 == 0o700
    );
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["temp", "keys", "bundle"] {
        fs::create_dir(base.join(name)).unwrap();
        fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let bundle = base.join("bundle");
    fs::copy(worker, bundle.join("agentic-org-office")).unwrap();
    fs::copy(web, bundle.join("agentic-org-web")).unwrap();
    copy_public(Path::new(&ui), &bundle.join("ui"));
    let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserved.local_addr().unwrap();
    drop(reserved);
    let origin = format!("http://{address}");
    let listen=serde_json::from_value(serde_json::json!({"address":"127.0.0.1","port":address.port(),"scheme":"http_loopback","origin":origin,"hsts":"off"})).unwrap();
    let setup = base.join("setup.sock");
    let listening = UnixListener::bind(&setup).unwrap();
    fs::set_permissions(&setup, fs::Permissions::from_mode(0o600)).unwrap();
    let mut config = Config::new(base.join("unused"), bundle.join("agentic-org-office"));
    config.dev_key_dir = Some(base.join("keys"));
    config.dev_app_path = Some(std::env::current_exe().unwrap());
    let host = BootstrapHost::start(
        config,
        &base.join("locator"),
        &base.join("temp"),
        None,
        &base.join("instance"),
        listen,
        &setup,
    )
    .unwrap();
    let (stream, _) = listening.accept().unwrap();
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut wire = BufReader::new(stream);
    let (instance, password, office, request) = bootstrap_browser_owner(&mut wire, address);
    let socket = {
        let state = host.state.state.lock().unwrap();
        state.host.as_ref().unwrap().socket_path().to_owned()
    };
    let public = base
        .join("instance/durable/state")
        .join(&office.organization_id);
    let import = Import {
        root: &base,
        socket: &socket,
        instance: &instance,
        office: &office,
        password: &password,
    };
    let (team, employee) = import_browser_directory(&import, &public);
    let tail = "Large imported content tail: café canonical bytes.";
    let browsers = import_browser_tasks(&import, &public, tail);
    await_browser_gateway(&host, address);
    let descriptor = base.join("browser-fixture.json");
    let stop = base.join("stop");
    let revoke = base.join("account-revoke.request.json");
    let revoke_receipt = base.join("account-revoke.receipt.json");
    assert!(!revoke.exists() && !revoke_receipt.exists());
    let db = {
        let state = host.state.state.lock().unwrap();
        host.state.database(&state).unwrap().unwrap()
    };
    let owner =
        crate::accounts::identity::AccountId::parse(&db.bootstrap_record().unwrap().account)
            .unwrap();
    let foreign_password = browser_foreign_account(&db, &office.organization_id, &employee);
    private_json(
        &descriptor,
        &serde_json::json!({"origin":origin,"login":"Owner","password":password,"organizationId":office.organization_id,"officeId":office.office_id,"accountableTeamId":office.home_team_id,"assignedTeamId":team,"employeeId":employee,"largeBodyTail":tail,"browsers":browsers,"foreignAccount":{"login":"Foreign","password":foreign_password},"accountRevocation":{"requestPath":revoke,"receiptPath":revoke_receipt}}),
    );
    private_json(
        &ready,
        &serde_json::json!({"fixture":descriptor,"stop":stop}),
    );
    println!("SELECTED_BROWSER_FIXTURE_READY (private descriptor path in explicit ready file)");
    let deadline = Instant::now() + Duration::from_secs(3600);
    let mut revoked = false;
    while !stop.exists() {
        assert!(Instant::now() < deadline, "fixture stop deadline");
        assert!(host.live());
        if !revoked {
            revoked = revoke_requested(&revoke, &revoke_receipt, &db, &owner);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    answer::<Exit>(&mut wire, request, &Empty {});
    drop(wire);
    host.shutdown();
    drop(temp);
    assert!(!base.exists());
    println!("SELECTED_BROWSER_FIXTURE_CLEANED");
}

fn import_browser_directory(import: &Import<'_>, public: &Path) -> (String, String) {
    let office = import.office;
    let team = crate::ids::uuid_v7().unwrap();
    let employee = crate::ids::uuid_v7().unwrap();
    let mut files = BTreeMap::<String, (adoption::DirectoryKind, Vec<u8>)>::new();
    let home = format!("teams/{}", office.home_team_id);
    let hr = public
        .join(&home)
        .join("agents")
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name();
    for (path, kind) in [
        (
            "organization.yaml".into(),
            adoption::DirectoryKind::Organization,
        ),
        (
            "repositories.yaml".into(),
            adoption::DirectoryKind::Repositories,
        ),
        (format!("{home}/team.yaml"), adoption::DirectoryKind::Team),
        (
            format!("{home}/agents/{}/profile.md", hr.to_str().unwrap()),
            adoption::DirectoryKind::Employee,
        ),
        (
            format!(
                "identity/allocations/organization/{}/team/1.yaml",
                office.organization_id
            ),
            adoption::DirectoryKind::Allocation,
        ),
    ] {
        let bytes = fs::read(public.join(&path)).unwrap();
        files.insert(path, (kind, bytes));
    }
    files.insert(
        format!(
            "identity/counters/organization/{}/team.yaml",
            office.organization_id
        ),
        (
            adoption::DirectoryKind::Counter,
            format!(
                "scope_kind: organization\nscope_id: {}\nentity_kind: team\nlast_number: 2\n",
                office.organization_id
            )
            .into_bytes(),
        ),
    );
    files.insert(format!("identity/allocations/organization/{}/team/2.yaml",office.organization_id),(adoption::DirectoryKind::Allocation,format!("scope_kind: organization\nscope_id: {}\nentity_kind: team\nnumber: 2\nentity_id: {team}\nallocated_at: 2026-01-01T00:00:00Z\n",office.organization_id).into_bytes()));
    files.insert(format!("teams/{team}/team.yaml"),(adoption::DirectoryKind::Team,format!("schema_version: 1\nrevision: 1\nid: {team}\nnumber: 2\nname: Assigned\nstatus: active\nparent_team_id: null\nlead_employee_id: null\n").into_bytes()));
    files.insert(format!("{home}/people/{employee}/profile.md"),(adoption::DirectoryKind::Employee,format!("---\nschema_version: 1\nrevision: 1\nid: {employee}\nname: Cross-team person\nhandle: browser-person\nkind: person\nstatus: active\nhome_team_id: {}\nreports_to_employee_id: null\nrole_id: null\nis_owner: false\n---\nSynthetic public imported person; not the protected owner account.\n",office.home_team_id).into_bytes()));
    let manifest = adoption::DirectoryManifest {
        version: 1,
        resource_profile: "selected-task-resources/1".into(),
        source_position: adoption::DirectoryPosition { epoch: 0, seq: 0 },
        files: files
            .iter()
            .map(|(path, (kind, bytes))| adoption::DirectoryFile {
                kind: *kind,
                canonical_path: path.clone(),
                blob: blob(bytes),
            })
            .collect(),
    };
    import.send(
        true,
        1,
        &serde_json::to_vec(&manifest).unwrap(),
        files.values().map(|(_, b)| b.clone()),
    );
    assert!(public.join(format!("teams/{team}/team.yaml")).is_file());
    (team, employee)
}

fn import_browser_tasks(
    import: &Import<'_>,
    public: &Path,
    tail: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let office = import.office;
    let home = format!("teams/{}", office.home_team_id);
    let mut browsers = serde_json::Map::new();
    for (i, browser) in ["chromium", "webkit"].into_iter().enumerate() {
        let task = crate::ids::uuid_v7().unwrap();
        let number = i + 1;
        let title = format!("Assignment {browser}");
        let revision = 9_223_372_036_854_775_700_i64;
        let snapshot=format!("---\nschema_version: 1\nid: {task}\nnumber: {number}\ntype: request\nteam_id: {}\nproject_id: null\ntitle: {title}\nstatus: todo\nrevision: {revision}\nassigned_team_id: null\nassignee_id: null\ncreated_at: null\ncreated_by_kind: unknown\ncreated_by_id: null\nupdated_at: null\nupdated_by_kind: unknown\nupdated_by_id: null\nextension: 1.234567890123456789E+012\n---\n{}\n{tail}\n",office.home_team_id,"public imported body ".repeat(16_000)).into_bytes();
        let counter = format!(
            "scope_kind: team\nscope_id: {}\nentity_kind: task\nlast_number: 2\n",
            office.home_team_id
        )
        .into_bytes();
        let allocation=format!("scope_kind: team\nscope_id: {}\nentity_kind: task\nnumber: {number}\nentity_id: {task}\nallocated_at: 2026-01-01T00:00:00Z\n",office.home_team_id).into_bytes();
        let baseline_snapshot = String::from_utf8(snapshot.clone()).unwrap().replacen(
            &format!("revision: {revision}\n"),
            &format!("revision: {}\n", revision - 1),
            1,
        );
        let baseline = fixture_event(
            &office.organization_id,
            &task,
            revision - 1,
            serde_json::json!({"type":"history_baseline","known_facts":{"id":task,"team_id":office.home_team_id,"project_id":null,"revision":revision-1,"assigned_team_id":null,"assignee_id":null},"public_snapshot":{"encoding":"utf-8-markdown","text":baseline_snapshot}}),
        );
        let unknown = fixture_event(
            &office.organization_id,
            &task,
            revision,
            serde_json::json!({"type":"browser_fixture_future_change","opaque":{"retained":"future public activity payload"}}),
        );
        let manifest = adoption::Manifest {
            version: 1,
            resource_profile: "selected-task-resources/1".into(),
            task_id: task.clone(),
            canonical_path: format!("{home}/work/tasks/{task}.md"),
            public_snapshot: blob(&snapshot),
            history_origin: adoption::HistoryOrigin::Baseline {
                revision: org_proto::selected_tasks::Revision::new(revision - 1).unwrap(),
            },
            events: vec![
                adoption::Event {
                    revision: org_proto::selected_tasks::Revision::new(revision - 1).unwrap(),
                    blob: blob(&baseline),
                },
                adoption::Event {
                    revision: org_proto::selected_tasks::Revision::new(revision).unwrap(),
                    blob: blob(&unknown),
                },
            ],
            public_blobs: vec![],
            number_counter: Some(blob(&counter)),
            number_allocation: Some(blob(&allocation)),
        };
        import.send(
            false,
            2,
            &serde_json::to_vec(&manifest).unwrap(),
            [
                snapshot.clone(),
                counter,
                allocation,
                baseline.clone(),
                unknown.clone(),
            ],
        );
        assert_eq!(
            fs::read(public.join(&manifest.canonical_path)).unwrap(),
            snapshot
        );
        assert_eq!(
            fs::read(public.join(format!("work-history/tasks/{task}/{revision}.json"))).unwrap(),
            unknown
        );
        assert_eq!(
            fs::read(public.join(format!("work-history/tasks/{task}/{}.json", revision - 1)))
                .unwrap(),
            baseline
        );
        browsers.insert(
            browser.into(),
            serde_json::json!({"taskId":task,"title":title,"initialRevision":revision.to_string()}),
        );
    }
    browsers
}

fn await_browser_gateway(host: &BootstrapHost, address: std::net::SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(host.live());
        let mut stream = match TcpStream::connect(address) {
            Ok(stream) => stream,
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => panic!("actual gateway readiness failed: {:?}", e.kind()),
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        write!(
            stream,
            "GET /api/v1/version HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut first = [0; 12];
        if stream.read_exact(&mut first).is_ok() && &first == b"HTTP/1.1 200" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual gateway readiness deadline"
        );
    }
}

fn bootstrap_browser_owner(
    wire: &mut BufReader<UnixStream>,
    address: std::net::SocketAddr,
) -> (String, String, provision::Provisioned, u64) {
    let hello = answer::<Hello>(wire, 1, &HelloParams::default());
    let instance = hello.instance_id.unwrap();
    let password = format!("fixture-{}", crate::ids::generate().unwrap());
    let bootstrap = BootstrapParams {
        operation_id: crate::ids::generate().unwrap(),
        hq_operation_id: crate::ids::generate().unwrap(),
        login: "Owner".into(),
        password: Password::new(password.clone()).unwrap(),
        tls: None,
    };
    let BootstrapOutcome::Completed { result } = answer::<Bootstrap>(wire, 2, &bootstrap) else {
        panic!("fixture bootstrap refused")
    };
    let inspected = answer::<ops::SessionInspect>(
        wire,
        3,
        &ops::SessionParams {
            session: org_host_control::SessionHandle::new(result.session.expose().to_owned())
                .unwrap(),
        },
    );
    assert!(inspected.admin && inspected.orgs.is_empty()); // Security principal, no fabricated employee.
    assert!(
        matches!(TcpStream::connect(address),Err(e) if e.kind()==io::ErrorKind::ConnectionRefused),
        "no public listener before an active HQ"
    );
    let params = provision::Params {
        session: result.session,
        operation_id: bootstrap.hq_operation_id.clone(),
        plan: org_proto::selected_bootstrap::PlanV1 {
            version: 1,
            name: "Browser organization".into(),
            slug: "browser-organization".into(),
            repository_remote_url: "https://example.invalid/organization.git".into(),
            state_branch: "org/state".into(),
            home_team_name: "Accountable".into(),
        },
    };
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut request = 4;
    let office = loop {
        let response = answer::<provision::OrganizationProvision>(wire, request, &params);
        request += 1;
        match response {
            HostOpResult::Completed { result } => break result,
            HostOpResult::Accepted => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => panic!("fixture provisioning refused"),
        }
    };
    (instance, password, office, request)
}

fn browser_foreign_account(
    db: &crate::hostdb::HostDb,
    organization: &str,
    employee: &str,
) -> String {
    let foreign = crate::ids::generate().unwrap();
    let foreign_password = format!("generated-{}", crate::ids::generate().unwrap());
    db.additional_account_fixture(
        &crate::hostdb::FirstAccount::new(
            &foreign,
            "Foreign",
            crate::service::authentication::hash_password(
                &Password::new(foreign_password.clone()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    // The explicitly imported canonical person is non-owner. An actual link in
    // the protected registry grants the same org/office scope, never owner
    // authority. The first administrator was created by real bootstrap above.
    db.link_account_fixture(
        &crate::accounts::identity::AccountId::parse(&foreign).unwrap(),
        organization,
        employee,
    )
    .unwrap();
    foreign_password
}
