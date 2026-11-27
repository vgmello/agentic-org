use org_store::{
    testkit::{id, MemoryEngine},
    CorePort, Engine, Timestamp,
};

use crate::organization::{
    ports::{OrganizationEngine, OrganizationRead, OrganizationWrite},
    records::{
        By, Employee, EmployeeKind, EmployeeStatus, HireProposal, HireRequest, HireState, Meta,
        Organization, ProviderAuthMode, ProviderConnection, Repository, RepositoryKind, Team,
        TeamStatus,
    },
};
use crate::store::sqlite::SqliteEngine;

const ORG: u64 = 1;
const REPO: u64 = 2;
const TEAM: u64 = 3;
const OWNER: u64 = 4;
const AGENT: u64 = 5;
const REQ_HIRED: u64 = 10;
const REQ_WITHDRAWN: u64 = 11;
const REQ_FAILED: u64 = 12;

fn test_meta() -> Meta {
    Meta::created(&By::Employee(id(OWNER)), Timestamp(1_700_000_000_000))
}

fn seed_org<E: OrganizationEngine>(tx: &mut E::Tx) {
    tx.put_organization(&Organization {
        id: id(ORG),
        slug: "example".into(),
        name: "Example".into(),
        organization_repository_id: id(REPO),
        state_branch: "org/state".into(),
        default_dod: None,
        meta: test_meta(),
    })
    .unwrap();
    tx.put_repositories(&[Repository {
        id: id(REPO),
        key: Some("organization".into()),
        kind: RepositoryKind::Remote,
        remote_url: Some("https://example.test/org.git".into()),
        meta: test_meta(),
    }])
    .unwrap();
    tx.put_team(&Team {
        id: id(TEAM),
        name: "Top".into(),
        number: 1,
        parent_team_id: None,
        lead_employee_id: Some(id(OWNER)),
        status: TeamStatus::Active,
        max_members: 6,
        meta: test_meta(),
    })
    .unwrap();
    tx.put_employee(&Employee {
        id: id(OWNER),
        handle: "owner".into(),
        kind: EmployeeKind::Person,
        name: "Owner".into(),
        home_team_id: id(TEAM),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: true,
        builtin_key: None,
        meta: test_meta(),
        body: String::new(),
    })
    .unwrap();
    tx.put_employee(&Employee {
        id: id(AGENT),
        handle: "builder".into(),
        kind: EmployeeKind::Agent,
        name: "Builder".into(),
        home_team_id: id(TEAM),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: false,
        builtin_key: None,
        meta: test_meta(),
        body: String::new(),
    })
    .unwrap();
}

fn base_hire(req_id: u64, state: HireState) -> HireRequest {
    let ts = "2026-11-26T00:00:00.000Z".to_owned();
    HireRequest {
        id: id(req_id),
        revision: 1,
        state,
        requested_by_employee_id: id(OWNER),
        home_team_id: id(TEAM),
        role_id: None,
        proposal: HireProposal {
            name: "Builder".into(),
            handle: "builder".into(),
            title: Some("Engineer".into()),
            ..HireProposal::default()
        },
        reason: "Initial hire".into(),
        confirmed_at: matches!(
            state,
            HireState::Hiring | HireState::Hired | HireState::Failed
        )
        .then(|| ts.clone()),
        closed_reason: matches!(state, HireState::Withdrawn | HireState::Failed)
            .then(|| "Closed".into()),
        employee_id: (state == HireState::Hired).then(|| id(AGENT)),
        failed_step: (state == HireState::Failed).then(|| "provision_home".into()),
        created_at: ts.clone(),
        updated_at: ts,
    }
}

fn verify_hire_request_immutability<E: OrganizationEngine>(engine: &E, label: &str) {
    let mut tx = engine.begin().unwrap();
    seed_org::<E>(&mut tx);

    // 1. Non-terminal transitions: Drafting -> Hiring -> Hired, Drafting -> Withdrawn,
    //    Drafting -> Hiring -> Failed all succeed cleanly.
    let mut hired = base_hire(REQ_HIRED, HireState::Drafting);
    tx.put_hire_request(&hired).unwrap();
    hired.revision = 2;
    hired.state = HireState::Hiring;
    hired.confirmed_at = Some("2026-11-26T00:00:01.000Z".into());
    tx.put_hire_request(&hired).unwrap();
    hired.revision = 3;
    hired.state = HireState::Hired;
    hired.employee_id = Some(id(AGENT));
    tx.put_hire_request(&hired).unwrap();

    let mut withdrawn = base_hire(REQ_WITHDRAWN, HireState::Drafting);
    tx.put_hire_request(&withdrawn).unwrap();
    withdrawn.revision = 2;
    withdrawn.state = HireState::Withdrawn;
    withdrawn.closed_reason = Some("No longer needed".into());
    tx.put_hire_request(&withdrawn).unwrap();

    let mut failed = base_hire(REQ_FAILED, HireState::Hiring);
    failed.failed_step = Some("provision_home".into());
    tx.put_hire_request(&failed).unwrap();
    failed.revision = 2;
    failed.state = HireState::Failed;
    failed.closed_reason = Some("Aborted".into());
    tx.put_hire_request(&failed).unwrap();

    // 2. Identical re-projection of every terminal state succeeds as a no-op.
    for req in [&hired, &withdrawn, &failed] {
        tx.put_hire_request(req)
            .unwrap_or_else(|e| panic!("{label}: identical {:?} must succeed: {e}", req.state));
        assert_eq!(tx.hire_request(&req.id).unwrap().as_ref(), Some(req));
    }

    // 3. Any genuine mutation of a terminal hire request fails closed on both engines.
    let mut changed_reason = hired.clone();
    changed_reason.reason = "Tampered reason".into();
    let mut changed_revision = hired.clone();
    changed_revision.revision += 1;
    let mut changed_proposal = hired.clone();
    changed_proposal.proposal.title = Some("Tampered title".into());
    let mut reopened = hired.clone();
    reopened.state = HireState::Drafting;
    reopened.confirmed_at = None;
    reopened.employee_id = None;
    let mut changed_withdrawn = withdrawn.clone();
    changed_withdrawn.closed_reason = Some("Rewritten reason".into());
    let mut changed_failed = failed.clone();
    changed_failed.failed_step = Some("other_step".into());

    for (case, mutated) in [
        ("reason", changed_reason),
        ("revision", changed_revision),
        ("proposal", changed_proposal),
        ("state", reopened),
        ("withdrawn.closed_reason", changed_withdrawn),
        ("failed.failed_step", changed_failed),
    ] {
        let err = tx
            .put_hire_request(&mutated)
            .expect_err(&format!("{label}: mutating {case} must fail closed"));
        assert!(
            err.message.contains("terminal hire request is immutable"),
            "{label} ({case}): unexpected error: {err}"
        );
    }

    // Stored terminal rows are completely unchanged after rejected mutations.
    assert_eq!(tx.hire_request(&hired.id).unwrap(), Some(hired));
    assert_eq!(tx.hire_request(&withdrawn.id).unwrap(), Some(withdrawn));
    assert_eq!(tx.hire_request(&failed.id).unwrap(), Some(failed));
    tx.check().unwrap();
}

#[test]
fn a_projection_opened_for_no_organization_refuses_the_family() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("office.db");
    let engine = SqliteEngine::open(&path).unwrap();
    assert!(engine.snapshot().unwrap().teams().is_err());
    assert!(engine.begin().unwrap().organization().is_err());
}

#[test]
fn terminal_hire_request_reprojection_is_idempotent_and_mutation_fails_closed_on_both_engines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("office.db");
    let sqlite = SqliteEngine::open(&path)
        .unwrap()
        .for_organization(&id(ORG));
    verify_hire_request_immutability(&sqlite, "sqlite");

    let memory = MemoryEngine::new();
    verify_hire_request_immutability(&memory, "memory");
}

fn sample_subscription_connection() -> ProviderConnection {
    ProviderConnection {
        id: id(20),
        key: "claude-sub".into(),
        name: "Claude Subscription".into(),
        provider_type: "anthropic".into(),
        auth_mode: ProviderAuthMode::SubscriptionCli,
        subscription_identity: Some("acct-1".into()),
        runtime_driver: "claude-code".into(),
        endpoint_url: None,
        enabled: true,
        archived: false,
        max_concurrent_turns: 4,
        meta: test_meta(),
    }
}

fn verify_provider_connection_update_and_immutability<E: OrganizationEngine>(
    tx: &mut E::Tx,
    conn: &ProviderConnection,
    label: &str,
) -> ProviderConnection {
    tx.put_provider_connection(conn).unwrap();
    // Identical re-projection (snapshot resync) succeeds without tripping BEFORE INSERT trigger.
    tx.put_provider_connection(conn).unwrap();

    // Legitimate mutable-field update runs UPDATE and succeeds.
    let mut updated = conn.clone();
    updated.meta.revision = 2;
    updated.max_concurrent_turns = 8;
    tx.put_provider_connection(&updated).unwrap();
    assert_eq!(
        tx.provider_connection(&conn.id).unwrap(),
        Some(updated.clone()),
        "{label}: updated provider connection mismatch"
    );

    let mut changed_sub = updated.clone();
    changed_sub.subscription_identity = Some("acct-2".into());
    let mut cleared_sub = updated.clone();
    cleared_sub.subscription_identity = None;
    let mut changed_provider = updated.clone();
    changed_provider.provider_type = "openai".into();
    for (case, mutated) in [
        ("subscription_identity", changed_sub),
        ("clear_subscription_identity", cleared_sub),
        ("provider_type", changed_provider),
    ] {
        let err = tx
            .put_provider_connection(&mutated)
            .expect_err(&format!("{label}: mutating {case} must fail closed"));
        assert!(
            err.message
                .contains("registered subscription identity is immutable"),
            "{label} ({case}): unexpected error: {err}"
        );
    }
    updated
}

fn verify_provider_connection_triggers<E: OrganizationEngine>(engine: &E, label: &str) {
    let mut tx = engine.begin().unwrap();
    seed_org::<E>(&mut tx);
    let updated = verify_provider_connection_update_and_immutability::<E>(
        &mut tx,
        &sample_subscription_connection(),
        label,
    );

    // Inserting a second connection claiming the same subscription_identity or colliding key fails.
    let mut dup_sub = updated.clone();
    dup_sub.id = id(21);
    dup_sub.key = "claude-sub-2".into();
    let mut dup_key = updated.clone();
    dup_key.id = id(21);
    dup_key.subscription_identity = None;
    dup_key.auth_mode = ProviderAuthMode::ApiKey;
    for (case, inserted) in [
        ("duplicate_subscription", dup_sub),
        ("duplicate_key", dup_key),
    ] {
        let err = tx
            .put_provider_connection(&inserted)
            .expect_err(&format!("{label}: inserting {case} must fail closed"));
        assert!(
            err.message
                .contains("registered subscription cannot be replaced"),
            "{label} ({case}): unexpected error: {err}"
        );
    }

    // Updating a distinct connection to displace the registered subscription by key or identity fails.
    let mut other = ProviderConnection {
        id: id(22),
        key: "other-conn".into(),
        name: "Other Connection".into(),
        provider_type: "anthropic".into(),
        auth_mode: ProviderAuthMode::ApiKey,
        subscription_identity: None,
        runtime_driver: "claude-code".into(),
        endpoint_url: None,
        enabled: true,
        archived: false,
        max_concurrent_turns: 2,
        meta: test_meta(),
    };
    tx.put_provider_connection(&other).unwrap();
    let mut displace_key = other.clone();
    displace_key.key = updated.key.clone();
    let mut displace_sub = other.clone();
    displace_sub.subscription_identity = updated.subscription_identity.clone();
    for (case, mutated) in [
        ("displace_by_key", displace_key),
        ("displace_by_identity", displace_sub),
    ] {
        let err = tx
            .put_provider_connection(&mutated)
            .expect_err(&format!("{label}: {case} must fail closed"));
        assert!(
            err.message
                .contains("registered subscription cannot be displaced"),
            "{label} ({case}): unexpected error: {err}"
        );
    }
    other.max_concurrent_turns = 3;
    tx.put_provider_connection(&other).unwrap();
    tx.check().unwrap();
}

#[test]
fn subscription_cli_provider_connection_reprojection_and_update_respect_sqlite_triggers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("office.db");
    let sqlite = SqliteEngine::open(&path)
        .unwrap()
        .for_organization(&id(ORG));
    verify_provider_connection_triggers(&sqlite, "sqlite");

    let memory = MemoryEngine::new();
    verify_provider_connection_triggers(&memory, "memory");
}
