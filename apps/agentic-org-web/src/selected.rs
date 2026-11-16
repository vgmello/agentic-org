//! Selected HTTP/native adaptation. The session authority is supplied by the
//! verified gateway composition; no query, capability bit or imported profile
//! can construct that authority. This module never opens an office path/database.
use crate::{
    body::BusinessBody,
    path::RequestPath,
    service::{ApiBackend, ApiReply, PreparedCall},
};
use hyper::{http::request::Parts, Method, StatusCode};
use org_client::{selected::ApprovedOffice, ClientError};
use org_proto::{
    gateway::{GatewayErrorCode as Code, OperationId},
    selected_tasks::{
        self as task,
        read::{ContentKind, ContentRef, Query, Queue, TargetKind},
        AssignmentResult, LookupOutcome, OfficeScope, Revision,
    },
    Principal,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
mod query;
#[cfg(test)]
mod tests;

/// Route authorization checks the protected cookie/proof pair without native I/O.
/// `office` must recheck the live host session and obtain a channel-bound grant
/// on its admitted control connection, using a finite deadline. It must not seed
/// an owner flag, trust an account from params, or return an agent transport.
pub trait SessionAuthority: Send + Sync + 'static {
    type Ticket: Send + 'static;
    fn authenticate(&self, request: &Parts) -> Result<Self::Ticket, ApiReply>;
    fn office(
        &self,
        ticket: Self::Ticket,
        scope: &OfficeScope,
        timeout: Duration,
    ) -> Result<OfficeAccess, ApiReply>;
}
pub struct OfficeAccess {
    pub office: Arc<ApprovedOffice>,
    pub principal: Principal,
}
pub struct SelectedApi<A> {
    authority: A,
}
impl<A> SelectedApi<A> {
    pub fn new(authority: A) -> Self {
        Self { authority }
    }
}
pub struct Route<T> {
    ticket: T,
    scope: Option<OfficeScope>,
    request: Request,
}
enum Request {
    Read(Query),
    Assign {
        task_id: String,
        organization_id: String,
    },
    Lookup(OperationId),
}
pub struct Call<T> {
    ticket: T,
    scope: OfficeScope,
    request: Command,
}
enum Command {
    Read(Query),
    Assign(task::NativeAssignment),
    Lookup(OperationId),
}
fn invalid() -> ApiReply {
    ApiReply::error(
        StatusCode::BAD_REQUEST,
        Code::InvalidRequest,
        "Invalid selected task request.",
    )
}
fn missing() -> ApiReply {
    ApiReply::error(StatusCode::NOT_FOUND, Code::NotFound, "Not found.")
}
fn unavailable() -> ApiReply {
    ApiReply::error(
        StatusCode::SERVICE_UNAVAILABLE,
        Code::Unavailable,
        "Selected task service unavailable.",
    )
}
fn client_error(error: ClientError) -> ApiReply {
    let (status, code) = match error {
        ClientError::Api(e) => match e.code {
            org_proto::ErrorCode::NotAuthorized => (StatusCode::FORBIDDEN, Code::NotAuthorized),
            org_proto::ErrorCode::NotFound => (StatusCode::NOT_FOUND, Code::NotFound),
            org_proto::ErrorCode::Expired => (StatusCode::GONE, Code::Expired),
            org_proto::ErrorCode::UnsupportedVersion => {
                (StatusCode::UPGRADE_REQUIRED, Code::UnsupportedVersion)
            }
            _ => return unavailable(),
        },
        _ => return unavailable(),
    };
    ApiReply::error(
        status,
        code,
        "Selected request could not be authorized or resolved.",
    )
}
fn bounded(value: serde_json::Value, status: StatusCode) -> ApiReply {
    // The complete encoded response, not only chunk data, obeys the wire budget.
    if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() <= org_proto::MAX_FRAME_BYTES) {
        ApiReply::json(status, value)
    } else {
        unavailable()
    }
}
fn mutation(id: &OperationId, result: AssignmentResult<i64>, lookup: bool) -> ApiReply {
    let Ok(result) = result.into_browser() else {
        return unavailable();
    };
    let (status, mut value) = match result {
        completed @ AssignmentResult::Completed { .. } => (
            StatusCode::OK,
            json!({"outcome":"completed","operation_id":id,"result":completed}),
        ),
        AssignmentResult::Rejected { code, current } => {
            let status = match code.as_str() {
                "conflict" | "operation_reused" => StatusCode::CONFLICT,
                "expired" => StatusCode::GONE,
                "not_authorized" => StatusCode::FORBIDDEN,
                "unsupported_version" => StatusCode::UPGRADE_REQUIRED,
                _ => StatusCode::BAD_REQUEST,
            };
            let mut error = json!({"code":code,"message":"Assignment was refused."});
            if let Some(current) = current {
                error["current"] = json!(current);
            }
            (
                status,
                json!({"outcome":"rejected","operation_id":id,"error":error}),
            )
        }
    };
    if lookup {
        value["kind"] = json!("work");
        bounded(json!({"result":value}), StatusCode::OK)
    } else {
        bounded(value, status)
    }
}
impl<A: SessionAuthority> ApiBackend for SelectedApi<A> {
    type Route = Route<A::Ticket>;
    type Call = Call<A::Ticket>;
    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Self::Route, ApiReply> {
        let parts = path.segments();
        // Session routes compose outside this adapter and use secret ingress.
        let selected = matches!(parts,[api,v,organizations,_,tasks,..] if api=="api"&&v=="v1"&&organizations=="organizations"&&tasks=="tasks")
            || matches!(parts,[api,v,operations,work,_] if api=="api"&&v=="v1"&&operations=="operations"&&work=="work");
        if !selected {
            return Err(missing());
        }
        let ticket = self.authority.authenticate(request)?;
        if request.method != Method::GET && request.method != Method::POST {
            return Err(invalid());
        }
        let mut query = query::decode(request.uri.query().unwrap_or_default())?;
        let (scope, route) = if parts[2] == "operations" {
            if request.method != Method::GET {
                return Err(invalid());
            }
            let scope = query::scope(&mut query, None)?;
            let id = OperationId::parse(parts[4].clone()).map_err(|_| invalid())?;
            (Some(scope), Request::Lookup(id))
        } else {
            if parts.len() == 7 && parts[6] == "assignment" {
                if request.method != Method::POST {
                    return Err(invalid());
                }
                (
                    None,
                    Request::Assign {
                        task_id: query::identity(&parts[5])?.into(),
                        organization_id: query::identity(&parts[3])?.into(),
                    },
                )
            } else {
                if request.method != Method::GET {
                    return Err(invalid());
                }
                let scope = query::scope(&mut query, Some(&parts[3]))?;
                let native = query::read(parts, &mut query)?;
                (Some(scope), Request::Read(native))
            }
        };
        if !query.is_empty() {
            return Err(invalid());
        }
        Ok(Route {
            ticket,
            scope,
            request: route,
        })
    }
    fn prepare(
        &self,
        route: Self::Route,
        body: BusinessBody,
    ) -> Result<PreparedCall<Self::Call>, ApiReply> {
        let bytes = body.as_bytes().len();
        let Route {
            ticket,
            scope,
            request,
        } = route;
        match request {
            Request::Assign {
                task_id,
                organization_id,
            } => {
                let request =
                    task::decode_http_assignment(body.as_bytes()).map_err(|_| invalid())?;
                if request.scope.org_id != organization_id {
                    return Err(invalid());
                }
                let scope = request.scope;
                let id = request.operation_id.clone();
                let native = task::NativeAssignment {
                    change_id: request.operation_id,
                    task_id,
                    command: request.params.into_native(),
                };
                Ok(PreparedCall::mutation(
                    Call {
                        ticket,
                        scope,
                        request: Command::Assign(native),
                    },
                    id,
                    bytes,
                ))
            }
            Request::Read(query) => {
                let scope = scope.ok_or_else(invalid)?;
                if bytes != 0 {
                    return Err(invalid());
                }
                Ok(PreparedCall::read(
                    Call {
                        ticket,
                        scope,
                        request: Command::Read(query),
                    },
                    0,
                ))
            }
            Request::Lookup(id) => {
                let scope = scope.ok_or_else(invalid)?;
                if bytes != 0 {
                    return Err(invalid());
                }
                Ok(PreparedCall::read(
                    Call {
                        ticket,
                        scope,
                        request: Command::Lookup(id),
                    },
                    0,
                ))
            }
        }
    }
    fn execute(&self, call: Self::Call) -> ApiReply {
        let timeout = Duration::from_secs(5);
        let access = match self.authority.office(call.ticket, &call.scope, timeout) {
            Ok(access) => access,
            Err(reply) => return reply,
        };
        if access.office.instance() != call.scope.instance_id {
            return unavailable();
        }
        match call.request {
            Command::Read(query) => match access.office.read(&access.principal, &query, timeout) {
                Ok(result) => match result.into_browser() {
                    Ok(result) => bounded(json!({"result":result}), StatusCode::OK),
                    Err(_) => unavailable(),
                },
                Err(error) => client_error(error),
            },
            Command::Assign(params) => {
                let id = params.change_id.clone();
                match access
                    .office
                    .set_assignment(&access.principal, &params, timeout)
                {
                    Ok(result) => mutation(&id, result, false),
                    // Even a connection-level refusal must not trigger a new-ID
                    // retry once submission was possible. Lookup remains usable.
                    Err(failure) if failure.may_have_submitted => bounded(
                        json!({"outcome":"unknown","operation_id":id}),
                        StatusCode::GATEWAY_TIMEOUT,
                    ),
                    Err(_) => bounded(
                        json!({"outcome":"rejected","operation_id":id,"error":{"code":"unavailable","message":"Assignment was not dispatched."}}),
                        StatusCode::SERVICE_UNAVAILABLE,
                    ),
                }
            }
            Command::Lookup(id) => {
                match access
                    .office
                    .lookup(&access.principal, id.as_str(), timeout)
                {
                    Ok(LookupOutcome::Committed { result }) => mutation(&id, result, true),
                    Ok(LookupOutcome::Rejected { refusal }) => mutation(
                        &id,
                        AssignmentResult::Rejected {
                            code: refusal.code,
                            current: refusal.current,
                        },
                        true,
                    ),
                    // Hidden and missing operation records share the exact
                    // response, including the body. Other refusals retain
                    // their authentication/authorization meaning.
                    Err(ClientError::Api(error))
                        if error.code == org_proto::ErrorCode::NotFound =>
                    {
                        missing()
                    }
                    Err(error) => client_error(error),
                }
            }
        }
    }
}
