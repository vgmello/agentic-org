//! A5 + B2.4 + FC-146 real-process proofs through the real `org-host`, `agentic-org-web`,
//! and `agentic-org-office` binaries:
//!
//! 1. Browser-approved terminal sign-in (`POST /api/v1/cli-approvals/decide` +
//!    `cli.approval.create`/`cli.approval.poll`, no password prompt, connection-bound `SES-8`
//!    session revoked on close) driving every B2.4/B3 organization & project CLI row via
//!    `org-agent-cli`, including `CLI-4` (`--dry-run`), `CLI-5` (`--idempotency-key` replay and
//!    `operation_reused`), and `CLI-6` (`--if-match` conflict + `retry_with` retry).
//! 2. FC-146 runtime-directory deletion (`runtime/<office-id>/`) rebuilds an equal
//!    `org.snapshot` and replays the same `command_id` to its original `Committed` reply through
//!    the real host.
//! 3. FC-146 journal rollback (older segment copy behind host checkpoint) and journal corruption
//!    (flipped byte at or below confirmed head) fail closed through the real host on restart.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use org_agent_cli::dispatch::{hint_for, OfficeTransport, TransportOffice};
use org_command::Refusal;
use org_host_control::{
    ops::{SessionGrant, SessionGrantParams},
    SessionHandle,
};
use org_proto::{ErrorCode, OfficeStatus};
use serde_json::{json, Value};

use super::{
    browser_sessions::wait_serving,
    launch,
    organizations::{all_files, checked, restart, stop, wait_offices_active, Browser},
    wait_until, Launched, Seed,
};
use crate::{
    setup_flow::{
        cli_session::Target,
        sign_in_command::{approve_on_target, open_approved_office, ApprovedOfficeChannel},
    },
    Host,
};

struct ChannelWriter(mpsc::Sender<String>);

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = self.0.send(String::from_utf8_lossy(buf).into_owned());
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct ApprovedChannelOffice<'a>(&'a mut ApprovedOfficeChannel);

impl OfficeTransport for ApprovedChannelOffice<'_> {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.0
            .call_method(method, params)
            .map_err(|api| Refusal::new(api.code, api.message, hint_for(api.code)))
    }
}

fn run_cli(channel: &mut ApprovedOfficeChannel, args: &[&str]) -> (u8, Value) {
    let mut office = TransportOffice(ApprovedChannelOffice(channel));
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let res = org_agent_cli::execute_with_office("agentic-org", &owned, &mut office);
    let json: Value = serde_json::from_str(res.stdout.trim())
        .unwrap_or_else(|e| panic!("non-json CLI output `{}`: {e}", res.stdout));
    (res.exit_code, json)
}

fn extract_link_token(rx: &mpsc::Receiver<String>) -> String {
    let mut collected = String::new();
    for _ in 0..20 {
        let chunk = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("approval link printed");
        collected.push_str(&chunk);
        if let Some((_, frag)) = collected.split_once("/cli-approve#") {
            let token = frag.split_whitespace().next().unwrap_or("");
            if !token.is_empty() {
                return token.to_owned();
            }
        }
    }
    panic!("no approval link in output: {collected}");
}

fn browser_approve_and_open(
    socket: &Path,
    instance_id: &str,
    office_id: &str,
    browser: &Browser,
) -> (ApprovedOfficeChannel, String) {
    let (tx, rx) = mpsc::channel();
    let sock = socket.to_path_buf();
    let inst = instance_id.to_owned();
    let worker = thread::spawn(move || {
        let mut out = ChannelWriter(tx);
        let target = Target {
            socket: &sock,
            instance_id: &inst,
            name: None,
        };
        approve_on_target(
            &target,
            Duration::from_millis(25),
            Duration::from_secs(15),
            &mut out,
        )
    });
    let token = extract_link_token(&rx);
    let decided = browser.post(
        "/api/v1/cli-approvals/decide",
        &json!({ "link": token, "approve": true }).to_string(),
    );
    assert_eq!(decided.status, 200, "{}", decided.body);
    assert_eq!(decided.body["result"]["status"], "approved");

    let (wire, session, _account, _orgs) = worker.join().unwrap().expect("approved sign-in");
    let raw_session = session.expose().to_owned();
    let channel = open_approved_office(wire, session, office_id.to_owned()).unwrap();
    (channel, raw_session)
}

fn verify_declined_link(socket: &Path, instance_id: &str, browser: &Browser) {
    let (tx, rx) = mpsc::channel();
    let sock = socket.to_path_buf();
    let inst = instance_id.to_owned();
    let worker = thread::spawn(move || {
        let mut out = ChannelWriter(tx);
        let target = Target {
            socket: &sock,
            instance_id: &inst,
            name: None,
        };
        approve_on_target(
            &target,
            Duration::from_millis(25),
            Duration::from_secs(15),
            &mut out,
        )
    });
    let token = extract_link_token(&rx);
    let decided = browser.post(
        "/api/v1/cli-approvals/decide",
        &json!({ "link": token, "approve": false }).to_string(),
    );
    assert_eq!(decided.status, 200, "{}", decided.body);
    assert_eq!(decided.body["result"]["status"], "declined");
    // Second use of the same approval token is 404.
    let again = browser.post(
        "/api/v1/cli-approvals/decide",
        &json!({ "link": token, "approve": true }).to_string(),
    );
    assert_eq!(again.status, 404, "{}", again.body);
    let err = worker.join().unwrap().err().expect("declined sign-in");
    assert!(err.message.contains("declined"), "{err:?}");
    assert_eq!(err.kind, crate::setup_flow::HostCliErrorKind::Auth);
}

fn exercise_team_and_cli_contracts(channel: &mut ApprovedOfficeChannel, root_team: &str) -> String {
    // CLI-4: --dry-run validates without mutating state.
    let (code, dry) = run_cli(
        channel,
        &[
            "team",
            "create",
            "--name",
            "DryRunOnly",
            "--parent-team-id",
            root_team,
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "{dry}");
    assert_eq!(dry["ok"], true);
    let (_, listed0) = run_cli(channel, &["team", "list"]);
    assert_eq!(listed0["data"].as_array().unwrap().len(), 1, "{listed0}");

    // CLI-5: --idempotency-key replays same payload and refuses different payload.
    let key = crate::ids::uuid_v7().unwrap();
    let (code, created) = run_cli(
        channel,
        &[
            "team",
            "create",
            "--name",
            "Platform",
            "--parent-team-id",
            root_team,
            "--idempotency-key",
            &key,
            "--durable",
        ],
    );
    assert_eq!(code, 0, "{created}");
    let team_id = created["data"]["id"].as_str().unwrap().to_owned();

    let (code, replay) = run_cli(
        channel,
        &[
            "team",
            "create",
            "--name",
            "Platform",
            "--parent-team-id",
            root_team,
            "--idempotency-key",
            &key,
        ],
    );
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["data"]["id"], team_id);

    let (code, reused) = run_cli(
        channel,
        &[
            "team",
            "create",
            "--name",
            "DifferentName",
            "--parent-team-id",
            root_team,
            "--idempotency-key",
            &key,
        ],
    );
    assert_eq!(code, 1, "{reused}");
    assert_eq!(reused["error"]["code"], "operation_reused");

    // CLI-6: update bumps version to 2; stale --if-match 1 returns conflict + retry_with.
    let (code, upd1) = run_cli(
        channel,
        &[
            "team",
            "update",
            "--team",
            &team_id,
            "--name",
            "Core Platform",
            "--if-match",
            "1",
        ],
    );
    assert_eq!(code, 0, "{upd1}");
    let (code, stale) = run_cli(
        channel,
        &[
            "team",
            "update",
            "--team",
            &team_id,
            "--name",
            "Stale",
            "--if-match",
            "1",
        ],
    );
    assert_eq!(code, 1, "{stale}");
    assert_eq!(stale["error"]["code"], "conflict");
    assert_eq!(stale["error"]["retry_with"]["if-match"], 2, "{stale}");

    let (code, upd2) = run_cli(
        channel,
        &[
            "team",
            "update",
            "--team",
            &team_id,
            "--name",
            "Core Platform v2",
            "--if-match",
            "2",
        ],
    );
    assert_eq!(code, 0, "{upd2}");
    let (code, insp) = run_cli(channel, &["team", "inspect", "--team", &team_id]);
    assert_eq!(code, 0, "{insp}");
    assert_eq!(insp["data"]["name"], "Core Platform v2");
    assert_eq!(insp["data"]["version"], 3);
    team_id
}

fn exercise_people_agent_project_and_archive(
    channel: &mut ApprovedOfficeChannel,
    root_team: &str,
    platform_team: &str,
) {
    let (code, org_view) = run_cli(channel, &["org", "inspect"]);
    assert_eq!(code, 0, "{org_view}");
    assert!(org_view["data"]["teams"].is_array());

    // Person lifecycle: add -> inspect -> update -> move -> deactivate.
    let (code, added) = run_cli(
        channel,
        &[
            "person",
            "add",
            "--name",
            "Ada Lovelace",
            "--handle",
            "ada",
            "--home-team-id",
            platform_team,
            "--contact",
            "ada@example.test",
        ],
    );
    assert_eq!(code, 0, "{added}");
    let person_id = added["data"]["id"].as_str().unwrap().to_owned();

    let (code, pinsp) = run_cli(channel, &["person", "inspect", "--member", "ada"]);
    assert_eq!(code, 0, "{pinsp}");
    assert_eq!(pinsp["data"]["id"], person_id);

    let (code, pupd) = run_cli(
        channel,
        &[
            "person",
            "update",
            "--member",
            &person_id,
            "--name",
            "Ada King",
            "--if-match",
            "1",
        ],
    );
    assert_eq!(code, 0, "{pupd}");

    let (code, pmove) = run_cli(
        channel,
        &[
            "person",
            "move",
            "--member",
            &person_id,
            "--to-team-id",
            root_team,
            "--if-match",
            "2",
        ],
    );
    assert_eq!(code, 0, "{pmove}");

    let (code, pdeact) = run_cli(
        channel,
        &[
            "person",
            "deactivate",
            "--member",
            &person_id,
            "--if-match",
            "3",
        ],
    );
    assert_eq!(code, 0, "{pdeact}");

    // Agent inspect (seeded HR agent).
    let (code, hr) = run_cli(channel, &["agent", "inspect", "--member", "hr"]);
    assert_eq!(code, 0, "{hr}");
    assert_eq!(hr["data"]["handle"], "hr");

    // Project create -> list -> inspect.
    let (code, pcreate) = run_cli(
        channel,
        &[
            "project",
            "create",
            "--name",
            "CLI Project",
            "--owner-team-id",
            platform_team,
        ],
    );
    assert_eq!(code, 0, "{pcreate}");
    let (code, plist) = run_cli(channel, &["project", "list"]);
    assert_eq!(code, 0, "{plist}");
    assert_eq!(plist["data"].as_array().unwrap().len(), 1);
    let (code, pinsp) = run_cli(channel, &["project", "inspect", "--project", "P-1"]);
    assert_eq!(code, 0, "{pinsp}");
    assert_eq!(pinsp["data"]["slug"], "cli-project");

    // Team move + archive on a disposable empty team.
    let (code, tmp) = run_cli(
        channel,
        &[
            "team",
            "create",
            "--name",
            "Temp",
            "--parent-team-id",
            root_team,
        ],
    );
    assert_eq!(code, 0, "{tmp}");
    let tmp_id = tmp["data"]["id"].as_str().unwrap().to_owned();
    let (code, moved) = run_cli(
        channel,
        &[
            "team",
            "move",
            "--team",
            &tmp_id,
            "--parent-team-id",
            platform_team,
            "--if-match",
            "1",
        ],
    );
    assert_eq!(code, 0, "{moved}");
    let (code, archived) = run_cli(
        channel,
        &["team", "archive", "--team", &tmp_id, "--if-match", "2"],
    );
    assert_eq!(code, 0, "{archived}");
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_browser_approved_cli_sign_in_and_org_cli_rows_cli4_cli5_cli6() {
    let Launched {
        temp: _temp,
        address,
        host,
        socket,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    let (_, view) = browser.create("Example", "example");
    let (_, office_id, _) = checked(&view, "Example", "example");
    wait_offices_active(&host, 1);

    let instance_id = browser.instance.clone();
    verify_declined_link(&socket, &instance_id, &browser);

    let (mut channel, ended_session) =
        browser_approve_and_open(&socket, &instance_id, &office_id, &browser);
    let (code, teams0) = run_cli(&mut channel, &["team", "list"]);
    assert_eq!(code, 0, "{teams0}");
    let root_team = teams0["data"][0]["id"].as_str().unwrap().to_owned();

    let platform_team = exercise_team_and_cli_contracts(&mut channel, &root_team);
    exercise_people_agent_project_and_archive(&mut channel, &root_team, &platform_team);

    // Dropping the approved channel sends session.end and closes host.sock (SES-8).
    drop(channel);

    // The ended session handle is immediately rejected on a fresh host.sock connection.
    let stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    stream.set_read_timeout(Some(super::WAIT)).unwrap();
    let mut wire = io::BufReader::new(stream);
    super::answer::<org_host_control::Hello>(&mut wire, 1, &Default::default()).unwrap();
    let err = super::answer::<SessionGrant>(
        &mut wire,
        2,
        &SessionGrantParams {
            session: SessionHandle::new(ended_session).unwrap(),
            office_id,
            channel_id: "0193a5c0-0000-7000-8000-000000000001".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized);

    drop(wire);
    stop(host, &base);
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_fc146_runtime_delete_rebuilds_and_replays_command_id_through_real_host() {
    let Launched {
        temp: _temp,
        address,
        host,
        socket,
        base,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let browser = Browser::sign_in(&host, address);
    let (_, view) = browser.create("Example", "example");
    let (organization, office_id, _) = checked(&view, "Example", "example");
    wait_offices_active(&host, 1);

    let instance_id = browser.instance.clone();
    let (mut channel, _) = browser_approve_and_open(&socket, &instance_id, &office_id, &browser);
    let (_, teams0) = run_cli(&mut channel, &["team", "list"]);
    let root_team = teams0["data"][0]["id"].as_str().unwrap().to_owned();

    let key = crate::ids::uuid_v7().unwrap();
    let (code, created) = run_cli(
        &mut channel,
        &[
            "team",
            "create",
            "--name",
            "CustodyTeam",
            "--parent-team-id",
            &root_team,
            "--idempotency-key",
            &key,
            "--durable",
        ],
    );
    assert_eq!(code, 0, "{created}");
    let team_id = created["data"]["id"].as_str().unwrap().to_owned();
    let (_, snap_before) = run_cli(&mut channel, &["org", "inspect"]);
    drop(channel);
    stop(host, &base);

    // Delete the entire `runtime/<office-id>/` projection directory while stopped.
    let runtime_dir = find_office_runtime_dir(&base, &organization, &office_id);
    fs::remove_dir_all(&runtime_dir).unwrap();
    assert!(!runtime_dir.exists());

    // Restart real host: office rebuilds projection from durable files + acceptance journal.
    let (host, socket2) = super::host_at(super::fixture_config(&base), &base, |_, _, _| {});
    wait_serving(&host, address);
    wait_offices_active(&host, 1);
    assert!(runtime_dir.join("office.db").is_file());

    let (mut channel2, _) = browser_approve_and_open(&socket2, &instance_id, &office_id, &browser);
    let (_, snap_after) = run_cli(&mut channel2, &["org", "inspect"]);
    assert_eq!(snap_after["data"], snap_before["data"]);

    // Replaying the same idempotency key (command_id) returns the exact original Committed reply,
    // while reusing it with a different payload returns `operation_reused`.
    let (code, replayed) = run_cli(
        &mut channel2,
        &[
            "team",
            "create",
            "--name",
            "CustodyTeam",
            "--parent-team-id",
            &root_team,
            "--idempotency-key",
            &key,
        ],
    );
    assert_eq!(code, 0, "{replayed}");
    assert_eq!(replayed["data"], created["data"]);
    assert_eq!(replayed["data"]["id"], team_id);
    let (code, reused) = run_cli(
        &mut channel2,
        &[
            "team",
            "create",
            "--name",
            "DifferentAfterRebuild",
            "--parent-team-id",
            &root_team,
            "--idempotency-key",
            &key,
        ],
    );
    assert_eq!(code, 1, "{reused}");
    assert_eq!(reused["error"]["code"], "operation_reused");
    let (_, teams_after) = run_cli(&mut channel2, &["team", "list"]);
    assert_eq!(teams_after["data"].as_array().unwrap().len(), 2);

    drop(channel2);
    stop(host, &base);
}

fn find_office_runtime_dir(base: &Path, organization: &str, office_id: &str) -> PathBuf {
    let needle = Path::new(organization)
        .join("runtime")
        .join(office_id)
        .join("office.db");
    all_files(base)
        .into_iter()
        .find(|p| p.ends_with(&needle))
        .unwrap_or_else(|| panic!("no {needle:?} under {base:?}"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn find_journal_segment(base: &Path, office_id: &str) -> PathBuf {
    all_files(base)
        .into_iter()
        .find(|p| {
            let text = p.to_string_lossy();
            text.contains(office_id)
                && text.contains("/journals/acceptance/")
                && text.contains("/segments/")
                && p.extension().is_some_and(|e| e == "log")
        })
        .unwrap_or_else(|| panic!("no acceptance journal segment for {office_id} under {base:?}"))
}

fn assert_office_fails_closed(host: &Host, browser: &Browser, organization: &str) {
    wait_until("office fails closed on tampered journal", || {
        host.offices.offices().iter().any(|o| {
            matches!(o.status, OfficeStatus::Offline | OfficeStatus::Unavailable)
                && o.status_reason.is_some()
        })
    });
    let snap = browser.get(&format!("/api/v1/organizations/{organization}/snapshot"));
    assert_eq!(
        snap.status, 503,
        "tampered office must fail closed: {}",
        snap.body
    );
}

struct OrgFixture {
    launched: Launched,
    browser: Browser,
    organization: String,
    office_id: String,
    root_team: String,
}

fn launch_org_with_durable_team(name: &str, slug: &str) -> OrgFixture {
    let launched = launch(Seed::Administrator);
    wait_serving(&launched.host, launched.address);
    let browser = Browser::sign_in(&launched.host, launched.address);
    let (_, view) = browser.create(name, slug);
    let (organization, office_id, _) = checked(&view, name, slug);
    wait_offices_active(&launched.host, 1);

    let (mut ch, _) =
        browser_approve_and_open(&launched.socket, &browser.instance, &office_id, &browser);
    let (_, teams0) = run_cli(&mut ch, &["team", "list"]);
    let root_team = teams0["data"][0]["id"].as_str().unwrap().to_owned();
    let (code, t1) = run_cli(
        &mut ch,
        &[
            "team",
            "create",
            "--name",
            "Team1",
            "--parent-team-id",
            &root_team,
            "--durable",
        ],
    );
    assert_eq!(code, 0, "{t1}");
    drop(ch);
    OrgFixture {
        launched,
        browser,
        organization,
        office_id,
        root_team,
    }
}

fn verify_rollback_fails_closed() {
    let OrgFixture {
        launched,
        browser,
        organization,
        office_id,
        root_team,
    } = launch_org_with_durable_team("RollbackOrg", "rollback-org");
    let Launched {
        temp: _temp,
        address,
        host,
        base,
        ..
    } = launched;
    stop(host, &base);

    let seg_path = find_journal_segment(&base, &office_id);
    let older_bytes = fs::read(&seg_path).unwrap();

    let (host, socket2) = super::host_at(super::fixture_config(&base), &base, |_, _, _| {});
    wait_serving(&host, address);
    wait_offices_active(&host, 1);
    let (mut ch2, _) = browser_approve_and_open(&socket2, &browser.instance, &office_id, &browser);
    let (code, t2) = run_cli(
        &mut ch2,
        &[
            "team",
            "create",
            "--name",
            "Team2",
            "--parent-team-id",
            &root_team,
            "--durable",
        ],
    );
    assert_eq!(code, 0, "{t2}");
    drop(ch2);
    stop(host, &base);

    fs::write(&seg_path, &older_bytes).unwrap();
    let host = restart(&base, address);
    assert_office_fails_closed(&host, &browser, &organization);
    stop(host, &base);
}

fn verify_corrupt_segment_fails_closed() {
    let OrgFixture {
        launched,
        browser,
        organization,
        office_id,
        ..
    } = launch_org_with_durable_team("CorruptOrg", "corrupt-org");
    let Launched {
        temp: _temp,
        address,
        host,
        base,
        ..
    } = launched;
    stop(host, &base);

    let seg_path = find_journal_segment(&base, &office_id);
    let mut bytes = fs::read(&seg_path).unwrap();
    assert!(!bytes.is_empty(), "journal segment must not be empty");
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    fs::write(&seg_path, &bytes).unwrap();

    let host = restart(&base, address);
    assert_office_fails_closed(&host, &browser, &organization);
    stop(host, &base);
}

fn verify_closed_custody_channel_fails_closed() {
    let OrgFixture {
        launched,
        browser,
        office_id,
        root_team,
        ..
    } = launch_org_with_durable_team("CustodyCloseOrg", "custody-close-org");
    let Launched {
        temp: _temp,
        address,
        host,
        socket,
        base,
        ..
    } = launched;
    let (mut ch, _) = browser_approve_and_open(&socket, &browser.instance, &office_id, &browser);

    // Close the host side of the live writer custody socket for this office.
    assert!(
        host.close_writer_channel_for_test(&office_id),
        "live office must have an active writer custody channel"
    );

    // 1. Durable write MUST fail closed when writer custody cannot confirm the flush.
    let (code, refused1) = run_cli(
        &mut ch,
        &[
            "team",
            "create",
            "--name",
            "FirstAfterCustodyClose",
            "--parent-team-id",
            &root_team,
            "--durable",
        ],
    );
    assert_ne!(
        code, 0,
        "durable write after custody channel close must fail: {refused1}"
    );

    // 2. Once custody confirmation has failed, the live store fails closed for ALL subsequent
    //    writes (never appending them to the journal).
    let (code, refused2) = run_cli(
        &mut ch,
        &[
            "team",
            "create",
            "--name",
            "SecondAfterCustodyClose",
            "--parent-team-id",
            &root_team,
        ],
    );
    assert_ne!(
        code, 0,
        "subsequent write after custody failure must also fail closed: {refused2}"
    );
    drop(ch);
    stop(host, &base);

    // On clean restart, Team1 survives and the post-failure write was never appended.
    let (host, socket2) = super::host_at(super::fixture_config(&base), &base, |_, _, _| {});
    wait_serving(&host, address);
    wait_offices_active(&host, 1);
    let (mut ch2, _) = browser_approve_and_open(&socket2, &browser.instance, &office_id, &browser);
    let (_, teams_after) = run_cli(&mut ch2, &["team", "list"]);
    let names: Vec<&str> = teams_after["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"Team1"), "{names:?}");
    assert!(!names.contains(&"SecondAfterCustodyClose"), "{names:?}");
    drop(ch2);
    stop(host, &base);
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_fc146_journal_corruption_and_rollback_fail_closed_through_real_host() {
    verify_rollback_fails_closed();
    verify_corrupt_segment_fails_closed();
    verify_closed_custody_channel_fails_closed();
}
