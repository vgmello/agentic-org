//! Strict `deny_unknown_fields` JSON request body decoders for `/api/v1/organizations/{org}/reviews*`
//! write endpoints (`TASK-92.2`).

use org_proto::reviews::{
    AnchorKind, FindingAnchor, FindingSeverity, FindingStatus, ReviewCloseParams,
    ReviewFindingDraft, ReviewFindingOp, ReviewFindingParams, ReviewRequestParams,
    ReviewResubmitParams, ReviewRouteParams, ReviewRoutingSpec, ReviewSessionMode,
    ReviewTargetKind, ReviewVerdict, ReviewVerdictParams, RoutingMode, REVIEW_BODY_MAX_CHARS,
    REVIEW_CLOSE_REASON_MAX_CHARS, REVIEW_CRITERION_MAX_CHARS, REVIEW_FINDING_BODY_MAX_CHARS,
    REVIEW_GUIDANCE_REF_MAX_CHARS, REVIEW_MAX_ATTACHMENTS, REVIEW_MAX_CRITERIA,
    REVIEW_MAX_FINDINGS_PER_VERDICT, REVIEW_MAX_GUIDANCE_REFS, REVIEW_REPLY_BODY_MAX_CHARS,
    REVIEW_SLUG_MAX_LEN, REVIEW_SUMMARY_MAX_CHARS, REVIEW_TITLE_MAX_CHARS,
};
use serde::Deserialize;

use super::super::invalid;
use super::query::{valid_optional_ref, valid_optional_token, valid_single_line_token, valid_uuid};
use crate::service::ApiReply;

const MAX_BRANCH_OR_TARGET_BYTES: usize = 512;
const MAX_OID_BYTES: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutingSpecBody {
    mode: RoutingMode,
    #[serde(default)]
    reviewer_ids: Vec<String>,
    #[serde(default)]
    team_id: Option<String>,
}

impl RoutingSpecBody {
    fn into_proto(self) -> Result<ReviewRoutingSpec, ApiReply> {
        for id in &self.reviewer_ids {
            valid_uuid(id)?;
        }
        valid_optional_ref(self.team_id.as_deref())?;
        let spec = ReviewRoutingSpec {
            mode: self.mode,
            reviewer_ids: self.reviewer_ids,
            team_id: self.team_id,
        };
        spec.check().map_err(|_| invalid())?;
        Ok(spec)
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingAnchorBody {
    #[serde(default)]
    kind: AnchorKind,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    start_line: Option<u32>,
    #[serde(default)]
    end_line: Option<u32>,
    #[serde(default)]
    anchor_snippet_sha256: Option<String>,
    #[serde(default)]
    doc_section: Option<String>,
    #[serde(default)]
    checklist_item_id: Option<String>,
}

impl FindingAnchorBody {
    fn into_proto(self) -> Result<FindingAnchor, ApiReply> {
        let anchor = FindingAnchor {
            kind: self.kind,
            path: self.path,
            start_line: self.start_line,
            end_line: self.end_line,
            anchor_snippet_sha256: self.anchor_snippet_sha256,
            doc_section: self.doc_section,
            checklist_item_id: self.checklist_item_id,
        };
        anchor.check().map_err(|_| invalid())?;
        Ok(anchor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingDraftBody {
    finding_id: String,
    severity: FindingSeverity,
    #[serde(default)]
    anchor: FindingAnchorBody,
    body_markdown: String,
    #[serde(default)]
    attachment_ids: Vec<String>,
}

impl FindingDraftBody {
    fn into_proto(self) -> Result<ReviewFindingDraft, ApiReply> {
        valid_uuid(&self.finding_id)?;
        let chars = self.body_markdown.chars().count();
        if self.body_markdown.trim().is_empty() || chars > REVIEW_FINDING_BODY_MAX_CHARS {
            return Err(invalid());
        }
        validate_attachment_ids(&self.attachment_ids)?;
        Ok(ReviewFindingDraft {
            finding_id: self.finding_id,
            severity: self.severity,
            anchor: self.anchor.into_proto()?,
            body_markdown: self.body_markdown,
            attachment_ids: self.attachment_ids,
        })
    }
}

fn validate_attachment_ids(ids: &[String]) -> Result<(), ApiReply> {
    if ids.len() > REVIEW_MAX_ATTACHMENTS {
        return Err(invalid());
    }
    for id in ids {
        valid_uuid(id)?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestBody {
    command_id: String,
    review_id: String,
    target_kind: ReviewTargetKind,
    title: String,
    #[serde(default)]
    body_markdown: String,
    kind_slug: String,
    #[serde(default)]
    category_slug: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    repository_id: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    target_ref: Option<String>,
    #[serde(default)]
    base_oid: Option<String>,
    #[serde(default)]
    expected_head_oid: Option<String>,
    #[serde(default)]
    independent: bool,
    #[serde(default)]
    routing: Option<RoutingSpecBody>,
    #[serde(default)]
    session_mode: Option<ReviewSessionMode>,
    #[serde(default)]
    guidance_refs: Vec<String>,
    #[serde(default)]
    criteria: Vec<String>,
}

pub(super) fn decode_request_body(bytes: &[u8]) -> Result<ReviewRequestParams, ApiReply> {
    let body: RequestBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    valid_uuid(&body.command_id)?;
    valid_uuid(&body.review_id)?;
    if body.title.trim().is_empty()
        || body.title.chars().count() > REVIEW_TITLE_MAX_CHARS
        || body.title.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    if body.body_markdown.chars().count() > REVIEW_BODY_MAX_CHARS {
        return Err(invalid());
    }
    valid_single_line_token(&body.kind_slug, REVIEW_SLUG_MAX_LEN)?;
    valid_optional_token(body.category_slug.as_deref(), REVIEW_SLUG_MAX_LEN)?;
    valid_optional_ref(body.project_id.as_deref())?;
    valid_optional_ref(body.task_id.as_deref())?;
    valid_optional_ref(body.repository_id.as_deref())?;
    valid_optional_token(body.branch.as_deref(), MAX_BRANCH_OR_TARGET_BYTES)?;
    valid_optional_token(body.target_ref.as_deref(), MAX_BRANCH_OR_TARGET_BYTES)?;
    valid_optional_token(body.base_oid.as_deref(), MAX_OID_BYTES)?;
    valid_optional_token(body.expected_head_oid.as_deref(), MAX_OID_BYTES)?;
    validate_string_list(
        &body.guidance_refs,
        REVIEW_MAX_GUIDANCE_REFS,
        REVIEW_GUIDANCE_REF_MAX_CHARS,
    )?;
    validate_string_list(
        &body.criteria,
        REVIEW_MAX_CRITERIA,
        REVIEW_CRITERION_MAX_CHARS,
    )?;
    let routing = body.routing.map(RoutingSpecBody::into_proto).transpose()?;
    Ok(ReviewRequestParams {
        command_id: body.command_id,
        review_id: body.review_id,
        target_kind: body.target_kind,
        title: body.title,
        body_markdown: body.body_markdown,
        kind_slug: body.kind_slug,
        category_slug: body.category_slug,
        project_id: body.project_id,
        task_id: body.task_id,
        repository_id: body.repository_id,
        branch: body.branch,
        target_ref: body.target_ref,
        base_oid: body.base_oid,
        expected_head_oid: body.expected_head_oid,
        independent: body.independent,
        routing,
        session_mode: body.session_mode,
        guidance_refs: body.guidance_refs,
        criteria: body.criteria,
    })
}

fn validate_string_list(
    items: &[String],
    max_items: usize,
    max_chars: usize,
) -> Result<(), ApiReply> {
    if items.len() > max_items {
        return Err(invalid());
    }
    let bad = items.iter().any(|item| {
        item.trim().is_empty()
            || item.chars().count() > max_chars
            || item.chars().any(char::is_control)
    });
    (!bad).then_some(()).ok_or_else(invalid)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteBody {
    command_id: String,
    expected_revision: u64,
    routing: RoutingSpecBody,
    #[serde(default)]
    session_mode: Option<ReviewSessionMode>,
}

pub(super) fn decode_route_body(
    review: String,
    bytes: &[u8],
) -> Result<ReviewRouteParams, ApiReply> {
    let body: RouteBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    valid_uuid(&body.command_id)?;
    Ok(ReviewRouteParams {
        command_id: body.command_id,
        review,
        expected_revision: body.expected_revision,
        routing: body.routing.into_proto()?,
        session_mode: body.session_mode,
    })
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum FindingOpBody {
    Add {
        finding_id: String,
        severity: FindingSeverity,
        #[serde(default)]
        anchor: FindingAnchorBody,
        body_markdown: String,
        #[serde(default)]
        attachment_ids: Vec<String>,
    },
    Reply {
        finding_id: String,
        reply_id: String,
        #[serde(default)]
        body_markdown: String,
        #[serde(default)]
        set_status: Option<FindingStatus>,
        #[serde(default)]
        attachment_ids: Vec<String>,
    },
    Resolve {
        finding_id: String,
        reply_id: String,
        #[serde(default)]
        body_markdown: String,
        #[serde(default)]
        attachment_ids: Vec<String>,
    },
    WontFix {
        finding_id: String,
        reply_id: String,
        #[serde(default)]
        body_markdown: String,
        #[serde(default)]
        attachment_ids: Vec<String>,
    },
    Reopen {
        finding_id: String,
        reply_id: String,
        #[serde(default)]
        body_markdown: String,
        #[serde(default)]
        attachment_ids: Vec<String>,
    },
}

impl FindingOpBody {
    fn into_proto(self) -> Result<ReviewFindingOp, ApiReply> {
        match self {
            Self::Add {
                finding_id,
                severity,
                anchor,
                body_markdown,
                attachment_ids,
            } => Ok(ReviewFindingOp::Add(
                FindingDraftBody {
                    finding_id,
                    severity,
                    anchor,
                    body_markdown,
                    attachment_ids,
                }
                .into_proto()?,
            )),
            Self::Reply {
                finding_id,
                reply_id,
                body_markdown,
                set_status,
                attachment_ids,
            } => build_reply_op(
                finding_id,
                reply_id,
                body_markdown,
                set_status,
                attachment_ids,
            ),
            Self::Resolve {
                finding_id,
                reply_id,
                body_markdown,
                attachment_ids,
            } => build_reply_op(
                finding_id,
                reply_id,
                body_markdown,
                Some(FindingStatus::Resolved),
                attachment_ids,
            ),
            Self::WontFix {
                finding_id,
                reply_id,
                body_markdown,
                attachment_ids,
            } => build_reply_op(
                finding_id,
                reply_id,
                body_markdown,
                Some(FindingStatus::WontFix),
                attachment_ids,
            ),
            Self::Reopen {
                finding_id,
                reply_id,
                body_markdown,
                attachment_ids,
            } => build_reply_op(
                finding_id,
                reply_id,
                body_markdown,
                Some(FindingStatus::Open),
                attachment_ids,
            ),
        }
    }
}

fn build_reply_op(
    finding_id: String,
    reply_id: String,
    body_markdown: String,
    set_status: Option<FindingStatus>,
    attachment_ids: Vec<String>,
) -> Result<ReviewFindingOp, ApiReply> {
    valid_uuid(&finding_id)?;
    valid_uuid(&reply_id)?;
    if set_status.is_none() && body_markdown.trim().is_empty() {
        return Err(invalid());
    }
    if body_markdown.chars().count() > REVIEW_REPLY_BODY_MAX_CHARS {
        return Err(invalid());
    }
    validate_attachment_ids(&attachment_ids)?;
    Ok(ReviewFindingOp::Reply {
        finding_id,
        reply_id,
        body_markdown,
        set_status,
        attachment_ids,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NestedFindingBody {
    command_id: String,
    op: FindingOpBody,
}

struct NoDuplicateKeys;

impl<'de> Deserialize<'de> for NoDuplicateKeys {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = NoDuplicateKeys;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("valid JSON with unique object keys")
            }

            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(NoDuplicateKeys)
            }

            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(NoDuplicateKeys)
            }

            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(NoDuplicateKeys)
            }

            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(NoDuplicateKeys)
            }

            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(NoDuplicateKeys)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(NoDuplicateKeys)
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                while seq.next_element::<NoDuplicateKeys>()?.is_some() {}
                Ok(NoDuplicateKeys)
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut seen = std::collections::HashSet::new();
                while let Some((key, _)) = map.next_entry::<String, NoDuplicateKeys>()? {
                    if !seen.insert(key) {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(NoDuplicateKeys)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Decodes either `{ "command_id", "op": { "action": ... } }` or flat
/// `{ "command_id", "action": ... }` while strictly rejecting duplicate and unknown fields.
pub(super) fn decode_finding_body(
    review: String,
    bytes: &[u8],
) -> Result<ReviewFindingParams, ApiReply> {
    serde_json::from_slice::<NoDuplicateKeys>(bytes).map_err(|_| invalid())?;
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let serde_json::Value::Object(mut map) = raw else {
        return Err(invalid());
    };
    let (command_id, op_body) = if map.contains_key("op") {
        let nested: NestedFindingBody =
            serde_json::from_value(serde_json::Value::Object(map)).map_err(|_| invalid())?;
        (nested.command_id, nested.op)
    } else {
        let cmd = map
            .remove("command_id")
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s),
                _ => None,
            })
            .ok_or_else(invalid)?;
        let op: FindingOpBody =
            serde_json::from_value(serde_json::Value::Object(map)).map_err(|_| invalid())?;
        (cmd, op)
    };
    valid_uuid(&command_id)?;
    Ok(ReviewFindingParams {
        command_id,
        review,
        op: op_body.into_proto()?,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerdictBody {
    command_id: String,
    round_number: u32,
    #[serde(default)]
    head_oid: Option<String>,
    verdict: ReviewVerdict,
    #[serde(default)]
    summary_markdown: String,
    #[serde(default)]
    findings: Vec<FindingDraftBody>,
    #[serde(default)]
    attachment_ids: Vec<String>,
}

pub(super) fn decode_verdict_body(
    review: String,
    bytes: &[u8],
) -> Result<ReviewVerdictParams, ApiReply> {
    let body: VerdictBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    valid_uuid(&body.command_id)?;
    if body.round_number == 0 || body.findings.len() > REVIEW_MAX_FINDINGS_PER_VERDICT {
        return Err(invalid());
    }
    if body.summary_markdown.chars().count() > REVIEW_SUMMARY_MAX_CHARS {
        return Err(invalid());
    }
    valid_optional_token(body.head_oid.as_deref(), MAX_OID_BYTES)?;
    validate_attachment_ids(&body.attachment_ids)?;
    let findings = body
        .findings
        .into_iter()
        .map(FindingDraftBody::into_proto)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ReviewVerdictParams {
        command_id: body.command_id,
        review,
        round_number: body.round_number,
        head_oid: body.head_oid,
        verdict: body.verdict,
        summary_markdown: body.summary_markdown,
        findings,
        attachment_ids: body.attachment_ids,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResubmitBody {
    command_id: String,
    expected_revision: u64,
    #[serde(default)]
    expected_head_oid: Option<String>,
    #[serde(default)]
    session_mode: Option<ReviewSessionMode>,
    #[serde(default)]
    fresh: bool,
    #[serde(default)]
    note_markdown: String,
}

pub(super) fn decode_resubmit_body(
    review: String,
    bytes: &[u8],
) -> Result<ReviewResubmitParams, ApiReply> {
    let body: ResubmitBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    valid_uuid(&body.command_id)?;
    valid_optional_token(body.expected_head_oid.as_deref(), MAX_OID_BYTES)?;
    if body.note_markdown.chars().count() > REVIEW_SUMMARY_MAX_CHARS {
        return Err(invalid());
    }
    let params = ReviewResubmitParams {
        command_id: body.command_id,
        review,
        expected_revision: body.expected_revision,
        expected_head_oid: body.expected_head_oid,
        session_mode: body.session_mode,
        fresh: body.fresh,
        note_markdown: body.note_markdown,
    };
    params.requested_session_mode().map_err(|_| invalid())?;
    Ok(params)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseBody {
    command_id: String,
    expected_revision: u64,
    #[serde(default)]
    reason: Option<String>,
}

pub(super) fn decode_close_body(
    review: String,
    bytes: &[u8],
) -> Result<ReviewCloseParams, ApiReply> {
    let body: CloseBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    valid_uuid(&body.command_id)?;
    if let Some(reason) = body.reason.as_deref() {
        if reason.trim().is_empty()
            || reason.chars().count() > REVIEW_CLOSE_REASON_MAX_CHARS
            || reason.chars().any(char::is_control)
        {
            return Err(invalid());
        }
    }
    Ok(ReviewCloseParams {
        command_id: body.command_id,
        review,
        expected_revision: body.expected_revision,
        reason: body.reason,
    })
}
