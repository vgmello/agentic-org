//! Read-only Inbox source for live supervised sessions' pending tool approvals (`TASK-102`).
//!
//! Surfaces only safe redacted metadata (employee identity, tool name, escalation warning,
//! timestamp, decision calls, and `OpenTarget::Member`); never raw tool inputs, secrets,
//! environment tokens, private DM bodies, or private task notes (`ADR-0031`, `doc-39`).

use std::collections::HashMap;
use std::sync::Arc;

use org_office::Office;
use org_office_sessions::{PendingToolApproval, SessionManager};
use org_proto::inbox::{
    Action, ActionInput, InboxClears, InboxCounts, InboxItem, InboxKind, InboxState, Open,
    OpenTarget,
};
use org_proto::sessions::MemberSessionApprovalDecide;
use org_proto::{ApiError, Call, ErrorCode, Member, MemberKind, OrgSnapshot};
use org_redact::sanitize::sanitize_single_line_display;
use serde_json::{json, Map};

use super::{Query, Source, SourceFailure, SourceRead};

const DENY_REASON_MAX_CHARS: u32 = 500;

type SnapshotFn = Box<dyn Fn() -> Result<OrgSnapshot, ApiError> + Send + Sync>;

/// Owner Inbox source exposing live `SessionState::WaitingApproval` tool escalations.
pub(crate) struct PendingApprovalsInboxSource {
    office: Arc<Office>,
    sessions: Arc<SessionManager>,
    office_id: String,
    snapshot_fn: SnapshotFn,
}

impl PendingApprovalsInboxSource {
    pub(crate) fn new(office: Arc<Office>, sessions: Arc<SessionManager>) -> Self {
        let office_id = office.info().office_id.clone();
        let snapshot_office = Arc::clone(&office);
        let snapshot_fn: SnapshotFn = Box::new(move || {
            snapshot_office.org_snapshot().map_err(|error| {
                crate::logged("inbox.tool_approvals", &error);
                ApiError::new(
                    ErrorCode::StorageError,
                    "organization snapshot unavailable for tool approvals",
                )
            })
        });
        Self {
            office,
            sessions,
            office_id,
            snapshot_fn,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_snapshot_for_test(
        office: Arc<Office>,
        sessions: Arc<SessionManager>,
        snapshot_fn: SnapshotFn,
    ) -> Self {
        let office_id = office.info().office_id.clone();
        Self {
            office,
            sessions,
            office_id,
            snapshot_fn,
        }
    }

    /// Reads pending tool approval Inbox items strictly authorized for `caller_id`.
    /// Only the live organization owner (matching `member.session_approval`) is admitted.
    pub(crate) fn read_for_caller(
        &self,
        caller_id: &str,
        query: &Query<'_>,
    ) -> Result<SourceRead, ApiError> {
        let approvals = self.sessions.pending_approvals_for(caller_id)?;
        let snapshot = (self.snapshot_fn)()?;
        ensure_live_owner_in_snapshot(&snapshot, caller_id)?;
        if approvals.is_empty() {
            return Ok(SourceRead {
                items: Vec::new(),
                counts: InboxCounts::default(),
            });
        }
        let count = approvals.len() as u64;
        let counts = InboxCounts {
            approvals: count,
            total: count,
            ..InboxCounts::default()
        };
        if query.limit == 0
            || query.state != InboxState::Open
            || query
                .kinds
                .is_some_and(|kinds| !kinds.contains(&InboxKind::Approval))
        {
            return Ok(SourceRead {
                items: Vec::new(),
                counts,
            });
        }
        let items = collect_filtered_items(&self.office_id, &snapshot, &approvals, query);
        Ok(SourceRead { items, counts })
    }
}

impl Source for PendingApprovalsInboxSource {
    fn name(&self) -> &'static str {
        "tool_approvals"
    }

    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        let owner_member_id = self.office.effective_owner_member_id();
        self.read_for_caller(&owner_member_id, query)
            .map_err(|_| SourceFailure)
    }
}

fn ensure_live_owner_in_snapshot(snapshot: &OrgSnapshot, caller_id: &str) -> Result<(), ApiError> {
    let is_live_owner = snapshot
        .members
        .iter()
        .any(|m| m.id == caller_id && m.kind == MemberKind::Person && m.owner);
    if is_live_owner {
        return Ok(());
    }
    Err(ApiError::new(
        ErrorCode::NotAuthorized,
        "only the live organization owner may view pending tool approvals",
    ))
}

fn collect_filtered_items(
    office_id: &str,
    snapshot: &OrgSnapshot,
    approvals: &[PendingToolApproval],
    query: &Query<'_>,
) -> Vec<InboxItem> {
    let members_by_id: HashMap<&str, &Member> = snapshot
        .members
        .iter()
        .map(|member| (member.id.as_str(), member))
        .collect();
    let mut items = Vec::new();
    for approval in approvals {
        let member = members_by_id.get(approval.employee_id.as_str()).copied();
        let home_team = member.and_then(|m| m.home_team_id.as_deref());
        if !matches_scope_filters(snapshot, home_team, query) {
            continue;
        }
        let item = build_approval_inbox_item(office_id, approval, member);
        if query.after.is_none_or(|after| after.admits(&item)) && query.admits_files(&item) {
            items.push(item);
        }
    }
    items.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| a.reference.cmp(&b.reference))
    });
    items.truncate(query.limit.min(500));
    items
}

fn matches_scope_filters(
    snapshot: &OrgSnapshot,
    home_team: Option<&str>,
    query: &Query<'_>,
) -> bool {
    if query.team.is_some_and(|team| home_team != Some(team)) {
        return false;
    }
    if let Some(project) = query.project {
        return home_team.is_some_and(|team_id| {
            snapshot
                .project_teams
                .iter()
                .any(|link| link.project_id == project && link.team_id == team_id)
        });
    }
    true
}

fn build_approval_inbox_item(
    office_id: &str,
    approval: &PendingToolApproval,
    member: Option<&Member>,
) -> InboxItem {
    let safe_tool = sanitize_single_line_display(&org_redact::redact_text(&approval.tool, &[]));
    let display_tool = if safe_tool.is_empty() {
        "unknown tool"
    } else {
        safe_tool.as_str()
    };
    let safe_name = sanitize_single_line_display(member.map_or("Agent", |m| m.name.as_str()));
    let employee_name = if safe_name.is_empty() {
        "Agent"
    } else {
        safe_name.as_str()
    };
    let summary = match approval.escalation.as_str() {
        "permission_change" => {
            format!("{employee_name} requests permission change for {display_tool}")
        }
        "none" => format!("{employee_name} requests approval to use {display_tool}"),
        _ => format!("{employee_name} requests escalated approval for {display_tool}"),
    };
    let context = approval
        .reason
        .as_deref()
        .map(|reason| sanitize_single_line_display(&org_redact::redact_text(reason, &[])))
        .filter(|reason| !reason.is_empty());
    InboxItem {
        reference: format!(
            "{office_id}:tool_approval:{}:{}",
            approval.employee_id, approval.approval_id
        ),
        source: "tool_approval".into(),
        kind: InboxKind::Approval,
        member: Some(approval.employee_id.clone()),
        project: None,
        team: member.and_then(|m| m.home_team_id.clone()),
        summary,
        context,
        recommendation: None,
        options: None,
        created_at_ms: org_office::parse_rfc3339_ms(&approval.requested_at),
        expires_at_ms: None,
        state: InboxState::Open,
        handled_at_ms: None,
        outcome: None,
        clears: InboxClears::Answered,
        dismissable: false,
        actions: build_approval_actions(approval),
        attachments: Vec::new(),
    }
}

fn build_approval_actions(approval: &PendingToolApproval) -> Vec<Action> {
    let mut actions = vec![approval_call_action(
        "Allow once",
        approval,
        "allow_once",
        None,
    )];
    if approval.allows_always() {
        actions.push(approval_call_action(
            "Allow always",
            approval,
            "allow_always",
            None,
        ));
    }
    actions.push(approval_call_action(
        "Deny",
        approval,
        "deny",
        Some(ActionInput {
            field: "reason".into(),
            required: false,
            max_chars: DENY_REASON_MAX_CHARS,
            attachments_field: None,
            max_attachments: 0,
        }),
    ));
    actions.push(Action::Open {
        label: "Open agent".into(),
        open: Open {
            target: OpenTarget::Member,
            id: approval.employee_id.clone(),
        },
    });
    actions
}

fn approval_call_action(
    label: &str,
    approval: &PendingToolApproval,
    decision: &str,
    input: Option<ActionInput>,
) -> Action {
    let mut params = Map::new();
    params.insert("employee_id".into(), json!(approval.employee_id));
    params.insert("approval_id".into(), json!(approval.approval_id));
    params.insert("decision".into(), json!(decision));
    Action::Call {
        label: label.into(),
        op: MemberSessionApprovalDecide::METHOD.into(),
        params,
        fresh: vec!["command_id".into()],
        input,
    }
}

#[cfg(test)]
mod tests;
