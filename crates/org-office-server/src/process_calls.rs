//! Office RPC dispatcher for organization processes, standards, and built-in skills
//! (`TASK-44`, `C5`, `doc-34` §6, `FC-110`, `FC-111`, `FC-112`, `FC-184`).

use org_office::Office;
use org_proto::processes::{
    ProcessEffective, ProcessShow, ProcessView, ProcessWhy, ProcessWrite, SkillList,
    SkillListFilterParams, SkillRead,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

/// Whether `request` belongs to the C5 processes/standards/skills RPC family.
pub(crate) fn is_process_method(request: &Request) -> bool {
    request.is::<ProcessEffective>()
        || request.is::<ProcessWhy>()
        || request.is::<ProcessShow>()
        || request.is::<ProcessView>()
        || request.is::<ProcessWrite>()
        || request.is::<SkillList>()
        || request.is::<SkillRead>()
}

/// Dispatches one C5 `process.*` or `skill.*` RPC against [`Office`].
pub(crate) fn dispatch_process_rpc(
    office: &Office,
    member: Option<&str>,
    request: &Request,
) -> Option<Response> {
    if !is_process_method(request) {
        return None;
    }
    let Some(caller) = member else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if request.is::<ProcessEffective>() {
        return Some(run_call::<ProcessEffective>(request, |p| {
            office
                .process_effective(caller, p)
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
                .and_then(|r| r)
        }));
    }
    if request.is::<ProcessWhy>() {
        return Some(run_call::<ProcessWhy>(request, |p| {
            office
                .process_why(caller, p)
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
                .and_then(|r| r)
        }));
    }
    if request.is::<ProcessShow>() {
        return Some(run_call::<ProcessShow>(request, |p| {
            office
                .process_show(caller, p)
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
                .and_then(|r| r)
        }));
    }
    if request.is::<ProcessView>() {
        return Some(run_call::<ProcessView>(request, |p| {
            office
                .process_view(caller, p)
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
                .and_then(|r| r)
        }));
    }
    if request.is::<ProcessWrite>() {
        return Some(run_call::<ProcessWrite>(request, |p| {
            office
                .process_write(caller, p)
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
                .and_then(|r| r)
        }));
    }
    if request.is::<SkillList>() {
        return Some(dispatch_skill_list(office, request));
    }
    if request.is::<SkillRead>() {
        return Some(run_call::<SkillRead>(request, |p| {
            office.skill_read(&p.name)
        }));
    }
    None
}

fn dispatch_skill_list(office: &Office, request: &Request) -> Response {
    let filter: SkillListFilterParams = match &request.params {
        serde_json::Value::Null => SkillListFilterParams::default(),
        v @ serde_json::Value::Object(_) => match serde_json::from_value(v.clone()) {
            Ok(parsed) => parsed,
            Err(_) => {
                return Response::error(
                    request.id,
                    ApiError::new(
                        ErrorCode::InvalidRequest,
                        "invalid parameters for `skill.list`",
                    ),
                );
            }
        },
        _ => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid parameters for `skill.list`",
                ),
            );
        }
    };
    match office.skill_list_filtered(filter.category) {
        Ok(result) => Response::ok::<SkillList>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "failed encoding response"),
            )
        }),
        Err(err) => Response::error(request.id, err),
    }
}

fn run_call<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(p) => p,
        Err(_) => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid parameters for `{}`", C::METHOD),
                ),
            );
        }
    };
    match handler(params) {
        Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "failed encoding response"),
            )
        }),
        Err(err) => Response::error(request.id, err),
    }
}
