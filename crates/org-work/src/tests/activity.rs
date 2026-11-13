//! The owner's activity filters (IA-12 decision 6): `member` and `team_members` match the
//! actor or any member field, AND-combined.

use org_proto::org::Position;
use org_proto::work::*;

use crate::WorkActivityQuery;

fn event(actor: Option<&str>, change: WorkEventKind) -> WorkEvent {
    WorkEvent {
        target_id: "item".into(),
        version: 1,
        at: "2026-11-13T00:00:00.000Z".into(),
        actor_id: actor.map(Into::into),
        auto: false,
        cause: None,
        org_position: Position::default(),
        change,
    }
}

fn query(member: Option<&str>, team: Option<&[&str]>) -> WorkActivityQuery {
    WorkActivityQuery {
        incarnation: None,
        ceiling: 0,
        before: None,
        prev_key: u64::MAX,
        member: member.map(Into::into),
        team_members: team.map(|t| t.iter().map(|m| (*m).to_owned()).collect()),
        project: None,
        limit: 1,
        scan: 1,
    }
}

#[test]
fn activity_member_filter_matches_the_actor_and_every_member_field() {
    let m = || Some("m".to_owned());
    let fields = [
        WorkEventKind::Assigned {
            from: m(),
            to: None,
        },
        WorkEventKind::Assigned {
            from: None,
            to: m(),
        },
        WorkEventKind::Triaged {
            area_id: "area".into(),
            severity: Default::default(),
            priority: Default::default(),
            assignee_id: m(),
        },
        WorkEventKind::Routed {
            from: Default::default(),
            to: Default::default(),
            scoper_from: m(),
            scoper_to: None,
            reason: None,
            routed_by: None,
        },
        WorkEventKind::Routed {
            from: Default::default(),
            to: Default::default(),
            scoper_from: None,
            scoper_to: m(),
            reason: None,
            routed_by: None,
        },
        WorkEventKind::WaitingStarted {
            on_member_id: "m".into(),
            reason: WaitReason::Triage,
        },
        WorkEventKind::WaitingEnded {
            on_member_id: "m".into(),
            reason: WaitReason::Triage,
        },
        WorkEventKind::Watched {
            member_id: "m".into(),
            watching: true,
        },
        WorkEventKind::Comment {
            comment_id: "c".into(),
            body: "m".into(),
            comment_kind: Default::default(),
            asks: m(),
            answers: None,
            watch: false,
        },
    ];
    for change in fields {
        let e = event(Some("other"), change.clone());
        assert!(query(Some("m"), None).involves(&e), "{change:?}");
        assert!(query(None, Some(&["x", "m"])).involves(&e), "{change:?}");
        assert!(!query(Some("n"), None).involves(&e), "{change:?}");
        assert!(!query(None, Some(&["x"])).involves(&e), "{change:?}");
    }
    let actor = event(Some("m"), WorkEventKind::Created);
    assert!(query(Some("m"), Some(&["m"])).involves(&actor));
    assert!(query(None, None).involves(&actor));
    // Free text and ids that aren't members never match.
    let text = event(
        None,
        WorkEventKind::Comment {
            comment_id: "m".into(),
            body: "m".into(),
            comment_kind: Default::default(),
            asks: None,
            answers: Some("m".into()),
            watch: false,
        },
    );
    assert!(!query(Some("m"), None).involves(&text));
    assert!(!query(None, Some(&["m"])).involves(&text));
}

#[test]
fn activity_member_and_team_filters_combine_with_and() {
    let assigned = |actor: &str, to: &str| {
        event(
            Some(actor),
            WorkEventKind::Assigned {
                from: None,
                to: Some(to.into()),
            },
        )
    };
    let both = query(Some("m"), Some(&["t1", "t2"]));
    assert!(both.involves(&assigned("m", "t2")));
    assert!(both.involves(&assigned("t1", "m")));
    assert!(!both.involves(&assigned("m", "x")));
    assert!(!both.involves(&assigned("t1", "x")));
    assert!(!query(None, Some(&[])).involves(&assigned("m", "x")));
}
