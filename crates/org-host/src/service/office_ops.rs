//! Live office listing and authorized office restart handlers for browser and native
//! sessions (implementation plan O1, FC-019, FC-021).

use org_host_control::{
    browser::BrowserOfficeRestart,
    ops::{OfficeJoin, OfficeList, OfficeReEnroll, OfficeRemovePending, OfficeRestart},
    HostOpResult,
};
use org_proto::{
    ApiError, Empty, ErrorCode, ListOfficesResult, OfficeInfo, OfficeRole, Request, Response,
};

use super::browser_sessions::{browser_now_ms, finish_ended_browser_sessions};
use super::office_join;
use super::reply::ok;
use super::session_office::{authorize_session_first, journal_error};
use super::{Authentication, Service};
use crate::control_listener::Connection;

pub(super) fn dispatch_session_office_op(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &std::sync::Arc<Connection>,
) -> bool {
    if request.is::<OfficeList>() {
        list_session_offices(service, authentication, request, connection);
    } else if request.is::<OfficeRestart>() {
        restart_session_office(service, authentication, request, connection);
    } else if request.is::<OfficeJoin>() {
        office_join::join_session_office(service, authentication, request, connection);
    } else if request.is::<OfficeRemovePending>() {
        office_join::remove_pending_session_office(service, authentication, request, connection);
    } else if request.is::<OfficeReEnroll>() {
        office_join::re_enroll_session_office(service, authentication, request, connection);
    } else {
        return false;
    }
    true
}
use crate::hostdb::{
    accounts::Account,
    journal::{Attempt, Effect, Lookup, Stored, Write},
    Error as DbError, HostAccountsStore, OperationJournalStore,
};

/// Offices visible to `account`: every office belonging to an organization the account is linked
/// to, plus—when `allow_unlinked_satellite` is true (native `host.sock` only)—any local satellite
/// office (`OfficeRole::Satellite`) when `account` is an instance administrator. Unlinked
/// `OfficeRole::Hq` offices remain strictly hidden, and browser (`ChannelKind::Web`) requests
/// require an explicit organization link.
pub(super) fn office_visible_to_account(
    account: &Account,
    office: &OfficeInfo,
    allow_unlinked_satellite: bool,
) -> bool {
    account
        .links()
        .iter()
        .any(|l| l.org_id() == office.organization_id)
        || (allow_unlinked_satellite && account.admin() && office.role == OfficeRole::Satellite)
}

/// Resolve an office visible to `account`. Reserved offices whose creation has not yet
/// linked the organization are intentionally absent from `offices` and answer `NotFound`,
/// preventing pre-link existence oracles; host startup waits still surface `NotReady`.
fn find_linked_office(
    offices: &[OfficeInfo],
    account: &Account,
    org_filter: Option<&str>,
    office_id: &str,
    allow_unlinked_satellite: bool,
) -> Result<(OfficeInfo, Option<String>), ApiError> {
    let office = offices
        .iter()
        .find(|o| o.id == office_id && org_filter.is_none_or(|org| o.organization_id == org))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "office not found"))?;
    if let Some(link) = account
        .links()
        .iter()
        .find(|l| l.org_id() == office.organization_id)
    {
        return Ok((office.clone(), Some(link.member_id().to_owned())));
    }
    if allow_unlinked_satellite && account.admin() && office.role == OfficeRole::Satellite {
        return Ok((office.clone(), None));
    }
    Err(ApiError::new(ErrorCode::NotFound, "office not found"))
}

fn check_restart_authority(
    accounts: &dyn HostAccountsStore,
    account: &Account,
    org_id: &str,
    member_id: Option<&str>,
) -> Result<(), ApiError> {
    if account.admin() {
        return Ok(());
    }
    let Some(member_id) = member_id else {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the administrator or organization owner may restart an office",
        ));
    };
    accounts
        .verify_selected_owner(org_id, account.account_id(), member_id)
        .map_err(|error| match error {
            DbError::Unsafe => ApiError::new(
                ErrorCode::NotAuthorized,
                "only the administrator or organization owner may restart an office",
            ),
            _ => ApiError::new(ErrorCode::Unavailable, "host registry unavailable"),
        })
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper threading explicit browser-vs-local satellite visibility gate"
)]
fn run_journaled_restart(
    service: &Service,
    accounts: &dyn HostAccountsStore,
    journal: &dyn OperationJournalStore,
    account: &Account,
    org_filter: Option<&str>,
    office_id: &str,
    allow_unlinked_satellite: bool,
    attempt: &Attempt<'_>,
    now: i64,
) -> Result<HostOpResult<Empty>, ApiError> {
    let (office, member_id) = find_linked_office(
        &service.offices.offices(),
        account,
        org_filter,
        office_id,
        allow_unlinked_satellite,
    )?;
    check_restart_authority(
        accounts,
        account,
        &office.organization_id,
        member_id.as_deref(),
    )?;
    // Serialize journal lookup -> restart -> journal write across concurrent worker threads
    // (mirroring `WebTls::set`) so simultaneous retries cannot race into false `Conflict` or
    // execute a duplicate restart before `OperationReused` is recorded.
    let _restart_guard = crate::sync::lock(&service.office_restarting);
    match journal
        .journal_lookup(attempt, now)
        .map_err(journal_error)?
    {
        Lookup::Replay(stored) => return decode_stored(stored),
        Lookup::NeedsVerification { .. } => {
            return Err(ApiError::new(
                ErrorCode::OperationReused,
                "operation ID already used",
            ));
        }
        Lookup::Vacant => {}
    }
    service.offices.restart_office(&office.id)?;
    let write = journal
        .journal_commit(
            attempt,
            now,
            Effect {
                stored: Stored::Completed {
                    result: serde_json::json!({}),
                },
                verifier: None,
            },
        )
        .map_err(journal_error)?;
    match write {
        Write::Committed(stored) | Write::Existing(Lookup::Replay(stored)) => decode_stored(stored),
        Write::Existing(_) => Err(ApiError::new(
            ErrorCode::OperationReused,
            "operation ID already used",
        )),
    }
}

fn decode_stored(stored: Stored) -> Result<HostOpResult<Empty>, ApiError> {
    match stored {
        Stored::Completed { .. } => Ok(HostOpResult::Completed { result: Empty {} }),
        Stored::Rejected { refusal } => Ok(HostOpResult::Rejected { refusal }),
        _ => Err(ApiError::new(
            ErrorCode::Unavailable,
            "unrecognized stored restart state",
        )),
    }
}

/// `browser.office_restart` on the admitted web connection only.
pub(super) fn restart_browser_office(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let p = request
            .params::<BrowserOfficeRestart>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid restart request"))?;
        let account = authentication.browser_auth_account(connection, &p.auth, now, &mut ended)?;
        let accounts = authentication.accounts_store();
        let journal = authentication.journal_store();
        let attempt = Attempt::new(&p, account.account_id()).map_err(journal_error)?;
        run_journaled_restart(
            service,
            &*accounts,
            &*journal,
            &account,
            Some(&p.org_id),
            &p.office_id,
            false,
            &attempt,
            now,
        )
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(res) => connection.send_response(ok::<BrowserOfficeRestart>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

/// `office.list` on `host.sock` for a native authenticated session.
pub(super) fn list_session_offices(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = authorize_session_first(authentication, request, connection).and_then(|account| {
        let p = request
            .params::<OfficeList>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid office list request"))?;
        let allow_unlinked_satellite =
            connection.channel_kind() == org_office_launch::ChannelKind::Local;
        let all_offices = service.offices.offices();
        if let Some(org_id) = &p.org_id {
            let visible_org = account.links().iter().any(|l| l.org_id() == org_id)
                || (allow_unlinked_satellite
                    && account.admin()
                    && all_offices
                        .iter()
                        .any(|o| o.role == OfficeRole::Satellite && o.organization_id == *org_id));
            if !visible_org {
                return Err(ApiError::new(ErrorCode::NotFound, "organization not found"));
            }
        }
        let offices = all_offices
            .into_iter()
            .filter(|o| {
                office_visible_to_account(&account, o, allow_unlinked_satellite)
                    && p.org_id.as_deref().is_none_or(|id| o.organization_id == id)
            })
            .map(|mut o| {
                o.endpoint = None;
                o
            })
            .collect();
        Ok(ListOfficesResult { offices })
    });
    match reply {
        Ok(res) => connection.send_response(ok::<OfficeList>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

/// `office.restart` on `host.sock` for a native authenticated session.
pub(super) fn restart_session_office(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = authorize_session_first(authentication, request, connection).and_then(|account| {
        let now = browser_now_ms()?;
        let p = request
            .params::<OfficeRestart>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid restart request"))?;
        let accounts = authentication.accounts_store();
        let journal = authentication.journal_store();
        let attempt = Attempt::new(&p, account.account_id()).map_err(journal_error)?;
        let allow_unlinked_satellite =
            connection.channel_kind() == org_office_launch::ChannelKind::Local;
        run_journaled_restart(
            service,
            &*accounts,
            &*journal,
            &account,
            None,
            &p.office_id,
            allow_unlinked_satellite,
            &attempt,
            now,
        )
    });
    match reply {
        Ok(res) => connection.send_response(ok::<OfficeRestart>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

#[cfg(test)]
mod tests;
