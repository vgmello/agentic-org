//! One-call agent wake/resume RPC dispatcher (`agent.resume`, `TASK-220`).
//!
//! Composes [`Office`] reads with [`GitService::active_worktrees_for_member`] into a single
//! bounded structured RPC response.

mod assemble;

use std::sync::Arc;

use org_office::{Office, OfficeError};
use org_proto::external_auth::SessionMode;
use org_proto::resume::AgentResume;
use org_proto::{ApiError, ErrorCode, Request, Response};

use crate::failure::logged;
use crate::git_calls::GitService;

use self::assemble::{assemble_resume, ResumeLimits};

/// Whether `request` is the `agent.resume` RPC.
pub(crate) fn is_resume_method(request: &Request) -> bool {
    request.is::<AgentResume>()
}

/// Dispatcher composing `Office` and `GitService` for `agent.resume`.
pub(crate) struct ResumeService {
    office: Arc<Office>,
    git: Arc<GitService>,
}

impl ResumeService {
    pub(crate) fn new(office: Arc<Office>, git: Arc<GitService>) -> Self {
        Self { office, git }
    }

    pub(crate) fn dispatch(
        &self,
        member: Option<&str>,
        session_mode: Option<SessionMode>,
        request: &Request,
    ) -> Option<Response> {
        if !is_resume_method(request) {
            return None;
        }
        let Some(member) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        let params = match request.params::<AgentResume>() {
            Ok(params) => params,
            Err(err) => {
                return Some(Response::error(
                    request.id,
                    ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
                ));
            }
        };
        let limits = match ResumeLimits::from_params(&params) {
            Ok(limits) => limits,
            Err(api_err) => return Some(Response::error(request.id, api_err)),
        };
        let worktrees = self
            .git
            .active_worktrees_for_member(member, limits.worktrees);
        let outcome = assemble_resume(&self.office, member, session_mode, worktrees, &limits);
        Some(match outcome {
            Ok(Ok(result)) => match Response::ok::<AgentResume>(request.id, &result) {
                Ok(resp) => resp,
                Err(err) => Response::error(
                    request.id,
                    ApiError::new(ErrorCode::StorageError, err.to_string()),
                ),
            },
            Ok(Err(api_err)) => Response::error(request.id, api_err),
            Err(OfficeError::SnapshotPending) => Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Unavailable,
                    "organization snapshot is not available yet",
                ),
            ),
            Err(err) => {
                logged("agent.resume", &err);
                Response::error(
                    request.id,
                    ApiError::new(ErrorCode::StorageError, "failed to load resume snapshot"),
                )
            }
        })
    }
}

#[cfg(test)]
#[path = "resume_calls_tests.rs"]
mod tests;
