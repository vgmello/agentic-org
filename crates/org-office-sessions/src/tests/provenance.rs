//! One office-verified sender relation (`doc-78`, `doc-101` §4C, `INSTR-5`) for every turn that
//! names its sender: a direct instruction, an HR hiring request and a queued wake all badge the
//! same sender the same way, and a sender the office cannot load is `unknown`, never `person`.

use std::sync::Arc;

use org_office::organization_family::records::{BuiltinKey, Employee};
use org_office::rules::OrgCommand;
use org_office::{Office, TestOffice};
use org_proto::hiring::{AgentUpdate, AgentUpdateParams};
use org_proto::org::{MemberAddPersonParams, TeamCreateParams, TeamSetLeadParams};
use org_proto::Request;

use super::{hire_agent_with_runtime, new_uuid_v7};
use crate::credentials::now_ms;
use crate::manager::{SessionManager, SessionManagerConfig};
use crate::wake::{PendingWakeItem, WakeKind};

/// An office with an agent `target` on team Core, which a person leads, reporting to another
/// person; plus an unrelated person, a peer agent, and a member ID nobody has.
struct Senders {
    office: Arc<Office>,
    target: Employee,
    owner: String,
    manager: String,
    lead: String,
    person: String,
    peer: String,
    stranger: String,
    _tmp: tempfile::TempDir,
}

fn add_person(office: &Office, owner: &str, name: &str, team: &str) -> String {
    let id = new_uuid_v7();
    office
        .submit_org(
            owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: new_uuid_v7(),
                member_id: id.clone(),
                name: name.to_owned(),
                handle: Some(name.to_lowercase()),
                home_team_id: Some(team.to_owned()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    id
}

fn senders() -> Senders {
    let _ = org_platform::process::harden_self();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root = office.org_snapshot().unwrap().teams[0].id.clone();
    let core = new_uuid_v7();
    office
        .submit_org(
            &owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: new_uuid_v7(),
                team_id: core.clone(),
                name: "Core".into(),
                owner_team_id: Some(root.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let lead = add_person(&office, &owner, "Lena", &core);
    let manager = add_person(&office, &owner, "Mona", &root);
    let person = add_person(&office, &owner, "Pat", &root);
    let version = office
        .org_snapshot()
        .unwrap()
        .teams
        .into_iter()
        .find(|t| t.id == core)
        .unwrap()
        .version;
    office
        .submit_org(
            &owner,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: new_uuid_v7(),
                team_id: core.clone(),
                expected_version: version,
                lead_member_id: Some(lead.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let (target_id, _) = hire_agent_with_runtime(
        &office,
        &owner,
        &core,
        "anthropic",
        "claude-code",
        "ada",
        "claude-sonnet-4-6",
    );
    let (peer, _) = hire_agent_with_runtime(
        &office,
        &owner,
        &root,
        "anthropic",
        "claude-code",
        "bob",
        "claude-sonnet-4-6",
    );
    // An agent's reporting line changes through HR.
    let revision = office
        .load_employee(&target_id)
        .unwrap()
        .unwrap()
        .meta
        .revision;
    let update = Request::new::<AgentUpdate>(
        3,
        &AgentUpdateParams {
            command_id: new_uuid_v7(),
            member_id: target_id.clone(),
            expected_version: revision,
            name: None,
            role_id: None,
            clear_role_id: false,
            reports_to_member_id: Some(manager.clone()),
            clear_reports_to: false,
            title: None,
            clear_title: false,
            focus: None,
            clear_focus: false,
            role_text: None,
            clear_role_text: false,
            title_in_team: None,
            clear_title_in_team: false,
            body: None,
        },
    )
    .unwrap();
    let updated = office.dispatch_hiring_rpc(Some(&owner), &update).unwrap();
    assert!(
        matches!(updated.outcome, org_proto::Outcome::Result(_)),
        "{:?}",
        updated.outcome
    );
    let target = office.load_employee(&target_id).unwrap().unwrap();
    assert_eq!(
        target.reports_to_employee_id.as_ref().map(|id| id.as_str()),
        Some(manager.as_str())
    );
    Senders {
        office,
        target,
        owner,
        manager,
        lead,
        person,
        peer,
        stranger: new_uuid_v7(),
        _tmp: tempfile::tempdir().unwrap(),
    }
}

impl Senders {
    fn manager(&self) -> SessionManager {
        let config = SessionManagerConfig::production(self._tmp.path().join("office.sock"));
        SessionManager::new(Arc::clone(&self.office), config)
    }

    /// Every sender with the relation the office must badge it with.
    fn expected(&self) -> Vec<(&str, &'static str)> {
        vec![
            (self.owner.as_str(), "owner"),
            (self.manager.as_str(), "reports_to"),
            (self.lead.as_str(), "team_lead"),
            (self.person.as_str(), "person"),
            (self.peer.as_str(), "peer"),
            (self.stranger.as_str(), "unknown"),
        ]
    }
}

#[test]
fn direct_instructions_and_hiring_requests_carry_the_shared_verified_relation() {
    let fx = senders();
    let mgr = fx.manager();
    let mut hr = fx.target.clone();
    hr.builtin_key = Some(BuiltinKey::Hr);
    for (sender, relation) in fx.expected() {
        let badge = format!("[verified: {relation}]");
        let direct = mgr.objective_text(&fx.target, sender, "Ship it.");
        assert!(direct.starts_with("Direct instruction from "), "{direct}");
        assert!(direct.contains(&badge), "{relation}: {direct}");
        let hiring = mgr.objective_text(&hr, sender, "Hire a tester.");
        assert!(hiring.starts_with("HR hiring request from "), "{hiring}");
        assert!(hiring.contains(&badge), "{relation}: {hiring}");
        assert_eq!(
            mgr.turn_sender(&fx.target, sender).relation,
            relation,
            "{sender}"
        );
    }
    // An unknown sender keeps its raw ID as its name and has no handle: nothing vouches for it.
    let unknown = mgr.turn_sender(&fx.target, &fx.stranger);
    assert_eq!(unknown.name, fx.stranger);
    assert_eq!(unknown.handle, None);
    let lead = mgr.turn_sender(&fx.target, &fx.lead);
    assert_eq!(
        (lead.name.as_str(), lead.handle.as_deref()),
        ("Lena", Some("lena"))
    );
}

#[test]
fn a_queued_wake_badges_every_sender_as_a_direct_instruction_does() {
    let fx = senders();
    let mgr = fx.manager();
    // Hold every start so the enriched items stay in the durable queue to be read back.
    mgr.set_budget_hold(Some("hold".into()));
    for (n, (sender, relation)) in fx.expected().into_iter().enumerate() {
        let item = PendingWakeItem {
            item_id: format!("prov-{n}"),
            kind: WakeKind::DirectMessage,
            sender_id: sender.to_owned(),
            sender_name: "Claimed Name".into(),
            sender_handle: Some("claimed".into()),
            sender_relation: Some("owner".into()),
            sender_is_person: true,
            conversation_id: Some("dm-1".into()),
            source_id: format!("msg-{n}"),
            summary: "Hello".into(),
            enqueued_at_ms: now_ms(),
        };
        let _ = mgr.enqueue_wake(fx.target.id.as_str(), item);
        let queue = mgr
            .store()
            .read_pending_wakes(fx.target.id.as_str())
            .unwrap()
            .unwrap();
        let queued = queue
            .items
            .iter()
            .find(|i| i.item_id == format!("prov-{n}"))
            .unwrap();
        assert_eq!(
            queued.sender_relation.as_deref(),
            Some(relation),
            "{sender}"
        );
        assert_eq!(
            mgr.turn_sender(&fx.target, sender).relation,
            relation,
            "the wake and the direct instruction agree for {sender}"
        );
    }
    mgr.stop_all("done");
}
