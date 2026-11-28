//! Office GitHub App service and RPC dispatcher (`TASK-272`, 3/3 ACs).
//!
//! Composes [`org_office::Office`] with [`org_github`] to provide:
//! - Owner-only wrapped GitHub App configuration, status, revocation, and repository bindings
//!   (`github.app.*`, `github.repo.*`, `via_grant = true`, NEVER for Tess/agents).
//! - Constant-time HMAC-SHA256 verified webhook ingestion (`github.webhook.ingest`) with bounded
//!   replay deduplication, untrusted-data sanitization, live SSE notifications, and linking to
//!   Agentic Org tasks (`T-`) and Universal Reviews (`RV-`).
//! - Project-scoped PR, review comment, CI failure analysis, and on-demand installation-token
//!   sync (`github.pr.*`) enforcing `ADR-0031` active-employee & project visibility.

mod linking;
mod pr_ops;
mod scope;
mod store;
#[cfg(test)]
mod tests;
mod vault;
mod webhook_ops;

use std::sync::{Arc, Mutex};

use org_github::{
    GitHubHttpClient, InstallationTokenManager, SystemGitHubHttpClient, ValidatedRepoSlug,
};
use org_office::Office;
use org_proto::github::{
    GitHubAppConfigure, GitHubAppConfigureParams, GitHubAppRevoke, GitHubAppStatus,
    GitHubAppStatusView, GitHubPrChecks, GitHubPrInspect, GitHubPrList, GitHubPrSync,
    GitHubRepoBind, GitHubRepoBindParams, GitHubRepoBindingView, GitHubRepoUnbind,
    GitHubRepoUnbindParams, GitHubRepoUnbindResult, GitHubWebhookIngest, WEBHOOK_PRINCIPAL_MEMBER,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::live::LiveEvents;

use self::store::{load_bindings, save_bindings, StoredRepoBinding};
use self::vault::{load_github_app_meta, now_rfc3339, revoke_github_app, store_github_app};

/// Matches every GitHub App integration RPC using explicit `request.is::<Call>()` checks so the
/// host operation inventory AST scan recognizes each served method directly inside
/// `org-office-server`.
pub(crate) fn is_github_method(request: &Request) -> bool {
    request.is::<GitHubAppConfigure>()
        || request.is::<GitHubAppStatus>()
        || request.is::<GitHubAppRevoke>()
        || request.is::<GitHubRepoBind>()
        || request.is::<GitHubRepoUnbind>()
        || request.is::<GitHubWebhookIngest>()
        || request.is::<GitHubPrList>()
        || request.is::<GitHubPrInspect>()
        || request.is::<GitHubPrChecks>()
        || request.is::<GitHubPrSync>()
}

/// Whether `request` is an owner-only GitHub App or repository binding management RPC.
pub(crate) fn requires_owner_grant(request: &Request) -> bool {
    request.is::<GitHubAppConfigure>()
        || request.is::<GitHubAppStatus>()
        || request.is::<GitHubAppRevoke>()
        || request.is::<GitHubRepoBind>()
        || request.is::<GitHubRepoUnbind>()
}

/// Office-side GitHub App integration service.
pub(crate) struct GitHubService {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    tokens: InstallationTokenManager,
    store_lock: Mutex<()>,
}

impl GitHubService {
    pub(crate) fn new(office: Arc<Office>, live: Arc<LiveEvents>) -> Self {
        Self::with_http_client(office, live, Arc::new(SystemGitHubHttpClient))
    }

    pub(crate) fn with_http_client(
        office: Arc<Office>,
        live: Arc<LiveEvents>,
        http: Arc<dyn GitHubHttpClient>,
    ) -> Self {
        Self {
            office,
            live,
            tokens: InstallationTokenManager::new(http),
            store_lock: Mutex::new(()),
        }
    }

    pub(crate) fn dispatch(
        &self,
        member: Option<&str>,
        via_grant: bool,
        request: &Request,
    ) -> Option<Response> {
        if !is_github_method(request) {
            return None;
        }
        let Some(member) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        if request.is::<GitHubAppConfigure>() {
            return Some(run_call::<GitHubAppConfigure>(request, |p| {
                self.require_owner_grant(member, via_grant)?;
                self.handle_configure(p)
            }));
        }
        if request.is::<GitHubAppStatus>() {
            return Some(run_call::<GitHubAppStatus>(request, |_| {
                self.require_owner_grant(member, via_grant)?;
                self.handle_status()
            }));
        }
        if request.is::<GitHubAppRevoke>() {
            return Some(run_call::<GitHubAppRevoke>(request, |_| {
                self.require_owner_grant(member, via_grant)?;
                self.handle_revoke()
            }));
        }
        if request.is::<GitHubRepoBind>() {
            return Some(run_call::<GitHubRepoBind>(request, |p| {
                self.require_owner_grant(member, via_grant)?;
                self.handle_repo_bind(p)
            }));
        }
        if request.is::<GitHubRepoUnbind>() {
            return Some(run_call::<GitHubRepoUnbind>(request, |p| {
                self.require_owner_grant(member, via_grant)?;
                self.handle_repo_unbind(p)
            }));
        }
        if request.is::<GitHubWebhookIngest>() {
            return Some(run_call::<GitHubWebhookIngest>(request, |p| {
                self.require_webhook_grant(member, via_grant)?;
                webhook_ops::handle_webhook_ingest(&self.office, &self.live, &self.store_lock, p)
            }));
        }
        if request.is::<GitHubPrList>() {
            return Some(run_call::<GitHubPrList>(request, |p| {
                pr_ops::handle_pr_list(&self.office, member, p)
            }));
        }
        if request.is::<GitHubPrInspect>() {
            return Some(run_call::<GitHubPrInspect>(request, |p| {
                pr_ops::handle_pr_inspect(&self.office, member, p)
            }));
        }
        if request.is::<GitHubPrChecks>() {
            return Some(run_call::<GitHubPrChecks>(request, |p| {
                pr_ops::handle_pr_checks(&self.office, member, p)
            }));
        }
        if request.is::<GitHubPrSync>() {
            return Some(run_call::<GitHubPrSync>(request, |p| {
                pr_ops::handle_pr_sync(
                    &self.office,
                    &self.live,
                    &self.tokens,
                    &self.store_lock,
                    member,
                    p,
                )
            }));
        }
        None
    }

    fn require_owner_grant(&self, member: &str, via_grant: bool) -> Result<(), ApiError> {
        if !via_grant || member != self.office.owner_member_id() {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "GitHub App configuration and repository bindings require a verified owner grant",
            ));
        }
        Ok(())
    }

    fn require_webhook_grant(&self, member: &str, via_grant: bool) -> Result<(), ApiError> {
        if !via_grant
            || (member != WEBHOOK_PRINCIPAL_MEMBER && member != self.office.owner_member_id())
        {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "GitHub webhook ingestion requires a verified webhook or owner host grant",
            ));
        }
        Ok(())
    }

    fn handle_configure(
        &self,
        params: GitHubAppConfigureParams,
    ) -> Result<GitHubAppStatusView, ApiError> {
        let _guard = self.store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let meta = store_github_app(&self.office, &params)?;
        self.tokens.clear();
        let bindings = self.load_binding_views()?;
        Ok(GitHubAppStatusView {
            office_id: self.office.info().office_id.clone(),
            configured: true,
            app_id: Some(meta.app_id),
            installation_id: Some(meta.installation_id),
            api_base_url: Some(meta.api_base_url),
            webhook_configured: meta.webhook_configured,
            account_hint: meta.account_hint,
            updated_at: Some(meta.updated_at),
            bindings,
        })
    }

    fn handle_status(&self) -> Result<GitHubAppStatusView, ApiError> {
        let _guard = self.store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let bindings = self.load_binding_views()?;
        let office_id = self.office.info().office_id.clone();
        match load_github_app_meta(&self.office)? {
            Some(meta) => Ok(GitHubAppStatusView {
                office_id,
                configured: true,
                app_id: Some(meta.app_id),
                installation_id: Some(meta.installation_id),
                api_base_url: Some(meta.api_base_url),
                webhook_configured: meta.webhook_configured,
                account_hint: meta.account_hint,
                updated_at: Some(meta.updated_at),
                bindings,
            }),
            None => Ok(GitHubAppStatusView {
                office_id,
                configured: false,
                app_id: None,
                installation_id: None,
                api_base_url: None,
                webhook_configured: false,
                account_hint: None,
                updated_at: None,
                bindings,
            }),
        }
    }

    fn handle_revoke(&self) -> Result<GitHubAppStatusView, ApiError> {
        let _guard = self.store_lock.lock().unwrap_or_else(|e| e.into_inner());
        revoke_github_app(&self.office)?;
        self.tokens.clear();
        let bindings = self.load_binding_views()?;
        Ok(GitHubAppStatusView {
            office_id: self.office.info().office_id.clone(),
            configured: false,
            app_id: None,
            installation_id: None,
            api_base_url: None,
            webhook_configured: false,
            account_hint: None,
            updated_at: None,
            bindings,
        })
    }

    fn handle_repo_bind(
        &self,
        params: GitHubRepoBindParams,
    ) -> Result<GitHubRepoBindingView, ApiError> {
        let slug = ValidatedRepoSlug::new(&params.owner, &params.repo)
            .map_err(|e| ApiError::new(ErrorCode::InvalidRequest, e.to_string()))?;
        let repo = self
            .office
            .resolve_repository(params.repository.trim())
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;

        let _guard = self.store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let installation_id = match params.installation_id {
            Some(0) => {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "installation_id must be positive",
                ));
            }
            Some(id) => id,
            None => load_github_app_meta(&self.office)?
                .ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::Conflict,
                        "configure the GitHub App before binding repositories without an explicit installation_id",
                    )
                })?
                .installation_id,
        };

        let mut bindings = load_bindings(&self.office)?;
        let full_name = slug.full_name();
        if bindings.iter().any(|b| {
            b.repository_id != repo.id.as_str() && b.full_name.eq_ignore_ascii_case(&full_name)
        }) {
            return Err(ApiError::new(
                ErrorCode::AlreadyExists,
                "GitHub repository slug is already bound to another repository",
            ));
        }

        let binding = StoredRepoBinding {
            repository_id: repo.id.as_str().to_owned(),
            owner: slug.owner().to_owned(),
            repo: slug.repo().to_owned(),
            full_name,
            installation_id,
            bound_at: now_rfc3339(),
        };
        if let Some(existing) = bindings
            .iter_mut()
            .find(|b| b.repository_id == binding.repository_id)
        {
            *existing = binding.clone();
        } else {
            bindings.push(binding.clone());
        }
        save_bindings(&self.office, &bindings)?;
        Ok(binding.to_view(repo.key))
    }

    fn handle_repo_unbind(
        &self,
        params: GitHubRepoUnbindParams,
    ) -> Result<GitHubRepoUnbindResult, ApiError> {
        let repo = self
            .office
            .resolve_repository(params.repository.trim())
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "repository not found"))?;

        let _guard = self.store_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut bindings = load_bindings(&self.office)?;
        let before = bindings.len();
        bindings.retain(|b| b.repository_id != repo.id.as_str());
        let unbound = bindings.len() != before;
        if unbound {
            save_bindings(&self.office, &bindings)?;
        }
        Ok(GitHubRepoUnbindResult {
            repository_id: repo.id.as_str().to_owned(),
            unbound,
        })
    }

    fn load_binding_views(&self) -> Result<Vec<GitHubRepoBindingView>, ApiError> {
        let bindings = load_bindings(&self.office)?;
        Ok(bindings
            .into_iter()
            .map(|b| {
                let key = self
                    .office
                    .resolve_repository(&b.repository_id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.key);
                b.to_view(key)
            })
            .collect())
    }
}

fn run_call<C: Call>(
    request: &Request,
    f: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(p) => p,
        Err(err) => {
            return Response::error(
                request.id,
                ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
            );
        }
    };
    match f(params) {
        Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "could not encode response"),
            )
        }),
        Err(err) => Response::error(request.id, err),
    }
}
