use org_office::CallerAuthority;

use super::*;

fn acting_for(caller: CallerAuthority) -> Delegation {
    Delegation {
        caller_id: Some("caller".into()),
        caller,
    }
}

fn owner() -> CallerAuthority {
    CallerAuthority {
        active: true,
        is_person: true,
        is_owner: true,
        may_hire: true,
    }
}

fn member(may_hire: bool) -> CallerAuthority {
    CallerAuthority {
        active: true,
        is_person: true,
        is_owner: false,
        may_hire,
    }
}

#[test]
fn secrets_and_owner_reads_are_never_hers_even_for_the_owner() {
    for method in [
        "chat.read_as_owner",
        "provider.credential_status",
        "provider.enrollment_start",
        "provider.connection.create",
        "cost.subscription_usage.credential_set",
        "auth.login_request",
        "office.invitation.create",
        "office.remove",
        "member.session_approval",
        "member.agent_execution_set",
        "member.set_may_hire",
        "member.external_approve",
        "git.push",
    ] {
        assert_eq!(access(method), Access::Never, "{method}");
        assert!(!allowed(access(method), &acting_for(owner())), "{method}");
    }
}

#[test]
fn owner_only_work_follows_the_caller() {
    for method in [
        "cost.query",
        "cost.export_csv",
        "budget.list",
        "budget.set",
        "inbox.list",
        "member.agent_offboard",
        "member.session_start",
        "team.set_max_members",
        "role.create",
        "process.write",
        "an.unknown.method",
    ] {
        assert_eq!(access(method), Access::CallerOwner, "{method}");
        assert!(allowed(access(method), &acting_for(owner())), "{method}");
        assert!(
            !allowed(access(method), &acting_for(member(true))),
            "{method}"
        );
        let nobody = Delegation::default();
        assert!(
            !allowed(access(method), &nobody),
            "{method} acting for nobody"
        );
    }
}

#[test]
fn hiring_needs_a_caller_who_may_hire() {
    for method in [
        "hr.hire",
        "hr.hire.draft",
        "hr.hire.confirm",
        "hr.hire.withdraw",
    ] {
        assert_eq!(access(method), Access::CallerHires, "{method}");
        assert!(allowed(access(method), &acting_for(owner())));
        assert!(allowed(access(method), &acting_for(member(true))));
        assert!(!allowed(access(method), &acting_for(member(false))));
        assert!(!allowed(access(method), &Delegation::default()));
    }
    // Reading hires is open; the method itself scopes what she sees.
    assert_eq!(access("hr.hire.list"), Access::Open);
}

#[test]
fn reads_and_messages_are_open_while_the_caller_is_an_active_person() {
    for method in [
        "who",
        "org.snapshot",
        "tasks.list",
        "tasks.notes.get",
        "msg.send",
        "chat.messages",
        "skill.read",
        "hello",
    ] {
        assert_eq!(access(method), Access::Open, "{method}");
        assert!(
            allowed(access(method), &acting_for(member(false))),
            "{method}"
        );
        // A wake (acting for nobody) keeps her own reads.
        assert!(allowed(access(method), &Delegation::default()), "{method}");
    }
    // A caller offboarded, deactivated or unknown since the run began narrows her to nothing.
    let gone = acting_for(CallerAuthority::default());
    let inactive = acting_for(CallerAuthority {
        active: false,
        ..owner()
    });
    let agent = acting_for(CallerAuthority {
        is_person: false,
        ..owner()
    });
    for delegation in [gone, inactive, agent] {
        for method in ["who", "msg.send", "hr.hire", "cost.query"] {
            assert!(
                !allowed(access(method), &delegation),
                "{method} {delegation:?}"
            );
        }
    }
}

/// Through the real dispatch: the gate answers before any method runs, for the Chief of Staff
/// alone, and only as her delegation allows.
#[test]
fn the_gate_refuses_her_before_any_method_runs_and_leaves_everyone_else_alone() {
    use std::sync::{Arc, Mutex};

    use crate::serve::tests::{launch_key, ports, test_info};
    use crate::serve::SnapshotFailure;

    let _spawn = crate::transfer::tests::without_process_spawn();
    let dir = tempfile::TempDir::new().unwrap();
    let current = Arc::new(Mutex::new(acting_for(member(false))));
    let seen = Arc::clone(&current);
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.delegation = Some(Box::new(move |member| {
        (member == "tess").then(|| seen.lock().unwrap().clone())
    }));
    let shared = Shared::new(
        &test_info(),
        &dir.path().join("d.sock"),
        ports,
        launch_key(),
    );
    let cost = Request {
        id: 7,
        method: "cost.query".into(),
        principal: None,
        params: serde_json::json!({}),
    };
    let refused = |member: &str, via_grant: bool| {
        let response = shared.answer_for_test(&cost, member, via_grant);
        matches!(
            response.outcome,
            org_proto::Outcome::Error(ref e)
                if e.code == ErrorCode::NotAuthorized && e.message == DELEGATION_DENIED
        )
    };
    // Acting for a member who is not the owner: org-wide spend is refused by the gate.
    assert!(refused("tess", false));
    // Another agent, and a person on a grant, never meet the gate.
    assert!(!refused("ada", false));
    assert!(!refused("tess", true));
    // Acting for the owner, the gate lets the call through to the method (which then
    // answers by its own rules: here no cost port is wired at all).
    *current.lock().unwrap() = acting_for(owner());
    assert!(!refused("tess", false));
    // A secret method stays refused even for the owner.
    let secret = Request {
        id: 8,
        method: "provider.credential_status".into(),
        principal: None,
        params: serde_json::json!({}),
    };
    let response = shared.answer_for_test(&secret, "tess", false);
    assert!(matches!(
        response.outcome,
        org_proto::Outcome::Error(ref e) if e.message == DELEGATION_DENIED
    ));
}
