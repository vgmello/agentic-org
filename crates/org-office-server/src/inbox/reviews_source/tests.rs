//! Unit and integration tests for [`ReviewInboxSource`] and live SSE notifications (`TASK-92.2`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_git::RepoLocks;
use org_office::rules::OrgCommand;
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::chat::{ChatEventsPoll, ChatEventsPollParams, ChatEventsPollResult};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::inbox::{
    Action, InboxChangedData, InboxClears, InboxKind, InboxListParams, InboxState, Open, OpenTarget,
};
use org_proto::org::{
    MemberAddPersonParams, OrgChangedData, Position, ProjectCreateParams, TeamCreateParams,
    TeamSetLeadParams,
};
use org_proto::private_notes::{PrivateNoteAddParams, PrivateNoteKind};
use org_proto::reviews::{
    FindingAnchor, FindingSeverity, FindingStatus, ReviewClose, ReviewCloseParams, ReviewFinding,
    ReviewFindingDraft, ReviewFindingOp, ReviewFindingParams, ReviewInspect, ReviewInspectParams,
    ReviewInspectResult, ReviewRequest, ReviewRequestParams, ReviewResubmit, ReviewResubmitParams,
    ReviewRoute, ReviewRouteParams, ReviewRoutingSpec, ReviewTargetKind, ReviewVerdict,
    ReviewVerdictCall, ReviewVerdictParams, RoutingMode,
};
use org_proto::tasks::{TaskPriority, TasksCreateParams};
use org_proto::{Call, ErrorCode, Outcome, Request};

use crate::chat_calls::ChatService;
use crate::follow::OrgFeed;
use crate::inbox::{Hint, Keyset, Query, Source};
use crate::live::{Live, LiveEvents};
use crate::office_runtime::hq_inbox;
use crate::review_calls::ReviewService;

use super::ReviewInboxSource;

fn new_uuid_v7() -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    static SEQ: AtomicU64 = AtomicU64::new(900);
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

fn open_query() -> Query<'static> {
    Query {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        after: None,
        has: Default::default(),
        limit: 50,
    }
}

struct ReviewInboxFixture {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    review_svc: ReviewService,
    chat_svc: ChatService,
    hq: crate::inbox::Hq,
    hint: Hint,
    source: ReviewInboxSource,
    owner_id: String,
    alice_id: String,
    carol_id: String,
    bob_outsider_id: String,
    agent_author_id: String,
    agent_reviewer_id: String,
    prof_id: String,
    team_a: String,
    team_b: String,
    project_a: String,
    task_a: String,
}

impl ReviewInboxFixture {
    fn new() -> Self {
        let office = Arc::new(Office::for_test(TestOffice {
            engine: EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()),
        }));
        let owner_id = office.owner_member_id().to_owned();
        let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

        let team_a = new_uuid_v7();
        let team_b = new_uuid_v7();
        for (tid, name) in [(&team_a, "Core Alpha"), (&team_b, "Private Beta")] {
            office
                .submit_org(
                    &owner_id,
                    OrgCommand::TeamCreate(TeamCreateParams {
                        command_id: new_uuid_v7(),
                        team_id: tid.clone(),
                        name: name.into(),
                        owner_team_id: Some(root_team.clone()),
                    }),
                )
                .unwrap()
                .unwrap();
        }

        let alice_id = add_person(&office, &owner_id, &team_a, "Alice Lead", "alice");
        let carol_id = add_person(&office, &owner_id, &team_a, "Carol Teammate", "carol");
        let bob_outsider_id = add_person(&office, &owner_id, &team_b, "Bob Outsider", "bob");

        office
            .submit_org(
                &owner_id,
                OrgCommand::TeamSetLead(TeamSetLeadParams {
                    command_id: new_uuid_v7(),
                    team_id: team_a.clone(),
                    lead_member_id: Some(alice_id.clone()),
                    expected_version: 1,
                }),
            )
            .unwrap()
            .unwrap();

        let prof_id = seed_provider(&office, &owner_id);
        let agent_author_id = hire_agent(&office, &owner_id, &team_a, &prof_id, "author-bot");
        let agent_reviewer_id = hire_agent(&office, &owner_id, &team_a, &prof_id, "reviewer-bot");

        let project_a = new_uuid_v7();
        office
            .submit_org(
                &owner_id,
                OrgCommand::ProjectCreate(ProjectCreateParams {
                    command_id: new_uuid_v7(),
                    project_id: project_a.clone(),
                    name: "Alpha Secret".into(),
                    key: None,
                    description: None,
                    teams: Vec::new(),
                    general_area_id: String::new(),
                    general_area_name: None,
                    repo: None,
                    integration_branch: None,
                    slug: Some("alpha-secret".into()),
                    owner_team_id: Some(team_a.clone()),
                    create_team_id: None,
                    parent_team_id: None,
                    contributing_team_ids: Vec::new(),
                }),
            )
            .unwrap()
            .unwrap();

        let task_a = new_uuid_v7();
        office
            .task_create(
                &owner_id,
                TasksCreateParams {
                    command_id: new_uuid_v7(),
                    task_id: task_a.clone(),
                    title: "Alpha task".into(),
                    body_markdown: "Deliver Alpha".into(),
                    task_type: Some("feature".into()),
                    work_type: None,
                    priority: Some(TaskPriority::High),
                    effort: None,
                    severity: None,
                    rank_order: None,
                    team_id: Some(team_a.clone()),
                    assigned_team_id: Some(team_a.clone()),
                    assignee_id: Some(agent_author_id.clone()),
                    project_id: Some(project_a.clone()),
                    labels: Vec::new(),
                    reference_files: Vec::new(),
                    modified_files: Vec::new(),
                    depends_on: Vec::new(),
                    details: None,
                    dod: None,
                    attachment_ids: Vec::new(),
                },
            )
            .unwrap()
            .unwrap();

        let wake = Arc::new(crate::org_feed::FeedWake::new());
        let live_wake = Arc::clone(&wake);
        let live = Arc::new(LiveEvents::for_owner(
            Box::new(move || live_wake.notify()),
            owner_id.clone(),
        ));
        let review_svc = ReviewService::new(
            Arc::clone(&office),
            Arc::clone(&live),
            Arc::new(RepoLocks::new()),
            None,
        );
        let directory: Arc<dyn org_mailbox::MemberDirectory + Send + Sync> =
            Arc::new(crate::directory::OfficeDirectory::of(Arc::clone(&office)));
        let mailbox = Arc::new(org_mailbox::Mailbox::new(
            Arc::new(crate::mailbox_store::OfficeMailboxStore::new(Arc::clone(
                &office,
            ))),
            directory,
            org_mailbox::OfficeIdentity {
                office_id: office.info().office_id.clone(),
                incarnation: office.journal_incarnation().to_owned(),
                is_hq: true,
            },
            Arc::new(org_mailbox::SystemClock),
            Arc::clone(&live) as Arc<dyn org_mailbox::MessageEvents>,
        ));
        struct NoopWatcherRuntime;
        impl crate::watcher::WatcherRuntimeProvider for NoopWatcherRuntime {
            fn runtime_for_profile(
                &self,
                _profile: &org_office::organization_family::records::ModelProfile,
                _connection: Option<&org_office::organization_family::records::ProviderConnection>,
            ) -> Option<Arc<dyn org_copilot::CopilotRuntime>> {
                None
            }
        }
        let watcher = Arc::new(crate::watcher::ChannelWatcher::new(
            Arc::clone(&office),
            None,
            Arc::new(NoopWatcherRuntime),
            crate::watcher::WatcherConfig::default(),
        ));
        let chat_svc = ChatService::with_watcher(
            Arc::clone(&office),
            Arc::clone(&live),
            mailbox,
            None,
            watcher,
        )
        .with_feed_wake(Arc::clone(&wake));
        let hq = hq_inbox(Arc::clone(&office)).unwrap();
        let hint = Hint::new(Position { epoch: 1, seq: 1 }, live.mailbox_head());
        let source = ReviewInboxSource::new(Arc::clone(&office));

        Self {
            office,
            live,
            review_svc,
            chat_svc,
            hq,
            hint,
            source,
            owner_id,
            alice_id,
            carol_id,
            bob_outsider_id,
            agent_author_id,
            agent_reviewer_id,
            prof_id,
            team_a,
            team_b,
            project_a,
            task_a,
        }
    }

    fn call_review<C: Call>(&self, member: &str, params: &C::Params) -> C::Result {
        let req = Request::new::<C>(1, params).unwrap();
        let resp = self
            .review_svc
            .dispatch(Some(member), &req)
            .expect("review call handled");
        match resp.outcome {
            Outcome::Result(val) => serde_json::from_value(val).unwrap(),
            Outcome::Error(err) => panic!("{} failed: {err:?}", C::METHOD),
        }
    }

    fn inspect(&self, member: &str, review: &str) -> ReviewInspectResult {
        self.call_review::<ReviewInspect>(
            member,
            &ReviewInspectParams {
                review: review.to_owned(),
                fields: None,
            },
        )
    }

    fn poll_events(
        &self,
        member: &str,
        via_grant: bool,
        after_org_seq: Option<u64>,
        after_live_seq: Option<u64>,
    ) -> ChatEventsPollResult {
        let req = Request::new::<ChatEventsPoll>(
            7,
            &ChatEventsPollParams {
                after_org_seq,
                after_live_seq,
                after_presence_seq: None,
                wait_ms: None,
            },
        )
        .unwrap();
        let stop = AtomicBool::new(false);
        let resp = self
            .chat_svc
            .dispatch(Some(member), via_grant, &stop, &req)
            .expect("events_poll handled");
        match resp.outcome {
            Outcome::Result(val) => serde_json::from_value(val).unwrap(),
            Outcome::Error(err) => panic!("chat.events_poll failed: {err:?}"),
        }
    }

    fn poll_inbox_hints(&self, at: Instant) -> Vec<InboxChangedData> {
        let before = self.live.head().messages;
        self.hint.poll(&self.hq, &QuietFeed, &self.live, || at);
        let after = self.live.head().messages;
        self.live
            .since(before, after)
            .events
            .into_iter()
            .filter_map(|ev| match ev {
                Live::InboxChanged(data) => Some(data),
                _ => None,
            })
            .collect()
    }
}

fn add_person(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
    let id = new_uuid_v7();
    office
        .submit_org(
            owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: new_uuid_v7(),
                member_id: id.clone(),
                name: name.into(),
                contact: None,
                handle: Some(handle.into()),
                home_team_id: Some(team_id.to_owned()),
            }),
        )
        .unwrap()
        .unwrap();
    id
}

fn seed_provider(office: &Office, owner: &str) -> String {
    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id,
            default_profile_id: prof_id.clone(),
            key: "rev-inbox-conn".into(),
            name: "Review Inbox Conn".into(),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
    prof_id
}

fn hire_agent(office: &Office, owner: &str, team_id: &str, prof_id: &str, handle: &str) -> String {
    let id = new_uuid_v7();
    let req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: id.clone(),
            home_team_id: team_id.to_owned(),
            role_id: None,
            proposal: HireProposalWire {
                name: format!("Agent {handle}"),
                handle: handle.to_owned(),
                title: Some("Engineer".into()),
                focus: None,
                role_text: None,
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner.to_owned()),
                office_id: None,
                model_profile_id: Some(prof_id.to_owned()),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("workspace_edits".into()),
                tools_config: Vec::new(),
                system_prompt_markdown: None,
                body: Some("Review code".into()),
                effort: None,
            },
            reason: "review inbox test".into(),
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
    id
}

fn doc_review_request(
    project_id: Option<String>,
    title: &str,
    independent: bool,
    routing: ReviewRoutingSpec,
) -> ReviewRequestParams {
    ReviewRequestParams {
        command_id: new_uuid_v7(),
        review_id: new_uuid_v7(),
        target_kind: ReviewTargetKind::Doc,
        title: title.into(),
        body_markdown: "Please inspect this doc.".into(),
        kind_slug: "doc-review".into(),
        category_slug: None,
        project_id,
        task_id: None,
        repository_id: None,
        branch: None,
        target_ref: Some("docs/spec.md".into()),
        base_oid: None,
        expected_head_oid: None,
        independent,
        routing: Some(routing),
        session_mode: None,
        guidance_refs: Vec::new(),
        criteria: Vec::new(),
    }
}

fn assert_no_control_ansi_or_bidi(text: &str, label: &str) {
    assert!(
        !text.chars().any(char::is_control),
        "{label} contains control char: {text:?}"
    );
    assert!(
        !text.contains('\x1b') && !text.contains("[31m") && !text.contains("\u{009B}"),
        "{label} contains ANSI/C1 sequence: {text:?}"
    );
    assert!(
        !text.chars().any(|c| matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2069}'
                | '\u{FEFF}'
        )),
        "{label} contains invisible/BiDi char: {text:?}"
    );
}

#[test]
fn assigned_open_review_surfaces_for_reviewer_and_clears_on_verdict_or_reassignment() {
    let fx = ReviewInboxFixture::new();

    let created = fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "Check architecture ADR",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner_id.clone()],
                team_id: None,
            },
        ),
    );
    let key = created.review.key.clone();

    // Surfaced through ReviewInboxSource and through HQ inbox.list.
    let read = fx
        .source
        .read_for_caller(&fx.owner_id, &open_query())
        .unwrap();
    assert_eq!(read.counts.reviews, 1);
    assert_eq!(read.counts.total, 1);
    assert_eq!(read.items.len(), 1);
    let item = &read.items[0];
    assert_eq!(item.kind, InboxKind::Review);
    assert_eq!(item.source, "reviews");
    assert_eq!(item.clears, InboxClears::ReviewDecided);
    assert!(!item.dismissable);
    assert_eq!(item.project.as_deref(), Some(fx.project_a.as_str()));
    assert_eq!(item.team.as_deref(), Some(fx.team_a.as_str()));
    assert!(item.summary.contains(&key));
    assert!(item.summary.contains("Check architecture ADR"));
    assert!(item.actions.iter().any(|a| matches!(
        a,
        Action::Open { open: Open { target: OpenTarget::Review, id }, .. } if id == &key
    )));

    let hq_page = fx
        .hq
        .list(InboxListParams {
            state: InboxState::Open,
            kinds: Some(vec![InboxKind::Review]),
            project: None,
            team: None,
            cursor: None,
            limit: None,
            has_image: None,
            has_file: None,
        })
        .unwrap();
    assert!(hq_page.degraded.is_empty());
    assert_eq!(hq_page.counts.reviews, 1);
    assert_eq!(hq_page.items.len(), 1);
    assert_eq!(hq_page.items[0].reference, item.reference);

    // Approving clears the review card immediately.
    fx.call_review::<ReviewVerdictCall>(
        &fx.owner_id,
        &ReviewVerdictParams {
            command_id: new_uuid_v7(),
            review: key,
            round_number: 1,
            head_oid: None,
            verdict: ReviewVerdict::Approve,
            summary_markdown: "Looks great".into(),
            findings: Vec::new(),
            attachment_ids: Vec::new(),
        },
    );
    let after_approve = fx
        .source
        .read_for_caller(&fx.owner_id, &open_query())
        .unwrap();
    assert_eq!(after_approve.counts.reviews, 0);
    assert!(after_approve.items.is_empty());

    // Reassigning away from Alice clears Alice's card immediately.
    let second = fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "Reassignable review",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.alice_id.clone()],
                team_id: None,
            },
        ),
    );
    assert_eq!(
        fx.source
            .read_for_caller(&fx.alice_id, &open_query())
            .unwrap()
            .items
            .len(),
        1
    );
    fx.call_review::<ReviewRoute>(
        &fx.owner_id,
        &ReviewRouteParams {
            command_id: new_uuid_v7(),
            review: second.review.key.clone(),
            expected_revision: second.review.revision,
            routing: ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.agent_reviewer_id.clone()],
                team_id: None,
            },
            session_mode: None,
        },
    );
    assert!(fx
        .source
        .read_for_caller(&fx.alice_id, &open_query())
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn claimable_team_pool_surfaces_for_eligible_humans_and_enforces_independent_self_exclusion() {
    let fx = ReviewInboxFixture::new();

    // Alice opens an `independent = true` pool review on her own team (`team_a`).
    // Because Alice is the author, `resolve_routing` picks another eligible member in `team_a`.
    let pool_rev = fx.call_review::<ReviewRequest>(
        &fx.alice_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "Independent security audit",
            true,
            ReviewRoutingSpec {
                mode: RoutingMode::Pool,
                reviewer_ids: Vec::new(),
                team_id: Some(fx.team_a.clone()),
            },
        ),
    );
    let inspected = fx.inspect(&fx.owner_id, &pool_rev.review.key);
    let picked = inspected
        .reviewers
        .iter()
        .find(|r| !r.removed)
        .expect("pool picked a reviewer")
        .reviewer_id
        .clone();
    assert_ne!(picked, fx.alice_id, "independent author cannot be picked");

    // 1. Alice (independent author) MUST NOT see her own review in the claimable pool!
    let alice_read = fx
        .source
        .read_for_caller(&fx.alice_id, &open_query())
        .unwrap();
    assert!(
        alice_read.items.is_empty(),
        "independent author must be excluded from claiming team pool: {:?}",
        alice_read.items
    );

    // 2. Ensure a second eligible human in `team_a` who was NOT the picked reviewer sees
    // the ClaimablePool card with both `review.route` Claim action and `OpenTarget::Review`.
    let unpicked_human = if picked == fx.carol_id {
        add_person(&fx.office, &fx.owner_id, &fx.team_a, "Dana Pool", "dana")
    } else {
        fx.carol_id.clone()
    };
    let pool_read = fx
        .source
        .read_for_caller(&unpicked_human, &open_query())
        .unwrap();
    assert_eq!(pool_read.items.len(), 1);
    let card = &pool_read.items[0];
    assert_eq!(card.kind, InboxKind::Review);
    assert!(card.summary.starts_with("Claim pool review "));
    assert!(card.actions.iter().any(|a| matches!(
        a,
        Action::Call { op, .. } if op == ReviewRoute::METHOD
    )));
    assert!(card.actions.iter().any(|a| matches!(
        a,
        Action::Open { open: Open { target: OpenTarget::Review, id }, .. } if id == &pool_rev.review.key
    )));

    // 3. Once claimed via `review.route` (explicit), other pool members no longer see a pool card.
    fx.call_review::<ReviewRoute>(
        &unpicked_human,
        &ReviewRouteParams {
            command_id: new_uuid_v7(),
            review: pool_rev.review.key.clone(),
            expected_revision: pool_rev.review.revision,
            routing: ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![unpicked_human.clone()],
                team_id: None,
            },
            session_mode: None,
        },
    );
    let extra_human = add_person(&fx.office, &fx.owner_id, &fx.team_a, "Erin Late", "erin");
    assert!(fx
        .source
        .read_for_caller(&extra_human, &open_query())
        .unwrap()
        .items
        .is_empty());
    // The claiming human now sees it as an AssignedPending review.
    let claimed_read = fx
        .source
        .read_for_caller(&unpicked_human, &open_query())
        .unwrap();
    assert_eq!(claimed_read.items.len(), 1);
    assert!(claimed_read.items[0].summary.starts_with("Review "));
}

#[test]
fn changes_requested_surfaces_sanitized_for_human_author_and_clears_on_resubmit() {
    let fx = ReviewInboxFixture::new();

    let created = fx.call_review::<ReviewRequest>(
        &fx.alice_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "API \x1b[31mDesign\u{202E} Bearer sk-secret-title-123",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.agent_reviewer_id.clone()],
                team_id: None,
            },
        ),
    );
    let key = created.review.key.clone();

    // While Open with agent reviewer, human author has 0 review inbox items.
    assert!(fx
        .source
        .read_for_caller(&fx.alice_id, &open_query())
        .unwrap()
        .items
        .is_empty());

    // Agent requests changes with hostile control/ANSI/BiDi + secret in verdict summary.
    let secret_tok = format!("Bearer {}{}", "sk-ant-verdict-", "secret999");
    fx.call_review::<ReviewVerdictCall>(
        &fx.agent_reviewer_id,
        &ReviewVerdictParams {
            command_id: new_uuid_v7(),
            review: key.clone(),
            round_number: 1,
            head_oid: None,
            verdict: ReviewVerdict::ChangesRequested,
            summary_markdown: format!(
                "\x1b[31mFix error path\r\n\u{202E}token {secret_tok}\x1b[0m"
            ),
            findings: vec![ReviewFindingDraft {
                finding_id: new_uuid_v7(),
                severity: FindingSeverity::Major,
                anchor: FindingAnchor::general(),
                body_markdown: "Missing validation".into(),
                attachment_ids: Vec::new(),
            }],
            attachment_ids: Vec::new(),
        },
    );

    let alice_read = fx
        .source
        .read_for_caller(&fx.alice_id, &open_query())
        .unwrap();
    assert_eq!(alice_read.items.len(), 1);
    let item = &alice_read.items[0];
    assert_eq!(item.kind, InboxKind::Review);
    assert!(item.summary.starts_with("Changes requested on "));
    assert_no_control_ansi_or_bidi(&item.summary, "changes_requested summary");
    let ctx = item.context.as_deref().expect("context present");
    assert_no_control_ansi_or_bidi(ctx, "changes_requested context");
    let wire = serde_json::to_string(item).unwrap();
    assert!(!wire.contains("sk-secret-title-123"), "title secret leaked");
    assert!(!wire.contains("secret999"), "verdict secret leaked");

    // Resubmitting round 2 clears the author's changes_requested card immediately.
    let current = fx.inspect(&fx.alice_id, &key).review;
    fx.call_review::<ReviewResubmit>(
        &fx.alice_id,
        &ReviewResubmitParams {
            command_id: new_uuid_v7(),
            review: key,
            expected_revision: current.revision,
            expected_head_oid: None,
            session_mode: None,
            fresh: false,
            note_markdown: "Addressed feedback".into(),
        },
    );
    assert!(fx
        .source
        .read_for_caller(&fx.alice_id, &open_query())
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn five_round_escalation_surfaces_once_for_owner_and_lead_deduplicates_and_clears_on_close() {
    let fx = ReviewInboxFixture::new();

    let created = fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "Looping protocol spec",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.agent_reviewer_id.clone()],
                team_id: None,
            },
        ),
    );
    let key = created.review.key.clone();

    // Advance rounds 1 -> 4: neither owner nor lead Alice nor non-lead Carol sees escalation.
    for round in 1..=4 {
        assert!(
            fx.source
                .read_for_caller(&fx.owner_id, &open_query())
                .unwrap()
                .items
                .is_empty(),
            "round {round} must not escalate yet"
        );
        assert!(fx
            .source
            .read_for_caller(&fx.alice_id, &open_query())
            .unwrap()
            .items
            .is_empty());
        fx.call_review::<ReviewVerdictCall>(
            &fx.agent_reviewer_id,
            &ReviewVerdictParams {
                command_id: new_uuid_v7(),
                review: key.clone(),
                round_number: round,
                head_oid: None,
                verdict: ReviewVerdict::ChangesRequested,
                summary_markdown: format!("Round {round} still needs work"),
                findings: Vec::new(),
                attachment_ids: Vec::new(),
            },
        );
        let rev = fx.inspect(&fx.agent_author_id, &key).review.revision;
        fx.call_review::<ReviewResubmit>(
            &fx.agent_author_id,
            &ReviewResubmitParams {
                command_id: new_uuid_v7(),
                review: key.clone(),
                expected_revision: rev,
                expected_head_oid: None,
                session_mode: None,
                fresh: false,
                note_markdown: format!("Round {} attempt", round + 1),
            },
        );
    }

    // Now on round 5: owner and accountable team lead Alice BOTH get an escalation card.
    for stakeholder in [&fx.owner_id, &fx.alice_id] {
        let read = fx
            .source
            .read_for_caller(stakeholder, &open_query())
            .unwrap();
        assert_eq!(read.items.len(), 1, "stakeholder {stakeholder}");
        assert!(read.items[0].summary.contains("Escalation (round 5)"));
        assert!(read.items[0]
            .recommendation
            .as_deref()
            .is_some_and(|r| r.contains(">= 5")));
        assert!(read.items[0].reference.ends_with(":r5"));
    }
    // Non-lead, non-participant teammate Carol does not get noisy escalation.
    assert!(fx
        .source
        .read_for_caller(&fx.carol_id, &open_query())
        .unwrap()
        .items
        .is_empty());

    // Deterministic deduplication: routing Alice as an assigned reviewer on round 5 still yields
    // EXACTLY ONE card for Alice (assigned priority + escalation recommendation).
    let rev5 = fx.inspect(&fx.owner_id, &key).review.revision;
    fx.call_review::<ReviewRoute>(
        &fx.owner_id,
        &ReviewRouteParams {
            command_id: new_uuid_v7(),
            review: key.clone(),
            expected_revision: rev5,
            routing: ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.alice_id.clone(), fx.agent_reviewer_id.clone()],
                team_id: None,
            },
            session_mode: None,
        },
    );
    let alice_dedup = fx
        .source
        .read_for_caller(&fx.alice_id, &open_query())
        .unwrap();
    assert_eq!(
        alice_dedup.items.len(),
        1,
        "must never produce duplicate cards for the same review/round"
    );
    assert!(alice_dedup.items[0].summary.starts_with("Review "));
    assert!(alice_dedup.items[0]
        .recommendation
        .as_deref()
        .is_some_and(|r| r.contains(">= 5")));

    // Closing the review clears escalation cards for both owner and Alice.
    let rev_close = fx.inspect(&fx.owner_id, &key).review.revision;
    fx.call_review::<ReviewClose>(
        &fx.owner_id,
        &ReviewCloseParams {
            command_id: new_uuid_v7(),
            review: key,
            expected_revision: rev_close,
            reason: Some("Superseded after alignment".into()),
        },
    );
    assert!(fx
        .source
        .read_for_caller(&fx.owner_id, &open_query())
        .unwrap()
        .items
        .is_empty());
    assert!(fx
        .source
        .read_for_caller(&fx.alice_id, &open_query())
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn private_project_isolation_agent_refusal_and_zero_private_note_leakage() {
    let fx = ReviewInboxFixture::new();

    // Attach a private task note on `task_a` with a unique sentinel.
    let sentinel = "PRIVATE-TASK-NOTE-SENTINEL-92";
    fx.office
        .task_note_add(
            &fx.agent_author_id,
            PrivateNoteAddParams {
                command_id: new_uuid_v7(),
                note_id: new_uuid_v7(),
                task: Some(fx.task_a.clone()),
                project_id: None,
                kind: PrivateNoteKind::Note,
                title: Some("Internal scratch".into()),
                why: None,
                alternatives: None,
                body_markdown: sentinel.into(),
                tags: Vec::new(),
                checkpoint: false,
            },
        )
        .unwrap()
        .unwrap();

    // Create both an explicit-to-Bob-attempt-free pool review and an explicit review in Project A.
    fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &ReviewRequestParams {
            command_id: new_uuid_v7(),
            review_id: new_uuid_v7(),
            target_kind: ReviewTargetKind::Task,
            title: "Task A private project review".into(),
            body_markdown: "Verify Alpha task".into(),
            kind_slug: "task-review".into(),
            category_slug: None,
            project_id: Some(fx.project_a.clone()),
            task_id: Some(fx.task_a.clone()),
            repository_id: None,
            branch: None,
            target_ref: None,
            base_oid: None,
            expected_head_oid: None,
            independent: false,
            routing: Some(ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner_id.clone(), fx.bob_outsider_id.clone()],
                team_id: None,
            }),
            session_mode: None,
            guidance_refs: Vec::new(),
            criteria: Vec::new(),
        },
    );

    // Owner sees Project A review, and zero private note sentinel appears in wire JSON.
    let owner_read = fx
        .source
        .read_for_caller(&fx.owner_id, &open_query())
        .unwrap();
    assert_eq!(owner_read.items.len(), 1);
    let wire = serde_json::to_string(&owner_read.items).unwrap();
    assert!(!wire.contains(sentinel), "private note leaked: {wire}");

    // Bob (non-owner human in Team B, outsider to Project A) sees ZERO items from Project A!
    let bob_read = fx
        .source
        .read_for_caller(&fx.bob_outsider_id, &open_query())
        .unwrap();
    assert_eq!(bob_read.counts.reviews, 0);
    assert!(bob_read.items.is_empty());

    // Agents and unknown IDs are refused with NotAuthorized.
    for unauthorized in [&fx.agent_author_id, &fx.agent_reviewer_id] {
        let err = fx
            .source
            .read_for_caller(unauthorized, &open_query())
            .err()
            .expect("agent must be refused");
        assert_eq!(err.code, ErrorCode::NotAuthorized);
    }

    // Snapshot error and deactivated/missing caller both fail closed even when 0 reviews match.
    let failing = ReviewInboxSource::with_snapshot_for_test(
        Arc::clone(&fx.office),
        Box::new(|| {
            Err(org_proto::ApiError::new(
                ErrorCode::StorageError,
                "injected snapshot failure",
            ))
        }),
    );
    assert_eq!(
        failing
            .read_for_caller(&fx.owner_id, &open_query())
            .err()
            .unwrap()
            .code,
        ErrorCode::StorageError
    );
    assert!(failing.read(&open_query()).is_err());

    let mut snap = fx.office.org_snapshot().unwrap();
    snap.members.retain(|m| m.id != fx.owner_id);
    let missing_owner = ReviewInboxSource::with_snapshot_for_test(
        Arc::clone(&fx.office),
        Box::new(move || Ok(snap.clone())),
    );
    assert_eq!(
        missing_owner
            .read_for_caller(&fx.owner_id, &open_query())
            .err()
            .unwrap()
            .code,
        ErrorCode::NotAuthorized
    );
}

#[test]
fn filters_keyset_pagination_and_live_sse_inbox_and_org_changed_fire_on_all_six_mutations() {
    let fx = ReviewInboxFixture::new();
    let t0 = Instant::now();

    // Bootstrap SSE poll cursors for owner and non-owner Alice.
    let boot_owner = fx.poll_events(&fx.owner_id, true, None, None);
    let boot_alice = fx.poll_events(&fx.alice_id, true, None, None);
    let mut owner_org = Some(boot_owner.org_head_seq);
    let mut owner_live = Some(boot_owner.live_head_seq);
    let mut alice_org = Some(boot_alice.org_head_seq);
    let mut alice_live = Some(boot_alice.live_head_seq);

    let assert_sse_fired = |step: &str,
                            prev_mb: u64,
                            prev_org: u64,
                            at: Instant,
                            want_reviews: u64,
                            o_org: &mut Option<u64>,
                            o_live: &mut Option<u64>,
                            a_org: &mut Option<u64>,
                            a_live: &mut Option<u64>| {
        assert!(
            fx.live.mailbox_head() > prev_mb,
            "{step}: inbox_changed did not advance"
        );
        assert!(
            fx.live.org_changed_head() > prev_org,
            "{step}: org_changed did not advance"
        );
        let hints = fx.poll_inbox_hints(at);
        assert_eq!(hints.len(), 1, "{step}: expected 1 inbox.changed hint");
        assert_eq!(
            hints[0].counts.reviews, want_reviews,
            "{step}: unexpected review count in hint"
        );
        let owner_poll = fx.poll_events(&fx.owner_id, true, *o_org, *o_live);
        assert!(
            !owner_poll.org_changes.is_empty(),
            "{step}: owner did not receive org.changed"
        );
        assert!(
            !owner_poll.inbox_changes.is_empty(),
            "{step}: owner did not receive inbox.changed"
        );
        assert_eq!(
            owner_poll.inbox_changes.last().unwrap().counts.reviews,
            want_reviews
        );
        *o_org = Some(owner_poll.org_head_seq);
        *o_live = Some(owner_poll.live_head_seq);

        let alice_poll = fx.poll_events(&fx.alice_id, true, *a_org, *a_live);
        assert!(
            !alice_poll.org_changes.is_empty(),
            "{step}: non-owner human did not receive org.changed"
        );
        *a_org = Some(alice_poll.org_head_seq);
        *a_live = Some(alice_poll.live_head_seq);
    };

    // 1. review.request
    let (mb, org) = (fx.live.mailbox_head(), fx.live.org_changed_head());
    let created = fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "SSE lifecycle review 1",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner_id.clone()],
                team_id: None,
            },
        ),
    );
    let key = created.review.key.clone();
    assert_sse_fired(
        "review.request",
        mb,
        org,
        t0,
        1,
        &mut owner_org,
        &mut owner_live,
        &mut alice_org,
        &mut alice_live,
    );

    // 2. review.route
    let (mb, org) = (fx.live.mailbox_head(), fx.live.org_changed_head());
    let routed = fx.call_review::<ReviewRoute>(
        &fx.owner_id,
        &ReviewRouteParams {
            command_id: new_uuid_v7(),
            review: key.clone(),
            expected_revision: created.review.revision,
            routing: ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner_id.clone(), fx.alice_id.clone()],
                team_id: None,
            },
            session_mode: None,
        },
    );
    assert_sse_fired(
        "review.route",
        mb,
        org,
        t0 + Duration::from_secs(1),
        1,
        &mut owner_org,
        &mut owner_live,
        &mut alice_org,
        &mut alice_live,
    );

    // 3. review.finding
    let (mb, org) = (fx.live.mailbox_head(), fx.live.org_changed_head());
    let finding_id = new_uuid_v7();
    let found = fx.call_review::<ReviewFinding>(
        &fx.owner_id,
        &ReviewFindingParams {
            command_id: new_uuid_v7(),
            review: key.clone(),
            op: ReviewFindingOp::Add(ReviewFindingDraft {
                finding_id: finding_id.clone(),
                severity: FindingSeverity::Minor,
                anchor: FindingAnchor::general(),
                body_markdown: "Clarify edge case".into(),
                attachment_ids: Vec::new(),
            }),
        },
    );
    assert_sse_fired(
        "review.finding",
        mb,
        org,
        t0 + Duration::from_secs(2),
        1,
        &mut owner_org,
        &mut owner_live,
        &mut alice_org,
        &mut alice_live,
    );

    // Resolve finding so approve/resubmit flow is clean.
    let _ = fx.call_review::<ReviewFinding>(
        &fx.owner_id,
        &ReviewFindingParams {
            command_id: new_uuid_v7(),
            review: key.clone(),
            op: ReviewFindingOp::Reply {
                finding_id,
                reply_id: new_uuid_v7(),
                body_markdown: "Done".into(),
                set_status: Some(FindingStatus::Resolved),
                attachment_ids: Vec::new(),
            },
        },
    );
    let _ = fx.poll_inbox_hints(t0 + Duration::from_secs(3));
    let drain_o = fx.poll_events(&fx.owner_id, true, owner_org, owner_live);
    owner_org = Some(drain_o.org_head_seq);
    owner_live = Some(drain_o.live_head_seq);
    let drain_a = fx.poll_events(&fx.alice_id, true, alice_org, alice_live);
    alice_org = Some(drain_a.org_head_seq);
    alice_live = Some(drain_a.live_head_seq);

    // 4. review.verdict (ChangesRequested clears reviewer card)
    let (mb, org) = (fx.live.mailbox_head(), fx.live.org_changed_head());
    let verdicted = fx.call_review::<ReviewVerdictCall>(
        &fx.owner_id,
        &ReviewVerdictParams {
            command_id: new_uuid_v7(),
            review: key.clone(),
            round_number: 1,
            head_oid: None,
            verdict: ReviewVerdict::ChangesRequested,
            summary_markdown: "Revise section 2".into(),
            findings: Vec::new(),
            attachment_ids: Vec::new(),
        },
    );
    assert_sse_fired(
        "review.verdict",
        mb,
        org,
        t0 + Duration::from_secs(4),
        0,
        &mut owner_org,
        &mut owner_live,
        &mut alice_org,
        &mut alice_live,
    );

    // 5. review.resubmit (opens round 2 -> owner assigned card returns!)
    let (mb, org) = (fx.live.mailbox_head(), fx.live.org_changed_head());
    let resubmitted = fx.call_review::<ReviewResubmit>(
        &fx.agent_author_id,
        &ReviewResubmitParams {
            command_id: new_uuid_v7(),
            review: key.clone(),
            expected_revision: verdicted.review.revision,
            expected_head_oid: None,
            session_mode: None,
            fresh: false,
            note_markdown: "Revised section 2".into(),
        },
    );
    assert_eq!(routed.review.revision + 2, found.review.revision + 1);
    assert_sse_fired(
        "review.resubmit",
        mb,
        org,
        t0 + Duration::from_secs(5),
        1,
        &mut owner_org,
        &mut owner_live,
        &mut alice_org,
        &mut alice_live,
    );

    // Exercise filters and keyset pagination with two open reviews assigned to owner.
    let second = fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "SSE lifecycle review 2",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner_id.clone()],
                team_id: None,
            },
        ),
    );
    let page1 = fx
        .source
        .read(&Query {
            limit: 1,
            ..open_query()
        })
        .unwrap();
    assert_eq!(page1.counts.reviews, 2);
    assert_eq!(page1.items.len(), 1);
    let after = Keyset::of(&page1.items[0]);
    let page2 = fx
        .source
        .read(&Query {
            after: Some(&after),
            limit: 1,
            ..open_query()
        })
        .unwrap();
    assert_eq!(page2.counts.reviews, 2);
    assert_eq!(page2.items.len(), 1);
    assert_ne!(page1.items[0].reference, page2.items[0].reference);

    let wrong_kind = fx
        .source
        .read(&Query {
            kinds: Some(&[InboxKind::Question]),
            ..open_query()
        })
        .unwrap();
    assert!(wrong_kind.items.is_empty());
    assert_eq!(wrong_kind.counts.reviews, 2);

    let matching_scope = fx
        .source
        .read(&Query {
            project: Some(&fx.project_a),
            team: Some(&fx.team_a),
            ..open_query()
        })
        .unwrap();
    assert_eq!(matching_scope.items.len(), 2);

    let other_team = fx
        .source
        .read(&Query {
            team: Some(&fx.team_b),
            ..open_query()
        })
        .unwrap();
    assert!(other_team.items.is_empty());
    assert_eq!(other_team.counts.reviews, 2);

    // 6. review.close (clears first review -> 1 remaining)
    let _ = fx.poll_inbox_hints(t0 + Duration::from_secs(6));
    let drain_o = fx.poll_events(&fx.owner_id, true, owner_org, owner_live);
    owner_org = Some(drain_o.org_head_seq);
    owner_live = Some(drain_o.live_head_seq);
    let drain_a = fx.poll_events(&fx.alice_id, true, alice_org, alice_live);
    alice_org = Some(drain_a.org_head_seq);
    alice_live = Some(drain_a.live_head_seq);

    let (mb, org) = (fx.live.mailbox_head(), fx.live.org_changed_head());
    fx.call_review::<ReviewClose>(
        &fx.owner_id,
        &ReviewCloseParams {
            command_id: new_uuid_v7(),
            review: key,
            expected_revision: resubmitted.review.revision,
            reason: Some("Merged".into()),
        },
    );
    assert_sse_fired(
        "review.close",
        mb,
        org,
        t0 + Duration::from_secs(7),
        1,
        &mut owner_org,
        &mut owner_live,
        &mut alice_org,
        &mut alice_live,
    );
    assert!(!second.review.key.is_empty());
}

#[test]
fn verdict_summary_redacts_secrets_straddling_char_200_before_truncation() {
    let fx = ReviewInboxFixture::new();

    // Build test secrets dynamically at runtime so source contains no static credential literals.
    let sk_secret = format!("{}{}{}", "sk", "-", "9f8e7d6c5b4a3210fedcba9876");
    let aws_secret = format!("{}{}{}", "AK", "IA", "0123456789ABCDEF");
    let url_secret = format!(
        "https://{}:{}@internal.example/repo",
        "deploy_user", "super_secret_pw_99"
    );

    let cases: [(&str, String, &[&str]); 4] = [
        (
            "sk-prefix straddling char 200",
            format!("{} {sk_secret} trailing text", "x".repeat(195)),
            &["sk-", "9f8e", "7d6c"],
        ),
        (
            "AKIA straddling char 200",
            format!("{} {aws_secret} trailing text", "y".repeat(195)),
            &["AKIA", "01234", "ABCDEF"],
        ),
        (
            "URL userinfo straddling char 200",
            format!("{} {url_secret} trailing text", "z".repeat(180)),
            &["deploy_user", "super_secret_pw", "super_sec"],
        ),
        (
            "multibyte UTF-8 before sk-prefix straddling char 200",
            format!("{} {sk_secret} end", "é".repeat(195)),
            &["sk-", "9f8e", "7d6c"],
        ),
    ];

    for (idx, (label, summary_markdown, forbidden)) in cases.into_iter().enumerate() {
        let created = fx.call_review::<ReviewRequest>(
            &fx.alice_id,
            &doc_review_request(
                Some(fx.project_a.clone()),
                &format!("Boundary redaction case {idx}"),
                false,
                ReviewRoutingSpec {
                    mode: RoutingMode::Explicit,
                    reviewer_ids: vec![fx.agent_reviewer_id.clone()],
                    team_id: None,
                },
            ),
        );
        let key = created.review.key.clone();
        fx.call_review::<ReviewVerdictCall>(
            &fx.agent_reviewer_id,
            &ReviewVerdictParams {
                command_id: new_uuid_v7(),
                review: key.clone(),
                round_number: 1,
                head_oid: None,
                verdict: ReviewVerdict::ChangesRequested,
                summary_markdown,
                findings: Vec::new(),
                attachment_ids: Vec::new(),
            },
        );

        let alice_read = fx
            .source
            .read_for_caller(&fx.alice_id, &open_query())
            .unwrap();
        let item = alice_read
            .items
            .iter()
            .find(|it| it.summary.contains(&key))
            .expect("changes_requested card surfaced for author");
        let ctx = item.context.as_deref().expect("context present");
        assert_no_control_ansi_or_bidi(ctx, label);
        let wire = serde_json::to_string(item).unwrap();
        for needle in forbidden {
            assert!(
                !ctx.contains(needle),
                "{label}: unredacted fragment {needle:?} leaked into context: {ctx:?}"
            );
            assert!(
                !wire.contains(needle),
                "{label}: unredacted fragment {needle:?} leaked into wire JSON: {wire:?}"
            );
        }

        // Close so subsequent cases isolate cleanly.
        let rev = fx.inspect(&fx.owner_id, &key).review.revision;
        fx.call_review::<ReviewClose>(
            &fx.owner_id,
            &ReviewCloseParams {
                command_id: new_uuid_v7(),
                review: key,
                expected_revision: rev,
                reason: Some("Test cleanup".into()),
            },
        );
    }
}

#[test]
fn cross_team_pool_matching_never_leaks_via_shared_role_id() {
    let fx = ReviewInboxFixture::new();

    // Org-wide (project_id: None) pool review on `team_a`: `may_see_project` admits everyone,
    // isolating `eligible_for_pool` so any cross-team role_id leak would surface to Bob (Team B).
    let pool_rev = fx.call_review::<ReviewRequest>(
        &fx.alice_id,
        &doc_review_request(
            None,
            "Team A pool only",
            true,
            ReviewRoutingSpec {
                mode: RoutingMode::Pool,
                reviewer_ids: Vec::new(),
                team_id: Some(fx.team_a.clone()),
            },
        ),
    );
    let picked = fx
        .inspect(&fx.owner_id, &pool_rev.review.key)
        .reviewers
        .iter()
        .find(|r| !r.removed)
        .expect("pool picked a reviewer")
        .reviewer_id
        .clone();
    let eligible_team_a_human = if picked == fx.carol_id {
        add_person(&fx.office, &fx.owner_id, &fx.team_a, "Dana Alpha", "dana-a")
    } else {
        fx.carol_id.clone()
    };

    // Give Bob (Team B) the exact same `role_id` as the seeded Team A reviewer.
    let mut snap = fx.office.org_snapshot().unwrap();
    for m in &mut snap.members {
        if m.id == picked || m.id == fx.bob_outsider_id || m.id == eligible_team_a_human {
            m.role_id = Some("role-security-reviewer".into());
        }
    }
    let source = ReviewInboxSource::with_snapshot_for_test(
        Arc::clone(&fx.office),
        Box::new(move || Ok(snap.clone())),
    );

    // Team B human with matching role_id MUST NOT receive Team A pool claim cards.
    let bob_read = source
        .read_for_caller(&fx.bob_outsider_id, &open_query())
        .unwrap();
    assert!(
        bob_read.items.is_empty(),
        "Team B human must not receive Team A pool card via shared role_id: {:?}",
        bob_read.items
    );
    assert_eq!(bob_read.counts.reviews, 0);

    // Eligible Team A human still receives the ClaimablePool card.
    let team_a_read = source
        .read_for_caller(&eligible_team_a_human, &open_query())
        .unwrap();
    assert_eq!(team_a_read.items.len(), 1);
    assert!(team_a_read.items[0]
        .summary
        .starts_with("Claim pool review "));
}

#[test]
fn non_owner_parent_team_lead_receives_subordinate_round_5_escalation_while_unrelated_lead_denied()
{
    let fx = ReviewInboxFixture::new();
    let root_team = fx.office.org_snapshot().unwrap().teams[0].id.clone();

    // Hierarchy: root -> parent_team -> child_team, and sibling unrelated_team under root.
    let parent_team = new_uuid_v7();
    let child_team = new_uuid_v7();
    let unrelated_team = new_uuid_v7();
    for (tid, name, owner_tid) in [
        (&parent_team, "Engineering Parent", root_team.clone()),
        (&child_team, "Payments Child", parent_team.clone()),
        (&unrelated_team, "Marketing Sibling", root_team),
    ] {
        fx.office
            .submit_org(
                &fx.owner_id,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: new_uuid_v7(),
                    team_id: tid.clone(),
                    name: name.into(),
                    owner_team_id: Some(owner_tid),
                }),
            )
            .unwrap()
            .unwrap();
    }

    let parent_lead = add_person(&fx.office, &fx.owner_id, &parent_team, "Pat Lead", "pat");
    let parent_member = add_person(
        &fx.office,
        &fx.owner_id,
        &parent_team,
        "Morgan Member",
        "morgan",
    );
    let unrelated_lead = add_person(
        &fx.office,
        &fx.owner_id,
        &unrelated_team,
        "Uma Unrelated",
        "uma",
    );
    for (tid, lead_id) in [
        (&parent_team, &parent_lead),
        (&unrelated_team, &unrelated_lead),
    ] {
        fx.office
            .submit_org(
                &fx.owner_id,
                OrgCommand::TeamSetLead(TeamSetLeadParams {
                    command_id: new_uuid_v7(),
                    team_id: tid.clone(),
                    lead_member_id: Some(lead_id.clone()),
                    expected_version: 1,
                }),
            )
            .unwrap()
            .unwrap();
    }

    let child_project = new_uuid_v7();
    fx.office
        .submit_org(
            &fx.owner_id,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: new_uuid_v7(),
                project_id: child_project.clone(),
                name: "Child Payments".into(),
                key: None,
                description: None,
                teams: Vec::new(),
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some("child-payments".into()),
                owner_team_id: Some(child_team.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: Vec::new(),
            }),
        )
        .unwrap()
        .unwrap();

    let child_author = hire_agent(
        &fx.office,
        &fx.owner_id,
        &child_team,
        &fx.prof_id,
        "child-a",
    );
    let child_reviewer = hire_agent(
        &fx.office,
        &fx.owner_id,
        &child_team,
        &fx.prof_id,
        "child-r",
    );

    let created = fx.call_review::<ReviewRequest>(
        &child_author,
        &doc_review_request(
            Some(child_project),
            "Subordinate round-5 loop",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![child_reviewer.clone()],
                team_id: None,
            },
        ),
    );
    let key = created.review.key.clone();

    // Advance rounds 1 -> 5.
    for round in 1..=4 {
        assert!(
            fx.source
                .read_for_caller(&parent_lead, &open_query())
                .unwrap()
                .items
                .is_empty(),
            "round {round} must not escalate yet for parent lead"
        );
        fx.call_review::<ReviewVerdictCall>(
            &child_reviewer,
            &ReviewVerdictParams {
                command_id: new_uuid_v7(),
                review: key.clone(),
                round_number: round,
                head_oid: None,
                verdict: ReviewVerdict::ChangesRequested,
                summary_markdown: format!("Round {round} feedback"),
                findings: Vec::new(),
                attachment_ids: Vec::new(),
            },
        );
        let rev = fx.inspect(&child_author, &key).review.revision;
        fx.call_review::<ReviewResubmit>(
            &child_author,
            &ReviewResubmitParams {
                command_id: new_uuid_v7(),
                review: key.clone(),
                expected_revision: rev,
                expected_head_oid: None,
                session_mode: None,
                fresh: false,
                note_markdown: format!("Round {} resubmit", round + 1),
            },
        );
    }

    // Round 5: non-owner parent-team lead receives the escalation card!
    let parent_read = fx
        .source
        .read_for_caller(&parent_lead, &open_query())
        .unwrap();
    assert_eq!(parent_read.items.len(), 1);
    assert!(parent_read.items[0]
        .summary
        .contains("Escalation (round 5)"));
    assert!(parent_read.items[0].summary.contains(&key));

    // Non-lead member of parent team does NOT receive child project escalation.
    assert!(fx
        .source
        .read_for_caller(&parent_member, &open_query())
        .unwrap()
        .items
        .is_empty());

    // Lead of unrelated sibling team stays denied!
    assert!(fx
        .source
        .read_for_caller(&unrelated_lead, &open_query())
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn chat_events_poll_synthetic_org_changed_never_skips_org_backlog_or_fires_on_resync() {
    let fx = ReviewInboxFixture::new();
    let boot = fx.poll_events(&fx.owner_id, true, None, None);

    // Commit 105 real org log entries (> PAGE=100 limit of `poll_org_changes`).
    for version in 1..=105u64 {
        fx.office
            .submit_org(
                &fx.owner_id,
                OrgCommand::TeamSetLead(TeamSetLeadParams {
                    command_id: new_uuid_v7(),
                    team_id: fx.team_b.clone(),
                    lead_member_id: Some(fx.bob_outsider_id.clone()),
                    expected_version: version,
                }),
            )
            .unwrap()
            .unwrap();
    }

    // Concurrently commit a review write so `Live::OrgChanged` is also pending.
    let _ = fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "Concurrent backlog review",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner_id.clone()],
                team_id: None,
            },
        ),
    );

    let poll1 = fx.poll_events(
        &fx.owner_id,
        true,
        Some(boot.org_head_seq),
        Some(boot.live_head_seq),
    );
    assert!(!poll1.snapshot_required);
    assert!(poll1.org_head_seq >= boot.org_head_seq + 105);

    let real_changes: Vec<_> = poll1
        .org_changes
        .iter()
        .filter(|c| !c.command_id.starts_with("live-org:"))
        .collect();
    let synth_changes: Vec<_> = poll1
        .org_changes
        .iter()
        .filter(|c| c.command_id.starts_with("live-org:"))
        .collect();
    assert_eq!(real_changes.len(), 100);
    assert!(!synth_changes.is_empty());

    let last_real_seq = real_changes.last().unwrap().position.seq;
    assert!(
        last_real_seq < poll1.org_head_seq,
        "backlog must still have undelivered org entries"
    );
    for synth in &synth_changes {
        assert!(
            synth.position.seq <= last_real_seq,
            "synthetic live-org:* seq {} must not exceed last real org_change seq {}",
            synth.position.seq,
            last_real_seq
        );
        assert!(
            synth.position.seq < poll1.org_head_seq,
            "synthetic live-org:* must never jump to org_head_seq {}",
            poll1.org_head_seq
        );
    }

    // Resuming from the last returned org_changes entry drains the remaining 5 entries with zero loss.
    let next_cursor = poll1.org_changes.last().unwrap().position.seq;
    assert_eq!(next_cursor, last_real_seq);
    let poll2 = fx.poll_events(
        &fx.owner_id,
        true,
        Some(next_cursor),
        Some(poll1.live_head_seq),
    );
    assert!(!poll2.snapshot_required);
    assert_eq!(poll2.org_changes.len(), 5);
    assert_eq!(
        poll2.org_changes.first().unwrap().position.seq,
        last_real_seq + 1
    );
    assert_eq!(
        poll2.org_changes.last().unwrap().position.seq,
        poll1.org_head_seq
    );

    // Non-owner human with an older `after_org_seq` and pending review write never jumps past `after_org_seq`.
    let alice_live_before = poll2.live_head_seq;
    let _ = fx.call_review::<ReviewRequest>(
        &fx.agent_author_id,
        &doc_review_request(
            Some(fx.project_a.clone()),
            "Non-owner cursor check",
            false,
            ReviewRoutingSpec {
                mode: RoutingMode::Explicit,
                reviewer_ids: vec![fx.owner_id.clone()],
                team_id: None,
            },
        ),
    );
    let alice_poll = fx.poll_events(
        &fx.alice_id,
        true,
        Some(boot.org_head_seq),
        Some(alice_live_before),
    );
    assert!(!alice_poll.snapshot_required);
    assert!(!alice_poll.org_changes.is_empty());
    assert!(alice_poll
        .org_changes
        .iter()
        .all(|c| c.command_id.starts_with("live-org:") && c.position.seq == boot.org_head_seq));

    // When `snapshot_required` is true, synthetic `live-org:*` MUST NOT be pushed.
    let resync_poll = fx.poll_events(
        &fx.owner_id,
        true,
        Some(poll2.org_head_seq + 50),
        Some(alice_live_before),
    );
    assert!(resync_poll.snapshot_required);
    assert!(
        resync_poll.org_changes.is_empty(),
        "synthetic live-org:* must be skipped when snapshot_required is true: {:?}",
        resync_poll.org_changes
    );
}
