//! Dual-engine (`Sqlite` + `Memory`) satellite snapshot resync tests for `TASK-243`:
//! - Re-sending a full authoritative state snapshot to a satellite that already holds terminal
//!   hire requests (`hired`, `withdrawn`, `failed`) and a `subscription_cli` provider connection
//!   succeeds cleanly on both engines without tripping `terminal hire request is immutable`.
//! - Standalone `MoveAgent` satellite snapshot resync (multi-chunk paged export/apply) moves the
//!   agent's `profile.md` and `execution.md` to the new home team while keeping the untouched
//!   terminal hire request at the original team path on both engines, surviving full rebuild.
//! - Negative tests: attempting to mutate any field/state of a terminal hire request (or send
//!   malformed YAML) in a resynced snapshot still fails closed on both engines.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_pki::{
    cert::sign_satellite_key_pop,
    invitation::{sign_exporter_proof, InvitationToken},
    SigningKey, Zeroizing,
};
use org_proto::hiring::{
    AgentMove, AgentMoveParams, HireProposalWire, HrHire, HrHireConfirm, HrHireConfirmParams,
    HrHireDraft, HrHireDraftParams, HrHireParams, HrHireWithdraw, HrHireWithdrawParams,
    ProviderConnectionCreate, ProviderConnectionCreateParams,
};
use org_proto::org::TeamCreateParams;
use org_proto::{Call, Request};
use org_store::{testkit::FakeCustody, DurablePath, Files, Id, OfficeIdentity, WriterAuthority};

use org_office::export_authoritative_state_snapshot_pages;
use org_office::organization_family::records::{
    self, decode_hire_request, encode_hire_request, HireRequest, HireState, Kind as OrgKind,
};
use org_office::rules::OrgCommand;
use org_office::testkit_support::store_open::{EngineChoice, HireInspectById, HiresList};
use org_office::testkit_support::Options;
use org_office::Key;
use org_office::{classify_replicated_snapshot_path, SnapshotPathClass};
use org_office::{
    CreateInvitationParams, Custody, DataRoots, JoinRequestParams, Office, TestOffice,
};
use org_office::{NewOffice, OfficeRole};
use org_office::{ReplicatedFileBatch, SatelliteProvisionSpec};

const ENGINES: [EngineChoice; 2] = [EngineChoice::Sqlite, EngineChoice::Memory];

struct SatelliteHarness {
    hq: Office,
    owner: String,
    top_team: String,
    _sat_dir: tempfile::TempDir,
    sat_folder: PathBuf,
    sat_roots: DataRoots,
    sat_custody: Custody,
    sat_key: Key,
    sat_opts: Options,
    sat: Office,
}

impl SatelliteHarness {
    fn new(engine: EngineChoice) -> Self {
        let hq = Office::for_test_with_real_ca(TestOffice { engine });
        let owner = hq.owner_member_id().to_owned();
        let org_id = hq.info().org_id.clone();
        let top_team = hq.org_snapshot().unwrap().teams[0].id.clone();

        let created = hq
            .create_satellite_invitation(
                &owner,
                CreateInvitationParams {
                    office_name: format!("Resync Satellite {engine:?}"),
                    hq_addresses: vec!["127.0.0.1:9443".into()],
                    ttl_secs: Some(600),
                    existing_office_id: None,
                },
            )
            .unwrap()
            .unwrap();
        let tok = InvitationToken::decode(&created.token).unwrap();
        let inv_key = tok.signing_key().unwrap();
        let sat_tls = SigningKey::generate().unwrap();
        let exporter = [0x42u8; 32];
        let pop = sign_satellite_key_pop(&sat_tls, &tok.org_id, &tok.office_id, &tok.invitation_id)
            .unwrap();
        let proof = sign_exporter_proof(
            &inv_key,
            &exporter,
            &tok.invitation_id,
            &tok.office_id,
            &sat_tls.public_key(),
        )
        .unwrap();
        let accepted = hq
            .register_satellite_join_request(JoinRequestParams {
                invitation_id: tok.invitation_id.clone(),
                request_id: "req-resync".into(),
                office_id: tok.office_id.clone(),
                joining_pub_key: sat_tls.public_key(),
                pop_signature_der: pop,
                exporter_binding: exporter,
                exporter_proof_der: proof,
            })
            .unwrap()
            .unwrap();
        let check_code = match accepted {
            org_office::JoinRequestAccepted::Pending { check_code, .. } => check_code,
            other => panic!("expected pending join request, got {other:?}"),
        };
        let approved = hq
            .approve_satellite_invitation(&owner, &tok.invitation_id, &check_code)
            .unwrap()
            .unwrap();

        let sat_dir = tempfile::tempdir().unwrap();
        let sat_root = sat_dir.path().canonicalize().unwrap();
        let sat_folder = sat_root.join("home/offices").join(&tok.office_id);
        let sat_roots = DataRoots {
            durable: sat_root.join("data").join(&org_id).join("durable"),
            runtime: sat_root.join("data").join(&org_id).join("runtime"),
        };
        std::fs::create_dir_all(&sat_roots.durable).unwrap();
        std::fs::create_dir_all(&sat_roots.runtime).unwrap();
        let sat_custody = Custody {
            identity: OfficeIdentity {
                instance_id: Id::parse("0195a000-0000-7000-8000-000000000243").unwrap(),
                organization_id: Id::parse(&org_id).unwrap(),
                office_id: Id::parse(&tok.office_id).unwrap(),
                journal_incarnation: Id::parse("0195a000-0000-7000-8000-000000000244").unwrap(),
                writer_epoch: 1,
                machine_id: Id::parse("0195a000-0000-7000-8000-000000000245").unwrap(),
            },
            authority: Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
        };
        let sat_key = Key::from_bytes([0x43; 32]);
        let sat_opts = Options {
            backend: None,
            engine,
            clock: Arc::new(org_store::SystemClock),
            testing: Default::default(),
            before_final_checks: None,
        };
        let sat = Office::provision_satellite_with(
            sat_folder.clone(),
            SatelliteProvisionSpec {
                new: NewOffice {
                    office_id: tok.office_id.clone(),
                    office_name: format!("Resync Satellite {engine:?}"),
                    role: OfficeRole::Satellite,
                    org_id,
                    org_name: hq.info().org_name.clone(),
                },
                operation_id: hq.next_uuid_v7(),
                hq_office_id: hq.info().office_id.clone(),
                hq_addresses: tok.hq_addresses,
                epoch: approved.epoch,
                ca_cert_der: approved.ca_cert_der,
                office_cert_der: approved.cert_der,
                office_key_der: Zeroizing::new(sat_tls.local_der().to_vec()),
            },
            sat_key.clone(),
            sat_roots.clone(),
            sat_custody.clone(),
            sat_opts.clone(),
            true,
        )
        .unwrap();

        Self {
            hq,
            owner,
            top_team,
            _sat_dir: sat_dir,
            sat_folder,
            sat_roots,
            sat_custody,
            sat_key,
            sat_opts,
            sat,
        }
    }

    fn rebuild_satellite(self) -> Office {
        let Self {
            sat,
            sat_folder,
            sat_roots,
            sat_custody,
            sat_key,
            sat_opts,
            _sat_dir,
            ..
        } = self;
        sat.close().unwrap();
        std::fs::remove_dir_all(&sat_roots.runtime).unwrap();
        std::fs::create_dir_all(&sat_roots.runtime).unwrap();
        let instance = sat_custody.identity.instance_id.to_string();
        let mut reopened = Office::open_with(
            sat_folder,
            Some(sat_key),
            &instance,
            sat_roots,
            sat_custody,
            sat_opts,
        )
        .unwrap();
        reopened.retain_test_root(_sat_dir);
        reopened
    }
}

fn call_hiring<C: Call>(office: &Office, actor: &str, params: &C::Params) -> C::Result {
    let req = Request::new::<C>(1, params).expect("encode hiring request");
    let resp = office
        .dispatch_hiring_rpc(Some(actor), &req)
        .expect("method handled by dispatch_hiring_rpc");
    resp.into_result::<C>().expect("hiring rpc succeeded")
}

fn create_team(hq: &Office, owner: &str, name: &str, parent: &str) -> String {
    let team_id = hq.next_uuid_v7();
    hq.submit_org(
        owner,
        OrgCommand::TeamCreate(TeamCreateParams {
            command_id: hq.next_uuid_v7(),
            team_id: team_id.clone(),
            name: name.into(),
            owner_team_id: Some(parent.into()),
        }),
    )
    .unwrap()
    .unwrap();
    team_id
}

fn create_provider_and_profile(hq: &Office, owner: &str, auth_mode: &str) -> (String, String) {
    let conn_id = hq.next_uuid_v7();
    let profile_id = hq.next_uuid_v7();
    let sub = (auth_mode == "subscription_cli").then(|| format!("sub-{conn_id}"));
    call_hiring::<ProviderConnectionCreate>(
        hq,
        owner,
        &ProviderConnectionCreateParams {
            command_id: hq.next_uuid_v7(),
            connection_id: conn_id.clone(),
            default_profile_id: profile_id.clone(),
            key: format!("prov-{auth_mode}"),
            name: format!("Provider {auth_mode}"),
            provider_type: "anthropic".into(),
            auth_mode: auth_mode.into(),
            subscription_identity: sub,
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 4,
        },
    );
    (conn_id, profile_id)
}

fn proposal(name: &str, handle: &str, profile_id: &str) -> HireProposalWire {
    HireProposalWire {
        name: name.into(),
        handle: handle.into(),
        title: Some("Engineer".into()),
        focus: Some("Snapshot resync".into()),
        model_profile_id: Some(profile_id.into()),
        model_id: Some("claude-sonnet-4-6".into()),
        permission_preset: Some("workspace_edits".into()),
        tools_config: vec!["read".into(), "edit".into()],
        system_prompt_markdown: Some("Be concise and accurate.".into()),
        ..HireProposalWire::default()
    }
}

fn hire_agent(
    hq: &Office,
    owner: &str,
    team_id: &str,
    profile_id: &str,
    name: &str,
    handle: &str,
) -> (String, String) {
    let request_id = hq.next_uuid_v7();
    let employee_id = hq.next_uuid_v7();
    let out = call_hiring::<HrHire>(
        hq,
        owner,
        &HrHireParams {
            command_id: hq.next_uuid_v7(),
            request_id: request_id.clone(),
            employee_id: employee_id.clone(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: proposal(name, handle, profile_id),
            reason: format!("Hire {name} into team"),
        },
    );
    assert_eq!(out.state, "hired");
    (request_id, employee_id)
}

fn create_withdrawn_hire(
    hq: &Office,
    owner: &str,
    team_id: &str,
    profile_id: &str,
    handle: &str,
) -> String {
    let request_id = hq.next_uuid_v7();
    call_hiring::<HrHireDraft>(
        hq,
        owner,
        &HrHireDraftParams {
            command_id: hq.next_uuid_v7(),
            request_id: request_id.clone(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: proposal("Withdrawn Candidate", handle, profile_id),
            reason: "Draft to withdraw".into(),
        },
    );
    let closed = call_hiring::<HrHireWithdraw>(
        hq,
        owner,
        &HrHireWithdrawParams {
            command_id: hq.next_uuid_v7(),
            request_id: request_id.clone(),
            expected_version: 1,
            closed_reason: "Position cancelled".into(),
        },
    );
    assert_eq!(closed.state, "withdrawn");
    request_id
}

fn create_failed_hire(
    hq: &Office,
    owner: &str,
    team_id: &str,
    profile_id: &str,
    handle: &str,
) -> String {
    let request_id = hq.next_uuid_v7();
    let employee_id = hq.next_uuid_v7();
    call_hiring::<HrHireDraft>(
        hq,
        owner,
        &HrHireDraftParams {
            command_id: hq.next_uuid_v7(),
            request_id: request_id.clone(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: proposal("Failed Candidate", handle, profile_id),
            reason: "Draft to fail".into(),
        },
    );
    let stalled = call_hiring::<HrHireConfirm>(
        hq,
        owner,
        &HrHireConfirmParams {
            command_id: hq.next_uuid_v7(),
            request_id: request_id.clone(),
            employee_id,
            expected_version: 1,
            fail_at_step: Some("provision_home".into()),
        },
    );
    assert_eq!(stalled.state, "hiring");
    let failed = call_hiring::<HrHireWithdraw>(
        hq,
        owner,
        &HrHireWithdrawParams {
            command_id: hq.next_uuid_v7(),
            request_id: request_id.clone(),
            expected_version: 2,
            closed_reason: "Provisioning aborted".into(),
        },
    );
    assert_eq!(failed.state, "failed");
    request_id
}

fn team_homed_record_key(path: &str) -> Option<(&'static str, String)> {
    let dp = DurablePath::parse(path).ok()?;
    match records::kind_of(&dp)? {
        OrgKind::Team(chain) => chain.last().map(|t| ("team", t.as_str().to_owned())),
        OrgKind::Employee(_, _, id) => Some(("employee", id.as_str().to_owned())),
        OrgKind::AgentExecution(_, id) => Some(("agent_execution", id.as_str().to_owned())),
        OrgKind::HireRequest(_, id) => Some(("hire", id.as_str().to_owned())),
        _ => None,
    }
}

fn tier_rank(class: SnapshotPathClass) -> u8 {
    match class {
        SnapshotPathClass::OrgCoreStructure => 0,
        SnapshotPathClass::OrgCoreEmployee => 1,
        SnapshotPathClass::ProvidersAndHires => 2,
        SnapshotPathClass::ModelProfilesAndProjects => 3,
        SnapshotPathClass::DependentExecutionDocsAndBudgets => 4,
        SnapshotPathClass::WorkCore => 5,
        SnapshotPathClass::WorkSidecar => 6,
    }
}

fn read_exported_snapshot_pages(dir: &std::path::Path, total_pages: u32) -> Vec<(String, Vec<u8>)> {
    let mut incoming = Vec::new();
    for page_idx in 0..total_pages {
        let raw = std::fs::read(dir.join(format!("page-{page_idx:06}.json"))).unwrap();
        let encoded: Vec<(String, String)> = serde_json::from_slice(&raw).unwrap();
        for (path, b64) in encoded {
            incoming.push((path, STANDARD.decode(b64).unwrap()));
        }
    }
    incoming
}

fn pair_superseded_team_paths(
    incoming: &[(String, Vec<u8>)],
    incoming_set: &BTreeSet<String>,
    local_paths: &[String],
) -> (BTreeMap<String, Vec<String>>, BTreeSet<String>) {
    let mut incoming_by_key: BTreeMap<(&'static str, String), String> = BTreeMap::new();
    for (path, _) in incoming {
        if let Some(key) = team_homed_record_key(path) {
            incoming_by_key.insert(key, path.clone());
        }
    }
    let mut superseded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut removed_already: BTreeSet<String> = BTreeSet::new();
    for old in local_paths {
        if incoming_set.contains(old) {
            continue;
        }
        if let Some(new_path) = team_homed_record_key(old).and_then(|k| incoming_by_key.get(&k)) {
            superseded
                .entry(new_path.clone())
                .or_default()
                .push(old.clone());
            removed_already.insert(old.clone());
        }
    }
    (superseded, removed_already)
}

fn group_snapshot_batches(
    mut incoming: Vec<(String, Vec<u8>)>,
    mut superseded: BTreeMap<String, Vec<String>>,
) -> (ReplicatedFileBatch, BTreeMap<u8, ReplicatedFileBatch>) {
    incoming.sort_by(|a, b| {
        tier_rank(classify_replicated_snapshot_path(&a.0))
            .cmp(&tier_rank(classify_replicated_snapshot_path(&b.0)))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut org_core: ReplicatedFileBatch = Vec::new();
    let mut later_tiers: BTreeMap<u8, ReplicatedFileBatch> = BTreeMap::new();
    for (path, bytes) in incoming {
        let rank = tier_rank(classify_replicated_snapshot_path(&path));
        let target = if rank <= 1 {
            &mut org_core
        } else {
            later_tiers.entry(rank).or_default()
        };
        if let Some(olds) = superseded.remove(&path) {
            for old in olds {
                target.push((old, None));
            }
        }
        target.push((path, Some(bytes)));
    }
    (org_core, later_tiers)
}

/// Exports HQ's snapshot into multiple pages (`max_page_bytes`) and applies it to `sat` in
/// dependency-ordered chunks via `apply_replicated_snapshot_batch`, pairing moved team-homed
/// record removals with their new paths just as production snapshot transfer does.
fn apply_multi_chunk_snapshot(hq: &Office, sat: &Office, max_page_bytes: usize) -> u32 {
    hq.sync_store().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let exported =
        export_authoritative_state_snapshot_pages(hq, staging.path(), max_page_bytes, 1024, |_| {
            true
        })
        .unwrap();

    let incoming = read_exported_snapshot_pages(staging.path(), exported.total_pages);
    let incoming_set: BTreeSet<String> = incoming.iter().map(|(p, _)| p.clone()).collect();
    let local_paths = sat.list_local_state_paths().unwrap();
    let (superseded, removed_already) =
        pair_superseded_team_paths(&incoming, &incoming_set, &local_paths);
    let (org_core, later_tiers) = group_snapshot_batches(incoming, superseded);

    if !org_core.is_empty() {
        sat.apply_replicated_snapshot_batch(&sat.next_uuid_v7(), org_core, BTreeMap::new())
            .unwrap();
    }
    for (_, chunk) in later_tiers {
        if !chunk.is_empty() {
            sat.apply_replicated_snapshot_batch(&sat.next_uuid_v7(), chunk, BTreeMap::new())
                .unwrap();
        }
    }

    let stale_deletes: Vec<(String, Option<Vec<u8>>)> = local_paths
        .into_iter()
        .filter(|p| !incoming_set.contains(p) && !removed_already.contains(p))
        .map(|p| (p, None))
        .collect();
    if !stale_deletes.is_empty() {
        sat.apply_replicated_snapshot_batch(&sat.next_uuid_v7(), stale_deletes, BTreeMap::new())
            .unwrap();
    }
    sat.sync_store().unwrap();
    exported.total_pages
}

fn files_under_teams(office: &Office) -> BTreeMap<String, Vec<u8>> {
    let files = Files::open(office.durable_root()).unwrap();
    let prefix = DurablePath::parse("state/teams").unwrap();
    let mut out = BTreeMap::new();
    for path in files.walk(&prefix).unwrap() {
        let bytes = files.read(&path).unwrap().expect("walked file readable");
        out.insert(path.as_str().to_owned(), bytes.to_vec());
    }
    out
}

fn load_hire(office: &Office, request_id: &str) -> Option<HireRequest> {
    let id = Id::parse(request_id).unwrap();
    office.store_for_test().read(HireInspectById(id)).unwrap().0
}

fn list_hires(office: &Office) -> Vec<HireRequest> {
    office
        .store_for_test()
        .read(HiresList {
            team_id: None,
            state: None,
        })
        .unwrap()
        .0
}

#[test]
fn full_state_snapshot_resync_with_terminal_hire_requests_succeeds_on_both_engines() {
    org_office_sqlite::install();
    for engine in ENGINES {
        let fx = SatelliteHarness::new(engine);
        let (_, profile_id) = create_provider_and_profile(&fx.hq, &fx.owner, "subscription_cli");
        let (req_hired, agent_id) = hire_agent(
            &fx.hq,
            &fx.owner,
            &fx.top_team,
            &profile_id,
            "Ada Agent",
            "ada-agent",
        );
        let req_withdrawn =
            create_withdrawn_hire(&fx.hq, &fx.owner, &fx.top_team, &profile_id, "withdrawn-a");
        let req_failed =
            create_failed_hire(&fx.hq, &fx.owner, &fx.top_team, &profile_id, "failed-a");

        // 1. Initial full snapshot install onto satellite.
        let initial_files: Vec<(String, Option<Vec<u8>>)> = fx
            .hq
            .collect_authoritative_state_snapshot()
            .unwrap()
            .into_iter()
            .map(|(p, b)| (p, Some(b)))
            .collect();
        fx.sat
            .apply_replicated_state_files(&fx.sat.next_uuid_v7(), initial_files.clone())
            .unwrap_or_else(|e| panic!("{engine:?}: initial snapshot apply failed: {e}"));
        assert_eq!(list_hires(&fx.sat).len(), 3, "{engine:?}");

        // 2. Re-sending the exact same full snapshot in a single batch must succeed cleanly.
        fx.sat
            .apply_replicated_state_files(&fx.sat.next_uuid_v7(), initial_files)
            .unwrap_or_else(|e| panic!("{engine:?}: full single-batch resync failed: {e}"));

        // 3. Re-sending the full snapshot via multi-page chunked batches must also succeed cleanly.
        let pages = apply_multi_chunk_snapshot(&fx.hq, &fx.sat, 4096);
        assert!(
            pages >= 2,
            "{engine:?}: expected multi-page snapshot, got {pages}"
        );

        assert_eq!(
            load_hire(&fx.sat, &req_hired).unwrap().state,
            HireState::Hired,
            "{engine:?}"
        );
        assert_eq!(
            load_hire(&fx.sat, &req_withdrawn).unwrap().state,
            HireState::Withdrawn,
            "{engine:?}"
        );
        assert_eq!(
            load_hire(&fx.sat, &req_failed).unwrap().state,
            HireState::Failed,
            "{engine:?}"
        );
        assert_eq!(
            fx.sat
                .load_employee(&agent_id)
                .unwrap()
                .unwrap()
                .home_team_id
                .as_str(),
            fx.top_team.as_str(),
            "{engine:?}"
        );

        // 4. Delete runtime/ and rebuild projection from durable files.
        let expected_hires = list_hires(&fx.sat);
        let rebuilt = fx.rebuild_satellite();
        assert_eq!(
            list_hires(&rebuilt),
            expected_hires,
            "{engine:?}: rebuild parity"
        );
    }
}

#[test]
fn standalone_move_agent_multi_chunk_snapshot_resync_succeeds_on_both_engines() {
    org_office_sqlite::install();
    for engine in ENGINES {
        let fx = SatelliteHarness::new(engine);
        let team_b = create_team(&fx.hq, &fx.owner, "Team B", &fx.top_team);
        let team_d = create_team(&fx.hq, &fx.owner, "Team D", &fx.top_team);
        let (_, profile_id) = create_provider_and_profile(&fx.hq, &fx.owner, "api_key");

        let (req_hired, agent_id) = hire_agent(
            &fx.hq,
            &fx.owner,
            &team_b,
            &profile_id,
            "Mover Agent",
            "mover-agent",
        );
        let req_withdrawn =
            create_withdrawn_hire(&fx.hq, &fx.owner, &team_b, &profile_id, "staying-hire");

        // 1. Satellite initially holds Team B, Team D, the hired agent in Team B, and both
        //    terminal hire requests under Team B.
        let initial_pages = apply_multi_chunk_snapshot(&fx.hq, &fx.sat, 4096);
        assert!(
            initial_pages >= 2,
            "{engine:?}: expected multi-page initial snapshot"
        );
        assert_eq!(
            fx.sat
                .load_employee(&agent_id)
                .unwrap()
                .unwrap()
                .home_team_id
                .as_str(),
            team_b.as_str(),
            "{engine:?}"
        );
        let hired_before = load_hire(&fx.sat, &req_hired).unwrap();
        let withdrawn_before = load_hire(&fx.sat, &req_withdrawn).unwrap();
        assert_eq!(hired_before.state, HireState::Hired);
        assert_eq!(hired_before.home_team_id.as_str(), team_b.as_str());

        // 2. Standalone MoveAgent on HQ (no MoveTeam): moves only profile.md and execution.md from
        //    Team B to Team D; the terminal hire requests remain untouched at Team B's path.
        let current_rev = fx
            .hq
            .load_employee(&agent_id)
            .unwrap()
            .unwrap()
            .meta
            .revision;
        call_hiring::<AgentMove>(
            &fx.hq,
            &fx.owner,
            &AgentMoveParams {
                command_id: fx.hq.next_uuid_v7(),
                member_id: agent_id.clone(),
                expected_version: current_rev,
                to_team_id: team_d.clone(),
            },
        );
        assert_eq!(
            fx.hq
                .load_employee(&agent_id)
                .unwrap()
                .unwrap()
                .home_team_id
                .as_str(),
            team_d.as_str(),
            "{engine:?}"
        );

        // 3. Resync via multi-chunk snapshot to the satellite that already holds Team B's
        //    terminal hire requests.
        let resync_pages = apply_multi_chunk_snapshot(&fx.hq, &fx.sat, 4096);
        assert!(
            resync_pages >= 2,
            "{engine:?}: expected multi-page resync snapshot"
        );

        // 4. Verify new home team, moved execution.md, untouched terminal hire requests, and
        //    exact file-for-file parity under state/teams/.
        let moved_emp = fx.sat.load_employee(&agent_id).unwrap().unwrap();
        assert_eq!(
            moved_emp.home_team_id.as_str(),
            team_d.as_str(),
            "{engine:?}"
        );
        assert!(
            fx.sat.load_agent_execution(&agent_id).unwrap().is_some(),
            "{engine:?}: agent execution survives MoveAgent resync"
        );
        assert_eq!(
            load_hire(&fx.sat, &req_hired).unwrap(),
            hired_before,
            "{engine:?}: terminal hired request must survive untouched"
        );
        assert_eq!(
            load_hire(&fx.sat, &req_withdrawn).unwrap(),
            withdrawn_before,
            "{engine:?}: terminal withdrawn request must survive untouched"
        );

        let hq_team_files = files_under_teams(&fx.hq);
        let sat_team_files = files_under_teams(&fx.sat);
        assert_eq!(
            sat_team_files, hq_team_files,
            "{engine:?}: durable state/teams parity"
        );

        let top_id = Id::parse(&fx.top_team).unwrap();
        let b_id = Id::parse(&team_b).unwrap();
        let d_id = Id::parse(&team_d).unwrap();
        let ag_id = Id::parse(&agent_id).unwrap();
        let req_id = Id::parse(&req_hired).unwrap();
        let chain_b = [top_id.clone(), b_id];
        let chain_d = [top_id, d_id];
        let old_profile = records::employee_path(&chain_b, records::EmployeeKind::Agent, &ag_id)
            .unwrap()
            .as_str()
            .to_owned();
        let old_exec = records::agent_execution_path(&chain_b, &ag_id)
            .unwrap()
            .as_str()
            .to_owned();
        let new_profile = records::employee_path(&chain_d, records::EmployeeKind::Agent, &ag_id)
            .unwrap()
            .as_str()
            .to_owned();
        let new_exec = records::agent_execution_path(&chain_d, &ag_id)
            .unwrap()
            .as_str()
            .to_owned();
        let hire_path = records::hire_request_path(&chain_b, &req_id)
            .unwrap()
            .as_str()
            .to_owned();
        assert!(
            !sat_team_files.contains_key(&old_profile),
            "{engine:?}: old profile removed"
        );
        assert!(
            !sat_team_files.contains_key(&old_exec),
            "{engine:?}: old execution removed"
        );
        assert!(
            sat_team_files.contains_key(&new_profile),
            "{engine:?}: new profile present"
        );
        assert!(
            sat_team_files.contains_key(&new_exec),
            "{engine:?}: new execution present"
        );
        assert!(
            sat_team_files.contains_key(&hire_path),
            "{engine:?}: hire stays at Team B"
        );

        // 5. Full delete-runtime + rebuild parity check on the satellite.
        let rebuilt = fx.rebuild_satellite();
        assert_eq!(
            rebuilt
                .load_employee(&agent_id)
                .unwrap()
                .unwrap()
                .home_team_id
                .as_str(),
            team_d.as_str(),
            "{engine:?}: home team after rebuild"
        );
        assert!(
            rebuilt.load_agent_execution(&agent_id).unwrap().is_some(),
            "{engine:?}: execution after rebuild"
        );
        assert_eq!(
            load_hire(&rebuilt, &req_hired).unwrap(),
            hired_before,
            "{engine:?}: hired request after rebuild"
        );
        assert_eq!(
            load_hire(&rebuilt, &req_withdrawn).unwrap(),
            withdrawn_before,
            "{engine:?}: withdrawn request after rebuild"
        );
    }
}

fn tamper_hire_snapshot_entry(
    base_snapshot: &[(String, Option<Vec<u8>>)],
    target_path: &str,
    mutate: impl FnOnce(&mut HireRequest),
) -> Vec<u8> {
    let orig = base_snapshot
        .iter()
        .find_map(|(p, b)| (p == target_path).then(|| b.clone().unwrap()))
        .expect("hire request present in snapshot");
    let mut rec = decode_hire_request(&orig).unwrap();
    mutate(&mut rec);
    encode_hire_request(&rec)
}

fn build_tampered_terminal_hire_cases<'a>(
    base_snapshot: &[(String, Option<Vec<u8>>)],
    hired_path: &'a str,
    withdrawn_path: &'a str,
    failed_path: &'a str,
) -> Vec<(&'static str, &'a str, Vec<u8>)> {
    vec![
        (
            "hired.reason",
            hired_path,
            tamper_hire_snapshot_entry(base_snapshot, hired_path, |r| {
                r.reason = "Tampered reason".into();
            }),
        ),
        (
            "hired.revision",
            hired_path,
            tamper_hire_snapshot_entry(base_snapshot, hired_path, |r| r.revision += 1),
        ),
        (
            "hired.proposal.title",
            hired_path,
            tamper_hire_snapshot_entry(base_snapshot, hired_path, |r| {
                r.proposal.title = Some("Tampered Title".into());
            }),
        ),
        (
            "hired.state_to_drafting",
            hired_path,
            tamper_hire_snapshot_entry(base_snapshot, hired_path, |r| {
                r.state = HireState::Drafting;
                r.confirmed_at = None;
                r.employee_id = None;
            }),
        ),
        (
            "withdrawn.closed_reason",
            withdrawn_path,
            tamper_hire_snapshot_entry(base_snapshot, withdrawn_path, |r| {
                r.closed_reason = Some("Altered close reason".into());
            }),
        ),
        (
            "failed.closed_reason",
            failed_path,
            tamper_hire_snapshot_entry(base_snapshot, failed_path, |r| {
                r.closed_reason = Some("Altered fail reason".into());
            }),
        ),
        (
            "hired.malformed_yaml",
            hired_path,
            b"not: [valid: hire: yaml\n".to_vec(),
        ),
    ]
}

fn assert_tampered_terminal_hires_rejected(
    sat: &Office,
    engine: EngineChoice,
    base_snapshot: &[(String, Option<Vec<u8>>)],
    cases: Vec<(&'static str, &str, Vec<u8>)>,
) {
    for (label, target_path, bad_bytes) in cases {
        let tampered_batch: Vec<(String, Option<Vec<u8>>)> = base_snapshot
            .iter()
            .map(|(p, b)| {
                if p == target_path {
                    (p.clone(), Some(bad_bytes.clone()))
                } else {
                    (p.clone(), b.clone())
                }
            })
            .collect();
        let err = sat
            .apply_replicated_snapshot_batch(&sat.next_uuid_v7(), tampered_batch, BTreeMap::new())
            .expect_err(&format!(
                "{engine:?} ({label}): mutating terminal hire request must fail closed"
            ));
        if label != "hired.malformed_yaml" {
            let cause = std::error::Error::source(&err)
                .map(ToString::to_string)
                .unwrap_or_else(|| err.to_string());
            assert!(
                cause.contains("terminal hire request is immutable"),
                "{engine:?} ({label}): unexpected error cause: {cause}"
            );
        }
    }
}

#[test]
fn snapshot_resync_mutating_terminal_hire_request_fails_closed_on_both_engines() {
    org_office_sqlite::install();
    for engine in ENGINES {
        let fx = SatelliteHarness::new(engine);
        let (_, profile_id) = create_provider_and_profile(&fx.hq, &fx.owner, "api_key");
        let (req_hired, _) = hire_agent(
            &fx.hq,
            &fx.owner,
            &fx.top_team,
            &profile_id,
            "Immutable Agent",
            "immutable-agent",
        );
        let req_withdrawn = create_withdrawn_hire(
            &fx.hq,
            &fx.owner,
            &fx.top_team,
            &profile_id,
            "imm-withdrawn",
        );
        let req_failed =
            create_failed_hire(&fx.hq, &fx.owner, &fx.top_team, &profile_id, "imm-failed");

        let base_snapshot: Vec<(String, Option<Vec<u8>>)> = fx
            .hq
            .collect_authoritative_state_snapshot()
            .unwrap()
            .into_iter()
            .map(|(p, b)| (p, Some(b)))
            .collect();
        fx.sat
            .apply_replicated_state_files(&fx.sat.next_uuid_v7(), base_snapshot.clone())
            .unwrap();

        let hired_before = load_hire(&fx.sat, &req_hired).unwrap();
        let withdrawn_before = load_hire(&fx.sat, &req_withdrawn).unwrap();
        let failed_before = load_hire(&fx.sat, &req_failed).unwrap();

        let hired_path = format!("state/teams/{}/hires/{req_hired}.yaml", fx.top_team);
        let withdrawn_path = format!("state/teams/{}/hires/{req_withdrawn}.yaml", fx.top_team);
        let failed_path = format!("state/teams/{}/hires/{req_failed}.yaml", fx.top_team);
        let cases = build_tampered_terminal_hire_cases(
            &base_snapshot,
            &hired_path,
            &withdrawn_path,
            &failed_path,
        );
        assert_tampered_terminal_hires_rejected(&fx.sat, engine, &base_snapshot, cases);

        assert_eq!(
            load_hire(&fx.sat, &req_hired).unwrap(),
            hired_before,
            "{engine:?}"
        );
        assert_eq!(
            load_hire(&fx.sat, &req_withdrawn).unwrap(),
            withdrawn_before,
            "{engine:?}"
        );
        assert_eq!(
            load_hire(&fx.sat, &req_failed).unwrap(),
            failed_before,
            "{engine:?}"
        );
    }
}
