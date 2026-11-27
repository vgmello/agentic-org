use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    AgentRuntime, ApprovalId, ApprovalKind, ApprovalRequest, Capabilities, Control, ControlError,
    CostBasis, Escalation, ExitInfo, LaunchContext, LaunchPlan, Output, PlanError, Probe,
    ProbeContext, ProbeOutput, ProbeResult, ProbeRun, RuntimeId, Session, SessionEvent, SessionId,
    SessionSpec, SignalTarget, Stdio, StopPlan, StopSignal, StopStep, ToolCapability, TurnOutcome,
    Unavailable, UsageScope, VersionRange,
};
use org_office::{Office, TestOffice};
use org_office_sessions::{RuntimeRegistry, SessionManager, SessionManagerConfig};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::inbox::{
    Action, InboxChangedData, InboxClears, InboxCounts, InboxKind, InboxList, InboxListParams,
    InboxListResult, InboxState, Open, OpenTarget,
};
use org_proto::org::{OrgChangedData, Position};
use org_proto::providers::{
    DiscoveredModelEntitlement, DiscoveredModelInput, ProviderBindingSetParams,
    ProviderConnectionRefreshParams,
};
use org_proto::sessions::{MemberSessionApprovalDecide, MemberSessionStartParams};
use org_proto::{Call, ErrorCode, Outcome, Request};
use serde_json::{json, Value};

use crate::follow::OrgFeed;
use crate::inbox::{Hint, Keyset, Query, Source};
use crate::live::{follow_session_approvals, Live, LiveEvents};
use crate::office_ports::{build_messaging_ports, build_office_ports};
use crate::office_runtime::hq_inbox_with_sessions;
use crate::serve;

use super::PendingApprovalsInboxSource;

fn new_uuid_v7() -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    static SEQ: AtomicU64 = AtomicU64::new(500);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
    bytes[7] = (seq & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[15] = (seq & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

const SCRIPT_HOLD_OPEN: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  echo '{"kind":"turn_received"}'
done
"#;

#[derive(Clone)]
struct ScriptedApproval {
    id: String,
    tool: String,
    escalation: Escalation,
    capability: Option<ToolCapability>,
    input: Value,
}

struct FakeApprovalSession {
    agent_handle: String,
    runtime_session_id: String,
    approvals: Arc<Mutex<Vec<ScriptedApproval>>>,
}

impl Session for FakeApprovalSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            return Vec::new();
        };
        match v.get("kind").and_then(Value::as_str) {
            Some("ready") => vec![
                Output::Event(SessionEvent::SessionStarted {
                    runtime_session_id: SessionId::try_from(self.runtime_session_id.as_str())
                        .unwrap(),
                }),
                Output::Event(SessionEvent::Ready {
                    runtime_version: Some("1.0.0".into()),
                    pid: None,
                    agents: vec![self.agent_handle.clone()],
                    permission_mode: Some("default".into()),
                }),
            ],
            Some("turn_received") => {
                let queued = std::mem::take(&mut *self.approvals.lock().unwrap());
                queued
                    .into_iter()
                    .map(|item| {
                        let mut req = ApprovalRequest::new(
                            ApprovalId::new(item.id),
                            ApprovalKind::Command,
                            item.escalation,
                            item.tool,
                            item.input,
                        );
                        req.capability = item.capability;
                        Output::Event(SessionEvent::ApprovalRequested(req))
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    fn on_truncated(&mut self, _len: usize, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_stderr_line(&mut self, _line: &[u8], _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_stdout_closed(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_exit(&mut self, _exit: ExitInfo, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_tick(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn control(&mut self, cmd: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        match cmd {
            Control::Send { .. } => {
                let line = org_agent_runtime::encode_line(&json!({ "go": true }))
                    .map_err(|_| ControlError::Closed)?;
                Ok(vec![
                    Output::Event(SessionEvent::TurnStarted {
                        turn_id: Some("t-1".into()),
                        solicited: true,
                    }),
                    Output::Write(line),
                ])
            }
            Control::Answer { .. } => Ok(Vec::new()),
            Control::Close => Ok(vec![
                Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Completed,
                    errors: Vec::new(),
                }),
                Output::CloseStdin,
            ]),
            _ => Ok(Vec::new()),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}

struct FakeApprovalRuntime {
    approvals: Arc<Mutex<Vec<ScriptedApproval>>>,
}

impl AgentRuntime for FakeApprovalRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["anthropic"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: org_agent_runtime::MidTurnDelivery::None,
            image_input: org_agent_runtime::ImageInputMode::StagedFileOnly,
            interrupt_in_band: true,
            idle_interrupt_ends_turn: false,
            approval_bridge: true,
            deny_message_reaches_model: true,
            runtime_cancels_approvals: true,
            unsolicited_turns: false,
            session_id_known_up_front: false,
            resume_ignores_cwd: false,
            resume_applies_instructions: false,
            reads_workspace_instruction_files: false,
            tools_detach_from_group: false,
            take_over: false,
            enforces_writable_roots: false,
            enforces_deny_write: false,
            enforces_deny_read: false,
            enforces_unix_socket_allowlist: false,
            enforces_tool_allowlist: false,
            enforces_deny_rules: false,
            usage_scope: UsageScope::TurnDelta,
            cost: CostBasis::ListPriceEstimate,
            tested_versions: VersionRange {
                min: "1.0.0".into(),
                max: "1.0.0".into(),
            },
        }
    }

    fn plan_session(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
        let rsid = format!("inbox-apr-{}", spec.agent.name.as_str());
        let plan = LaunchPlan {
            program: ctx.runtime_path.clone(),
            args: Vec::new(),
            cwd: spec.workspace.clone(),
            stdio: Stdio::Piped,
            files: Vec::new(),
            dirs: Vec::new(),
            env_add: Vec::new(),
            auth_env_names: Vec::new(),
            secret_env_names: Vec::new(),
            stop: StopPlan {
                steps: vec![
                    StopStep::CloseStdin { grace_ms: 200 },
                    StopStep::Signal {
                        signal: StopSignal::Terminate,
                        target: SignalTarget::Group,
                        grace_ms: 200,
                    },
                ],
            },
            cleanup_after_exit: Vec::new(),
            runtime_session_id: SessionId::try_from(rsid.as_str()).ok(),
        };
        Ok((
            plan,
            Box::new(FakeApprovalSession {
                agent_handle: spec.agent.name.as_str().to_owned(),
                runtime_session_id: rsid,
                approvals: Arc::clone(&self.approvals),
            }),
        ))
    }

    fn plan_take_over(
        &self,
        _spec: &SessionSpec,
        _ctx: &LaunchContext,
        _cols: u16,
        _rows: u16,
    ) -> Result<LaunchPlan, PlanError> {
        Err(PlanError::Unsupported("no takeover"))
    }

    fn plan_probe(&self, _probe: Probe, _ctx: &ProbeContext) -> Option<ProbeRun> {
        None
    }

    fn parse_probe(&self, _probe: Probe, _out: &ProbeOutput) -> ProbeResult {
        ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled,
        }
    }
}

struct QuietFeed;
impl OrgFeed for QuietFeed {
    fn head(&self) -> Result<Position, String> {
        Ok(Position { epoch: 1, seq: 1 })
    }
    fn can_resume_from(&self, _: Position) -> Result<bool, String> {
        Ok(true)
    }
    fn changes_after(&self, _: Position, _: usize) -> Result<Vec<OrgChangedData>, String> {
        Ok(Vec::new())
    }
    fn token(&self) -> u64 {
        0
    }
    fn wait(&self, _: u64, _: Duration) -> u64 {
        0
    }
}

struct ApprovalInboxFixture {
    _dir: tempfile::TempDir,
    office: Arc<Office>,
    owner_id: String,
    root_team_id: String,
    agent_id: String,
    sessions: Arc<SessionManager>,
    live: Arc<LiveEvents>,
    shared: serve::Shared,
    hint: Hint,
    approvals_queue: Arc<Mutex<Vec<ScriptedApproval>>>,
}

impl ApprovalInboxFixture {
    fn new(initial_approvals: Vec<ScriptedApproval>) -> Self {
        let _ = org_platform::process::harden_self();
        let dir = tempfile::Builder::new()
            .prefix("inb-apr")
            .tempdir()
            .unwrap();
        let socket = dir.path().join("office.sock");
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let owner_id = office.owner_member_id().to_owned();
        let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

        let script_path = dir.path().join("fake-claude");
        fs::write(&script_path, SCRIPT_HOLD_OPEN).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

        let approvals_queue = Arc::new(Mutex::new(initial_approvals));
        let mut reg = RuntimeRegistry::empty();
        reg.register(
            Arc::new(FakeApprovalRuntime {
                approvals: Arc::clone(&approvals_queue),
            }),
            Some(script_path),
        );
        let cfg = SessionManagerConfig::with_runtimes(socket.clone(), reg);
        let sessions = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
        SessionManager::attach_self(&sessions);

        let agent_id = hire_agent(&office, &owner_id, &root_team_id, "ada-inbox");

        let wake = Arc::new(crate::org_feed::FeedWake::new());
        let live_wake = Arc::clone(&wake);
        let live = Arc::new(LiveEvents::for_owner(
            Box::new(move || live_wake.notify()),
            owner_id.clone(),
        ));
        follow_session_approvals(&live, &sessions);

        let hq = hq_inbox_with_sessions(Arc::clone(&office), Some(Arc::clone(&sessions))).unwrap();
        let watcher = Arc::new(crate::watcher::ChannelWatcher::production(
            Arc::clone(&office),
            Some(Arc::clone(&sessions)),
            sessions.runtimes().clone(),
        ));
        let hq_fed_slot: crate::federation_runtime::HqFederationSlot = Arc::default();
        let sync_wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let messaging = build_messaging_ports(
            &office,
            &live,
            &wake,
            &sessions,
            &watcher,
            Arc::clone(&sync_wake),
        );
        let ports = build_office_ports(
            Arc::clone(&office),
            Arc::clone(&sessions),
            watcher,
            Arc::clone(&live),
            &wake,
            messaging,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            hq_fed_slot,
            sync_wake,
        );
        let mut shared = serve::Shared::new(
            office.info(),
            &socket,
            ports,
            crate::test_support::grant_key(),
        );
        shared.register_inbox(hq, office.is_hq());
        shared.enable_inbox_hints().unwrap();

        let hint = Hint::new(Position { epoch: 1, seq: 1 }, live.mailbox_head());
        Self {
            _dir: dir,
            office,
            owner_id,
            root_team_id,
            agent_id,
            sessions,
            live,
            shared,
            hint,
            approvals_queue,
        }
    }

    fn start_agent_turn(&self) {
        self.sessions
            .start_session(
                &self.owner_id,
                &MemberSessionStartParams {
                    command_id: new_uuid_v7(),
                    employee_id: self.agent_id.clone(),
                    instruction: Some("Run requested tool".into()),
                    workspace: None,
                },
            )
            .expect("agent session started");
    }

    fn wait_for_approvals(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self
                .sessions
                .pending_approvals_for(&self.owner_id)
                .unwrap()
                .len()
                == expected
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "timed out waiting for {expected} pending approvals; got {:?}",
            self.sessions.pending_approvals_for(&self.owner_id)
        );
    }

    fn inbox_list_as(&self, member: &str, via_grant: bool) -> org_proto::Response {
        let req = Request::new::<InboxList>(
            1,
            &InboxListParams {
                state: InboxState::Open,
                kinds: None,
                project: None,
                team: None,
                cursor: None,
                limit: None,
                has_image: None,
                has_file: None,
            },
        )
        .unwrap();
        self.shared.answer_for_test(&req, member, via_grant)
    }

    fn owner_inbox_page(&self) -> InboxListResult {
        match self.inbox_list_as(&self.owner_id, true).outcome {
            Outcome::Result(val) => serde_json::from_value(val).unwrap(),
            Outcome::Error(err) => panic!("expected owner inbox.list success, got {err:?}"),
        }
    }

    fn poll_inbox_hints(&self, at: Instant) -> Vec<InboxChangedData> {
        let before = self.live.head().messages;
        self.hint.poll(
            self.shared.inbox.as_ref().unwrap(),
            &QuietFeed,
            &self.live,
            || at,
        );
        self.live
            .since(before, self.live.head().messages)
            .events
            .into_iter()
            .filter_map(|ev| match ev {
                Live::InboxChanged(data) => Some(data),
                _ => None,
            })
            .collect()
    }
}

impl Drop for ApprovalInboxFixture {
    fn drop(&mut self) {
        self.sessions.shutdown("test teardown");
    }
}

fn hire_agent(office: &Office, owner_id: &str, team_id: &str, handle: &str) -> String {
    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id.clone(),
            default_profile_id: prof_id.clone(),
            key: format!("{handle}-conn"),
            name: format!("{handle} conn"),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(owner_id), &conn_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));
    office
        .provider_binding_set(
            owner_id,
            ProviderBindingSetParams {
                operation_id: new_uuid_v7(),
                connection_id: conn_id.clone(),
                expected_revision: None,
                auth_context_ref: format!("vault://{conn_id}"),
                runtime_binary_path: None,
                account_identity_ref: None,
            },
        )
        .unwrap()
        .unwrap();
    office
        .provider_connection_refresh(
            owner_id,
            ProviderConnectionRefreshParams {
                connection_id: conn_id,
                driver_version: Some("1.0.0".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: None,
                operator_attested_subscription: false,
                models: vec![DiscoveredModelInput {
                    model_id: "claude-sonnet-4-6".into(),
                    driver_model_id: "claude-sonnet-4-6".into(),
                    entitlement: DiscoveredModelEntitlement::Confirmed,
                    capabilities_json: "{}".into(),
                }],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();

    let emp_id = new_uuid_v7();
    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: emp_id.clone(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: "Ada Inbox".into(),
                handle: handle.into(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner_id.into()),
                office_id: None,
                model_profile_id: Some(prof_id),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("safe_ask".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Work safely.".into()),
                effort: None,
            },
            reason: "inbox approval test".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(owner_id), &hire_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));
    emp_id
}

fn open_query() -> Query<'static> {
    Query {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        after: None,
        has: Default::default(),
        limit: 100,
    }
}

fn approval_decisions(item: &org_proto::inbox::InboxItem) -> Vec<&str> {
    item.actions
        .iter()
        .filter_map(|action| match action {
            Action::Call { op, params, .. } if op == MemberSessionApprovalDecide::METHOD => {
                params.get("decision").and_then(Value::as_str)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn pending_tool_approval_surfaces_in_owner_inbox_with_safe_metadata_and_actions() {
    let bearer_tok = ["tok", "synth", "999"].join("-");
    let url_pass = ["pass", "synth", "42"].join("-");
    let api_key = ["key", "synth", "123"].join("-");
    let private_body = "do_not_leak_private_body";
    let cmd = format!(
        "http-client --auth 'Bearer {bearer_tok}' https://alice:{url_pass}@db.internal/prod"
    );
    let fx = ApprovalInboxFixture::new(vec![
        ScriptedApproval {
            id: "apr-bash-1".into(),
            tool: "Bash".into(),
            escalation: Escalation::None,
            capability: Some(ToolCapability::Shell),
            input: json!({ "command": cmd, "api_key": api_key }),
        },
        ScriptedApproval {
            id: "apr-perm-2".into(),
            tool: "ExitPlanMode".into(),
            escalation: Escalation::PermissionChange,
            capability: None,
            input: json!({ "private_note": private_body }),
        },
    ]);

    let t0 = Instant::now();
    let head_before = fx.live.mailbox_head();
    assert_eq!(fx.owner_inbox_page().counts.approvals, 0);

    fx.start_agent_turn();
    fx.wait_for_approvals(2);

    assert!(fx.live.mailbox_head() > head_before);
    let hints = fx.poll_inbox_hints(t0);
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].counts.approvals, 2);

    let page = fx.owner_inbox_page();
    assert!(page.degraded.is_empty());
    assert_eq!(page.counts.approvals, 2);
    let items: Vec<_> = page
        .items
        .iter()
        .filter(|i| i.source == "tool_approval")
        .collect();
    assert_eq!(items.len(), 2);

    let wire = serde_json::to_string(&items).unwrap();
    for secret in [
        bearer_tok.as_str(),
        url_pass.as_str(),
        api_key.as_str(),
        private_body,
    ] {
        assert!(!wire.contains(secret), "leaked `{secret}` in {wire}");
    }

    let bash_item = items
        .iter()
        .find(|i| i.reference.ends_with(":apr-bash-1"))
        .expect("Bash approval present");
    assert_eq!(bash_item.kind, InboxKind::Approval);
    assert_eq!(bash_item.member.as_deref(), Some(fx.agent_id.as_str()));
    assert_eq!(bash_item.team.as_deref(), Some(fx.root_team_id.as_str()));
    assert_eq!(bash_item.clears, InboxClears::Answered);
    assert!(!bash_item.dismissable);
    assert!(bash_item.created_at_ms.is_some());
    assert!(bash_item.summary.contains("Ada Inbox") && bash_item.summary.contains("Bash"));
    assert!(bash_item.actions.iter().any(|a| matches!(
        a,
        Action::Open { open: Open { target: OpenTarget::Member, id }, .. } if id == &fx.agent_id
    )));
    assert_eq!(
        approval_decisions(bash_item),
        vec!["allow_once", "allow_always", "deny"]
    );

    let perm_item = items
        .iter()
        .find(|i| i.reference.ends_with(":apr-perm-2"))
        .expect("PermissionChange approval present");
    assert_eq!(approval_decisions(perm_item), vec!["allow_once", "deny"]);
}

#[test]
fn approve_deny_and_stop_remove_tool_approvals_and_advance_live_events() {
    let fx = ApprovalInboxFixture::new(vec![
        ScriptedApproval {
            id: "apr-ok-1".into(),
            tool: "Bash".into(),
            escalation: Escalation::None,
            capability: Some(ToolCapability::Shell),
            input: json!({ "command": "echo 1" }),
        },
        ScriptedApproval {
            id: "apr-deny-2".into(),
            tool: "ExitPlanMode".into(),
            escalation: Escalation::PermissionChange,
            capability: None,
            input: json!({}),
        },
    ]);
    let t0 = Instant::now();
    fx.start_agent_turn();
    fx.wait_for_approvals(2);
    let _ = fx.poll_inbox_hints(t0);

    let head_approve = fx.live.mailbox_head();
    fx.sessions
        .decide_approval(&fx.owner_id, &fx.agent_id, "apr-ok-1", "allow_once", None)
        .expect("allow_once accepted");
    assert!(fx.live.mailbox_head() > head_approve);
    assert_eq!(fx.owner_inbox_page().counts.approvals, 1);
    assert_eq!(
        fx.poll_inbox_hints(t0 + Duration::from_secs(1))[0]
            .counts
            .approvals,
        1
    );

    let head_deny = fx.live.mailbox_head();
    fx.sessions
        .decide_approval(
            &fx.owner_id,
            &fx.agent_id,
            "apr-deny-2",
            "deny",
            Some("no permission change".into()),
        )
        .expect("deny accepted");
    assert!(fx.live.mailbox_head() > head_deny);
    assert_eq!(fx.owner_inbox_page().counts.approvals, 0);
    assert_eq!(
        fx.poll_inbox_hints(t0 + Duration::from_secs(2))[0]
            .counts
            .approvals,
        0
    );

    fx.sessions.stop_session(&fx.agent_id, "reset").unwrap();
    fx.approvals_queue.lock().unwrap().push(ScriptedApproval {
        id: "apr-stop-3".into(),
        tool: "Bash".into(),
        escalation: Escalation::None,
        capability: Some(ToolCapability::Shell),
        input: json!({ "command": "echo hi" }),
    });
    fx.start_agent_turn();
    fx.wait_for_approvals(1);
    let _ = fx.poll_inbox_hints(t0 + Duration::from_secs(3));
    assert_eq!(fx.owner_inbox_page().counts.approvals, 1);

    let head_stop = fx.live.mailbox_head();
    fx.sessions
        .stop_session(&fx.agent_id, "owner stopped")
        .expect("session stopped");
    assert!(fx.live.mailbox_head() > head_stop);
    assert_eq!(fx.owner_inbox_page().counts.approvals, 0);
    assert_eq!(
        fx.poll_inbox_hints(t0 + Duration::from_secs(4))[0]
            .counts
            .approvals,
        0
    );
}

#[test]
fn unauthorized_non_owner_person_and_agent_cannot_see_pending_tool_approvals() {
    let fx = ApprovalInboxFixture::new(vec![ScriptedApproval {
        id: "apr-auth-1".into(),
        tool: "Bash".into(),
        escalation: Escalation::None,
        capability: Some(ToolCapability::Shell),
        input: json!({ "command": "echo secret" }),
    }]);

    let bob_id = new_uuid_v7();
    fx.office
        .submit_org(
            &fx.owner_id,
            org_office::rules::OrgCommand::MemberAddPerson(org_proto::org::MemberAddPersonParams {
                command_id: new_uuid_v7(),
                member_id: bob_id.clone(),
                name: "Bob Teammate".into(),
                contact: None,
                handle: Some("bob".into()),
                home_team_id: Some(fx.root_team_id.clone()),
            }),
        )
        .unwrap()
        .unwrap();

    fx.start_agent_turn();
    fx.wait_for_approvals(1);

    // Owner sees the approval.
    let owner_page = fx.owner_inbox_page();
    assert_eq!(owner_page.counts.approvals, 1);
    assert!(owner_page
        .items
        .iter()
        .any(|i| i.reference.ends_with(":apr-auth-1")));

    // Non-owner person (even with grant) and agent are refused at inbox.list RPC dispatch.
    for (caller, via_grant) in [(&bob_id, true), (&fx.agent_id, false)] {
        match fx.inbox_list_as(caller, via_grant).outcome {
            Outcome::Error(err) => assert_eq!(err.code, ErrorCode::NotAuthorized),
            Outcome::Result(val) => {
                panic!("unauthorized caller `{caller}` must never see inbox.list: {val}")
            }
        }
    }

    // Source itself also enforces strict owner-only caller authorization.
    let source = PendingApprovalsInboxSource::new(Arc::clone(&fx.office), Arc::clone(&fx.sessions));
    for unauthorized in [&bob_id, &fx.agent_id] {
        let err = source
            .read_for_caller(unauthorized, &open_query())
            .err()
            .expect("source must refuse non-owner caller");
        assert_eq!(err.code, ErrorCode::NotAuthorized);
    }
}

#[test]
fn crafted_tool_secrets_are_redacted_and_filters_and_keyset_pagination_preserve_counts() {
    let fx = ApprovalInboxFixture::new(vec![
        ScriptedApproval {
            id: "apr-filt-1".into(),
            tool: "mcp__svc__Bearer sk-crafted-secret-777".into(),
            escalation: Escalation::None,
            capability: Some(ToolCapability::Unknown),
            input: json!({}),
        },
        ScriptedApproval {
            id: "apr-filt-2".into(),
            tool: "Bash".into(),
            escalation: Escalation::None,
            capability: Some(ToolCapability::Shell),
            input: json!({}),
        },
    ]);

    // Link root_team_id to a project so project filtering is exercised.
    let project_id = new_uuid_v7();
    fx.office
        .submit_org(
            &fx.owner_id,
            org_office::rules::OrgCommand::ProjectCreate(org_proto::org::ProjectCreateParams {
                command_id: new_uuid_v7(),
                project_id: project_id.clone(),
                name: "Core".into(),
                key: None,
                description: None,
                teams: Vec::new(),
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some("core".into()),
                owner_team_id: Some(fx.root_team_id.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: Vec::new(),
            }),
        )
        .unwrap()
        .unwrap();

    fx.start_agent_turn();
    fx.wait_for_approvals(2);

    let source = PendingApprovalsInboxSource::new(Arc::clone(&fx.office), Arc::clone(&fx.sessions));
    assert_eq!(source.name(), "tool_approvals");

    let all = source.read(&open_query()).unwrap();
    assert_eq!(
        all.counts,
        InboxCounts {
            approvals: 2,
            total: 2,
            ..InboxCounts::default()
        }
    );
    assert_eq!(all.items.len(), 2);
    let wire = serde_json::to_string(&all.items).unwrap();
    assert!(
        !wire.contains("sk-crafted-secret-777"),
        "crafted secret in tool name must be redacted: {wire}"
    );

    // Filters narrow items without altering unfiltered open counts.
    let handled = source
        .read(&Query {
            state: InboxState::Handled,
            ..open_query()
        })
        .unwrap();
    assert!(handled.items.is_empty());
    assert_eq!(handled.counts.approvals, 2);

    let questions_only = source
        .read(&Query {
            kinds: Some(&[InboxKind::Question]),
            ..open_query()
        })
        .unwrap();
    assert!(questions_only.items.is_empty());
    assert_eq!(questions_only.counts.approvals, 2);

    let matching_team = source
        .read(&Query {
            team: Some(&fx.root_team_id),
            project: Some(&project_id),
            ..open_query()
        })
        .unwrap();
    assert_eq!(matching_team.items.len(), 2);

    let other_team = source
        .read(&Query {
            team: Some("other-team"),
            ..open_query()
        })
        .unwrap();
    assert!(other_team.items.is_empty());
    assert_eq!(other_team.counts.approvals, 2);

    let has_image = source
        .read(&Query {
            has: org_proto::attachments::AttachmentFilter::new(Some(true), None),
            ..open_query()
        })
        .unwrap();
    assert!(has_image.items.is_empty());
    assert_eq!(has_image.counts.approvals, 2);

    // Keyset pagination yields both items without duplicates or gaps.
    let page1 = source
        .read(&Query {
            limit: 1,
            ..open_query()
        })
        .unwrap();
    assert_eq!(page1.items.len(), 1);
    let after = Keyset::of(&page1.items[0]);
    let page2 = source
        .read(&Query {
            after: Some(&after),
            limit: 1,
            ..open_query()
        })
        .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert_ne!(page1.items[0].reference, page2.items[0].reference);
}
