//! Integration tests for `TASK-48` (`C9`: Channel question watcher & `channel summarize`,
//! `ADR-0024`, `WATCH-1`, `WATCH-3..8`, `WATCH-10`).

use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::chat_calls::ChatService;
use crate::watcher_store::ChannelWatermark;
use org_agent_runtime::{
    encode_line, AgentRuntime, Capabilities, Control, ControlError, CostBasis, ExitInfo,
    LaunchContext, LaunchPlan, Output, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult,
    ProbeRun, RuntimeId, Session, SessionEvent, SessionId, SessionSpec, SignalTarget, Stdio,
    StopPlan, StopSignal, StopStep, TurnOutcome, Unavailable, UsageScope, VersionRange,
};
use org_copilot::watcher::DERIVED_SUMMARY_NOTICE;
use org_copilot::{CopilotRuntime, FakeRuntime, Limits, Reply};
use org_office::organization_family::records::{ModelProfile, ProviderConnection};
use org_office::{Office, TestOffice};
use org_office_sessions::{RuntimeRegistry, SessionManager, SessionManagerConfig};
use org_proto::ask::{AskQuestion, AskQuestionType, ChatAsk, ChatAskParams, ChatAskResult};
use org_proto::chat::{
    ChatChannelCreate, ChatChannelCreateParams, ChatChannelLeave, ChatChannelMembershipParams,
    ChatChannelSummarize, ChatChannelSummarizeParams, ChatChannelSummarizeResult, ChatInbox,
    ChatInboxParams, ChatInboxResult, ChatSend, ChatSendParams, ChatWatchingModelGet,
    ChatWatchingModelGetParams, ChatWatchingModelResult, ChatWatchingModelSet,
    ChatWatchingModelSetParams, DeliveredBecause,
};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::msg::{MsgDone, MsgDoneParams};
use org_proto::sessions::MemberSessionStartParams;
use org_proto::{ErrorCode, Outcome, Request};
use serde_json::json;

use super::summarize::{record_usage, UsageContext};
use super::{ChannelWatcher, PassReport, WatcherConfig, WatcherRuntimeProvider};

mod catchup;

fn next_id() -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(9000);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

struct ScriptedProvider {
    replies: Mutex<VecDeque<Reply>>,
    /// Every runtime handed out, so a test can read the prompts a run was given.
    runtimes: Mutex<Vec<Arc<FakeRuntime>>>,
}

impl ScriptedProvider {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().collect()),
            runtimes: Mutex::new(Vec::new()),
        })
    }

    fn push(&self, reply: Reply) {
        self.replies.lock().unwrap().push_back(reply);
    }

    /// The prompts of every run so far, oldest first.
    fn prompts(&self) -> Vec<String> {
        self.runtimes
            .lock()
            .unwrap()
            .iter()
            .flat_map(|runtime| runtime.prompts())
            .collect()
    }
}

impl WatcherRuntimeProvider for ScriptedProvider {
    fn runtime_for_profile(
        &self,
        _profile: &ModelProfile,
        _connection: Option<&ProviderConnection>,
    ) -> Option<Arc<dyn CopilotRuntime>> {
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Reply::Text(r#"{"draft":{"matches":[]},"notes":[]}"#.into()));
        let runtime = Arc::new(FakeRuntime::new([reply]));
        self.runtimes.lock().unwrap().push(Arc::clone(&runtime));
        Some(runtime)
    }
}

struct WatcherFixture {
    office: Arc<Office>,
    owner_id: String,
    profile_id: String,
    ada: String,
    ben: String,
    cy: String,
    provider: Arc<ScriptedProvider>,
    watcher: Arc<ChannelWatcher>,
    chat: ChatService,
}

impl WatcherFixture {
    fn new(config: WatcherConfig, enable_model: bool) -> Self {
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let owner_id = office.owner_member_id().to_owned();
        let snap = office.org_snapshot().unwrap();
        let root_team = snap.teams[0].id.clone();

        let conn_id = next_id();
        let profile_id = next_id();
        let conn_req = Request::new::<ProviderConnectionCreate>(
            1,
            &ProviderConnectionCreateParams {
                command_id: next_id(),
                connection_id: conn_id.clone(),
                default_profile_id: profile_id.clone(),
                key: "watcher-haiku".into(),
                name: "Watcher Haiku".into(),
                provider_type: "anthropic".into(),
                auth_mode: "api_key".into(),
                subscription_identity: None,
                runtime_driver: "claude-code".into(),
                endpoint_url: None,
                max_concurrent_turns: 4,
            },
        )
        .unwrap();
        let _ = office.dispatch_hiring_rpc(Some(&owner_id), &conn_req);
        office
            .provider_binding_set(
                &owner_id,
                org_proto::providers::ProviderBindingSetParams {
                    operation_id: next_id(),
                    connection_id: conn_id.clone(),
                    expected_revision: None,
                    auth_context_ref: "vault://watcher-haiku".into(),
                    runtime_binary_path: None,
                    account_identity_ref: None,
                },
            )
            .unwrap()
            .unwrap();
        office
            .provider_connection_refresh(
                &owner_id,
                org_proto::providers::ProviderConnectionRefreshParams {
                    connection_id: conn_id,
                    driver_version: Some("1.0.0".into()),
                    auth_signed_in: Some(true),
                    observed_subscription_identity: None,
                    operator_attested_subscription: false,
                    models: vec![org_proto::providers::DiscoveredModelInput {
                        model_id: "claude-sonnet-4-6".into(),
                        driver_model_id: "claude-sonnet-4-6".into(),
                        entitlement: org_proto::providers::DiscoveredModelEntitlement::Confirmed,
                        capabilities_json: "{}".into(),
                    }],
                    issue_code: None,
                    ttl_seconds: Some(300),
                },
            )
            .unwrap()
            .unwrap();

        let ada = Self::hire(&office, &owner_id, &root_team, &profile_id, "Ada", "ada");
        let ben = Self::hire(&office, &owner_id, &root_team, &profile_id, "Ben", "ben");
        let cy = Self::hire(&office, &owner_id, &root_team, &profile_id, "Cy", "cy");

        if enable_model {
            office
                .set_watching_model_setting(&owner_id, &next_id(), Some(&profile_id))
                .unwrap()
                .unwrap();
        }

        let provider = ScriptedProvider::new([]);
        let watcher = Arc::new(ChannelWatcher::new(
            Arc::clone(&office),
            None,
            provider.clone(),
            config,
        ));
        let live = Arc::new(crate::live::LiveEvents::new(Box::new(|| {})));
        let dir = Arc::new(crate::directory::OfficeDirectory::of(Arc::clone(&office)));
        let mb_store = Arc::new(crate::mailbox_store::OfficeMailboxStore::new(Arc::clone(
            &office,
        )));
        let mailbox = Arc::new(
            org_mailbox::Mailbox::with_triage(
                Arc::clone(&mb_store) as Arc<dyn org_mailbox::MessageStore + Send + Sync>,
                mb_store as Arc<dyn org_mailbox::TriageStore + Send + Sync>,
                dir,
                org_mailbox::OfficeIdentity {
                    office_id: office.info().office_id.clone(),
                    incarnation: office.journal_incarnation().to_owned(),
                    is_hq: true,
                },
                Arc::new(org_mailbox::SystemClock),
                Arc::clone(&live) as Arc<dyn org_mailbox::MessageEvents>,
            )
            .expect("mailbox with triage"),
        );
        let chat = ChatService::with_watcher(
            Arc::clone(&office),
            live,
            mailbox,
            None,
            Arc::clone(&watcher),
        );
        Self {
            office,
            owner_id,
            profile_id,
            ada,
            ben,
            cy,
            provider,
            watcher,
            chat,
        }
    }

    fn hire(
        office: &Office,
        owner: &str,
        team: &str,
        profile_id: &str,
        name: &str,
        handle: &str,
    ) -> String {
        let req = Request::new::<HrHire>(
            2,
            &HrHireParams {
                command_id: next_id(),
                request_id: next_id(),
                employee_id: next_id(),
                home_team_id: team.to_owned(),
                role_id: None,
                proposal: HireProposalWire {
                    name: name.into(),
                    handle: handle.into(),
                    title: Some("Engineer".into()),
                    focus: Some("Backend".into()),
                    role_text: None,
                    title_in_team: None,
                    reports_to_employee_id: Some(owner.to_owned()),
                    office_id: Some(office.info().office_id.clone()),
                    model_profile_id: Some(profile_id.to_owned()),
                    model_id: Some("claude-sonnet-4-6".into()),
                    permission_preset: Some("safe_ask".into()),
                    tools_config: Vec::new(),
                    system_prompt_markdown: Some("Be helpful.".into()),
                    body: None,
                    effort: None,
                },
                reason: "test".into(),
            },
        )
        .unwrap();
        let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
        match resp.outcome {
            Outcome::Result(v) => v["employee_id"].as_str().unwrap().to_owned(),
            Outcome::Error(e) => panic!("hire failed: {e:?}"),
        }
    }

    fn ask_channel(&self, member: &str, channel: &str, prompt: &str) -> ChatAskResult {
        let stopping = std::sync::atomic::AtomicBool::new(false);
        let req = Request::new::<ChatAsk>(
            10,
            &ChatAskParams {
                id: next_id(),
                to: channel.to_owned(),
                subject: None,
                body: Some(prompt.to_owned()),
                meta: Default::default(),
                questions: vec![AskQuestion {
                    id: "q1".into(),
                    question_type: AskQuestionType::FreeText,
                    prompt: prompt.to_owned(),
                    options: vec![],
                    allow_multiple: false,
                    default_value: None,
                    optional: false,
                }],
                required: false,
                wait_ms: Some(60_000),
                hold_ms: Some(0),
                blocks: None,
                approval: false,
                addressed_to: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap();
        let resp = self
            .chat
            .dispatch(Some(member), false, &stopping, &req)
            .unwrap();
        match resp.outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("chat.ask failed: {e:?}"),
        }
    }

    fn send_channel(&self, member: &str, channel: &str, body: &str) -> String {
        let stopping = std::sync::atomic::AtomicBool::new(false);
        let id = next_id();
        let req = Request::new::<ChatSend>(
            11,
            &ChatSendParams {
                id: id.clone(),
                to: channel.to_owned(),
                kind: "msg".into(),
                priority: None,
                subject: None,
                body: body.to_owned(),
                meta: Default::default(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap();
        let resp = self
            .chat
            .dispatch(Some(member), false, &stopping, &req)
            .unwrap();
        match resp.outcome {
            Outcome::Result(_) => id,
            Outcome::Error(e) => panic!("chat.send failed: {e:?}"),
        }
    }

    fn inbox(&self, member: &str) -> ChatInboxResult {
        let stopping = std::sync::atomic::AtomicBool::new(false);
        let req = Request::new::<ChatInbox>(
            12,
            &ChatInboxParams {
                include_all: true,
                state: None,
                because: None,
                from: None,
                thread_id: None,
                cursor: None,
                limit: Some(50),
                has_file: None,
                has_image: None,
            },
        )
        .unwrap();
        let resp = self
            .chat
            .dispatch(Some(member), false, &stopping, &req)
            .unwrap();
        match resp.outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("chat.inbox failed: {e:?}"),
        }
    }
}

#[test]
fn watch_3_three_agents_one_pass_delivers_answer_candidates_and_records_one_off_usage() {
    let fx = WatcherFixture::new(WatcherConfig::default(), true);

    let q_ada = fx.ask_channel(&fx.ada, "#general", "Which Postgres version?");
    let q_ben = fx.ask_channel(&fx.ben, "#general", "Is dark mode in v1?");
    let q_cy = fx.ask_channel(&fx.cy, "#general", "Which S3 region for staging?");
    assert_eq!(q_ada.watched, Some(true));
    assert_eq!(q_ben.watched, Some(true));
    assert_eq!(q_cy.watched, Some(true));
    let conv_id = q_ada.conversation_id.clone();

    let m_noise = fx.send_channel(&fx.owner_id, "#general", "Morning everyone!");
    let m_ada_self = fx.send_channel(&fx.ada, "#general", "Checking Postgres myself");
    let m_ans_1 = fx.send_channel(&fx.owner_id, "#general", "Postgres 16.4 and eu-west-1.");
    let m_ans_2 = fx.send_channel(&fx.owner_id, "#general", "Dark mode is deferred to v2.");

    // Script ONE pass matching all three agents (plus an asker self-match that must be dropped).
    fx.provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [
                    { "message_id": m_ans_1, "question_id": q_ada.ask_id, "confident": true },
                    { "message_id": m_ans_1, "question_id": q_cy.ask_id, "confident": true },
                    { "message_id": m_ans_2, "question_id": q_ben.ask_id, "confident": false },
                    { "message_id": m_ada_self, "question_id": q_ada.ask_id, "confident": true }
                ]
            },
            "notes": []
        })
        .to_string(),
    ));

    let report = fx.watcher.run_channel_pass(&conv_id, true);
    assert_eq!(
        report,
        PassReport::Delivered {
            judged_messages: 4,
            delivered_entries: 3,
            woke_agents: Vec::new(),
            skipped_secrets: 0,
        }
    );

    let ada_inbox = fx.inbox(&fx.ada);
    let ada_entry = ada_inbox
        .items
        .iter()
        .find(|i| i.id == m_ans_1)
        .expect("Ada receives answer_candidate");
    assert_eq!(
        ada_entry.delivered_because,
        DeliveredBecause::AnswerCandidate
    );
    assert_eq!(ada_entry.related_message_ids, vec![q_ada.ask_id.clone()]);
    assert!(ada_entry
        .reason
        .as_deref()
        .is_some_and(|r| r.contains("Postgres")));
    // Unrelated noise and Ada's own message never reach Ada's inbox.
    assert!(!ada_inbox.items.iter().any(|i| i.id == m_noise));
    assert!(!ada_inbox.items.iter().any(|i| i.id == m_ada_self));

    let ben_inbox = fx.inbox(&fx.ben);
    assert!(ben_inbox.items.iter().any(|i| i.id == m_ans_2));
    assert!(!ben_inbox.items.iter().any(|i| i.id == m_noise));

    let cy_inbox = fx.inbox(&fx.cy);
    assert!(cy_inbox.items.iter().any(|i| i.id == m_ans_1));

    // One `one_off` usage record naming all 3 open questions (WATCH-6, ADR-0024 d15).
    let usage = fx.watcher.store().read_usage_records();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].consumer_kind, "one_off");
    assert!(usage[0].employee_id.is_none());
    assert_eq!(usage[0].question_ids.len(), 3);
}

#[test]
fn watch_4_secret_skipped_fabricated_id_fails_closed_and_3_failures_advance() {
    let fx = WatcherFixture::new(WatcherConfig::default(), true);
    let q = fx.ask_channel(&fx.ada, "#general", "What auth mode for CI?");
    let conv_id = q.conversation_id.clone();

    let m_secret = fx.send_channel(
        &fx.owner_id,
        "#general",
        "oops token ghp_1234567890abcdefghij1234567890abcd",
    );
    let m_good = fx.send_channel(&fx.owner_id, "#general", "Use FakeRuntime in CI.");

    // 1. Fabricated ID fails the pass, delivers nothing, records usage, does NOT advance yet.
    fx.provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [
                    { "message_id": m_good, "question_id": q.ask_id, "confident": true },
                    { "message_id": "invented-id", "question_id": q.ask_id, "confident": true }
                ]
            },
            "notes": []
        })
        .to_string(),
    ));
    let r1 = fx.watcher.run_channel_pass(&conv_id, true);
    assert!(matches!(
        r1,
        PassReport::FailedRetrying {
            consecutive_failures: 1,
            error: org_copilot::watcher::WatchError::InvalidReference
        }
    ));
    assert!(!fx.inbox(&fx.ada).items.iter().any(|i| i.id == m_good));
    assert_eq!(fx.watcher.store().read_usage_records().len(), 1);

    // 2. Retry succeeds on the same batch; secret message skipped without failing the pass.
    fx.provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [
                    { "message_id": m_good, "question_id": q.ask_id, "confident": true }
                ]
            },
            "notes": []
        })
        .to_string(),
    ));
    let r2 = fx.watcher.run_channel_pass(&conv_id, true);
    assert_eq!(
        r2,
        PassReport::Delivered {
            judged_messages: 2,
            delivered_entries: 1,
            woke_agents: Vec::new(),
            skipped_secrets: 1,
        }
    );
    assert!(fx.inbox(&fx.ada).items.iter().any(|i| i.id == m_good));
    assert!(!fx.inbox(&fx.ada).items.iter().any(|i| i.id == m_secret));

    // 3. Three consecutive failures on a new message advance the watermark past it (ADR-0024 d9).
    let m_poison = fx.send_channel(&fx.owner_id, "#general", "Triggering bad model output");
    for _ in 0..3 {
        fx.provider
            .push(Reply::Text("not valid json at all".into()));
    }
    assert!(matches!(
        fx.watcher.run_channel_pass(&conv_id, true),
        PassReport::FailedRetrying {
            consecutive_failures: 1,
            ..
        }
    ));
    assert!(matches!(
        fx.watcher.run_channel_pass(&conv_id, true),
        PassReport::FailedRetrying {
            consecutive_failures: 2,
            ..
        }
    ));
    assert!(matches!(
        fx.watcher.run_channel_pass(&conv_id, true),
        PassReport::FailedAdvanced { skipped_message_ids, .. } if skipped_message_ids == vec![m_poison]
    ));
    // Next pass sees no unjudged messages.
    assert_eq!(
        fx.watcher.run_channel_pass(&conv_id, true),
        PassReport::Idle
    );
}

#[test]
fn watch_5_and_6_unwatched_without_model_sixth_limit_and_lifecycle_closures() {
    let fx = WatcherFixture::new(WatcherConfig::default(), false);
    let stopping = std::sync::atomic::AtomicBool::new(false);

    // 1. No watching model set -> enabled: false, ask says unwatched, non-owner set refused (WATCH-5).
    let get_req = Request::new::<ChatWatchingModelGet>(20, &ChatWatchingModelGetParams {}).unwrap();
    let get_resp = fx
        .chat
        .dispatch(Some(&fx.ada), false, &stopping, &get_req)
        .unwrap();
    let view: ChatWatchingModelResult = match get_resp.outcome {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("get failed: {e:?}"),
    };
    assert!(!view.enabled);

    let non_owner_set = Request::new::<ChatWatchingModelSet>(
        21,
        &ChatWatchingModelSetParams {
            command_id: next_id(),
            model_profile_id: Some(fx.profile_id.clone()),
        },
    )
    .unwrap();
    let denied = fx
        .chat
        .dispatch(Some(&fx.ada), false, &stopping, &non_owner_set)
        .unwrap();
    assert!(matches!(
        denied.outcome,
        Outcome::Error(ref e) if e.code == ErrorCode::NotAuthorized
    ));

    let unwatched = fx.ask_channel(&fx.ada, "#general", "Unwatched question?");
    assert_eq!(unwatched.watched, Some(false));
    assert_eq!(
        unwatched.watch_not_started_reason.as_deref(),
        Some("no_watching_model")
    );
    assert_eq!(
        fx.watcher
            .run_channel_pass(&unwatched.conversation_id, true),
        PassReport::Skipped("no_watching_model")
    );

    // 2. Owner without host grant is refused (Low #13); owner with grant enables watching model;
    // 5 watches open, 6th posts unwatched (WATCH-6).
    let owner_no_grant = Request::new::<ChatWatchingModelSet>(
        220,
        &ChatWatchingModelSetParams {
            command_id: next_id(),
            model_profile_id: Some(fx.profile_id.clone()),
        },
    )
    .unwrap();
    let denied_owner = fx
        .chat
        .dispatch(Some(&fx.owner_id), false, &stopping, &owner_no_grant)
        .unwrap();
    assert!(matches!(
        denied_owner.outcome,
        Outcome::Error(ref e) if e.code == ErrorCode::NotAuthorized
    ));

    let owner_set = Request::new::<ChatWatchingModelSet>(
        22,
        &ChatWatchingModelSetParams {
            command_id: next_id(),
            model_profile_id: Some(fx.profile_id.clone()),
        },
    )
    .unwrap();
    let ok = fx
        .chat
        .dispatch(Some(&fx.owner_id), true, &stopping, &owner_set)
        .unwrap();
    let enabled_view: ChatWatchingModelResult = match ok.outcome {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("owner set failed: {e:?}"),
    };
    assert!(enabled_view.enabled);

    let mut ask_ids = Vec::new();
    for idx in 1..=5 {
        let r = fx.ask_channel(&fx.ada, "#general", &format!("Question {idx}?"));
        assert_eq!(r.watched, Some(true), "watch {idx} must open");
        ask_ids.push(r.ask_id);
    }
    let sixth = fx.ask_channel(&fx.ada, "#general", "Sixth question?");
    assert_eq!(sixth.watched, Some(false));
    assert_eq!(
        sixth.watch_not_started_reason.as_deref(),
        Some("watch_limit_reached")
    );
}

#[test]
fn watch_6_msg_done_and_membership_lifecycle_closures() {
    let fx = WatcherFixture::new(WatcherConfig::default(), true);
    let stopping = std::sync::atomic::AtomicBool::new(false);
    let mut ask_ids = Vec::new();
    for idx in 1..=5 {
        let r = fx.ask_channel(&fx.ada, "#general", &format!("Lifecycle Q{idx}?"));
        assert_eq!(r.watched, Some(true));
        ask_ids.push(r.ask_id);
    }

    // 1. High #1: another agent's `msg.done` must NOT close Ada's watch; Ada's own `msg.done` DOES.
    let ben_done = Request::new::<MsgDone>(
        221,
        &MsgDoneParams {
            id: ask_ids[0].clone(),
            note: None,
        },
    )
    .unwrap();
    let _ = fx.chat.dispatch(Some(&fx.ben), false, &stopping, &ben_done);
    assert!(
        fx.watcher
            .store()
            .read_watches()
            .watches
            .iter()
            .any(|w| w.question_id == ask_ids[0] && w.is_open()),
        "non-asker msg.done must not close asker's watch"
    );
    let ada_done = Request::new::<MsgDone>(
        222,
        &MsgDoneParams {
            id: ask_ids[0].clone(),
            note: None,
        },
    )
    .unwrap();
    let ada_done_resp = fx
        .chat
        .dispatch(Some(&fx.ada), false, &stopping, &ada_done)
        .unwrap();
    assert!(
        matches!(ada_done_resp.outcome, Outcome::Result(_)),
        "asker msg.done on own watched question must succeed: {ada_done_resp:?}"
    );
    assert!(
        fx.watcher
            .store()
            .read_watches()
            .watches
            .iter()
            .any(|w| w.question_id == ask_ids[0] && !w.is_open()),
        "asker msg.done must close asker's watch"
    );

    // 4. Leaving manual channel / offboard / move closes watches and records notices.
    let create_ch = Request::new::<ChatChannelCreate>(
        23,
        &ChatChannelCreateParams {
            id: None,
            name: "ops-watch".into(),
            members: vec![fx.ada.clone()],
        },
    )
    .unwrap();
    let ch_resp = fx
        .chat
        .dispatch(Some(&fx.owner_id), false, &stopping, &create_ch)
        .unwrap();
    let ch_id = match ch_resp.outcome {
        Outcome::Result(v) => v["channel"]["id"].as_str().unwrap().to_owned(),
        Outcome::Error(e) => panic!("channel create failed: {e:?}"),
    };
    let ops_ask = fx.ask_channel(&fx.ada, "#ops-watch", "Ops question?");
    assert_eq!(ops_ask.watched, Some(true));
    let leave_req = Request::new::<ChatChannelLeave>(
        24,
        &ChatChannelMembershipParams {
            channel: ch_id.clone(),
        },
    )
    .unwrap();
    let _ = fx
        .chat
        .dispatch(Some(&fx.ada), false, &stopping, &leave_req)
        .unwrap();
    let notices = fx.watcher.store().drain_notices_for(&fx.ada);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].reason, "left_channel");

    fx.watcher.on_agent_moved(&fx.ada);
    let move_notices = fx.watcher.store().drain_notices_for(&fx.ada);
    assert_eq!(move_notices.len(), 4);
    assert!(move_notices.iter().all(|n| n.reason == "moved_office"));
}

#[test]
fn watch_7_restart_survival_crash_replay_idempotence_and_multi_origin_watermarks() {
    let fx = WatcherFixture::new(WatcherConfig::default(), true);
    let q = fx.ask_channel(&fx.ada, "#general", "Does state survive restart?");
    let conv_id = q.conversation_id.clone();
    let ans = fx.send_channel(&fx.owner_id, "#general", "Yes, persisted on disk.");

    // Drop watcher and build a brand-new watcher instance on the same office store (restart).
    let restarted_provider = ScriptedProvider::new([Reply::Text(
        json!({
            "draft": {
                "matches": [{ "message_id": ans, "question_id": q.ask_id, "confident": true }]
            },
            "notes": []
        })
        .to_string(),
    )]);
    let restarted = ChannelWatcher::new(
        Arc::clone(&fx.office),
        None,
        restarted_provider.clone(),
        WatcherConfig::default(),
    );
    let report = restarted.run_channel_pass(&conv_id, true);
    assert_eq!(
        report,
        PassReport::Delivered {
            judged_messages: 1,
            delivered_entries: 1,
            woke_agents: Vec::new(),
            skipped_secrets: 0,
        }
    );

    // Simulate crash before watermark commit by resetting watermarks to 0 and re-running the pass:
    // inbox delivery is idempotent with zero duplicate entries (WATCH-1, WATCH-7).
    let mut wm = restarted.store().read_watermarks();
    wm.channels.clear();
    restarted.store().write_watermarks(&wm).unwrap();
    restarted_provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [{ "message_id": ans, "question_id": q.ask_id, "confident": true }]
            },
            "notes": []
        })
        .to_string(),
    ));
    let replay_report = restarted.run_channel_pass(&conv_id, true);
    assert_eq!(
        replay_report,
        PassReport::Delivered {
            judged_messages: 1,
            delivered_entries: 0,
            woke_agents: Vec::new(),
            skipped_secrets: 0,
        }
    );
    let matching: Vec<_> = fx
        .inbox(&fx.ada)
        .items
        .into_iter()
        .filter(|i| i.id == ans)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "message must appear exactly once in inbox"
    );

    // Multi-origin watermarks track (origin_office_id, origin_incarnation) independently (WATCH-7).
    let mut multi = ChannelWatermark::default();
    multi.advance("office-a", "inc-1", 42);
    assert_eq!(multi.last_seq("office-a", "inc-1"), 42);
    assert_eq!(multi.last_seq("office-b", "inc-1"), 0);
    assert_eq!(multi.last_seq("office-a", "inc-2"), 0);
    multi.advance("office-b", "inc-1", 7);
    assert_eq!(multi.last_seq("office-b", "inc-1"), 7);
    assert_eq!(multi.last_seq("office-a", "inc-1"), 42);
}

#[test]
fn watch_8_and_10_summarize_and_confident_person_only_wake_rule() {
    let cfg = WatcherConfig {
        cadence: Duration::ZERO,
        limits: Limits::default(),
        ..WatcherConfig::default()
    };
    let fx = WatcherFixture::new(cfg, true);
    let stopping = std::sync::atomic::AtomicBool::new(false);

    // 1. WATCH-10: channel.summarize returns summary, cited message_ids, derived_notice, and
    // records caller-attributed `employee` usage.
    let m1 = fx.send_channel(&fx.owner_id, "#general", "Postgres 16.4 is the standard.");
    let m2 = fx.send_channel(&fx.ben, "#general", "Noted, updating Cargo.toml.");
    fx.provider.push(Reply::Text(
        json!({
            "draft": {
                "summary": "The team standardized on Postgres 16.4.",
                "message_ids": [m1, m2]
            },
            "notes": []
        })
        .to_string(),
    ));
    let sum_req = Request::new::<ChatChannelSummarize>(
        30,
        &ChatChannelSummarizeParams {
            channel: "#general".into(),
            question: "What Postgres version did the team pick?".into(),
            limit: Some(50),
            since_read: false,
        },
    )
    .unwrap();
    let sum_resp = fx
        .chat
        .dispatch(Some(&fx.ada), false, &stopping, &sum_req)
        .unwrap();
    let sum_res: ChatChannelSummarizeResult = match sum_resp.outcome {
        Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        Outcome::Error(e) => panic!("summarize failed: {e:?}"),
    };
    assert!(sum_res.summary.contains("Postgres 16.4"));
    assert_eq!(sum_res.message_ids, vec![m1.clone(), m2.clone()]);
    assert_eq!(sum_res.derived_notice, DERIVED_SUMMARY_NOTICE);
    assert!(sum_res.usage_record_id.is_some());

    let usage = fx.watcher.store().read_usage_records();
    let entry = usage.last().expect("summarize usage recorded");
    assert_eq!(entry.consumer_kind, "employee");
    assert_eq!(entry.employee_id.as_deref(), Some(fx.ada.as_str()));
    assert_eq!(entry.feature, "channel.summarize");
    // When watcher provider omits token usage (`usage: None`), accounting ledger records Unavailable (`Pass 2 Low #7`).
    let _ = record_usage(
        &fx.office,
        fx.watcher.store(),
        UsageContext {
            consumer_kind: "office",
            employee_id: None,
            project_id: None,
            conversation_id: "conv-no-usage",
            feature: "watcher.no_usage_probe",
            model_profile_id: None,
            question_ids: Vec::new(),
            outcome: "ok",
            usage: None,
            duration: Duration::from_millis(5),
        },
    )
    .unwrap();
    let q_res = fx
        .office
        .cost_query(
            &fx.owner_id,
            org_proto::costs::CostQueryParams {
                include_records: true,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    let ledger_rec = q_res
        .records
        .iter()
        .find(|r| r.runtime == "watcher.no_usage_probe")
        .expect("watcher no-usage record mirrored to office accounting ledger");
    assert_eq!(
        ledger_rec.token_status,
        org_proto::costs::CompletenessStatus::Unavailable
    );
    assert_eq!(ledger_rec.cache_read_tokens, None);
    assert_eq!(ledger_rec.cache_write_tokens, None);
    assert_eq!(ledger_rec.reasoning_tokens, None);

    // 2. WATCH-8: verify confident-match wake filter rejects unconfident and agent-authored
    // matches (and passes only confident person-authored matches).
    let q = fx.ask_channel(&fx.ada, "#general", "Ready to deploy?");
    let conv_id = q.conversation_id.clone();
    let m_agent = fx.send_channel(&fx.ben, "#general", "Yes, agent says ready.");
    let m_person_unconf = fx.send_channel(&fx.owner_id, "#general", "Maybe later today.");
    fx.provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [
                    { "message_id": m_agent, "question_id": q.ask_id, "confident": true },
                    { "message_id": m_person_unconf, "question_id": q.ask_id, "confident": false }
                ]
            },
            "notes": []
        })
        .to_string(),
    ));
    let rep = fx.watcher.run_channel_pass(&conv_id, true);
    assert_eq!(
        rep,
        PassReport::Delivered {
            judged_messages: 2,
            delivered_entries: 2,
            woke_agents: Vec::new(),
            skipped_secrets: 0,
        }
    );

    // 3. Medium #9: asker-only follow-up message advances watermark without calling the model.
    let before_usage = fx.watcher.store().read_usage_records().len();
    let _self_msg = fx.send_channel(&fx.ada, "#general", "Still checking on my side");
    assert_eq!(
        fx.watcher.run_channel_pass(&conv_id, true),
        PassReport::Idle
    );
    assert_eq!(
        fx.watcher.store().read_usage_records().len(),
        before_usage,
        "asker-only self-message must not invoke watching model"
    );
}

#[derive(Default)]
struct WatchRecordedTurns {
    turns: Mutex<Vec<String>>,
}

struct WatchScriptedRuntime {
    recorded: Arc<WatchRecordedTurns>,
}

struct WatchScriptedSession {
    recorded: Arc<WatchRecordedTurns>,
    runtime_session_id: String,
}

impl Session for WatchScriptedSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("ready") => vec![
                Output::Event(SessionEvent::SessionStarted {
                    runtime_session_id: SessionId::try_from(self.runtime_session_id.as_str())
                        .unwrap(),
                }),
                Output::Event(SessionEvent::Ready {
                    runtime_version: Some("1.0.0".into()),
                    pid: None,
                    agents: vec!["ada".into()],
                    permission_mode: Some("default".into()),
                }),
            ],
            Some("reply") => vec![
                Output::Event(SessionEvent::AssistantText {
                    message_id: Some("m1".into()),
                    text: v["text"].as_str().unwrap_or("ack").to_owned(),
                }),
                Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Completed,
                    errors: Vec::new(),
                }),
            ],
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
            Control::Send { text, .. } => {
                self.recorded.turns.lock().unwrap().push(text.clone());
                let line =
                    encode_line(&json!({ "text": text })).map_err(|_| ControlError::Closed)?;
                Ok(vec![
                    Output::Event(SessionEvent::TurnStarted {
                        turn_id: Some("t1".into()),
                        solicited: true,
                    }),
                    Output::Write(line),
                ])
            }
            Control::Close => Ok(vec![Output::CloseStdin]),
            _ => Ok(Vec::new()),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}

impl AgentRuntime for WatchScriptedRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }
    fn binary_name(&self) -> &'static str {
        "watch-scripted"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["anthropic", "claude-code"]
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mid_turn_delivery: org_agent_runtime::MidTurnDelivery::None,
            image_input: org_agent_runtime::ImageInputMode::StagedFileOnly,
            interrupt_in_band: false,
            idle_interrupt_ends_turn: false,
            approval_bridge: false,
            deny_message_reaches_model: false,
            runtime_cancels_approvals: false,
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
        let rsid = ctx
            .resume
            .as_ref()
            .or(ctx.new_session_id.as_ref())
            .map(|s| s.as_str().to_owned())
            .unwrap_or_else(|| "watch-sess-001".into());
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
            Box::new(WatchScriptedSession {
                recorded: Arc::clone(&self.recorded),
                runtime_session_id: rsid,
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

const WATCH_LOOP_ECHO_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  printf '{"kind":"reply","text":"ack"}\n'
done
"#;

fn make_real_session_manager(
    office: &Arc<Office>,
    script_path: &Path,
    sock_dir: &tempfile::TempDir,
    recorded: &Arc<WatchRecordedTurns>,
) -> Arc<SessionManager> {
    let mut reg = RuntimeRegistry::empty();
    reg.register(
        Arc::new(WatchScriptedRuntime {
            recorded: Arc::clone(recorded),
        }),
        Some(script_path.to_path_buf()),
    );
    let cfg = SessionManagerConfig::with_runtimes(sock_dir.path().join("office.sock"), reg);
    Arc::new(SessionManager::new(Arc::clone(office), cfg))
}

fn wait_for_turns(recorded: &WatchRecordedTurns, expected: usize, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if recorded.turns.lock().unwrap().len() >= expected {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    recorded.turns.lock().unwrap().len() >= expected
}

#[test]
fn watch_8_real_session_manager_wakes_only_on_confident_person_match_and_prepends_notices() {
    let _ = org_platform::process::harden_self();
    let bin_dir = tempfile::tempdir().unwrap();
    let sock_dir = tempfile::tempdir().unwrap();
    let script_path = bin_dir.path().join("watch-scripted");
    fs::write(&script_path, WATCH_LOOP_ECHO_SCRIPT).unwrap();
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

    // 1. Confident wake disabled: even confident person match must stay inbox-only with 0 wakes.
    let recorded_disabled = Arc::new(WatchRecordedTurns::default());
    let fx_off = WatcherFixture::new(
        WatcherConfig {
            cadence: Duration::ZERO,
            confident_wake_enabled: false,
            ..WatcherConfig::default()
        },
        true,
    );
    let mgr_off =
        make_real_session_manager(&fx_off.office, &script_path, &sock_dir, &recorded_disabled);
    let watcher_off = Arc::new(ChannelWatcher::new(
        Arc::clone(&fx_off.office),
        Some(Arc::clone(&mgr_off)),
        fx_off.provider.clone(),
        WatcherConfig {
            cadence: Duration::ZERO,
            confident_wake_enabled: false,
            ..WatcherConfig::default()
        },
    ));
    let q_off = fx_off.ask_channel(&fx_off.ada, "#general", "Can we ship?");
    let m_off = fx_off.send_channel(&fx_off.owner_id, "#general", "Yes, ship it.");
    fx_off.provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [{ "message_id": m_off, "question_id": q_off.ask_id, "confident": true }]
            },
            "notes": []
        })
        .to_string(),
    ));
    let rep_off = watcher_off.run_channel_pass(&q_off.conversation_id, true);
    assert_eq!(
        rep_off,
        PassReport::Delivered {
            judged_messages: 1,
            delivered_entries: 1,
            woke_agents: Vec::new(),
            skipped_secrets: 0,
        }
    );
    assert_eq!(recorded_disabled.turns.lock().unwrap().len(), 0);

    // 2. Confident wake enabled with real SessionManager:
    //    (a) unconfident person + confident agent -> inbox only, 0 wakes;
    //    (b) confident person -> wakes idle Ada and starts a real turn;
    //    (c) watch-ended notice is prepended to Ada's next turn (Medium #8).
    let recorded = Arc::new(WatchRecordedTurns::default());
    let cfg = WatcherConfig {
        cadence: Duration::ZERO,
        confident_wake_enabled: true,
        ..WatcherConfig::default()
    };
    let fx = WatcherFixture::new(cfg.clone(), true);
    let mgr = make_real_session_manager(&fx.office, &script_path, &sock_dir, &recorded);
    let watcher = Arc::new(ChannelWatcher::new(
        Arc::clone(&fx.office),
        Some(Arc::clone(&mgr)),
        fx.provider.clone(),
        cfg,
    ));
    let notice_watcher = Arc::downgrade(&watcher);
    mgr.set_watch_notice_provider(Arc::new(move |agent_id| {
        notice_watcher
            .upgrade()
            .map(|w| w.drain_watch_end_notices(agent_id))
            .unwrap_or_default()
    }));

    let q = fx.ask_channel(&fx.ada, "#general", "Which region for prod?");
    let conv_id = q.conversation_id.clone();

    // (a) Unconfident person + confident agent -> 0 wakes.
    let m_unconf = fx.send_channel(&fx.owner_id, "#general", "Maybe eu-west-1?");
    let m_agent = fx.send_channel(&fx.ben, "#general", "Definitely us-east-1.");
    fx.provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [
                    { "message_id": m_unconf, "question_id": q.ask_id, "confident": false },
                    { "message_id": m_agent, "question_id": q.ask_id, "confident": true }
                ]
            },
            "notes": []
        })
        .to_string(),
    ));
    let rep_a = watcher.run_channel_pass(&conv_id, true);
    assert_eq!(
        rep_a,
        PassReport::Delivered {
            judged_messages: 2,
            delivered_entries: 2,
            woke_agents: Vec::new(),
            skipped_secrets: 0,
        }
    );
    assert_eq!(recorded.turns.lock().unwrap().len(), 0);

    // (b) Confident person match -> inbox delivered AND starts 1 real turn for Ada.
    let m_conf = fx.send_channel(&fx.owner_id, "#general", "Production is eu-central-1.");
    fx.provider.push(Reply::Text(
        json!({
            "draft": {
                "matches": [
                    { "message_id": m_conf, "question_id": q.ask_id, "confident": true }
                ]
            },
            "notes": []
        })
        .to_string(),
    ));
    let rep_b = watcher.run_channel_pass(&conv_id, true);
    assert_eq!(
        rep_b,
        PassReport::Delivered {
            judged_messages: 1,
            delivered_entries: 1,
            woke_agents: vec![fx.ada.clone()],
            skipped_secrets: 0,
        }
    );
    assert!(
        wait_for_turns(&recorded, 1, Duration::from_secs(5)),
        "confident person match must start Ada's turn"
    );
    assert!(
        recorded.turns.lock().unwrap()[0].contains(&m_conf),
        "wake turn should reference the matched answer candidate"
    );

    // (c) Watch-ended notice prepended to Ada's next turn (Medium #8).
    watcher.on_member_left_channel(&fx.ada, &conv_id);
    // Wait briefly for turn 1 to finish so Ada returns to idle before sending turn 2.
    std::thread::sleep(Duration::from_millis(200));
    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.ada.clone(),
            instruction: Some("Check status".into()),
            workspace: None,
        },
    )
    .expect("send second turn to Ada");
    assert!(
        wait_for_turns(&recorded, 2, Duration::from_secs(5)),
        "second turn should execute"
    );
    let second_turn = recorded.turns.lock().unwrap()[1].clone();
    assert!(
        second_turn.contains("[Watch ended]") && second_turn.contains("left_channel"),
        "watch-ended notice must be prepended to next turn: {second_turn}"
    );
    mgr.stop_all("test_done");
    mgr_off.stop_all("test_done");
}

#[test]
fn cli_watcher_runtime_provider_delegates_via_toolless_capability_and_fails_closed() {
    use org_agent_runtime::{ToollessPromptAdapter, ToollessPromptOutput, ToollessPromptUsage};
    use org_copilot::{CancelToken, RunRequest};

    struct StubToollessAdapter;
    static STUB_TOOLLESS: StubToollessAdapter = StubToollessAdapter;

    impl ToollessPromptAdapter for StubToollessAdapter {
        fn availability_args(&self) -> &'static [&'static str] {
            &["--check"]
        }
        fn run_args(&self, model_key: &str) -> Vec<String> {
            vec!["--run".to_owned(), model_key.to_owned()]
        }
        fn parse_output(&self, stdout_bytes: &[u8]) -> Option<ToollessPromptOutput> {
            let text = std::str::from_utf8(stdout_bytes).ok()?.trim().to_owned();
            Some(ToollessPromptOutput {
                text,
                usage: Some(ToollessPromptUsage {
                    input_tokens: Some(7),
                    output_tokens: Some(3),
                }),
            })
        }
    }

    struct CapabilityRuntime {
        id: &'static str,
        aliases: &'static [&'static str],
        supports_toolless: bool,
    }

    impl AgentRuntime for CapabilityRuntime {
        fn id(&self) -> RuntimeId {
            RuntimeId::try_from(self.id).unwrap()
        }
        fn binary_name(&self) -> &'static str {
            "fake-toolless-cli"
        }
        fn aliases(&self) -> &'static [&'static str] {
            self.aliases
        }
        fn capabilities(&self) -> Capabilities {
            WatchScriptedRuntime {
                recorded: Arc::new(WatchRecordedTurns::default()),
            }
            .capabilities()
        }
        fn plan_session(
            &self,
            _spec: &SessionSpec,
            _ctx: &LaunchContext,
        ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
            Err(PlanError::Unsupported("not used"))
        }
        fn plan_take_over(
            &self,
            _spec: &SessionSpec,
            _ctx: &LaunchContext,
            _cols: u16,
            _rows: u16,
        ) -> Result<LaunchPlan, PlanError> {
            Err(PlanError::Unsupported("not used"))
        }
        fn plan_probe(&self, _probe: Probe, _ctx: &ProbeContext) -> Option<ProbeRun> {
            None
        }
        fn parse_probe(&self, _probe: Probe, _out: &ProbeOutput) -> ProbeResult {
            ProbeResult::Unavailable {
                reason: Unavailable::NotInstalled,
            }
        }
        fn toolless_prompt(&self) -> Option<&dyn ToollessPromptAdapter> {
            self.supports_toolless
                .then_some(&STUB_TOOLLESS as &dyn ToollessPromptAdapter)
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("fake-toolless.sh");
    fs::write(
        &script,
        "#!/bin/sh\nif [ \"$1\" = \"--check\" ]; then exit 0; fi\ncat >/dev/null\nprintf '{\"ok\":true}\\n'\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let mut registry = RuntimeRegistry::empty();
    registry.register(
        Arc::new(CapabilityRuntime {
            id: "alpha",
            aliases: &["alpha-cli"],
            supports_toolless: true,
        }),
        Some(script),
    );
    registry.register(
        Arc::new(CapabilityRuntime {
            id: "beta",
            aliases: &["beta-cli"],
            supports_toolless: false,
        }),
        None,
    );

    let fx = WatcherFixture::new(WatcherConfig::default(), false);
    let profile = fx
        .office
        .load_model_profile(&fx.profile_id)
        .unwrap()
        .expect("fixture profile exists");
    let base_conn = fx
        .office
        .load_provider_connection(profile.connection_ids[0].as_str())
        .unwrap()
        .expect("fixture connection exists");
    let make_conn = |driver: &str, enabled: bool, archived: bool| {
        let mut conn = base_conn.clone();
        conn.runtime_driver = driver.to_owned();
        conn.enabled = enabled;
        conn.archived = archived;
        conn
    };

    let provider = super::runtime::CliWatcherRuntimeProvider::new(registry);

    // 1. Adapter declaring `toolless_prompt()` is admitted and runs end-to-end.
    let conn_ok = make_conn("alpha-cli", true, false);
    let rt = provider
        .runtime_for_profile(&profile, Some(&conn_ok))
        .expect("toolless-capable adapter must resolve");
    assert!(rt.available());
    let output = rt
        .run(
            &RunRequest {
                prompt: "hello".into(),
                output_schema: json!({ "type": "object" }),
                timeout: Duration::from_secs(5),
                max_output_bytes: 1024,
            },
            &CancelToken::new(),
        )
        .expect("toolless CLI run succeeds");
    assert_eq!(output.text, r#"{"ok":true}"#);
    assert_eq!(output.usage.and_then(|u| u.input_tokens), Some(7));

    // 2. Adapter without `toolless_prompt()`, unregistered driver, disabled, archived, or
    //    missing connection all fail closed (`None`).
    assert!(provider
        .runtime_for_profile(&profile, Some(&make_conn("beta-cli", true, false)))
        .is_none());
    assert!(provider
        .runtime_for_profile(&profile, Some(&make_conn("unknown-driver", true, false)))
        .is_none());
    assert!(provider
        .runtime_for_profile(&profile, Some(&make_conn("alpha-cli", false, false)))
        .is_none());
    assert!(provider
        .runtime_for_profile(&profile, Some(&make_conn("alpha-cli", true, true)))
        .is_none());
    assert!(provider.runtime_for_profile(&profile, None).is_none());
}
