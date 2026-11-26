//! Executing project list, inspect and create calls.

use org_host_control::organization::BrowserAuth;
use org_proto::web::org_mutations::{classify_org_reply, OrgReplyClassification};
use org_proto::web::{MutationOutcome, OperationId, WebError, WebErrorCode};

use super::super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::super::{failure, mutation, result, unavailable, CallerAuth, SessionApi};
use super::map_read_office_error;
use super::org_calls::{finish_named_org_mutation, mutation_setup_failure};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::ApiReply;

pub(in crate::api) async fn execute_projects_list(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    include_archived: bool,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .projects_list(&auth, &office_id, include_archived)
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
            .projects_list(&auth, &office_id, include_archived)
            .await;
    }
    match reply {
        Ok(listed) => result(listed),
        Err(error) => map_read_office_error(
            error,
            "You are not authorized to view projects in this organization.",
        ),
    }
}

pub(in crate::api) async fn execute_project_inspect(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    project_ref: &str,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .project_inspect(&auth, &office_id, project_ref)
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
            .project_inspect(&auth, &office_id, project_ref)
            .await;
    }
    match reply {
        Ok(project) => result(project),
        Err(error) => map_read_office_error(error, "You are not authorized to view this project."),
    }
}

pub(in crate::api) async fn execute_project_create(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    op_id: OperationId,
    params: org_proto::projects::ProjectCreateWebParams,
) -> ApiReply {
    let office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(error) => return mutation_setup_failure(error, op_id),
    };
    let auth = api.auth(&pair);
    let native = org_proto::org::ProjectCreateParams {
        project_id: params.project_id,
        name: params.name,
        key: None,
        description: None,
        teams: Vec::new(),
        general_area_id: String::new(),
        general_area_name: None,
        repo: None,
        integration_branch: None,
        slug: params.slug,
        owner_team_id: params.owner_team_id,
        create_team_id: params.create_team_id,
        parent_team_id: params.parent_team_id,
        contributing_team_ids: params.contributing_team_ids,
        command_id: op_id.as_str().to_owned(),
    };
    let mut classified =
        match dispatch_project_create(api, &auth, &office_id, &native, &op_id).await {
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
        classified = match dispatch_project_create(api, &auth, &fresh_office, &native, &op_id).await
        {
            Ok(c) => c,
            Err(reply) => return reply,
        };
    }
    finish_named_org_mutation(api, org_id, "project.create", op_id, classified)
}

pub(super) async fn dispatch_project_create(
    api: &SessionApi,
    auth: &BrowserAuth,
    office_id: &str,
    params: &org_proto::org::ProjectCreateParams,
    op_id: &OperationId,
) -> Result<OrgReplyClassification, ApiReply> {
    match api.offices.project_create(auth, office_id, params).await {
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
