//! Executing organization snapshot, mutation, operation and CLI-approval calls.

use super::*;

pub(in crate::api) async fn execute_org_snapshot(
    api: &SessionApi,
    pair: CallerAuth,
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

pub(in crate::api) async fn execute_org_mutation(
    api: &SessionApi,
    pair: CallerAuth,
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

pub(in crate::api) async fn execute_org_operation(
    api: &SessionApi,
    pair: CallerAuth,
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

pub(super) fn mutation_setup_failure(error: Failure, op_id: OperationId) -> ApiReply {
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

pub(super) async fn dispatch_org_mutation(
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

pub(super) fn finish_org_mutation(
    api: &SessionApi,
    org_id: &str,
    command: &OrgMutation,
    op_id: OperationId,
    classified: OrgReplyClassification,
) -> ApiReply {
    finish_named_org_mutation(api, org_id, command.operation().name(), op_id, classified)
}

pub(super) fn finish_named_org_mutation(
    api: &SessionApi,
    org_id: &str,
    event_name: &str,
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
                    event: event_name.to_owned(),
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

pub(in crate::api) async fn execute_cli_approval_decide(
    api: &SessionApi,
    pair: CallerAuth,
    link: LinkToken,
    approve: bool,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
    let reply = api
        .host
        .request_async::<BrowserCliApprovalDecide>(&BrowserCliApprovalDecideParams {
            auth: api.auth(&pair),
            link,
            approve,
        })
        .await;
    match reply {
        Ok(decided) => result(decided),
        Err(Failure::NotFound) => ApiReply::error(
            StatusCode::NOT_FOUND,
            WebErrorCode::NotFound,
            "This link has expired or was already used.",
        ),
        Err(Failure::Denied) => signed_out(),
        Err(_) => unavailable(),
    }
}
