//! Protected account authority; imported directory flags grant nothing.
use crate::{store::Store, AcceptanceIdentity, OfficeError};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;
/// Returned only by a protected host session/grant/account verifier. It must
/// recheck channel, session expiry/revocation, linked scope and live ownership.
/// Implementations must not derive this identity from imported profile flags.
pub trait OwnerAuthority: Send + Sync + 'static {
    fn verify_live(&self, scope: &AcceptanceIdentity) -> Result<OwnerAccount, OfficeError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerAccount {
    pub account_id: String,
    pub member_id: String,
}

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("selected directory source rejected")
}
fn unauthorized() -> OfficeError {
    OfficeError::Invalid("not authorized")
}
pub(super) fn owner(
    db: &Connection,
    id: &AcceptanceIdentity,
    authority: &dyn OwnerAuthority,
) -> Result<OwnerAccount, OfficeError> {
    let owner = authority.verify_live(id)?;
    if owner.account_id.is_empty() || owner.member_id.is_empty() {
        return Err(unauthorized());
    }
    if !owner_binding(db, id, &owner)? {
        return Err(unauthorized());
    }
    Ok(owner)
}
fn owner_binding(
    db: &Connection,
    id: &AcceptanceIdentity,
    subject: &OwnerAccount,
) -> Result<bool, OfficeError> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM selected_owner_authority WHERE organization_id=?1 AND account_id=?2 AND member_id=?3 AND active=1)",params![id.organization_id,subject.account_id,subject.member_id],|r|r.get(0)).map_err(storage)
}
pub(crate) fn bind_owner(
    store: &Store,
    authority: Arc<dyn OwnerAuthority>,
) -> Result<(), OfficeError> {
    // A verified existing binding is a coherent read, including after ordinal
    // exhaustion. Reopening must not allocate a redundant transaction merely
    // to regain protected receipt reads. Fresh installation remains writer-bound.
    let verification = Arc::clone(&authority);
    if store.selected_read(move |db, id| {
        let prior: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM selected_owner_authority WHERE organization_id=?1)",
                [&id.organization_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if prior {
            owner(db, id, &*verification)?;
        }
        Ok(prior)
    })? {
        return Ok(());
    }
    store.selected_transaction(move|db,id| {
        let binding=authority.verify_live(id)?;
        if binding.account_id.is_empty()||binding.member_id.is_empty() {return Err(invalid());}
        let prior:Option<(String,String,i64)>=db.query_row("SELECT account_id,member_id,active FROM selected_owner_authority WHERE organization_id=?1",[&id.organization_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
        if let Some(prior)=prior {
            if prior!=(binding.account_id,binding.member_id,1) {return Err(OfficeError::Invalid("protected owner binding requires recovery"));}
            return Ok(());
        }
        db.execute("INSERT INTO selected_owner_authority VALUES(?1,?2,?3,1,1)",params![id.organization_id,binding.account_id,binding.member_id]).map_err(storage)?;
        Ok(())
    })
}
