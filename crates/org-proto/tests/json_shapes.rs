//! Exact JSON shapes: each test pins the JSON a type produces to what the UI's draft
//! (`app/src/lib/api/types.ts`, with the approved changes) expects, and reads it back.

use std::fmt::Debug;

use org_proto::rpc::{
    methods, ErrorObject, Notification, Outcome, Request, RequestId, Response, ServerMessage,
};
use org_proto::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

/// `value` serializes to exactly `expected`, and `expected` reads back as `value`.
#[track_caller]
fn assert_shape<T>(value: &T, expected: Value)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    assert_eq!(serde_json::to_value(value).unwrap(), expected);
    let back: T = serde_json::from_value(expected).unwrap();
    assert_eq!(&back, value);
}

#[track_caller]
fn assert_rejects<T: DeserializeOwned + Debug>(json: Value) {
    let result = serde_json::from_value::<T>(json.clone());
    assert!(result.is_err(), "{json} should not parse, got {result:?}");
}

const M: &str = "6f1c2b9e-0d3a-4c55-9a51-1f7d2e8b4c10";
const T0: &str = "2026-11-07T10:00:00Z";

fn machine() -> Machine {
    Machine {
        machine_id: M.into(),
        label: "This computer".into(),
        is_local: true,
        connection: MachineConnection::Connected,
        last_seen_at: None,
    }
}

fn machine_json() -> Value {
    json!({
        "machine_id": M,
        "label": "This computer",
        "is_local": true,
        "connection": "connected",
        "last_seen_at": null
    })
}

fn agent() -> Agent {
    Agent {
        machine_id: M.into(),
        id: "ac-software-engineer-remy".into(),
        name: "Remy".into(),
        project: "ac".into(),
        role: "software-engineer".into(),
        title: "Senior Backend Engineer".into(),
        department: "Engineering".into(),
        focus: None,
        reports_to: ReportsTo::Agent("ac-staff-engineer-ada".into()),
        model: "opus".into(),
        tools: Tools::Inherit,
        status: EmploymentStatus::Active,
        hired_on: "2026-11-01".into(),
        offboarded_on: None,
        home: "projects/ac/agents/ac-software-engineer-remy".into(),
        registration: ".claude/agents/ac-software-engineer-remy.md".into(),
    }
}

fn agent_json() -> Value {
    json!({
        "machine_id": M,
        "id": "ac-software-engineer-remy",
        "name": "Remy",
        "project": "ac",
        "role": "software-engineer",
        "title": "Senior Backend Engineer",
        "department": "Engineering",
        "focus": null,
        "reports_to": "ac-staff-engineer-ada",
        "model": "opus",
        "tools": { "kind": "inherit" },
        "status": "active",
        "hired_on": "2026-11-01",
        "offboarded_on": null,
        "home": "projects/ac/agents/ac-software-engineer-remy",
        "registration": ".claude/agents/ac-software-engineer-remy.md"
    })
}

fn presence() -> Presence {
    Presence {
        machine_id: M.into(),
        agent_id: "ac-software-engineer-remy".into(),
        status: AgentStatus::NeedsYou,
        since: T0.into(),
        reasons: vec![NeedsYouKind::Permission, NeedsYouKind::Waiting],
        session_id: Some("0b8f6c1e-7a2d-4f3b-9c4e-5d6a7b8c9d0e".into()),
        session_kind: Some(SessionKind::Background),
        has_presence: true,
        source: PresenceSource::Presence,
        as_of: T0.into(),
    }
}

fn presence_json() -> Value {
    json!({
        "machine_id": M,
        "agent_id": "ac-software-engineer-remy",
        "status": "needs_you",
        "since": T0,
        "reasons": ["permission", "waiting"],
        "session_id": "0b8f6c1e-7a2d-4f3b-9c4e-5d6a7b8c9d0e",
        "session_kind": "background",
        "has_presence": true,
        "source": "presence",
        "as_of": T0
    })
}

fn preview() -> HirePreview {
    HirePreview {
        machine_id: M.into(),
        agent_id: "ac-qa-engineer-rita".into(),
        name: "Rita".into(),
        title: "QA Engineer".into(),
        focus: None,
        project: "ac".into(),
        home: "projects/ac/agents/ac-qa-engineer-rita".into(),
        registration: ".claude/agents/ac-qa-engineer-rita.md".into(),
        reports_to: ReportsTo::User,
        model: "opus".into(),
        tools: Tools::List {
            tools: vec!["Read".into(), "Bash".into()],
        },
        peers: vec![],
        session_command: "bin/agent ac-qa-engineer-rita".into(),
        files: vec![PlannedFile {
            path: "hr/roles/qa-engineer.toml".into(),
            change: FileChange::Created,
        }],
        new_project: None,
        new_role: Some(NewRolePreview {
            draft: NewRoleDraft {
                id: "qa-engineer".into(),
                title: "QA Engineer".into(),
                department: "Engineering".into(),
                summary: "Use for testing.".into(),
                responsibilities: vec!["Test".into()],
                boundaries: vec![],
                tools: Tools::Inherit,
                directories: vec![],
            },
            path: "hr/roles/qa-engineer.toml".into(),
        }),
        errors: vec![FieldError {
            field: HireField::ProjectKey,
            message: "taken".into(),
        }],
        workspace_revision: "rev-1".into(),
    }
}

// ─── Unions and nulls ───────────────────────────────────────────────────────

#[test]
fn tools_is_kind_tagged() {
    assert_shape(&Tools::Inherit, json!({ "kind": "inherit" }));
    assert_shape(
        &Tools::List {
            tools: vec!["Read".into(), "Bash".into()],
        },
        json!({ "kind": "list", "tools": ["Read", "Bash"] }),
    );
    // The draft's old forms are gone.
    assert_rejects::<Tools>(json!("inherit"));
    assert_rejects::<Tools>(json!(["Read"]));
}

#[test]
fn proposed_is_kind_tagged() {
    assert_shape(
        &Proposed::Settled {
            value: "QA Engineer".to_owned(),
        },
        json!({ "kind": "settled", "value": "QA Engineer" }),
    );
    assert_shape(
        &Proposed::<String>::NeedsChoice { value: None },
        json!({ "kind": "needs_choice", "value": null }),
    );
    assert_shape(
        &Proposed::NeedsChoice {
            value: Some(RoleChoice::Catalog {
                role_id: "qa-engineer".into(),
            }),
        },
        json!({
            "kind": "needs_choice",
            "value": { "kind": "catalog", "role_id": "qa-engineer" }
        }),
    );
    // A settled optional field (focus) can settle on null.
    assert_shape(
        &Proposed::<Option<String>>::Settled { value: None },
        json!({ "kind": "settled", "value": null }),
    );
    assert_shape(
        &Proposed::Settled {
            value: ProposedName {
                value: "Rita".into(),
                picked_by_hr: true,
            },
        },
        json!({ "kind": "settled", "value": { "value": "Rita", "picked_by_hr": true } }),
    );
    // The draft's boolean form is gone.
    assert_rejects::<Proposed<String>>(json!({ "value": "x", "needs_choice": false }));
}

#[test]
fn none_is_null_not_omitted() {
    assert_shape(&machine(), machine_json());
    assert_shape(&agent(), agent_json());
    assert_shape(&presence(), presence_json());

    let json = serde_json::to_value(agent()).unwrap();
    assert!(json.as_object().unwrap().contains_key("focus"));
    assert!(json.as_object().unwrap().contains_key("offboarded_on"));
}

#[test]
fn reports_to_is_a_plain_string() {
    assert_shape(&ReportsTo::User, json!("user"));
    assert_shape(
        &ReportsTo::Agent("ac-staff-engineer-ada".into()),
        json!("ac-staff-engineer-ada"),
    );
    assert_eq!(ReportsTo::from("user"), ReportsTo::User);
    assert_eq!(ReportsTo::from("user".to_owned()), ReportsTo::User);
    // Equality goes by the wire string, so a hand-built variant can't disagree with JSON.
    assert_eq!(ReportsTo::Agent("user".into()), ReportsTo::User);
}

#[test]
fn hr_agent_has_literal_id_and_name() {
    let hr = HrAgent {
        machine_id: M.into(),
        id: HrAgentId::Hr,
        name: HrAgentName::Hr,
        description: "Creates projects and hires agents.".into(),
        registration: ".claude/agents/hr.md".into(),
    };
    assert_shape(
        &hr,
        json!({
            "machine_id": M,
            "id": "hr",
            "name": "HR",
            "description": "Creates projects and hires agents.",
            "registration": ".claude/agents/hr.md"
        }),
    );
    assert_rejects::<HrAgent>(json!({
        "machine_id": M, "id": "ada", "name": "HR", "description": "", "registration": ""
    }));
    assert_eq!(hr.id.agent_id(), "hr");
}

#[test]
fn claude_code_state_variants() {
    assert_shape(
        &ClaudeCodeState::Ok {
            version: "2.1.310".into(),
        },
        json!({ "kind": "ok", "version": "2.1.310" }),
    );
    assert_shape(&ClaudeCodeState::Missing, json!({ "kind": "missing" }));
    assert_shape(
        &ClaudeCodeState::TooOld {
            version: "2.0.0".into(),
            minimum: "2.1.302".into(),
        },
        json!({ "kind": "too_old", "version": "2.0.0", "minimum": "2.1.302" }),
    );
    assert_shape(
        &ClaudeCodeState::SignedOut {
            version: "2.1.310".into(),
        },
        json!({ "kind": "signed_out", "version": "2.1.310" }),
    );
}

#[test]
fn session_has_id_and_attach_id() {
    let session = Session {
        machine_id: M.into(),
        id: "0b8f6c1e-7a2d-4f3b-9c4e-5d6a7b8c9d0e".into(),
        attach_id: "1a2b3c4d".into(),
        agent_id: "ac-software-engineer-remy".into(),
        kind: SessionKind::Background,
        origin: SessionOrigin::Outside,
        has_presence: true,
        state: SessionState::Running {
            error: Some(TurnError {
                at: T0.into(),
                message: "API error".into(),
            }),
        },
        started_at: T0.into(),
        last_event_at: None,
        command: "bin/agent ac-software-engineer-remy".into(),
    };
    assert_shape(
        &session,
        json!({
            "machine_id": M,
            "id": "0b8f6c1e-7a2d-4f3b-9c4e-5d6a7b8c9d0e",
            "attach_id": "1a2b3c4d",
            "agent_id": "ac-software-engineer-remy",
            "kind": "background",
            "origin": "outside",
            "has_presence": true,
            "state": { "kind": "running", "error": { "at": T0, "message": "API error" } },
            "started_at": T0,
            "last_event_at": null,
            "command": "bin/agent ac-software-engineer-remy"
        }),
    );
    assert_shape(&SessionState::Starting, json!({ "kind": "starting" }));
    assert_shape(
        &SessionState::Running { error: None },
        json!({ "kind": "running", "error": null }),
    );
    assert_shape(
        &SessionState::Stopped {
            stopped_at: T0.into(),
        },
        json!({ "kind": "stopped", "stopped_at": T0 }),
    );
    assert_shape(
        &SessionState::EndedUnexpectedly {
            at: T0.into(),
            exit_reason: Some("killed".into()),
        },
        json!({ "kind": "ended_unexpectedly", "at": T0, "exit_reason": "killed" }),
    );
}

#[test]
fn task_status_is_any_string() {
    assert_shape(
        &TaskStatus::Known(KnownTaskStatus::InProgress),
        json!("in-progress"),
    );
    assert_shape(&TaskStatus::Known(KnownTaskStatus::Todo), json!("todo"));
    assert_shape(
        &TaskStatus::Other("waiting on ada".into()),
        json!("waiting on ada"),
    );
    // Equality goes by the wire string, so a hand-built variant can't disagree with JSON.
    assert_eq!(
        TaskStatus::Other("todo".into()),
        TaskStatus::Known(KnownTaskStatus::Todo)
    );
    assert_ne!(
        TaskStatus::Other("Done".into()),
        TaskStatus::Known(KnownTaskStatus::Done)
    );
    assert_eq!(TaskStatus::from("blocked").as_str(), "blocked");
    for known in KnownTaskStatus::ALL {
        assert_eq!(serde_json::to_value(known).unwrap(), json!(known.as_str()));
        assert!(matches!(
            TaskStatus::parse(known.as_str()),
            TaskStatus::Known(k) if k == known
        ));
    }
    for status in ["todo", "in-progress", "blocked", "done", "Done", "wip"] {
        let parsed = TaskStatus::parse(status);
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json!(status));
        assert_eq!(
            serde_json::from_value::<TaskStatus>(json!(status)).unwrap(),
            parsed
        );
    }
}

#[test]
fn backlog_variants() {
    let ok = Backlog::Ok {
        machine_id: M.into(),
        agent_id: "ac-software-engineer-remy".into(),
        path: "projects/ac/agents/ac-software-engineer-remy/tasks/BACKLOG.md".into(),
        tasks: vec![Task {
            machine_id: M.into(),
            agent_id: "ac-software-engineer-remy".into(),
            number: 3,
            text: "Build org-proto".into(),
            status: TaskStatus::Known(KnownTaskStatus::InProgress),
            requested_by: "user".into(),
            notes: "".into(),
        }],
    };
    assert_shape(
        &ok,
        json!({
            "kind": "ok",
            "machine_id": M,
            "agent_id": "ac-software-engineer-remy",
            "path": "projects/ac/agents/ac-software-engineer-remy/tasks/BACKLOG.md",
            "tasks": [{
                "machine_id": M,
                "agent_id": "ac-software-engineer-remy",
                "number": 3,
                "text": "Build org-proto",
                "status": "in-progress",
                "requested_by": "user",
                "notes": ""
            }]
        }),
    );
    assert_shape(
        &Backlog::Unreadable {
            machine_id: M.into(),
            agent_id: "a".into(),
            path: "p".into(),
            reason: "the task table is malformed".into(),
        },
        json!({
            "kind": "unreadable",
            "machine_id": M,
            "agent_id": "a",
            "path": "p",
            "reason": "the task table is malformed"
        }),
    );
}

#[test]
fn activity_event_flattens_its_kind() {
    let event = ActivityEvent {
        machine_id: M.into(),
        id: "act-1".into(),
        at: T0.into(),
        detail: ActivityDetail::SessionChanged {
            agent_id: "ac-software-engineer-remy".into(),
            session_id: "s-1".into(),
            change: SessionChange::StoppedWithError,
        },
    };
    assert_shape(
        &event,
        json!({
            "machine_id": M,
            "id": "act-1",
            "at": T0,
            "kind": "session_changed",
            "agent_id": "ac-software-engineer-remy",
            "session_id": "s-1",
            "change": "stopped_with_error"
        }),
    );
    let task_event = ActivityEvent {
        machine_id: M.into(),
        id: "act-2".into(),
        at: T0.into(),
        detail: ActivityDetail::TaskStatusChanged {
            agent_id: "a".into(),
            task_number: 4,
            from: "todo".into(),
            to: "done".into(),
        },
    };
    assert_shape(
        &task_event,
        json!({
            "machine_id": M, "id": "act-2", "at": T0,
            "kind": "task_status_changed", "agent_id": "a", "task_number": 4,
            "from": "todo", "to": "done"
        }),
    );
    let goal_event = ActivityEvent {
        machine_id: M.into(),
        id: "act-4".into(),
        at: T0.into(),
        detail: ActivityDetail::GoalChanged {
            agent_id: "a".into(),
            session_id: "s-1".into(),
            change: GoalChange::Paused,
            reason: Some(GoalChangeReason::UnmetChecks),
            paused_after_checks: Some(8),
        },
    };
    assert_shape(
        &goal_event,
        json!({
            "machine_id": M, "id": "act-4", "at": T0,
            "kind": "goal_changed", "agent_id": "a", "session_id": "s-1", "change": "paused",
            "reason": "unmet_checks", "paused_after_checks": 8
        }),
    );
    let project_event = ActivityEvent {
        machine_id: M.into(),
        id: "act-3".into(),
        at: T0.into(),
        detail: ActivityDetail::ProjectCreated {
            project_key: "pay".into(),
        },
    };
    assert_shape(
        &project_event,
        json!({ "machine_id": M, "id": "act-3", "at": T0, "kind": "project_created", "project_key": "pay" }),
    );
}

#[test]
fn document_flattens_its_ref() {
    let document = Document {
        meta: DocumentRef {
            machine_id: M.into(),
            path: "projects/ac/CLAUDE.md".into(),
            kind: DocumentKind::Brief,
            agent_id: None,
            title: "Agentic consulting".into(),
            modified_at: T0.into(),
            size_bytes: 5_000_000_000,
        },
        content: "# Brief".into(),
    };
    assert_shape(
        &document,
        json!({
            "machine_id": M,
            "path": "projects/ac/CLAUDE.md",
            "kind": "brief",
            "agent_id": null,
            "title": "Agentic consulting",
            "modified_at": T0,
            "size_bytes": 5_000_000_000u64,
            "content": "# Brief"
        }),
    );
}

// ─── Hire ───────────────────────────────────────────────────────────────────

#[test]
fn hire_request_and_plan() {
    assert_shape(
        &HireRequest {
            description: "someone to test the app".into(),
            project: Some(ProjectRequest::New),
            role_id: None,
        },
        json!({ "description": "someone to test the app", "project": { "kind": "new" }, "role_id": null }),
    );
    assert_shape(
        &HireRequest {
            description: "".into(),
            project: None,
            role_id: Some("qa-engineer".into()),
        },
        json!({ "description": "", "project": null, "role_id": "qa-engineer" }),
    );
    assert_shape(
        &HirePlan {
            role: RoleChoice::Catalog {
                role_id: "qa-engineer".into(),
            },
            title: "QA Engineer".into(),
            name: "".into(),
            focus: None,
            project: ProjectChoice::New {
                draft: NewProjectDraft {
                    key: "pay".into(),
                    name: "Payments".into(),
                    description: "".into(),
                },
            },
            reports_to: ReportsTo::User,
        },
        json!({
            "role": { "kind": "catalog", "role_id": "qa-engineer" },
            "title": "QA Engineer",
            "name": "",
            "focus": null,
            "project": { "kind": "new", "draft": { "key": "pay", "name": "Payments", "description": "" } },
            "reports_to": "user"
        }),
    );
}

#[test]
fn hire_preview_flattens_the_new_role() {
    let json = serde_json::to_value(preview()).unwrap();
    assert_eq!(
        json["new_role"],
        json!({
            "id": "qa-engineer",
            "title": "QA Engineer",
            "department": "Engineering",
            "summary": "Use for testing.",
            "responsibilities": ["Test"],
            "boundaries": [],
            "tools": { "kind": "inherit" },
            "directories": [],
            "path": "hr/roles/qa-engineer.toml"
        })
    );
    assert_eq!(
        json["tools"],
        json!({ "kind": "list", "tools": ["Read", "Bash"] })
    );
    assert_eq!(json["reports_to"], json!("user"));
    assert_eq!(json["new_project"], json!(null));
    assert_eq!(
        json["errors"],
        json!([{ "field": "project_key", "message": "taken" }])
    );
    assert_eq!(
        json["files"],
        json!([{ "path": "hr/roles/qa-engineer.toml", "change": "created" }])
    );
    assert_shape(&preview(), json);
}

#[test]
fn every_hire_progress_variant_has_a_draft_id() {
    let suggestion = HireSuggestion {
        machine_id: M.into(),
        draft_id: "d-1".into(),
        role: Proposed::Settled {
            value: RoleChoice::Catalog {
                role_id: "qa-engineer".into(),
            },
        },
        title: Proposed::Settled {
            value: "QA Engineer".into(),
        },
        name: Proposed::Settled {
            value: ProposedName {
                value: "Rita".into(),
                picked_by_hr: false,
            },
        },
        focus: Proposed::NeedsChoice { value: None },
        project: Proposed::Settled {
            value: ProjectChoice::Existing { key: "ac".into() },
        },
        reports_to: Proposed::NeedsChoice {
            value: Some(ReportsTo::User),
        },
        reason: "A QA role exists.".into(),
    };
    let variants = [
        HireProgress::HrWorking {
            draft_id: "d-1".into(),
            message: "HR is writing the role description…".into(),
        },
        HireProgress::SuggestionReady {
            draft_id: "d-1".into(),
            suggestion: suggestion.clone(),
        },
        HireProgress::HrFailed {
            draft_id: "d-1".into(),
            message: "HR stopped".into(),
            details: "exit 1".into(),
        },
        HireProgress::Step {
            draft_id: "d-1".into(),
            step: HireStep::AddToRoster,
            state: StepState::Running,
        },
        HireProgress::Hired {
            draft_id: "d-1".into(),
            agent: agent(),
            files: vec![],
        },
        HireProgress::WorkspaceChanged {
            draft_id: "d-1".into(),
            preview: preview(),
        },
        HireProgress::Failed {
            draft_id: "d-1".into(),
            step: HireStep::Register,
            reason: "disk full".into(),
            created: vec!["projects/ac/agents/ac-qa-engineer-rita".into()],
            details: "ENOSPC".into(),
        },
    ];
    let kinds: Vec<Value> = variants
        .iter()
        .map(|progress| {
            let json = serde_json::to_value(progress).unwrap();
            assert_eq!(json["draft_id"], json!("d-1"), "{json}");
            assert_eq!(progress.draft_id(), "d-1");
            let back: HireProgress = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(&back, progress);
            json["kind"].clone()
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            json!("hr_working"),
            json!("suggestion_ready"),
            json!("hr_failed"),
            json!("step"),
            json!("hired"),
            json!("workspace_changed"),
            json!("failed")
        ]
    );

    assert_shape(
        &variants[3],
        json!({ "kind": "step", "draft_id": "d-1", "step": "add_to_roster", "state": "running" }),
    );
    assert_shape(
        &variants[6],
        json!({
            "kind": "failed",
            "draft_id": "d-1",
            "step": "register",
            "reason": "disk full",
            "created": ["projects/ac/agents/ac-qa-engineer-rita"],
            "details": "ENOSPC"
        }),
    );
    assert_shape(
        &suggestion,
        json!({
            "machine_id": M,
            "draft_id": "d-1",
            "role": { "kind": "settled", "value": { "kind": "catalog", "role_id": "qa-engineer" } },
            "title": { "kind": "settled", "value": "QA Engineer" },
            "name": { "kind": "settled", "value": { "value": "Rita", "picked_by_hr": false } },
            "focus": { "kind": "needs_choice", "value": null },
            "project": { "kind": "settled", "value": { "kind": "existing", "key": "ac" } },
            "reports_to": { "kind": "needs_choice", "value": "user" },
            "reason": "A QA role exists."
        }),
    );
}

// ─── Events ─────────────────────────────────────────────────────────────────

#[test]
fn org_event_variants() {
    assert_shape(
        &OrgEvent::PresenceChanged {
            presence: presence(),
        },
        json!({ "kind": "presence_changed", "presence": presence_json() }),
    );
    assert_shape(
        &OrgEvent::FileChanged {
            machine_id: M.into(),
            path: "projects/ac/agents/ac-software-engineer-remy/tasks/BACKLOG.md".into(),
            agent_id: Some("ac-software-engineer-remy".into()),
            by_open_session: true,
        },
        json!({
            "kind": "file_changed",
            "machine_id": M,
            "path": "projects/ac/agents/ac-software-engineer-remy/tasks/BACKLOG.md",
            "agent_id": "ac-software-engineer-remy",
            "by_open_session": true
        }),
    );
    assert_shape(
        &OrgEvent::FileChanged {
            machine_id: M.into(),
            path: "hr/roles/qa-engineer.toml".into(),
            agent_id: None,
            by_open_session: false,
        },
        json!({
            "kind": "file_changed",
            "machine_id": M,
            "path": "hr/roles/qa-engineer.toml",
            "agent_id": null,
            "by_open_session": false
        }),
    );
    assert_shape(
        &OrgEvent::NeedsYouCleared {
            machine_id: M.into(),
            item_id: "ny-1".into(),
            reason: NeedsYouClearReason::CarriedOn,
            at: T0.into(),
        },
        json!({
            "kind": "needs_you_cleared",
            "machine_id": M,
            "item_id": "ny-1",
            "reason": "carried_on",
            "at": T0
        }),
    );
    assert_shape(
        &OrgEvent::NeedsYouOpened {
            item: NeedsYouItem {
                machine_id: M.into(),
                id: "ny-2".into(),
                agent_id: "a".into(),
                kind: NeedsYouKind::Permission,
                since: T0.into(),
                session_id: None,
                tool_name: Some("Bash".into()),
                detail: None,
                task_number: None,
                dismissible: false,
                dismissed_at: None,
            },
        },
        json!({
            "kind": "needs_you_opened",
            "item": {
                "machine_id": M, "id": "ny-2", "agent_id": "a", "kind": "permission",
                "since": T0, "session_id": null, "tool_name": "Bash", "detail": null,
                "task_number": null, "dismissible": false, "dismissed_at": null
            }
        }),
    );
    let session = Session {
        machine_id: M.into(),
        id: "s-1".into(),
        attach_id: "1a2b3c4d".into(),
        agent_id: "a".into(),
        kind: SessionKind::Interactive,
        origin: SessionOrigin::Outside,
        has_presence: false,
        state: SessionState::Stopped {
            stopped_at: T0.into(),
        },
        started_at: T0.into(),
        last_event_at: Some(T0.into()),
        command: "bin/agent a".into(),
    };
    assert_shape(
        &OrgEvent::SessionChanged { session },
        json!({
            "kind": "session_changed",
            "session": {
                "machine_id": M, "id": "s-1", "attach_id": "1a2b3c4d", "agent_id": "a",
                "kind": "interactive", "origin": "outside", "has_presence": false,
                "state": { "kind": "stopped", "stopped_at": T0 },
                "started_at": T0, "last_event_at": T0, "command": "bin/agent a"
            }
        }),
    );
    // Unknown or missing kinds are errors, not silently dropped.
    assert_rejects::<OrgEvent>(json!({ "kind": "presence_lost" }));
    assert_rejects::<OrgEvent>(json!({ "presence": presence_json() }));
    // The wire path: read from text, as the socket reader does.
    let from_text: OrgEvent = serde_json::from_str(
        r#"{"kind":"needs_you_cleared","machine_id":"m","item_id":"i","reason":"dismissed","at":"t"}"#,
    )
    .unwrap();
    assert!(matches!(
        from_text,
        OrgEvent::NeedsYouCleared {
            reason: NeedsYouClearReason::Dismissed,
            ..
        }
    ));
    assert_shape(
        &OrgEvent::MachineChanged { machine: machine() },
        json!({ "kind": "machine_changed", "machine": machine_json() }),
    );
    assert_shape(
        &OrgEvent::GoalChanged {
            machine_id: M.into(),
            agent_id: "a".into(),
            goal: Some(Goal {
                machine_id: M.into(),
                agent_id: "a".into(),
                session_id: "s-1".into(),
                state: GoalState::Paused,
                paused_after_checks: Some(8),
                updated_at: T0.into(),
            }),
        },
        json!({
            "kind": "goal_changed",
            "machine_id": M,
            "agent_id": "a",
            "goal": {
                "machine_id": M, "agent_id": "a", "session_id": "s-1", "state": "paused",
                "paused_after_checks": 8, "updated_at": T0
            }
        }),
    );
    assert_shape(
        &OrgEvent::GoalChanged {
            machine_id: M.into(),
            agent_id: "a".into(),
            goal: None,
        },
        json!({ "kind": "goal_changed", "machine_id": M, "agent_id": "a", "goal": null }),
    );
    assert_shape(
        &OrgEvent::Activity {
            event: ActivityEvent {
                machine_id: M.into(),
                id: "act-1".into(),
                at: T0.into(),
                detail: ActivityDetail::AgentHired {
                    agent_id: "a".into(),
                },
            },
        },
        json!({
            "kind": "activity",
            "event": { "machine_id": M, "id": "act-1", "at": T0, "kind": "agent_hired", "agent_id": "a" }
        }),
    );
    assert_shape(
        &OrgEvent::HireProgress {
            progress: Box::new(HireProgress::HrWorking {
                draft_id: "d-1".into(),
                message: "…".into(),
            }),
        },
        json!({
            "kind": "hire_progress",
            "progress": { "kind": "hr_working", "draft_id": "d-1", "message": "…" }
        }),
    );
}

// ─── Errors ─────────────────────────────────────────────────────────────────

#[test]
fn api_error_variants() {
    assert_shape(
        &ApiError::not_found("no agent ac-x"),
        json!({ "kind": "not_found", "message": "no agent ac-x" }),
    );
    assert_shape(
        &ApiError::conflict("the workspace changed"),
        json!({ "kind": "conflict", "message": "the workspace changed" }),
    );
    assert_shape(
        &ApiError::invalid(
            "check the form",
            vec![FieldError {
                field: HireField::Name,
                message: "ada is already a name on this project".into(),
            }],
        ),
        json!({
            "kind": "invalid",
            "message": "check the form",
            "errors": [{ "field": "name", "message": "ada is already a name on this project" }]
        }),
    );
    assert_shape(
        &ApiError::ClaudeCodeUnavailable {
            message: "Claude Code is signed out".into(),
            state: ClaudeCodeState::SignedOut {
                version: "2.1.310".into(),
            },
        },
        json!({
            "kind": "claude_code_unavailable",
            "message": "Claude Code is signed out",
            "state": { "kind": "signed_out", "version": "2.1.310" }
        }),
    );
    assert_shape(
        &ApiError::unavailable("orgd is starting"),
        json!({ "kind": "unavailable", "message": "orgd is starting" }),
    );
    assert_shape(
        &ApiError::internal("something broke", "stack"),
        json!({ "kind": "internal", "message": "something broke", "details": "stack" }),
    );
}

#[test]
fn api_error_travels_as_json_rpc_error_data() {
    let error = ApiError::unavailable("orgd is starting");
    let object = ErrorObject::from(&error);
    assert_eq!(
        serde_json::to_value(&object).unwrap(),
        json!({
            "code": -32005,
            "message": "orgd is starting",
            "data": { "kind": "unavailable", "message": "orgd is starting" }
        })
    );
    assert_eq!(ApiError::from(object), error);

    // Protocol errors carry no ApiError and map by code.
    let invalid_params = ErrorObject {
        code: -32602,
        message: "bad params".into(),
        data: None,
    };
    assert_eq!(
        ApiError::from(invalid_params),
        ApiError::invalid("bad params", vec![])
    );
    // An orgd that doesn't offer a method yet: the UI shows the area as unavailable.
    let method_not_found = ErrorObject {
        code: -32601,
        message: "no such method".into(),
        data: Some(json!("not an ApiError")),
    };
    assert_eq!(
        ApiError::from(method_not_found),
        ApiError::unavailable("no such method")
    );
    let parse_error = ErrorObject {
        code: -32700,
        message: "parse error".into(),
        data: None,
    };
    assert!(matches!(
        ApiError::from(parse_error),
        ApiError::Internal { .. }
    ));

    // Every kind has its own code.
    let codes: std::collections::BTreeSet<i32> = [
        ApiError::not_found(""),
        ApiError::conflict(""),
        ApiError::invalid("", vec![]),
        ApiError::ClaudeCodeUnavailable {
            message: "".into(),
            state: ClaudeCodeState::Missing,
        },
        ApiError::unavailable(""),
        ApiError::internal("", ""),
    ]
    .iter()
    .map(ApiError::rpc_code)
    .collect();
    assert_eq!(codes.len(), 6);
}

// ─── Methods and params ─────────────────────────────────────────────────────

#[test]
fn method_names_are_snake_case_and_unique() {
    let names: Vec<&str> = MethodName::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "hello",
            "snapshot",
            "organization",
            "environment",
            "projects",
            "roles",
            "hr",
            "agents",
            "agent",
            "presence",
            "goals",
            "backlog",
            "sessions",
            "needs_you",
            "activity",
            "documents",
            "document",
            "dismiss",
            "assign_task",
            "open_session",
            "resume_session",
            "stop_session",
            "propose_hire",
            "preview_hire",
            "hire",
            "subscribe",
            "unsubscribe",
        ]
    );
    for method in MethodName::ALL {
        assert_eq!(
            serde_json::to_value(method).unwrap(),
            json!(method.as_str())
        );
        assert_eq!(MethodName::from_name(method.as_str()), Some(*method));
    }
    assert_eq!(MethodName::from_name("needsYou"), None);
    assert_eq!(<methods::NeedsYou as Method>::NAME, MethodName::NeedsYou);
}

#[test]
fn params_shapes() {
    assert_shape(&NoParams {}, json!({}));
    // Optional params are omitted when unset, and may be left out.
    assert_shape(&AgentsParams::default(), json!({}));
    assert_shape(
        &AgentsParams {
            include_offboarded: Some(true),
        },
        json!({ "include_offboarded": true }),
    );
    assert_shape(&NeedsYouParams::default(), json!({}));
    assert_shape(&ActivityParams { limit: Some(50) }, json!({ "limit": 50 }));
    assert_shape(
        &DocumentsParams {
            agent_id: "a".into(),
            kind: Some(DocumentKind::Journal),
        },
        json!({ "agent_id": "a", "kind": "journal" }),
    );
    assert_shape(
        &DocumentsParams {
            agent_id: "a".into(),
            kind: None,
        },
        json!({ "agent_id": "a" }),
    );
    assert_shape(
        &ResumeSessionParams {
            agent_id: "a".into(),
            session_id: None,
        },
        json!({ "agent_id": "a" }),
    );
    // No more `start_now`.
    assert_shape(
        &AssignTaskRequest {
            agent_id: "a".into(),
            text: "Build org-proto".into(),
            notes: "DoD (goal): tests pass".into(),
        },
        json!({ "agent_id": "a", "text": "Build org-proto", "notes": "DoD (goal): tests pass" }),
    );
    assert_shape(
        &ProposeHireResult {
            draft_id: "d-1".into(),
        },
        json!({ "draft_id": "d-1" }),
    );
}

#[test]
fn hello_and_snapshot_shapes() {
    assert_shape(
        &ApiInfo {
            api_version: API_VERSION,
            orgd_version: "0.1.0".into(),
            machine: machine(),
        },
        json!({ "api_version": 1, "orgd_version": "0.1.0", "machine": machine_json() }),
    );
    let snapshot = Snapshot {
        organization: Organization {
            machine_id: M.into(),
            name: "agents".into(),
            workspace_path: "/Users/me/agents".into(),
            format: 1,
        },
        machine: machine(),
        projects: vec![],
        roles: vec![],
        hr: HrAgent {
            machine_id: M.into(),
            id: HrAgentId::Hr,
            name: HrAgentName::Hr,
            description: "".into(),
            registration: ".claude/agents/hr.md".into(),
        },
        agents: vec![agent()],
        presence: vec![presence()],
        backlogs: vec![],
        needs_you: None,
    };
    let json = serde_json::to_value(&snapshot).unwrap();
    let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "agents",
            "backlogs",
            "hr",
            "machine",
            "needs_you",
            "organization",
            "presence",
            "projects",
            "roles"
        ]
    );
    assert_eq!(json["agents"], json!([agent_json()]));
    // M1: an orgd that can't report Needs you yet says so with null.
    assert_eq!(json["needs_you"], json!(null));
    assert_eq!(
        json["organization"],
        json!({ "machine_id": M, "name": "agents", "workspace_path": "/Users/me/agents", "format": 1 })
    );
    assert_shape(&snapshot, json);
    let with_items = Snapshot {
        needs_you: Some(vec![]),
        ..snapshot
    };
    assert_eq!(
        serde_json::to_value(&with_items).unwrap()["needs_you"],
        json!([])
    );
}

// ─── Envelope ───────────────────────────────────────────────────────────────

#[test]
fn json_rpc_envelope() {
    let request = Request::new::<methods::Agents>(
        RequestId::Number(7),
        &AgentsParams {
            include_offboarded: Some(true),
        },
    )
    .unwrap();
    assert_shape(
        &request,
        json!({ "jsonrpc": "2.0", "id": 7, "method": "agents", "params": { "include_offboarded": true } }),
    );

    // A request that omits params reads as `{}`.
    let bare: Request =
        serde_json::from_value(json!({ "jsonrpc": "2.0", "id": "a", "method": "hello" })).unwrap();
    assert_eq!(bare.params::<methods::Hello>().unwrap(), NoParams {});
    assert_eq!(
        bare.params::<methods::Agents>().unwrap(),
        AgentsParams::default()
    );
    let wrong: Request = serde_json::from_value(
        json!({ "jsonrpc": "2.0", "id": 1, "method": "agent", "params": { "id": "a" } }),
    )
    .unwrap();
    assert_eq!(wrong.params::<methods::Agent>().unwrap_err().code, -32602);
    assert_rejects::<Request>(json!({ "jsonrpc": "1.0", "id": 1, "method": "hello" }));

    let ok = Response::success::<methods::Dismiss>(RequestId::Number(7), &()).unwrap();
    assert_shape(&ok, json!({ "jsonrpc": "2.0", "id": 7, "result": null }));
    assert_eq!(ok.into_result::<methods::Dismiss>(), Ok(()));

    let failed = Response::error(
        Some(RequestId::String("x".into())),
        ApiError::not_found("no agent a"),
    );
    let failed_json = json!({
        "jsonrpc": "2.0",
        "id": "x",
        "error": {
            "code": -32001,
            "message": "no agent a",
            "data": { "kind": "not_found", "message": "no agent a" }
        }
    });
    assert_shape(&failed, failed_json.clone());
    assert_eq!(
        failed.clone().into_result::<methods::Agent>(),
        Err(ApiError::not_found("no agent a"))
    );
    assert!(matches!(failed.outcome, Outcome::Error(_)));

    let event = OrgEvent::FileChanged {
        machine_id: M.into(),
        path: "x".into(),
        agent_id: None,
        by_open_session: false,
    };
    let notification = Notification::event(&event).unwrap();
    let notification_json = json!({
        "jsonrpc": "2.0",
        "method": "event",
        "params": {
            "kind": "file_changed", "machine_id": M, "path": "x",
            "agent_id": null, "by_open_session": false
        }
    });
    assert_shape(&notification, notification_json.clone());

    // Responses are read strictly: `id` present (null allowed), exactly one of result/error.
    let null_id: Response = serde_json::from_str(
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}"#,
    )
    .unwrap();
    assert_eq!(null_id.id, None);
    assert_rejects::<Response>(json!({ "jsonrpc": "2.0", "result": 1 }));
    assert_rejects::<Response>(json!({ "jsonrpc": "2.0", "id": 1 }));
    assert_rejects::<Response>(json!(["2.0", 1, 5]));
    assert_rejects::<ServerMessage>(json!(["2.0", 1, 5]));
    assert_rejects::<Response>(json!({
        "jsonrpc": "2.0", "id": 1, "result": 1,
        "error": { "code": -32603, "message": "x" }
    }));
    let both = serde_json::from_value::<ServerMessage>(json!({
        "jsonrpc": "2.0", "id": 1, "result": 1,
        "error": { "code": -32603, "message": "x" }
    }))
    .unwrap_err();
    assert!(both.to_string().contains("both"), "{both}");

    // The client tells responses from notifications.
    assert!(matches!(
        serde_json::from_value::<ServerMessage>(failed_json).unwrap(),
        ServerMessage::Response(_)
    ));
    match serde_json::from_value::<ServerMessage>(notification_json).unwrap() {
        ServerMessage::Notification(n) => {
            assert_eq!(n.method, EVENT_NOTIFICATION);
            let back: OrgEvent = serde_json::from_value(n.params.unwrap()).unwrap();
            assert_eq!(back, event);
        }
        other => panic!("expected a notification, got {other:?}"),
    }
}
