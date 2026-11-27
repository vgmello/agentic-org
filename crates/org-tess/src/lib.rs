//! Server-validated Chief of Staff (Tess) `suggestion.*` and `ui.present` RPC handlers
//! (`TASK-80.4` Part A).
//!
//! Suggestions live in bounded, runtime-ephemeral office RAM scoped strictly to
//! `(created_by_member_id, for_caller_id)` with TTL expiration so `schema.sql` stays untouched.
//! Both `suggestion.create` and `ui.present` validate every target command, input field, entity
//! reference, and caller permission live against [`Office`] and fail closed on any mismatch.

#![forbid(unsafe_code)]

mod preflight;
mod suggestions;
mod ui;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use org_office::{CallerAuthority, Office, OfficeError};
use org_proto::tess::{
    SuggestionCreate, SuggestionDismiss, SuggestionInspect, SuggestionList, SuggestionWithdraw,
    UiPresent,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use self::suggestions::SuggestionStore;

/// Looks up the active human caller a bound Chief of Staff run acts for (`None` for wakes/others).
pub type ActingForFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Looks up the human caller on whose behalf Tess drafted a hire request in RAM (`None` if unknown).
pub type DraftCallerFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Delegation access tier supplied by the office server's delegation table (`serve::delegation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationAccess {
    Open,
    CallerOwner,
    CallerHires,
    Never,
}

/// Evaluates whether a target RPC call is allowed through Chief of Staff delegation.
pub type DelegationPolicyFn = fn(&str) -> DelegationAccess;

/// Whether `request` belongs to the Chief of Staff `suggestion.*` or `ui.present` RPC family.
fn is_tess_method(request: &Request) -> bool {
    request.is::<SuggestionCreate>()
        || request.is::<SuggestionList>()
        || request.is::<SuggestionInspect>()
        || request.is::<SuggestionWithdraw>()
        || request.is::<SuggestionDismiss>()
        || request.is::<UiPresent>()
}

/// Verified caller context for one `suggestion.*` or `ui.present` RPC.
#[derive(Debug, Clone)]
pub(crate) enum CallerContext {
    /// Chief of Staff acting for a verified active human person.
    TessActingFor {
        tess_id: String,
        caller_id: String,
        authority: CallerAuthority,
    },
    /// Chief of Staff running a wake with no human caller (refused for proposals and UI blocks).
    TessWake,
    /// A verified active human person calling directly (e.g. listing, inspecting, or dismissing
    /// suggestions addressed to them).
    HumanCaller {
        caller_id: String,
        authority: CallerAuthority,
    },
}

/// Runtime-ephemeral Chief of Staff suggestion store and live UI verifier.
pub struct TessService {
    office: Arc<Office>,
    acting_for: ActingForFn,
    draft_caller: DraftCallerFn,
    delegation_policy: DelegationPolicyFn,
    suggestions: Mutex<SuggestionStore>,
    id_seq: AtomicU64,
}

impl TessService {
    pub fn new(
        office: Arc<Office>,
        acting_for: ActingForFn,
        delegation_policy: DelegationPolicyFn,
    ) -> Self {
        Self {
            office,
            acting_for,
            draft_caller: Arc::new(|_| None),
            delegation_policy,
            suggestions: Mutex::new(SuggestionStore::default()),
            id_seq: AtomicU64::new(1),
        }
    }

    pub fn with_draft_caller(mut self, draft_caller: DraftCallerFn) -> Self {
        self.draft_caller = draft_caller;
        self
    }

    pub(crate) fn delegation_access(&self, call: &str) -> DelegationAccess {
        (self.delegation_policy)(call)
    }

    pub(crate) fn draft_caller(&self, request_id: &str) -> Option<String> {
        (self.draft_caller)(request_id)
    }

    pub fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if !is_tess_method(request) {
            return None;
        }
        let Some(member_id) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        let context = match self.resolve_context(member_id) {
            Ok(ctx) => ctx,
            Err(err) => return Some(Response::error(request.id, err)),
        };
        if request.is::<SuggestionCreate>() {
            return Some(run_call::<SuggestionCreate>(request, |p| {
                suggestions::handle_create(self, &context, p)
            }));
        }
        if request.is::<SuggestionList>() {
            return Some(run_call::<SuggestionList>(request, |p| {
                suggestions::handle_list(self, &context, p)
            }));
        }
        if request.is::<SuggestionInspect>() {
            return Some(run_call::<SuggestionInspect>(request, |p| {
                suggestions::handle_inspect(self, &context, p)
            }));
        }
        if request.is::<SuggestionWithdraw>() {
            return Some(run_call::<SuggestionWithdraw>(request, |p| {
                suggestions::handle_withdraw(self, &context, p)
            }));
        }
        if request.is::<SuggestionDismiss>() {
            return Some(run_call::<SuggestionDismiss>(request, |p| {
                suggestions::handle_dismiss(self, &context, p)
            }));
        }
        if request.is::<UiPresent>() {
            return Some(run_call::<UiPresent>(request, |p| {
                ui::handle_present(self, &context, p)
            }));
        }
        None
    }

    pub(crate) fn office(&self) -> &Office {
        &self.office
    }

    pub(crate) fn now_ms(&self) -> u64 {
        self.office.current_time_ms()
    }

    /// Generates a canonical lowercase UUIDv7 using the office clock and a monotonic sequence.
    pub(crate) fn next_uuid_v7(&self) -> String {
        let ms = self.now_ms();
        let seq = self.id_seq.fetch_add(1, Ordering::Relaxed);
        let mut bytes = [0u8; 16];
        bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
        bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
        bytes[7] = (seq & 0xff) as u8;
        bytes[8] = 0x80 | ((seq >> 56) as u8 & 0x3f);
        bytes[9..16].copy_from_slice(&seq.to_be_bytes()[1..8]);
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }

    fn resolve_context(&self, member_id: &str) -> Result<CallerContext, ApiError> {
        let tess_id = self
            .office
            .chief_of_staff()
            .map_err(|e| {
                unavailable(
                    "tess_calls.chief_of_staff",
                    &e,
                    "unable to verify Chief of Staff identity",
                )
            })?
            .map(|tess| tess.id.to_string());
        if tess_id.as_deref() == Some(member_id) {
            let Some(caller_id) = (self.acting_for)(member_id) else {
                return Ok(CallerContext::TessWake);
            };
            let authority = self.office.caller_authority(&caller_id).map_err(|e| {
                unavailable(
                    "tess_calls.caller_authority",
                    &e,
                    "unable to verify caller authority",
                )
            })?;
            if !(authority.active && authority.is_person) {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "Chief of Staff can only act for an active person",
                ));
            }
            return Ok(CallerContext::TessActingFor {
                tess_id: member_id.to_owned(),
                caller_id,
                authority,
            });
        }
        let authority = self.office.caller_authority(member_id).map_err(|e| {
            unavailable(
                "tess_calls.member_authority",
                &e,
                "unable to verify member authority",
            )
        })?;
        if authority.active && authority.is_person {
            Ok(CallerContext::HumanCaller {
                caller_id: member_id.to_owned(),
                authority,
            })
        } else {
            Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only the Chief of Staff or an active person may use this endpoint",
            ))
        }
    }
}

pub(crate) fn unavailable(tag: &str, err: &OfficeError, msg: &str) -> ApiError {
    eprintln!("agentic-org-office: {tag} failed: {err}");
    let mut cause = std::error::Error::source(err);
    while let Some(inner) = cause {
        eprintln!("agentic-org-office:   caused by: {inner}");
        cause = inner.source();
    }
    ApiError::new(ErrorCode::Unavailable, msg)
}

fn run_call<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(params) => params,
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
