use super::*;
use org_office::{Office, TestOffice};
use org_proto::inbox::InboxListParams;
use org_proto::OrgSnapshot;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn snapshot() -> OrgSnapshot {
    serde_json::from_value(json!({
        "position":{"epoch":1,"seq":1},"teams":[],"members":[],
        "project_teams":[]
    }))
    .unwrap()
}

fn member(id: &str, kind: &str, office: Option<&str>) -> org_proto::Member {
    let mut member: org_proto::Member = serde_json::from_value(json!({
        "id":id,"kind":kind,"name":id,"office_id":office,
        "created_at":"2026-01-01T00:00:00.000Z","version":1
    }))
    .unwrap();
    // Keep a known Active profile even for malformed non-agent records so the kind
    // condition is independently exercised.
    member.agent = org_mailbox::testkit::fixture::members()
        .into_iter()
        .find(|member| member.id == org_mailbox::testkit::fixture::ADA_ID)
        .unwrap()
        .agent;
    member
}

fn query() -> Query<'static> {
    Query {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        after: None,
        limit: 100,
    }
}

fn source(snapshot: OrgSnapshot) -> OrgSource {
    OrgSource::new(Box::new(move || Ok(snapshot.clone())), "HQ".into())
}

fn office() -> Arc<Office> {
    Arc::new(Office::for_test(TestOffice::default()))
}

/// A `team.create` under the organization's top team, as its owner.
fn create_team(office: &Office, n: u64) {
    use org_office::rules::OrgCommand;
    use org_proto::org::TeamCreateParams;
    let snapshot = office.org_snapshot().unwrap();
    let owner = snapshot
        .members
        .iter()
        .find(|m| m.owner)
        .unwrap()
        .id
        .clone();
    let top = snapshot
        .teams
        .iter()
        .find(|t| t.owner_team_id.is_none())
        .unwrap()
        .id
        .clone();
    // `Office::for_test` uses a fixed clock at `CREATION_AT` (1_700_000_000_000 ms = 0x018bce27f800).
    let command = OrgCommand::TeamCreate(TeamCreateParams {
        command_id: format!("018bce27-f800-7000-8000-{n:012x}"),
        team_id: format!("01900000-0000-7000-8000-{n:012x}"),
        name: format!("Team {n}"),
        owner_team_id: Some(top),
    });
    office.submit_org(&owner, command).unwrap().unwrap();
}

fn open_items() -> InboxListParams {
    InboxListParams {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        cursor: None,
        limit: None,
    }
}

/// A source that cannot read is degraded; the page still answers, at the position the
/// office's directory generation gave.
#[test]
fn inbox_list_reports_a_degraded_source_and_keeps_the_office_position() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let office = office();
    let inbox = crate::office_runtime::hq_inbox(Arc::clone(&office)).unwrap();
    let before = inbox.list(open_items()).unwrap();
    assert!(before.degraded.is_empty());
    assert!(before.as_of.seq > 0);

    let failing = super::super::Hq {
        sources: vec![Arc::new(OrgSource::new(
            Box::new(|| Err(SourceFailure)),
            "HQ".into(),
        ))],
        clock: super::super::Clock::new("degraded".into()),
        list_office: Some(Arc::clone(&office)),
    };
    let page = failing.list(open_items()).unwrap();
    assert_eq!(page.degraded, ["org"]);
    assert_eq!(page.counts, InboxCounts::default());
    assert!(page.items.is_empty());
    assert!(page.next_cursor.is_none());
    assert_eq!(page.as_of.seq, before.as_of.seq);
}

/// The page's position is the organization's committed directory generation: it advances
/// with a committed `team.create`, and a replay commits nothing.
#[test]
fn inbox_list_position_advances_with_a_committed_organization_change() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let office = office();
    let inbox = crate::office_runtime::hq_inbox(Arc::clone(&office)).unwrap();
    let list = || inbox.list(open_items()).unwrap();
    let before = list();
    create_team(&office, 1);
    let after = list();
    assert_eq!(after.as_of.epoch, before.as_of.epoch);
    assert!(after.as_of.seq > before.as_of.seq);
    assert_eq!(list().as_of, after.as_of);
    // The same command again: the same answer, nothing committed.
    create_team(&office, 1);
    assert_eq!(list().as_of, after.as_of);
}

#[test]
fn d26_unplaced_mapping_uses_only_authoritative_absence_not_foreign_office_ids() {
    let mut view = snapshot();
    view.members = vec![
        member("none", "agent", None),
        member("missing", "agent", Some("OLD")),
        member("placed", "agent", Some("HQ")),
        member("person", "person", None),
        member("future", "future", None),
    ];
    let result = source(view).read(&query()).unwrap();
    assert_eq!(
        result.counts,
        InboxCounts {
            problems: 1,
            total: 1,
            ..InboxCounts::default()
        }
    );
    assert_eq!(
        result
            .items
            .iter()
            .map(|item| item.reference.as_str())
            .collect::<Vec<_>>(),
        ["HQ:org:none:unplaced"]
    );
    for item in result.items {
        assert_eq!(item.kind, InboxKind::Problem);
        assert_eq!(item.clears, InboxClears::Placed);
        assert_eq!(item.state, InboxState::Open);
        assert!(!item.dismissable);
        assert_eq!(
            (item.created_at_ms, item.handled_at_ms, item.expires_at_ms),
            (None, None, None)
        );
        assert_eq!(
            (&item.project, &item.team, &item.outcome),
            (&None, &None, &None)
        );
        assert_eq!(item.source, "org");
        assert_eq!(
            item.actions,
            vec![Action::Open {
                label: "Open".into(),
                open: Open {
                    target: OpenTarget::Member,
                    id: item.member.unwrap()
                }
            }]
        );
    }
}

#[test]
fn home_team_and_project_links_filter_items_never_open_counts() {
    let mut view = snapshot();
    let mut a = member("a", "agent", None);
    a.home_team_id = Some("T2".into());
    let mut b = member("b", "agent", None);
    b.home_team_id = Some("T3".into());
    view.members = vec![a, b];
    view.project_teams = serde_json::from_value(json!([
        {"id":"pt","project_id":"P","team_id":"T2","version":1},
        {"id":"pt2","project_id":"Q","team_id":"T3","version":1}
    ]))
    .unwrap();
    let source = source(view);
    for (team, project, expected) in [
        (Some("T2"), None, vec!["a"]),
        (Some("T3"), None, vec!["b"]),
        (Some("missing"), None, vec![]),
        (None, Some("P"), vec!["a"]),
        (None, Some("Q"), vec!["b"]),
        (None, Some("missing"), vec![]),
        (Some("T2"), Some("Q"), vec![]),
        (Some("T2"), Some("P"), vec!["a"]),
    ] {
        let result = source
            .read(&Query {
                team,
                project,
                ..query()
            })
            .unwrap();
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.member.as_deref().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(result.counts.total, 2);
    }
    for (state, kinds, limit) in [
        (InboxState::Handled, None, 100),
        (
            InboxState::Open,
            Some([InboxKind::Question].as_slice()),
            100,
        ),
        (InboxState::Open, Some([].as_slice()), 100),
        (InboxState::Open, None, 0),
    ] {
        let result = source
            .read(&Query {
                state,
                kinds,
                limit,
                ..query()
            })
            .unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.counts.problems, 2);
    }
    let result = source
        .read(&Query {
            kinds: Some(&[InboxKind::Problem]),
            ..query()
        })
        .unwrap();
    assert_eq!(result.items.len(), 2);
}

#[test]
fn exact_counts_and_keyset_before_cap_reach_all_six_hundred_conditions() {
    let mut view = snapshot();
    view.members = (0..600)
        .rev()
        .map(|n| member(&format!("m{n:03}"), "agent", None))
        .collect();
    let source = source(view);
    let capped = source
        .read(&Query {
            limit: 501,
            ..query()
        })
        .unwrap();
    assert_eq!(capped.items.len(), 500);
    assert_eq!(capped.counts.total, 600);
    let mut after = None;
    let mut references = Vec::new();
    for _ in 0..7 {
        let result = source
            .read(&Query {
                after: after.as_ref(),
                limit: 100,
                ..query()
            })
            .unwrap();
        assert_eq!(result.counts.total, 600);
        assert!(
            result.items.len() <= 100,
            "source honors the requested page cap"
        );
        if result.items.is_empty() {
            break;
        }
        after = result.items.last().map(Keyset::of);
        references.extend(result.items.into_iter().map(|item| item.reference));
    }
    assert_eq!(
        references,
        (0..600)
            .map(|n| format!("HQ:org:m{n:03}:unplaced"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn snapshot_is_read_once_and_failures_are_atomic() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let source = OrgSource::new(
        Box::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Err(SourceFailure)
        }),
        "HQ".into(),
    );
    assert!(source.read(&query()).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let source = OrgSource::new(
        Box::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            let mut view = snapshot();
            view.members.push(member("a", "agent", None));
            Ok(view)
        }),
        "HQ".into(),
    );
    let result = source.read(&query()).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.counts.total, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn offboarding_an_unplaced_agent_clears_items_and_open_totals_without_deleting_history() {
    for status in [
        org_proto::AgentStatus::Active,
        org_proto::AgentStatus::Paused,
    ] {
        let mut view = snapshot();
        let mut agent = org_mailbox::testkit::fixture::members()
            .into_iter()
            .find(|member| member.id == org_mailbox::testkit::fixture::ADA_ID)
            .unwrap();
        agent.office_id = None;
        agent.agent.as_mut().unwrap().status = status;
        view.members.push(agent);
        let before = source(view.clone()).read(&query()).unwrap();
        assert_eq!(before.items.len(), 1);
        assert_eq!(before.counts.problems, 1);
        assert_eq!(before.counts.total, 1);
        // member.offboard changes the profile and retains the record/office assignment.
        let agent = &mut view.members[0];
        agent.agent.as_mut().unwrap().status = org_proto::AgentStatus::Offboarded;
        agent.agent.as_mut().unwrap().offboarded_at = Some("2026-11-12T20:00:00.000Z".into());
        assert_eq!(agent.kind, MemberKind::Agent);
        assert!(agent.office_id.is_none());
        let source = source(view);
        let after = source.read(&query()).unwrap();
        assert!(
            after.items.is_empty(),
            "offboarded records are never unplaced Problems"
        );
        assert_eq!(after.counts, InboxCounts::default());
        let handled = source
            .read(&Query {
                state: InboxState::Handled,
                ..query()
            })
            .unwrap();
        assert!(handled.items.is_empty());
        assert_eq!(handled.counts, InboxCounts::default());
    }
}

#[test]
fn offboarded_unknown_or_absent_profiles_never_create_unplaced_problems() {
    for status in [
        Some(AgentStatus::Offboarded),
        Some(AgentStatus::Unknown),
        None,
    ] {
        let mut agent = member("a", "agent", None);
        match status {
            Some(status) => agent.agent.as_mut().unwrap().status = status,
            None => agent.agent = None,
        }
        let mut view = snapshot();
        view.members.push(agent);
        let source = source(view);
        for state in [InboxState::Open, InboxState::Handled] {
            let result = source.read(&Query { state, ..query() }).unwrap();
            assert!(
                result.items.is_empty(),
                "uncertain or offboarded profile must not nag: {status:?}"
            );
            assert_eq!(
                result.counts,
                InboxCounts::default(),
                "excluded profile must not contribute to open totals: {status:?}"
            );
        }
    }
}

#[test]
fn late_matching_team_and_project_members_remain_reachable_before_the_page_cap() {
    let mut view = snapshot();
    view.members = (0..102)
        .map(|n| {
            let mut m = member(&format!("m{n:03}"), "agent", None);
            if n == 101 {
                m.home_team_id = Some("T".into());
            }
            m
        })
        .collect();
    view.project_teams = serde_json::from_value(json!([
        {"id":"project","project_id":"P","team_id":"T","version":1}
    ]))
    .unwrap();
    let source = source(view);
    for (team, project) in [(Some("T"), None), (None, Some("P"))] {
        let result = source
            .read(&Query {
                team,
                project,
                limit: 3,
                ..query()
            })
            .unwrap();
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.member.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["m101"],
            "matching rows must be filtered before cap: {team:?}/{project:?}"
        );
        assert_eq!(result.counts.problems, 102);
        assert_eq!(result.counts.total, 102);
    }
}
