//! Organization snapshot, mutation, and operation-lookup handlers for [`SessionApi`].
//! Extracted from `api.rs` to keep the session/host API surface decoupled from
//! browser-to-office organization routing and `web-api-v1` §9 authorization rechecks.

use hyper::StatusCode;
use org_host_control::organization::{self as host_org, BrowserAuth};
use org_proto::web::{
    org_mutations::{classify_org_reply, OrgMutation, OrgOperation, OrgReplyClassification},
    MutationOutcome, OperationId, OperationKind, OperationLookup, ResumeCursor, SnapshotResponse,
    WebError, WebErrorCode,
};

use super::{
    failure, mutation, not_found, result, signed_out, unavailable, PresentedPair, SessionApi,
};
use crate::{host::Failure, office::OfficeFailure, service::ApiReply, sse::HubEvent};

/// Maps the `(resource, action)` segments of `/api/v1/organizations/{org}/{resource}/{action}`
/// to a closed typed [`OrgOperation`].
pub(super) fn route_org_action(resource: &str, action: &str) -> Option<OrgOperation> {
    const TABLE: &[(&str, &str, OrgOperation)] = &[
        ("teams", "create", OrgOperation::TeamCreate),
        ("teams", "rename", OrgOperation::TeamRename),
        ("teams", "move", OrgOperation::TeamMove),
        ("teams", "set-lead", OrgOperation::TeamSetLead),
        ("teams", "delete", OrgOperation::TeamDelete),
        ("teams", "set-title", OrgOperation::TeamSetTitle),
        ("members", "add-person", OrgOperation::MemberAddPerson),
        ("members", "update-person", OrgOperation::MemberUpdatePerson),
        ("members", "remove", OrgOperation::MemberRemove),
        (
            "members",
            "set-reports-to",
            OrgOperation::MemberSetReportsTo,
        ),
        ("members", "move", OrgOperation::MemberMove),
    ];
    TABLE
        .iter()
        .find(|(r, a, _)| *r == resource && *a == action)
        .map(|(_, _, op)| *op)
}

fn find_hq_office(list: host_org::ListResult, org_id: &str) -> Result<String, Failure> {
    let org = list
        .organizations
        .into_iter()
        .find(|org| org.id == org_id)
        .ok_or(Failure::NotFound)?;
    org.offices
        .iter()
        .find(|office| office.role == org_proto::OfficeRole::Hq)
        .or_else(|| org.offices.first())
        .map(|office| office.id.clone())
        .ok_or(Failure::Unavailable)
}

fn map_read_office_error(error: OfficeFailure, forbidden_message: &'static str) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Refused(e) if e.code == org_proto::ErrorCode::NotAuthorized => {
            ApiReply::error(
                StatusCode::FORBIDDEN,
                WebErrorCode::NotAuthorized,
                forbidden_message,
            )
        }
        OfficeFailure::Refused(e) if e.code == org_proto::ErrorCode::NotFound => not_found(),
        OfficeFailure::Refused(e) if e.code == org_proto::ErrorCode::Expired => ApiReply::error(
            StatusCode::GONE,
            WebErrorCode::Expired,
            "This operation is too old to look up.",
        ),
        OfficeFailure::Refused(_) | OfficeFailure::Transport => unavailable(),
    }
}

fn classify_stored_outcome(op_id: OperationId, outcome: org_proto::OperationOutcome) -> ApiReply {
    let synthesized = match outcome {
        org_proto::OperationOutcome::Committed { result } => {
            serde_json::json!({ "id": 1, "result": result })
        }
        org_proto::OperationOutcome::Rejected { refusal } => {
            serde_json::json!({ "id": 1, "error": refusal })
        }
        _ => return unavailable(),
    };
    match classify_org_reply(op_id, Some(&synthesized)) {
        OrgReplyClassification::Outcome { outcome, issue }
            if matches!(
                outcome,
                MutationOutcome::Completed { .. } | MutationOutcome::Rejected { .. }
            ) =>
        {
            if let Some(issue) = issue {
                eprintln!("agentic-org-web: unexpected org lookup outcome issue: {issue:?}");
            }
            result(OperationLookup::Outcome {
                kind: OperationKind::Org,
                state: None,
                outcome,
            })
        }
        _ => unavailable(),
    }
}

pub(super) async fn execute_org_snapshot(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api.offices.org_snapshot(&auth, &office_id).await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return unavailable(),
            Err(error) => return failure(error),
        };
        reply = api.offices.org_snapshot(&auth, &office_id).await;
    }
    let snapshot = match reply {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return map_read_office_error(
                error,
                "You are not authorized to view this organization.",
            );
        }
    };
    let cursor = format!(
        "org:{}:{}:{}",
        org_id, snapshot.position.epoch, snapshot.position.seq
    );
    match ResumeCursor::parse(cursor) {
        Ok(resume_cursor) => result(SnapshotResponse {
            snapshot,
            resume_cursor,
        }),
        Err(_) => unavailable(),
    }
}

pub(super) async fn execute_org_mutation(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    command: OrgMutation,
) -> ApiReply {
    let op_id = command.operation_id().clone();
    let office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(error) => return mutation_setup_failure(error, op_id),
    };
    let auth = api.auth(&pair);
    let mut classified = match dispatch_org_mutation(api, &auth, &office_id, &command, &op_id).await
    {
        Ok(c) => c,
        Err(reply) => return reply,
    };
    if matches!(
        classified,
        OrgReplyClassification::NeedsAuthorizationRecheck
    ) {
        let fresh_office = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(error) => return mutation_setup_failure(error, op_id),
        };
        classified = match dispatch_org_mutation(api, &auth, &fresh_office, &command, &op_id).await
        {
            Ok(c) => c,
            Err(reply) => return reply,
        };
    }
    finish_org_mutation(api, org_id, &command, op_id, classified)
}

pub(super) async fn execute_org_operation(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    op_id: OperationId,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .org_operation_get(&auth, &office_id, op_id.as_str())
        .await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return unavailable(),
            Err(error) => return failure(error),
        };
        reply = api
            .offices
            .org_operation_get(&auth, &office_id, op_id.as_str())
            .await;
    }
    match reply {
        Ok(outcome) => classify_stored_outcome(op_id, outcome),
        Err(error) => {
            map_read_office_error(error, "You are not authorized to view this operation.")
        }
    }
}

async fn resolve_hq_office(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
) -> Result<String, Failure> {
    api.validate_async(pair).await?;
    let list = api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await?;
    find_hq_office(list, org_id)
}

async fn revalidate_and_resolve_hq(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
) -> Result<String, Failure> {
    api.revalidate_session_async(pair).await?;
    let list = api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await?;
    find_hq_office(list, org_id)
}

fn mutation_setup_failure(error: Failure, op_id: OperationId) -> ApiReply {
    match error {
        Failure::Denied => signed_out(),
        Failure::NotFound => not_found(),
        _ => mutation(
            MutationOutcome::<org_proto::Empty>::Rejected {
                operation_id: op_id,
                error: WebError::new(WebErrorCode::Unavailable, "Service unavailable."),
            },
            false,
        ),
    }
}

async fn dispatch_org_mutation(
    api: &SessionApi,
    auth: &BrowserAuth,
    office_id: &str,
    command: &OrgMutation,
    op_id: &OperationId,
) -> Result<OrgReplyClassification, ApiReply> {
    match api.offices.org_mutation(auth, office_id, command).await {
        Ok(reply) => Ok(classify_org_reply(op_id.clone(), reply.as_ref())),
        Err(OfficeFailure::BeforeDispatch(error)) => {
            Err(mutation_setup_failure(error, op_id.clone()))
        }
        Err(_) => Err(mutation(
            MutationOutcome::<org_proto::Empty>::Rejected {
                operation_id: op_id.clone(),
                error: WebError::new(WebErrorCode::Unavailable, "Service unavailable."),
            },
            false,
        )),
    }
}

fn finish_org_mutation(
    api: &SessionApi,
    org_id: &str,
    command: &OrgMutation,
    op_id: OperationId,
    classified: OrgReplyClassification,
) -> ApiReply {
    match classified {
        OrgReplyClassification::Outcome { outcome, issue } => {
            if let Some(issue) = issue {
                eprintln!("agentic-org-web: unexpected org mutation reply issue: {issue:?}");
            }
            if matches!(outcome, MutationOutcome::Completed { .. }) {
                api.hub.publish(HubEvent::OrganizationChanged {
                    org_id: org_id.to_owned(),
                    cursor: format!("org:{org_id}:{}", op_id.as_str()),
                    event: command.operation().name().to_owned(),
                    data: serde_json::json!({ "operation_id": op_id.as_str() }),
                });
            }
            mutation(outcome, false)
        }
        OrgReplyClassification::NeedsAuthorizationRecheck => mutation(
            MutationOutcome::<org_proto::Empty>::Rejected {
                operation_id: op_id,
                error: WebError::new(
                    WebErrorCode::NotAuthorized,
                    "You are not authorized to perform this action.",
                ),
            },
            false,
        ),
    }
}
