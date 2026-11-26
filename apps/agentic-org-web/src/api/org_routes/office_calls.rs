//! Executing office listing and office restart calls.

use super::*;

pub(in crate::api) async fn execute_org_offices(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
    let list = match api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await
    {
        Ok(list) => list,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let Some(org) = list.organizations.into_iter().find(|org| org.id == org_id) else {
        return not_found();
    };
    let mut offices: Vec<OfficeView> = org.offices.into_iter().map(OfficeView::from).collect();
    // Prefer Active local offices first, falling back through any other local office if offline.
    let mut candidate_ids: Vec<String> = offices
        .iter()
        .filter(|o| o.status == org_proto::OfficeStatus::Active)
        .map(|o| o.id.clone())
        .collect();
    for office in &offices {
        if !candidate_ids.iter().any(|id| id == &office.id) {
            candidate_ids.push(office.id.clone());
        }
    }
    let auth = api.auth(&pair);
    for local_office_id in candidate_ids {
        if let Ok(dir) = api
            .offices
            .call_typed::<org_proto::federation::OfficeDirectoryList>(
                &auth,
                &local_office_id,
                &org_proto::federation::OfficeDirectoryListParams {},
                true,
            )
            .await
        {
            for remote in dir.offices {
                if !offices.iter().any(|existing| existing.id == remote.id) {
                    offices.push(OfficeView {
                        id: remote.id,
                        organization_id: remote.organization_id,
                        name: remote.name,
                        role: remote.role,
                        host: format!("Remote ({})", remote.slug),
                        status: org_proto::OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                    });
                }
            }
            break;
        }
    }
    result(OfficeList { offices })
}

pub(super) fn office_restart_error(host: WebError) -> WebError {
    use WebErrorCode::*;
    let message = match host.code {
        InvalidRequest => "Invalid office restart request.",
        NotFound => "Office not found.",
        NotAuthorized => "Only the administrator or organization owner may restart an office.",
        Conflict => "This office is not offline or unavailable.",
        OperationReused => "This operation ID was already used for another request.",
        ClockSkew => "This device's clock differs too much from the server's.",
        NotReady => "This office is still starting or being created.",
        _ => return WebError::new(Unavailable, "Service unavailable."),
    };
    let mut error = WebError::new(host.code, message);
    (error.field, error.rule) = (host.field, host.rule);
    error
}

pub(in crate::api) async fn execute_office_restart(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    office_id: &str,
    op_id: OperationId,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
    let reply = api
        .host
        .request_async::<BrowserOfficeRestart>(&BrowserOfficeRestartParams {
            auth: api.auth(&pair),
            operation_id: op_id.as_str().to_owned(),
            org_id: org_id.to_owned(),
            office_id: office_id.to_owned(),
        })
        .await;
    if matches!(&reply, Err(Failure::Refused(e)) if e.code == org_proto::ErrorCode::Expired) {
        return ApiReply::error(
            StatusCode::GONE,
            WebErrorCode::Expired,
            "This request expired. Start again.",
        );
    }
    let outcome = match reply {
        Ok(HostOpResult::Completed { .. }) => MutationOutcome::Completed {
            operation_id: op_id,
            result: OfficeRestartResult {},
        },
        Ok(HostOpResult::Rejected { refusal }) => MutationOutcome::Rejected {
            operation_id: op_id,
            error: office_restart_error(WebError::from(refusal)),
        },
        Ok(_) => MutationOutcome::Unknown {
            operation_id: op_id,
        },
        Err(Failure::Denied) => {
            if let Err(error) = api.revalidate_session_async(pair).await {
                return failure(error);
            }
            MutationOutcome::Rejected {
                operation_id: op_id,
                error: office_restart_error(WebError::new(WebErrorCode::NotAuthorized, "")),
            }
        }
        Err(Failure::NotFound) => MutationOutcome::Rejected {
            operation_id: op_id,
            error: office_restart_error(WebError::new(WebErrorCode::NotFound, "")),
        },
        Err(Failure::Refused(error)) => {
            let error = WebError::from(*error);
            if matches!(
                error.code,
                WebErrorCode::InvalidRequest
                    | WebErrorCode::Conflict
                    | WebErrorCode::OperationReused
                    | WebErrorCode::ClockSkew
                    | WebErrorCode::NotReady
            ) {
                MutationOutcome::Rejected {
                    operation_id: op_id,
                    error: office_restart_error(error),
                }
            } else {
                MutationOutcome::Unknown {
                    operation_id: op_id,
                }
            }
        }
        Err(_) => MutationOutcome::Unknown {
            operation_id: op_id,
        },
    };
    mutation(outcome, false)
}
