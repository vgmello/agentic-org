//! Multi-person account SQLite operations (`TASK-38` / A6):
//! organization invitations, existing-account invitation attachment (SIGN-35),
//! set-password links (`admin reset` and People › New sign-in link), own password change,
//! access removal, organization owner handover, and instance administrator handover.

use std::collections::{BTreeMap, BTreeSet};

use org_host_control::browser_accounts::OrgAccessEntry;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use super::super::{administrator, clock, local_auth, Error, HostDb};
use crate::accounts::identity::AccountId;
use crate::accounts::store::StoreError;
pub(crate) use crate::hostdb::multi_person::{
    account_subject, invite_subject, parse_account_subject, parse_invite_subject,
    AdminHandoverOutcome, InviteAttach, InviteIssueError, InviteRegistration,
    MemberResetIssueError, OrgOwnerHandoverOutcome, RemoveAccessOutcome, SetPasswordOutcome,
    ACCOUNT_PREFIX, INVITE_PREFIX,
};
use crate::sessions::Ended;

mod invites;
mod passwords;

fn unavailable<T>(_: T) -> StoreError {
    StoreError::Unavailable
}

fn delete_account_sessions(
    tx: &Transaction<'_>,
    account_id: &str,
    except: Option<&str>,
) -> Result<Vec<Ended>, rusqlite::Error> {
    let mut ended = tx
        .prepare(
            "DELETE FROM browser_sessions WHERE account_id=?1 AND id IS NOT ?2 RETURNING id, account_id",
        )?
        .query_map((account_id, except), |r| {
            Ok(Ended {
                id: r.get(0)?,
                account_id: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    ended.sort();
    Ok(ended)
}

fn active_admin_in(tx: &Transaction<'_>) -> Result<Option<(AccountId, String)>, rusqlite::Error> {
    let row: Option<(String, String)> = tx
        .query_row(
            "SELECT account_id, login FROM accounts WHERE admin=1 AND revoked_at_ms IS NULL AND password_phc <> ?1 LIMIT 1",
            [local_auth::LOCAL_AUTHLESS_VERIFIER],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(id, login)| AccountId::parse(&id).ok().map(|a| (a, login))))
}

impl HostDb {
    /// Returns the unrevoked password-authenticated administrator's `(account_id, display_login)`.
    pub(crate) fn active_administrator_account(
        &self,
    ) -> Result<Option<(AccountId, String)>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let found = active_admin_in(&tx)?;
        tx.commit()?;
        Ok(found)
    }

    /// Returns `(owner_account_id, owner_member_id)` for `org_id`, if registered.
    pub(crate) fn org_owner(&self, org_id: &str) -> Result<Option<(AccountId, String)>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let row: Option<(String, String)> = db
            .query_row(
                "SELECT account, owner_member_id FROM org_owners WHERE org_id=?1",
                [org_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((account, member)) => {
                let id = AccountId::parse(&account).map_err(|_| Error::Unsafe)?;
                Ok(Some((id, member)))
            }
        }
    }

    /// Lists account links and pending invites/password-resets scoped strictly to `org_id`.
    pub(crate) fn list_org_access(
        &self,
        org_id: &str,
        now_ms: i64,
    ) -> Result<Vec<OrgAccessEntry>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction()?;
        let now = clock::now(&tx, now_ms)?;
        let owner_member: Option<String> = tx
            .query_row(
                "SELECT owner_member_id FROM org_owners WHERE org_id=?1",
                [org_id],
                |r| r.get(0),
            )
            .optional()?;

        let mut pending_invites = BTreeSet::new();
        {
            let prefix = format!("{INVITE_PREFIX}{org_id}:");
            let mut stmt = tx.prepare(
                "SELECT subject FROM one_time_links WHERE kind='registration' AND state='pending' AND expires_at_ms > ?1",
            )?;
            let rows = stmt.query_map([now], |r| r.get::<_, String>(0))?;
            for row in rows {
                let subject = row?;
                if let Some(member_id) = subject.strip_prefix(&prefix) {
                    pending_invites.insert(member_id.to_owned());
                }
            }
        }

        let mut pending_resets = BTreeSet::new();
        {
            let mut stmt = tx.prepare(
                "SELECT subject FROM one_time_links WHERE kind='set_password' AND state='pending' AND expires_at_ms > ?1",
            )?;
            let rows = stmt.query_map([now], |r| r.get::<_, String>(0))?;
            for row in rows {
                let subject = row?;
                if let Some(account_id) = subject.strip_prefix(ACCOUNT_PREFIX) {
                    pending_resets.insert(account_id.to_owned());
                }
            }
        }

        let mut by_member: BTreeMap<String, OrgAccessEntry> = BTreeMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT l.member_id, a.account_id, a.login, a.admin \
                 FROM account_links l \
                 JOIN accounts a ON a.account_id = l.account_id \
                 WHERE l.org_id=?1 AND a.revoked_at_ms IS NULL AND a.password_phc <> ?2",
            )?;
            let rows = stmt.query_map((org_id, local_auth::LOCAL_AUTHLESS_VERIFIER), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            })?;
            for row in rows {
                let (member_id, account_id, login, admin) = row?;
                let pending_invite = pending_invites.remove(&member_id);
                let pending_password_reset = pending_resets.contains(&account_id);
                let owner = owner_member.as_deref() == Some(member_id.as_str());
                by_member.insert(
                    member_id.clone(),
                    OrgAccessEntry {
                        member_id,
                        account_id: Some(account_id),
                        login: Some(login),
                        admin,
                        owner,
                        pending_invite,
                        pending_password_reset,
                    },
                );
            }
        }
        for member_id in pending_invites {
            let owner = owner_member.as_deref() == Some(member_id.as_str());
            by_member.insert(
                member_id.clone(),
                OrgAccessEntry {
                    member_id,
                    account_id: None,
                    login: None,
                    admin: false,
                    owner,
                    pending_invite: true,
                    pending_password_reset: false,
                },
            );
        }
        tx.commit()?;
        Ok(by_member.into_values().collect())
    }

    /// Removes a person's sign-in access to `org_id` (cancels pending invite, deletes the
    /// `(account_id, org_id)` link, and ends every browser session of that account so access
    /// drops immediately). Refuses with `IsOwner` if `member_id` is the organization owner.
    pub(crate) fn remove_org_access(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<RemoveAccessOutcome, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock::now(&tx, now_ms)?;
        let owner_member: Option<String> = tx
            .query_row(
                "SELECT owner_member_id FROM org_owners WHERE org_id=?1",
                [org_id],
                |r| r.get(0),
            )
            .optional()?;
        if owner_member.as_deref() == Some(member_id) {
            return Ok(RemoveAccessOutcome::IsOwner);
        }
        let invite_subj = format!("{INVITE_PREFIX}{org_id}:{member_id}");
        let cancelled_invites = tx.execute(
            "UPDATE one_time_links SET state='cancelled', ended_at_ms=?2 WHERE kind='registration' AND subject=?1 AND state='pending'",
            (&invite_subj, now),
        )?;
        let linked_account: Option<String> = tx
            .query_row(
                "SELECT account_id FROM account_links WHERE org_id=?1 AND member_id=?2",
                (org_id, member_id),
                |r| r.get(0),
            )
            .optional()?;
        let Some(raw_account) = linked_account else {
            if cancelled_invites > 0 {
                clock::advance_clock(&tx, now)?;
                tx.commit()?;
                return Ok(RemoveAccessOutcome::Removed {
                    account_id: None,
                    ended: Vec::new(),
                });
            }
            return Ok(RemoveAccessOutcome::NotFound);
        };
        let account_id = AccountId::parse(&raw_account).map_err(|_| Error::Unsafe)?;
        tx.execute(
            "DELETE FROM account_links WHERE account_id=?1 AND org_id=?2 AND member_id=?3",
            (account_id.as_str(), org_id, member_id),
        )?;
        let ended = delete_account_sessions(&tx, account_id.as_str(), None)?;
        clock::advance_clock(&tx, now)?;
        tx.commit()?;
        Ok(RemoveAccessOutcome::Removed {
            account_id: Some(account_id),
            ended,
        })
    }

    /// Hands organization ownership from `from_account` to another person (`to_member_id`) who
    /// already has an active account linked to `org_id`.
    pub(crate) fn handover_org_owner(
        &self,
        org_id: &str,
        from_account: &AccountId,
        to_member_id: &str,
    ) -> Result<OrgOwnerHandoverOutcome, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_owner: Option<(String, String)> = tx
            .query_row(
                "SELECT account, owner_member_id FROM org_owners WHERE org_id=?1",
                [org_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((current_account, previous_member_id)) = current_owner else {
            return Ok(OrgOwnerHandoverOutcome::NotOwner);
        };
        if current_account != from_account.as_str() {
            return Ok(OrgOwnerHandoverOutcome::NotOwner);
        }
        let target: Option<String> = tx
            .query_row(
                "SELECT a.account_id FROM account_links l JOIN accounts a ON a.account_id=l.account_id WHERE l.org_id=?1 AND l.member_id=?2 AND a.revoked_at_ms IS NULL AND a.password_phc <> ?3",
                (org_id, to_member_id, local_auth::LOCAL_AUTHLESS_VERIFIER),
                |r| r.get(0),
            )
            .optional()?;
        let Some(raw_target) = target else {
            return Ok(OrgOwnerHandoverOutcome::InvalidTarget);
        };
        let to_account_id = AccountId::parse(&raw_target).map_err(|_| Error::Unsafe)?;
        if to_account_id == *from_account {
            return Ok(OrgOwnerHandoverOutcome::InvalidTarget);
        }
        tx.execute(
            "UPDATE org_owners SET account=?1, owner_member_id=?2 WHERE org_id=?3",
            (to_account_id.as_str(), to_member_id, org_id),
        )?;
        tx.commit()?;
        Ok(OrgOwnerHandoverOutcome::Transferred {
            previous_member_id,
            to_account_id,
            to_member_id: to_member_id.to_owned(),
        })
    }

    /// Restores the previous organization owner row if updating office writer custody failed
    /// after [`Self::handover_org_owner`].
    pub(crate) fn restore_org_owner(
        &self,
        org_id: &str,
        account: &AccountId,
        member_id: &str,
    ) -> Result<(), Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE org_owners SET account=?1, owner_member_id=?2 WHERE org_id=?3",
            (account.as_str(), member_id, org_id),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically transfers the single instance administrator role from `from_account` to
    /// `to_account` (which must be an active account sharing at least one organization).
    pub(crate) fn handover_administrator(
        &self,
        from_account: &AccountId,
        to_account: &AccountId,
    ) -> Result<AdminHandoverOutcome, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !administrator::exists(&tx)? {
            return Ok(AdminHandoverOutcome::NotAdmin);
        }
        let is_admin: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND admin=1 AND revoked_at_ms IS NULL AND password_phc <> ?2)",
            (from_account.as_str(), local_auth::LOCAL_AUTHLESS_VERIFIER),
            |r| r.get(0),
        )?;
        if !is_admin {
            return Ok(AdminHandoverOutcome::NotAdmin);
        }
        if from_account == to_account {
            return Ok(AdminHandoverOutcome::InvalidTarget);
        }
        let target_valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND revoked_at_ms IS NULL AND password_phc <> ?2)",
            (to_account.as_str(), local_auth::LOCAL_AUTHLESS_VERIFIER),
            |r| r.get(0),
        )?;
        if !target_valid {
            return Ok(AdminHandoverOutcome::InvalidTarget);
        }
        let shared_org: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM account_links a JOIN account_links b ON a.org_id=b.org_id WHERE a.account_id=?1 AND b.account_id=?2)",
            (from_account.as_str(), to_account.as_str()),
            |r| r.get(0),
        )?;
        if !shared_org {
            return Ok(AdminHandoverOutcome::InvalidTarget);
        }
        tx.execute(
            "UPDATE accounts SET admin=0 WHERE account_id=?1",
            [from_account.as_str()],
        )?;
        tx.execute(
            "UPDATE accounts SET admin=1 WHERE account_id=?1",
            [to_account.as_str()],
        )?;
        tx.commit()?;
        Ok(AdminHandoverOutcome::Transferred)
    }

    /// Reads the raw display login of an active account by ID (for link inspection preview).
    pub(crate) fn active_account_login(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<String>, Error> {
        let db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        Ok(db
            .query_row(
                "SELECT login FROM accounts WHERE account_id=?1 AND revoked_at_ms IS NULL AND password_phc <> ?2",
                (account_id.as_str(), local_auth::LOCAL_AUTHLESS_VERIFIER),
                |r| r.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests;
