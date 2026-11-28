//! Write handlers for `review request|route|finding|verdict|resubmit|close` (`TASK-92.1`).

use org_command::{Answer, Refusal, Request};
use org_proto::reviews::{
    AnchorKind, FindingAnchor, ReviewClose, ReviewCloseParams, ReviewFinding, ReviewFindingDraft,
    ReviewFindingOp, ReviewFindingParams, ReviewFindingResult, ReviewMutationResult, ReviewRequest,
    ReviewRequestParams, ReviewResubmit, ReviewResubmitParams, ReviewRoundResult, ReviewRoute,
    ReviewRouteParams, ReviewRouteResult, ReviewRoutingSpec, ReviewTargetKind, ReviewVerdictCall,
    ReviewVerdictParams, ReviewVerdictResult, RoutingMode, REVIEW_MAX_ATTACHMENTS,
};
use org_proto::Call;
use serde_json::{json, Map, Value};

use crate::runtime::domain::CommandCue;
use crate::runtime::file_transfer::resolve_attach_ids;
use crate::runtime::input::{
    encode, entity_id, is_set, opt_str_list, opt_string, require_if_match, require_str,
    write_answer,
};
use crate::runtime::transport::OfficeTransport;

use super::parse::{
    cmd_id, invalid, opt_u32, parse_finding_status, parse_session_mode, parse_severity,
    parse_target_kind, parse_verdict, unreadable,
};

/// `Use when:` guidance of each write `review` verb.
pub(super) const CUES: &[CommandCue] = &[
    ("review", "close", "closing a review permanently once merged, abandoned, or superseded (requires `--if-match`)."),
    ("review", "finding", "raising a finding (`--action add`) or replying to / resolving an existing finding (`--action reply`)."),
    ("review", "request", "opening a review for code, docs, tasks, or artifacts and pinning round 1."),
    ("review", "resubmit", "submitting a new review round after fixes (warm by default; `--fresh` for a clean final pass; requires `--if-match`)."),
    ("review", "route", "replacing or reassigning reviewers on an open review (requires `--if-match`)."),
    ("review", "verdict", "submitting a reviewer verdict and optional atomic findings batch on the current round."),
];

pub(super) fn write_review_request<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let i = &request.input;
    let target_kind = opt_string(i, "target_kind")
        .map(|s| parse_target_kind(&s))
        .transpose()?
        .unwrap_or(ReviewTargetKind::Code);
    let (session_mode, _) = parse_session_mode(i)?;
    let routing = parse_routing(i, false)?;
    let params = ReviewRequestParams {
        command_id,
        review_id: entity_id(request, None, "review-request")?,
        target_kind,
        title: require_str(i, "title")?.to_owned(),
        body_markdown: opt_string(i, "body").unwrap_or_default(),
        kind_slug: opt_string(i, "kind").unwrap_or_else(|| "code-review".to_owned()),
        category_slug: opt_string(i, "category"),
        project_id: opt_string(i, "project_id"),
        task_id: opt_string(i, "task_id"),
        repository_id: opt_string(i, "repository_id"),
        branch: opt_string(i, "branch"),
        target_ref: opt_string(i, "target_ref"),
        base_oid: opt_string(i, "base_oid"),
        expected_head_oid: opt_string(i, "expected_head_oid"),
        independent: is_set(i, "independent"),
        routing,
        session_mode,
        guidance_refs: opt_str_list(i, "guidance_refs").unwrap_or_default(),
        criteria: opt_str_list(i, "criteria").unwrap_or_default(),
    };
    let raw = transport.call_method(ReviewRequest::METHOD, json!(params))?;
    let res: ReviewRoundResult = serde_json::from_value(raw).map_err(unreadable)?;
    let rev = res.review.revision;
    Ok(write_answer(
        request,
        encode(&res, ReviewRequest::METHOD)?,
        Some(rev),
    ))
}

pub(super) fn write_review_route<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let routing = parse_routing(i, true)?.ok_or_else(|| {
        invalid(
            "review route requires --reviewer, --team, or --auto-assign.",
            "Specify explicit reviewers, a reviewer pool team, or --auto-assign.",
        )
    })?;
    let (session_mode, _) = parse_session_mode(i)?;
    let params = ReviewRouteParams {
        command_id,
        review: require_str(i, "review")?.to_owned(),
        expected_revision,
        routing,
        session_mode,
    };
    let raw = transport.call_method(ReviewRoute::METHOD, json!(params))?;
    let res: ReviewRouteResult = serde_json::from_value(raw).map_err(unreadable)?;
    let rev = res.review.revision;
    Ok(write_answer(
        request,
        encode(&res, ReviewRoute::METHOD)?,
        Some(rev),
    ))
}

pub(super) fn write_review_finding<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let i = &request.input;
    let review = require_str(i, "review")?.to_owned();
    let action = require_str(i, "action")?;
    let attach = opt_str_list(i, "attach").unwrap_or_default();
    let att_ids = opt_str_list(i, "attachment_id").unwrap_or_default();
    let resolved = resolve_attach_ids(transport, &attach, &att_ids, REVIEW_MAX_ATTACHMENTS)?;
    let op = match build_finding_op(request, action, resolved.ids.clone()) {
        Ok(op) => op,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
    let params = ReviewFindingParams {
        command_id,
        review,
        op,
    };
    let raw = match transport.call_method(ReviewFinding::METHOD, json!(params)) {
        Ok(raw) => raw,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
    let res: ReviewFindingResult = serde_json::from_value(raw).map_err(unreadable)?;
    let rev = res.review.revision;
    Ok(write_answer(
        request,
        encode(&res, ReviewFinding::METHOD)?,
        Some(rev),
    ))
}

pub(super) fn write_review_verdict<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let i = &request.input;
    let review = require_str(i, "review")?.to_owned();
    let round_number = opt_u32(i, "round")?.ok_or_else(|| {
        invalid(
            "Missing required --round flag.",
            "Pass --round <round_number>.",
        )
    })?;
    let verdict = parse_verdict(require_str(i, "verdict")?)?;
    let findings = parse_verdict_findings(request)?;
    let attach = opt_str_list(i, "attach").unwrap_or_default();
    let att_ids = opt_str_list(i, "attachment_id").unwrap_or_default();
    let resolved = resolve_attach_ids(transport, &attach, &att_ids, REVIEW_MAX_ATTACHMENTS)?;
    let params = ReviewVerdictParams {
        command_id,
        review,
        round_number,
        head_oid: opt_string(i, "head_oid"),
        verdict,
        summary_markdown: opt_string(i, "summary").unwrap_or_default(),
        findings,
        attachment_ids: resolved.ids.clone(),
    };
    let raw = match transport.call_method(ReviewVerdictCall::METHOD, json!(params)) {
        Ok(raw) => raw,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
    let res: ReviewVerdictResult = serde_json::from_value(raw).map_err(unreadable)?;
    let rev = res.review.revision;
    Ok(write_answer(
        request,
        encode(&res, ReviewVerdictCall::METHOD)?,
        Some(rev),
    ))
}

pub(super) fn write_review_resubmit<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let (session_mode, fresh) = parse_session_mode(i)?;
    let params = ReviewResubmitParams {
        command_id,
        review: require_str(i, "review")?.to_owned(),
        expected_revision,
        expected_head_oid: opt_string(i, "expected_head_oid"),
        session_mode,
        fresh,
        note_markdown: opt_string(i, "note").unwrap_or_default(),
    };
    let raw = transport.call_method(ReviewResubmit::METHOD, json!(params))?;
    let res: ReviewRoundResult = serde_json::from_value(raw).map_err(unreadable)?;
    let rev = res.review.revision;
    Ok(write_answer(
        request,
        encode(&res, ReviewResubmit::METHOD)?,
        Some(rev),
    ))
}

pub(super) fn write_review_close<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let params = ReviewCloseParams {
        command_id,
        review: require_str(i, "review")?.to_owned(),
        expected_revision,
        reason: opt_string(i, "reason"),
    };
    let raw = transport.call_method(ReviewClose::METHOD, json!(params))?;
    let res: ReviewMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    let rev = res.review.revision;
    Ok(write_answer(
        request,
        encode(&res, ReviewClose::METHOD)?,
        Some(rev),
    ))
}

fn parse_routing(
    input: &Map<String, Value>,
    required: bool,
) -> Result<Option<ReviewRoutingSpec>, Refusal> {
    let reviewers = opt_str_list(input, "reviewer").unwrap_or_default();
    let team = opt_string(input, "team");
    let auto_assign = is_set(input, "auto_assign");
    let modes =
        usize::from(!reviewers.is_empty()) + usize::from(team.is_some()) + usize::from(auto_assign);
    if modes > 1 {
        return Err(invalid(
            "--reviewer, --team, and --auto-assign are mutually exclusive.",
            "Choose one routing mode.",
        ));
    }
    if !reviewers.is_empty() {
        return Ok(Some(ReviewRoutingSpec {
            mode: RoutingMode::Explicit,
            reviewer_ids: reviewers,
            team_id: None,
        }));
    }
    if let Some(team_id) = team {
        return Ok(Some(ReviewRoutingSpec {
            mode: RoutingMode::Pool,
            reviewer_ids: Vec::new(),
            team_id: Some(team_id),
        }));
    }
    if auto_assign {
        return Ok(Some(ReviewRoutingSpec {
            mode: RoutingMode::SingleAgent,
            reviewer_ids: Vec::new(),
            team_id: None,
        }));
    }
    if required {
        return Ok(None);
    }
    Ok(None)
}

fn build_finding_op(
    request: &Request,
    action: &str,
    attachment_ids: Vec<String>,
) -> Result<ReviewFindingOp, Refusal> {
    let i = &request.input;
    match action {
        "add" => {
            let severity = parse_severity(require_str(i, "severity")?)?;
            let body_markdown = require_str(i, "body")?.to_owned();
            let anchor = parse_anchor(i)?;
            let finding_id = entity_id(request, Some("finding_id"), "review-finding")?;
            Ok(ReviewFindingOp::Add(ReviewFindingDraft {
                finding_id,
                severity,
                anchor,
                body_markdown,
                attachment_ids,
            }))
        }
        "reply" => {
            let finding_id = require_str(i, "finding_id")?.to_owned();
            let reply_id = entity_id(request, None, "review-reply")?;
            let set_status = opt_string(i, "set_status")
                .map(|s| parse_finding_status(&s))
                .transpose()?;
            Ok(ReviewFindingOp::Reply {
                finding_id,
                reply_id,
                body_markdown: opt_string(i, "body").unwrap_or_default(),
                set_status,
                attachment_ids,
            })
        }
        other => Err(invalid(
            format!("invalid action `{other}`"),
            "Use --action add or --action reply.",
        )),
    }
}

fn parse_anchor(i: &Map<String, Value>) -> Result<FindingAnchor, Refusal> {
    let explicit_kind = opt_string(i, "anchor_kind");
    let path = opt_string(i, "path");
    let start_line = opt_u32(i, "start_line")?;
    let end_line = opt_u32(i, "end_line")?;
    let doc_section = opt_string(i, "doc_section");
    let checklist_item_id = opt_string(i, "checklist_item_id");

    let kind = match explicit_kind.as_deref() {
        Some(raw) => AnchorKind::parse(raw).ok_or_else(|| {
            invalid(
                format!("invalid anchor_kind `{raw}`"),
                "Use general, file_line, doc_section, or checklist_item.",
            )
        })?,
        None if start_line.is_some() || end_line.is_some() => AnchorKind::FileLine,
        None if doc_section.is_some() => AnchorKind::DocSection,
        None if checklist_item_id.is_some() => AnchorKind::ChecklistItem,
        None => AnchorKind::General,
    };
    let anchor = FindingAnchor {
        kind,
        path,
        start_line,
        end_line,
        anchor_snippet_sha256: None,
        doc_section,
        checklist_item_id,
    };
    anchor
        .check()
        .map_err(|msg| invalid(msg, "Check anchor flags for the chosen anchor kind."))?;
    Ok(anchor)
}

fn parse_verdict_findings(request: &Request) -> Result<Vec<ReviewFindingDraft>, Refusal> {
    let Some(raw) = request.input.get("findings") else {
        return Ok(Vec::new());
    };
    let items = raw.as_array().ok_or_else(|| {
        invalid(
            "`findings` must be a JSON array of finding draft objects.",
            "Pass `--input-json '{\"findings\":[...]}'`.",
        )
    })?;
    let mut out = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let mut obj = item.as_object().cloned().ok_or_else(|| {
            invalid(
                format!("`findings[{idx}]` must be a JSON object."),
                "Each item must be a ReviewFindingDraft object.",
            )
        })?;
        if !obj.contains_key("finding_id") {
            let derived = entity_id(request, None, &format!("review-verdict-finding-{idx}"))?;
            obj.insert("finding_id".into(), Value::String(derived));
        }
        let draft: ReviewFindingDraft =
            serde_json::from_value(Value::Object(obj)).map_err(|e| {
                invalid(
                    format!("invalid finding draft at `findings[{idx}]`: {e}"),
                    "Provide severity, body_markdown, and optional anchor/attachment_ids.",
                )
            })?;
        out.push(draft);
    }
    Ok(out)
}
