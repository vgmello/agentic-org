//! Server-validated Chief of Staff (Tess) `suggestion.*` and `ui.present` RPC handlers
//! (`TASK-80.4` Part A).
//!
//! Suggestions live in bounded, runtime-ephemeral office RAM scoped strictly to
//! `(created_by_member_id, for_caller_id)` with TTL expiration so `schema.sql` stays untouched.
//! Both `suggestion.create` and `ui.present` validate every target command, input field, entity
//! reference, and caller permission live against [`Office`] and fail closed on any mismatch.

mod preflight;
mod suggestions;
mod ui;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use org_office::{CallerAuthority, Office};
use org_proto::tess::{
    SuggestionCreate, SuggestionDismiss, SuggestionInspect, SuggestionList, SuggestionWithdraw,
    UiPresent,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use self::suggestions::SuggestionStore;

/// Looks up the active human caller a bound Chief of Staff run acts for (`None` for wakes/others).
pub(crate) type ActingForFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Looks up the human caller on whose behalf Tess drafted a hire request in RAM (`None` if unknown).
pub(crate) type DraftCallerFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

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
pub(super) enum CallerContext {
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
pub(crate) struct TessService {
    office: Arc<Office>,
    acting_for: ActingForFn,
    draft_caller: DraftCallerFn,
    suggestions: Mutex<SuggestionStore>,
    id_seq: AtomicU64,
}

impl TessService {
    pub(crate) fn new(office: Arc<Office>, acting_for: ActingForFn) -> Self {
        Self {
            office,
            acting_for,
            draft_caller: Arc::new(|_| None),
            suggestions: Mutex::new(SuggestionStore::default()),
            id_seq: AtomicU64::new(1),
        }
    }

    pub(crate) fn with_draft_caller(mut self, draft_caller: DraftCallerFn) -> Self {
        self.draft_caller = draft_caller;
        self
    }

    pub(super) fn draft_caller(&self, request_id: &str) -> Option<String> {
        (self.draft_caller)(request_id)
    }

    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
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

    pub(super) fn office(&self) -> &Office {
        &self.office
    }

    pub(super) fn now_ms(&self) -> u64 {
        self.office.current_time_ms()
    }

    /// Generates a canonical lowercase UUIDv7 using the office clock and a monotonic sequence.
    pub(super) fn next_uuid_v7(&self) -> String {
        let ms = self.now_ms();
        let seq = self.id_seq.fetch_add(1, Ordering::Relaxed);
        let mut bytes = [0u8; 16];
        bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
        bytes[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
        bytes[7] = (seq & 0xff) as u8;
        bytes[8] = 0x80 | ((seq >> 56) as u8 & 0x3f);
        bytes[9..16].copy_from_slice(&seq.to_be_bytes()[1..8]);
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15],
        )
    }

    fn resolve_context(&self, member_id: &str) -> Result<CallerContext, ApiError> {
        let tess_id = match self.office.chief_of_staff() {
            Ok(Some(tess)) => Some(tess.id.to_string()),
            Ok(None) => None,
            Err(err) => {
                crate::logged("tess_calls.chief_of_staff", &err);
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "unable to verify Chief of Staff identity",
                ));
            }
        };
        if tess_id.as_deref() == Some(member_id) {
            let Some(caller_id) = (self.acting_for)(member_id) else {
                return Ok(CallerContext::TessWake);
            };
            let authority = self.office.caller_authority(&caller_id).map_err(|err| {
                crate::logged("tess_calls.caller_authority", &err);
                ApiError::new(ErrorCode::Unavailable, "unable to verify caller authority")
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
        let authority = self.office.caller_authority(member_id).map_err(|err| {
            crate::logged("tess_calls.member_authority", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to verify member authority")
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

#[cfg(test)]
mod tests;
