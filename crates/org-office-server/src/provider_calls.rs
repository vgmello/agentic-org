//! Office RPC dispatcher for `L4` provider connection pools, office-local bindings,
//! per-connection model discovery, readiness, and dual-authority quota permits & cooldowns
//! (`TASK-54`, `FC-098`, `FC-099`, `doc-88`, `doc-89`).

use std::sync::Arc;

use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::providers::{
    AgentReadinessGet, ModelProfileCreate, ModelProfileModels, ModelProfileUpdate,
    ProviderBindingList, ProviderBindingSet, ProviderConnectionList, ProviderConnectionRefresh,
    ProviderConnectionUpdate, ProviderCooldownRecord, ProviderQuotaStatus,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::org_feed::FeedWake;

/// Whether `request` belongs to the `L4` provider-pool / binding / quota / readiness RPC family.
pub(crate) fn is_provider_pool_method(request: &Request) -> bool {
    request.is::<ProviderConnectionList>()
        || request.is::<ProviderConnectionUpdate>()
        || request.is::<ModelProfileCreate>()
        || request.is::<ModelProfileUpdate>()
        || request.is::<ProviderBindingSet>()
        || request.is::<ProviderBindingList>()
        || request.is::<ProviderConnectionRefresh>()
        || request.is::<ModelProfileModels>()
        || request.is::<AgentReadinessGet>()
        || request.is::<ProviderQuotaStatus>()
        || request.is::<ProviderCooldownRecord>()
}

/// Dispatches one `L4` provider-pool RPC against [`Office`], waking organization feeds on
/// org-wide mutations and retrying queued agent wakes when connection readiness improves.
pub(crate) fn dispatch_provider_pool_rpc(
    office: &Office,
    sessions: Option<&Arc<SessionManager>>,
    wake: Option<&Arc<FeedWake>>,
    member: Option<&str>,
    request: &Request,
) -> Option<Response> {
    if !is_provider_pool_method(request) {
        return None;
    }
    let Some(caller) = member else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    dispatch_org_pool_rpc(office, sessions, wake, caller, request)
        .or_else(|| dispatch_local_pool_rpc(office, sessions, caller, request))
}

fn dispatch_org_pool_rpc(
    office: &Office,
    sessions: Option<&Arc<SessionManager>>,
    wake: Option<&Arc<FeedWake>>,
    caller: &str,
    request: &Request,
) -> Option<Response> {
    if request.is::<ProviderConnectionList>() {
        return Some(run_call::<ProviderConnectionList>(request, |p| {
            map_office(office.provider_connection_list(caller, p))
        }));
    }
    if request.is::<ProviderConnectionUpdate>() {
        return Some(run_call::<ProviderConnectionUpdate>(request, |p| {
            let res = map_office(office.provider_connection_update(caller, p))?;
            if let Some(w) = wake {
                w.notify();
            }
            if let Some(mgr) = sessions {
                mgr.process_due_wakes();
            }
            Ok(res)
        }));
    }
    if request.is::<ModelProfileCreate>() {
        return Some(run_call::<ModelProfileCreate>(request, |p| {
            let res = map_office(office.model_profile_create(caller, p))?;
            if let Some(w) = wake {
                w.notify();
            }
            Ok(res)
        }));
    }
    if request.is::<ModelProfileUpdate>() {
        return Some(run_call::<ModelProfileUpdate>(request, |p| {
            let res = map_office(office.model_profile_update(caller, p))?;
            if let Some(w) = wake {
                w.notify();
            }
            if let Some(mgr) = sessions {
                mgr.process_due_wakes();
            }
            Ok(res)
        }));
    }
    None
}

fn dispatch_local_pool_rpc(
    office: &Office,
    sessions: Option<&Arc<SessionManager>>,
    caller: &str,
    request: &Request,
) -> Option<Response> {
    if request.is::<ProviderBindingSet>() {
        return Some(run_call::<ProviderBindingSet>(request, |p| {
            map_office(office.provider_binding_set(caller, p))
        }));
    }
    if request.is::<ProviderBindingList>() {
        return Some(run_call::<ProviderBindingList>(request, |p| {
            map_office(office.provider_binding_list(caller, p))
        }));
    }
    if request.is::<ProviderConnectionRefresh>() {
        return Some(run_call::<ProviderConnectionRefresh>(request, |p| {
            let res = map_office(office.provider_connection_refresh(caller, p))?;
            if let Some(mgr) = sessions {
                mgr.process_due_wakes();
            }
            Ok(res)
        }));
    }
    if request.is::<ModelProfileModels>() {
        return Some(run_call::<ModelProfileModels>(request, |p| {
            map_office(office.model_profile_models(caller, p))
        }));
    }
    if request.is::<AgentReadinessGet>() {
        return Some(run_call::<AgentReadinessGet>(request, |p| {
            map_office(office.agent_readiness_get(caller, p))
        }));
    }
    if request.is::<ProviderQuotaStatus>() {
        return Some(run_call::<ProviderQuotaStatus>(request, |p| {
            map_office(office.provider_quota_status(caller, p))
        }));
    }
    if request.is::<ProviderCooldownRecord>() {
        return Some(run_call::<ProviderCooldownRecord>(request, |p| {
            map_office(office.provider_cooldown_record(caller, p))
        }));
    }
    None
}

fn map_office<T>(res: Result<Result<T, ApiError>, org_office::OfficeError>) -> Result<T, ApiError> {
    res.map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
        .and_then(|r| r)
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
