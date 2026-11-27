//! Thin office-server adapter wiring [`org_tess::TessService`] to [`crate::serve::delegation`].

use std::sync::Arc;

use org_office::Office;
use org_proto::{Request, Response};
use org_tess::{ActingForFn, DelegationAccess, DraftCallerFn};

use crate::serve::delegation::Access;

fn delegation_policy(call: &str) -> DelegationAccess {
    match crate::serve::delegation::access(call) {
        Access::Open => DelegationAccess::Open,
        Access::CallerOwner => DelegationAccess::CallerOwner,
        Access::CallerHires => DelegationAccess::CallerHires,
        Access::Never => DelegationAccess::Never,
    }
}

pub(crate) struct TessService(org_tess::TessService);

impl TessService {
    pub(crate) fn new(office: Arc<Office>, acting_for: ActingForFn) -> Self {
        Self(org_tess::TessService::new(
            office,
            acting_for,
            delegation_policy,
        ))
    }

    pub(crate) fn with_draft_caller(self, draft_caller: DraftCallerFn) -> Self {
        Self(self.0.with_draft_caller(draft_caller))
    }

    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        self.0.dispatch(member, request)
    }
}

#[cfg(test)]
#[path = "tess_calls/tests.rs"]
mod tests;
