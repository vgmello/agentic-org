//! Channel summaries and usage accounting for watcher model calls.

use std::time::Duration;

use crate::watcher_store::{UsageLedgerEntry, WatcherStore};
use org_copilot::watcher::{
    run_channel_summarize, ChannelSummarizeInput, SummarizeMessageInput, WatchError,
};
use org_copilot::Usage;
use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::chat::{ChatChannelSummarizeParams, ChatChannelSummarizeResult};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use super::watches::resolve_watching_runtime;
use super::{next_local_id, now_ms, WatcherConfig, WatcherRuntimeProvider};

pub(super) struct UsageContext<'a> {
    pub(super) consumer_kind: &'a str,
    pub(super) employee_id: Option<&'a str>,
    pub(super) project_id: Option<&'a str>,
    pub(super) conversation_id: &'a str,
    pub(super) feature: &'a str,
    pub(super) model_profile_id: Option<&'a str>,
    pub(super) question_ids: Vec<String>,
    pub(super) outcome: &'a str,
    pub(super) usage: Option<Usage>,
    pub(super) duration: Duration,
}

pub(super) fn summarize_channel_impl(
    office: &Office,
    sessions: Option<&SessionManager>,
    store: &WatcherStore,
    provider: &dyn WatcherRuntimeProvider,
    config: &WatcherConfig,
    caller_id: &str,
    params: &ChatChannelSummarizeParams,
) -> Result<ChatChannelSummarizeResult, ApiError> {
    let question = params.question.trim();
    if question.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "question must not be empty",
        ));
    }
    let limit = params.limit.unwrap_or(200).clamp(1, 200) as usize;
    let (conv, msgs) = office
        .channel_messages_for_caller(caller_id, &params.channel, limit)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read channel"))??;

    let (profile_id, runtime) =
        resolve_watching_runtime(office, sessions, provider).map_err(|reason| {
            ApiError::new(
                ErrorCode::Unavailable,
                format!("channel summarize unavailable: {reason}"),
            )
        })?;
    let input = ChannelSummarizeInput {
        channel: conv
            .name
            .clone()
            .unwrap_or_else(|| conv.id.as_str().to_owned()),
        question: question.to_owned(),
        messages: msgs
            .iter()
            .map(|m| SummarizeMessageInput {
                id: m.id.clone(),
                author: m.sender_display_name.clone(),
                text: m.body_markdown.clone(),
                created_at: m.created_at.clone(),
            })
            .collect(),
    };
    let outcome = run_channel_summarize(runtime, config.limits.clone(), caller_id, &input);
    let usage_record_id = if outcome.model_called {
        record_usage(
            office,
            store,
            UsageContext {
                consumer_kind: "employee",
                employee_id: Some(caller_id),
                project_id: conv.project_id.as_ref().map(Id::as_str),
                conversation_id: conv.id.as_str(),
                feature: "channel.summarize",
                model_profile_id: Some(&profile_id),
                question_ids: Vec::new(),
                outcome: if outcome.result.is_ok() {
                    "ok"
                } else {
                    "failed"
                },
                usage: outcome.usage,
                duration: outcome.duration,
            },
        )
        .ok()
    } else {
        None
    };

    let res = outcome.result.map_err(|err| match err {
        WatchError::Timeout => ApiError::new(ErrorCode::Timeout, err.to_string()),
        WatchError::InputRejected => ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
        _ => ApiError::new(ErrorCode::Unavailable, err.to_string()),
    })?;
    Ok(ChatChannelSummarizeResult {
        conversation_id: conv.id.as_str().to_owned(),
        summary: res.summary,
        message_ids: res.message_ids,
        derived_notice: res.derived_notice,
        usage_record_id,
    })
}

pub(super) fn record_usage(
    office: &Office,
    store: &WatcherStore,
    ctx: UsageContext<'_>,
) -> std::io::Result<String> {
    let id = next_local_id("usage");
    let entry = UsageLedgerEntry {
        id: id.clone(),
        consumer_kind: ctx.consumer_kind.to_owned(),
        employee_id: ctx.employee_id.map(str::to_owned),
        organization_id: office.info().org_id.clone(),
        project_id: ctx.project_id.map(str::to_owned),
        conversation_id: ctx.conversation_id.to_owned(),
        feature: ctx.feature.to_owned(),
        model_profile_id: ctx.model_profile_id.map(str::to_owned),
        question_ids: ctx.question_ids,
        outcome: ctx.outcome.to_owned(),
        input_tokens: ctx.usage.and_then(|u| u.input_tokens),
        output_tokens: ctx.usage.and_then(|u| u.output_tokens),
        duration_ms: ctx.duration.as_millis() as u64,
        recorded_at: org_office::format_rfc3339_ms(now_ms()),
    };
    store.append_usage(&entry)?;
    let _ = office.record_office_usage(org_proto::costs::CostUsageRecordParams {
        id: None,
        producer_id: format!("watcher-{}", office.info().office_id),
        source_event_id: id.clone(),
        run_id: None,
        actor_kind: if entry.employee_id.is_some() {
            org_proto::costs::UsageActorKind::Agent
        } else {
            org_proto::costs::UsageActorKind::OneOff
        },
        employee_id: entry.employee_id.clone(),
        runtime: entry.feature.clone(),
        provider: None,
        model: entry.model_profile_id.clone(),
        occurred_at: entry.recorded_at.clone(),
        source_metadata_json: Some(
            serde_json::json!({
                "conversation_id": entry.conversation_id,
                "outcome": entry.outcome,
                "duration_ms": entry.duration_ms,
            })
            .to_string(),
        ),
        input_tokens: entry.input_tokens,
        output_tokens: entry.output_tokens,
        cache_read_tokens: (entry.input_tokens.is_some() || entry.output_tokens.is_some())
            .then_some(0),
        cache_write_tokens: (entry.input_tokens.is_some() || entry.output_tokens.is_some())
            .then_some(0),
        reasoning_tokens: (entry.input_tokens.is_some() || entry.output_tokens.is_some())
            .then_some(0),
        token_issue: None,
        runtime_cost_micro_usd: None,
        is_subscription: false,
        scope_kind: if entry.project_id.is_some() {
            org_proto::costs::UsageScopeKind::Project
        } else {
            org_proto::costs::UsageScopeKind::Organization
        },
        task_id: None,
        project_id: entry.project_id.clone(),
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: None,
        task_type_snapshot: None,
        work_type: None,
        attribution_basis: if entry.project_id.is_some() {
            org_proto::costs::AttributionBasis::ExplicitContext
        } else {
            org_proto::costs::AttributionBasis::Unattributed
        },
    });
    Ok(id)
}
