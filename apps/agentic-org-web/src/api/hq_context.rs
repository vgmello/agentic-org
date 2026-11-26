//! The HQ office of an organization, as the caller's session sees it: the shared lookup every
//! `*_routes` slice makes before it forwards a call to the organization's office.
//!
//! A leaf of `api`: it uses the parent's [`SessionApi`] and the host's organization list,
//! and no route module, so the route slices depend on it and never on each other.

use org_host_control::organization as host_org;

use super::{CallerAuth, SessionApi};
use crate::host::Failure;

/// The organization's HQ office (its first office when none is HQ), after validating the
/// caller's session.
pub(super) async fn resolve_hq_office(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
) -> Result<String, Failure> {
    api.validate_async(pair).await?;
    let list = api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await?;
    find_hq_office(list, org_id)
}

/// As [`resolve_hq_office`], after revalidating the session with the host (the retry after a
/// stale grant).
pub(super) async fn revalidate_and_resolve_hq(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
) -> Result<String, Failure> {
    api.revalidate_session_async(pair).await?;
    let list = api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await?;
    find_hq_office(list, org_id)
}

fn find_hq_office(list: host_org::ListResult, org_id: &str) -> Result<String, Failure> {
    let org = list
        .organizations
        .into_iter()
        .find(|org| org.id == org_id)
        .ok_or(Failure::NotFound)?;
    org.offices
        .iter()
        .find(|office| office.role == org_proto::OfficeRole::Hq)
        .or_else(|| org.offices.first())
        .map(|office| office.id.clone())
        .ok_or(Failure::Unavailable)
}
