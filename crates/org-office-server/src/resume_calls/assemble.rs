//! Single-snapshot assembly of [`AgentResumeResult`] from [`Office`] reads and caller-owned
//! [`ResumeWorktreeItem`]s (`TASK-220`, `ADR-0031`).

use std::collections::BTreeMap;

use org_office::attachments::ports::AttachmentsRead;
use org_office::messaging::ask_card::{parse_ask_card, AskCard};
use org_office::messaging::records::MessageRecord;
use org_office::organization_family::records::{Employee, EmployeeKind, EmployeeStatus};
use org_office::private_notes::PrivateNoteRecord;
use org_office::project_docs::ports::ProjectDocsRead;
use org_office::reviews::ports::ReviewsRead;
use org_office::search::ports::SearchRead;
use org_office::tasks::ports::TasksRead;
use org_office::tasks::query::{TaskListQuery, TaskStatusSet};
use org_office::tasks::records::TaskRecord;
use org_office::{format_rfc3339_ms, parse_rfc3339_ms, Office, OfficeError, ReadOp};
use org_proto::ask::AskStatus;
use org_proto::budgets::{
    BudgetAgentOwnParams, BudgetAgentOwnResult, BudgetStateSummary, BudgetView,
};
use org_proto::external_auth::SessionMode;
use org_proto::msg::OFFICE_SENDER;
use org_proto::org::MemberKind;
use org_proto::private_notes::PRIVATE_NOTE_CHECKPOINT_TAG;
use org_proto::resume::{
    AgentResumeParams, AgentResumeResult, ResumeAskDirection, ResumeBudgetPeriod,
    ResumeBudgetSummary, ResumeIdentity, ResumeInboxSummary, ResumeOpenAsk, ResumeTaskItem,
    ResumeTaskNotePointer, ResumeWorktreeItem, RESUME_ASK_EXCERPT_MAX_BYTES,
    RESUME_DEFAULT_ASKS_LIMIT, RESUME_DEFAULT_INBOX_LIMIT, RESUME_DEFAULT_TASKS_LIMIT,
    RESUME_DEFAULT_WORKTREES_LIMIT, RESUME_MAX_ASKS_LIMIT, RESUME_MAX_INBOX_LIMIT,
    RESUME_MAX_TASKS_LIMIT, RESUME_MAX_WORKTREES_LIMIT, RESUME_NOTE_EXCERPT_MAX_BYTES,
};
use org_proto::tasks::{TaskSortMode, TaskStatus};
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, StoreError};

const ACTIVE_BUCKETS: [TaskStatus; 3] = [
    TaskStatus::InProgress,
    TaskStatus::Blocked,
    TaskStatus::Todo,
];

pub(super) struct ResumeLimits {
    pub(super) tasks: usize,
    pub(super) inbox: usize,
    pub(super) asks: usize,
    pub(super) worktrees: usize,
}

fn resolve_limit(raw: Option<u32>, default: u32, max: u32, field: &str) -> Result<usize, ApiError> {
    let val = raw.unwrap_or(default);
    if val == 0 || val > max {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` must be between 1 and {max}"),
        ));
    }
    Ok(val as usize)
}

impl ResumeLimits {
    pub(super) fn from_params(params: &AgentResumeParams) -> Result<Self, ApiError> {
        Ok(Self {
            tasks: resolve_limit(
                params.tasks_limit,
                RESUME_DEFAULT_TASKS_LIMIT,
                RESUME_MAX_TASKS_LIMIT,
                "tasks_limit",
            )?,
            inbox: resolve_limit(
                params.inbox_limit,
                RESUME_DEFAULT_INBOX_LIMIT,
                RESUME_MAX_INBOX_LIMIT,
                "inbox_limit",
            )?,
            asks: resolve_limit(
                params.asks_limit,
                RESUME_DEFAULT_ASKS_LIMIT,
                RESUME_MAX_ASKS_LIMIT,
                "asks_limit",
            )?,
            worktrees: resolve_limit(
                params.worktrees_limit,
                RESUME_DEFAULT_WORKTREES_LIMIT,
                RESUME_MAX_WORKTREES_LIMIT,
                "worktrees_limit",
            )?,
        })
    }
}

struct ResumeSnapshot {
    caller: Employee,
    active_tasks: Vec<TaskRecord>,
    active_tasks_total: u64,
    project_keys: BTreeMap<Id, String>,
    unread_inbox: ResumeInboxSummary,
    open_ask_candidates: Vec<(MessageRecord, bool)>,
}

struct AgentResumeRead {
    actor: String,
    now_utc: String,
    tasks_limit: usize,
    inbox_limit: usize,
    asks_limit: usize,
}

impl ReadOp for AgentResumeRead {
    type Out = Result<ResumeSnapshot, ApiError>;

    fn read<S: TasksRead + AttachmentsRead + ProjectDocsRead + ReviewsRead + SearchRead>(
        self,
        snap: &S,
    ) -> Result<Self::Out, StoreError> {
        let Ok(caller_id) = Id::parse(&self.actor) else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not a member of this organization",
            )));
        };
        let Some(caller) = snap.employee(&caller_id).map_err(StoreError::from)? else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not a member of this organization",
            )));
        };
        if caller.status != EmployeeStatus::Active {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "inactive or paused employees cannot call agent.resume",
            )));
        }

        let mut active_tasks = Vec::with_capacity(self.tasks_limit);
        let mut active_tasks_total: u64 = 0;
        for status in ACTIVE_BUCKETS {
            let rem = self.tasks_limit.saturating_sub(active_tasks.len());
            let page = snap
                .task_page(&TaskListQuery {
                    statuses: TaskStatusSet::only(status),
                    assignee_id: Some(caller.id.as_str().to_owned()),
                    sort: TaskSortMode::Order,
                    offset: 0,
                    limit: Some(rem as u64),
                    ..TaskListQuery::default()
                })
                .map_err(StoreError::from)?;
            active_tasks_total = active_tasks_total.saturating_add(page.total);
            if rem > 0 {
                active_tasks.extend(page.tasks.into_iter().take(rem));
            }
        }

        let mut project_keys = BTreeMap::new();
        if active_tasks.iter().any(|t| t.project_id.is_some()) {
            for project in snap.projects().map_err(StoreError::from)? {
                project_keys.insert(project.id.clone(), project.key());
            }
        }

        let unread_inbox = snap
            .unread_inbox_summary(&caller.id, self.inbox_limit)
            .map_err(StoreError::from)?;
        let caller_is_person = caller.kind == EmployeeKind::Person;
        let open_ask_candidates = snap
            .open_asks_for_employee(&caller.id, &self.now_utc, caller_is_person, self.asks_limit)
            .map_err(StoreError::from)?;

        Ok(Ok(ResumeSnapshot {
            caller,
            active_tasks,
            active_tasks_total,
            project_keys,
            unread_inbox,
            open_ask_candidates,
        }))
    }
}

/// Normalizes whitespace and truncates `raw` strictly within `max_bytes` UTF-8 bytes using
/// `str::floor_char_boundary`, appending `"…"` when truncated.
pub(super) fn truncate_utf8_excerpt(raw: &str, max_bytes: usize) -> String {
    const ELLIPSIS: &str = "…";
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= max_bytes {
        return normalized;
    }
    if max_bytes <= ELLIPSIS.len() {
        let end = normalized.floor_char_boundary(max_bytes);
        return normalized[..end].to_owned();
    }
    let cut = normalized.floor_char_boundary(max_bytes - ELLIPSIS.len());
    let mut out = normalized[..cut].trim_end().to_owned();
    out.push_str(ELLIPSIS);
    out
}

fn build_identity(
    caller: &Employee,
    office_id: &str,
    session_mode: Option<SessionMode>,
) -> ResumeIdentity {
    ResumeIdentity {
        member_id: caller.id.as_str().to_owned(),
        handle: (!caller.handle.is_empty()).then(|| caller.handle.clone()),
        name: caller.name.clone(),
        kind: match caller.kind {
            EmployeeKind::Person => MemberKind::Person,
            EmployeeKind::Agent => MemberKind::Agent,
        },
        office_id: office_id.to_owned(),
        session_mode,
    }
}

fn build_note_pointer(record: &PrivateNoteRecord) -> ResumeTaskNotePointer {
    let body = record.body_markdown.as_deref().unwrap_or("");
    let is_checkpoint = record.tags.iter().any(|t| t == PRIVATE_NOTE_CHECKPOINT_TAG);
    ResumeTaskNotePointer {
        note_id: record.id.clone(),
        revision: record.revision,
        kind: record.kind,
        title: record.title.clone(),
        excerpt: truncate_utf8_excerpt(body, RESUME_NOTE_EXCERPT_MAX_BYTES),
        tags: record.tags.clone(),
        is_checkpoint,
        updated_at: record.updated_at.clone(),
    }
}

fn build_task_items(
    tasks: Vec<TaskRecord>,
    project_keys: &BTreeMap<Id, String>,
    notes: &BTreeMap<String, PrivateNoteRecord>,
) -> Vec<ResumeTaskItem> {
    tasks
        .into_iter()
        .map(|task| {
            let task_id = task.id.as_str().to_owned();
            let project_key = task
                .project_id
                .as_ref()
                .and_then(|pid| project_keys.get(pid).cloned());
            let latest_note = notes.get(&task_id).map(build_note_pointer);
            ResumeTaskItem {
                key: task.key(),
                id: task_id,
                title: task.title,
                status: task.status,
                priority: task.priority,
                revision: task.meta.revision,
                project_id: task.project_id.map(|id| id.as_str().to_owned()),
                project_key,
                blocked_reason: task.blocked_reason,
                latest_note,
            }
        })
        .collect()
}

fn open_ask_status(card: &AskCard, now_ms: u64) -> Option<AskStatus> {
    let wait_until_ms = parse_rfc3339_ms(&card.wait_until).unwrap_or(now_ms);
    if now_ms < wait_until_ms {
        Some(AskStatus::Waiting)
    } else if card.required {
        Some(AskStatus::RequiredPending)
    } else {
        None
    }
}

fn open_ask_direction(
    message: &MessageRecord,
    card: &AskCard,
    caller_id: &Id,
    caller_is_person: bool,
    caller_is_recipient: bool,
) -> Option<ResumeAskDirection> {
    if message.sender_employee_id.as_ref() == Some(caller_id) {
        return Some(ResumeAskDirection::SentByCaller);
    }
    let addressed_to_caller = card
        .addressed_to
        .as_deref()
        .is_none_or(|target| target == caller_id.as_str());
    let eligible_for_approval = !card.approval || caller_is_person;
    (caller_is_recipient && addressed_to_caller && eligible_for_approval)
        .then_some(ResumeAskDirection::AwaitingCaller)
}

fn to_resume_open_ask(
    message: MessageRecord,
    caller_id: &Id,
    caller_is_person: bool,
    caller_is_recipient: bool,
    now_ms: u64,
) -> Option<ResumeOpenAsk> {
    let card = parse_ask_card(&message)?;
    let status = open_ask_status(&card, now_ms)?;
    let direction = open_ask_direction(
        &message,
        &card,
        caller_id,
        caller_is_person,
        caller_is_recipient,
    )?;
    let raw_prompt = card
        .questions
        .first()
        .map(|q| q.prompt.as_str())
        .unwrap_or(message.body_markdown.as_str());
    let from_member_id = message
        .sender_employee_id
        .as_ref()
        .map(|id| id.as_str().to_owned())
        .unwrap_or_else(|| OFFICE_SENDER.to_owned());
    Some(ResumeOpenAsk {
        ask_id: message.id,
        conversation_id: message.conversation_id.as_str().to_owned(),
        direction,
        status,
        from_member_id,
        from_display_name: message.sender_display_name,
        addressed_to: card.addressed_to,
        blocks: card.blocks,
        required: card.required,
        approval: card.approval,
        question_count: card.questions.len() as u32,
        prompt_excerpt: truncate_utf8_excerpt(raw_prompt, RESUME_ASK_EXCERPT_MAX_BYTES),
        wait_until: card.wait_until,
        created_at: message.created_at,
    })
}

fn build_open_asks(
    candidates: Vec<(MessageRecord, bool)>,
    caller_id: &Id,
    caller_is_person: bool,
    now_ms: u64,
    limit: usize,
) -> Vec<ResumeOpenAsk> {
    candidates
        .into_iter()
        .filter_map(|(msg, is_recipient)| {
            to_resume_open_ask(msg, caller_id, caller_is_person, is_recipient, now_ms)
        })
        .take(limit)
        .collect()
}

fn budget_severity(state: BudgetStateSummary) -> u8 {
    match state {
        BudgetStateSummary::Held => 5,
        BudgetStateSummary::ReachedCap => 4,
        BudgetStateSummary::StopPending => 3,
        BudgetStateSummary::Warning80 => 2,
        BudgetStateSummary::Ok => 1,
        _ => 0,
    }
}

fn to_budget_period(view: BudgetView) -> ResumeBudgetPeriod {
    ResumeBudgetPeriod {
        state_summary: view.state_summary,
        hold_state: view.hold_state,
        unit: view.unit,
        effective_limit: view.effective_limit,
        used_amount: view.used_amount,
        remaining_amount: view.remaining_amount,
        period_key: view.period_key,
    }
}

fn build_budget_summary(own: BudgetAgentOwnResult) -> Option<ResumeBudgetSummary> {
    let daily = own.daily.map(to_budget_period);
    let monthly = own.monthly.map(to_budget_period);
    if daily.is_none() && monthly.is_none() {
        return None;
    }
    let status = [
        daily.as_ref().map(|p| p.state_summary),
        monthly.as_ref().map(|p| p.state_summary),
    ]
    .into_iter()
    .flatten()
    .max_by_key(|s| budget_severity(*s))
    .unwrap_or(BudgetStateSummary::Ok);
    Some(ResumeBudgetSummary {
        status,
        daily,
        monthly,
    })
}

pub(super) fn assemble_resume(
    office: &Office,
    actor: &str,
    session_mode: Option<SessionMode>,
    mut worktrees: Vec<ResumeWorktreeItem>,
    limits: &ResumeLimits,
) -> Result<Result<AgentResumeResult, ApiError>, OfficeError> {
    let now_ms = office.now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let snap = match office.read_snapshot(AgentResumeRead {
        actor: actor.to_owned(),
        now_utc,
        tasks_limit: limits.tasks,
        inbox_limit: limits.inbox,
        asks_limit: limits.asks,
    })? {
        Ok(snap) => snap,
        Err(err) => return Ok(Err(err)),
    };
    let task_ids: Vec<String> = snap
        .active_tasks
        .iter()
        .map(|t| t.id.as_str().to_owned())
        .collect();
    let latest_notes = if task_ids.is_empty() {
        BTreeMap::new()
    } else {
        match office.latest_author_notes_for_tasks(snap.caller.id.as_str(), &task_ids)? {
            Ok(notes) => notes,
            Err(err) if err.code == ErrorCode::Unavailable => BTreeMap::new(),
            Err(err) => return Ok(Err(err)),
        }
    };

    let identity = build_identity(&snap.caller, &office.info().office_id, session_mode);
    let active_tasks = build_task_items(snap.active_tasks, &snap.project_keys, &latest_notes);
    let caller_is_person = snap.caller.kind == EmployeeKind::Person;
    let open_asks = build_open_asks(
        snap.open_ask_candidates,
        &snap.caller.id,
        caller_is_person,
        now_ms,
        limits.asks,
    );
    worktrees.truncate(limits.worktrees);

    let budget = if snap.caller.kind == EmployeeKind::Agent {
        match office.budget_agent_own(
            snap.caller.id.as_str(),
            BudgetAgentOwnParams { task_id: None },
        )? {
            Ok(own) => build_budget_summary(own),
            Err(_) => None,
        }
    } else {
        None
    };

    Ok(Ok(AgentResumeResult {
        identity,
        unread_inbox: snap.unread_inbox,
        active_tasks,
        active_tasks_total: snap.active_tasks_total,
        open_asks,
        active_worktrees: worktrees,
        budget,
    }))
}
