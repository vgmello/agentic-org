//! Password reset links for administrators and members, setting a password from one, and
//! changing one's own password.

use rusqlite::{OptionalExtension, TransactionBehavior};

use crate::accounts::identity::AccountId;
use crate::accounts::store::StoreError;
use crate::hostdb::journal::Verifier;
use crate::hostdb::sqlite::{clock, links, local_auth, Error, HostDb};
use crate::links::{Issued, LinkDigest, LinkKind, Refused, Token};
use crate::sessions::Ended;

use super::{
    account_subject, active_admin_in, delete_account_sessions, parse_account_subject, unavailable,
    MemberResetIssueError, SetPasswordOutcome,
};

impl HostDb {
    /// Atomically issues a one-time `set_password` link for the active administrator
    /// (`admin reset` when an administrator already exists, TASK-38). Returns `Ok(None)` if no
    /// password-authenticated administrator currently exists.
    pub(crate) fn issue_admin_password_reset_link(
        &self,
        now_ms: i64,
    ) -> Result<Option<(String, Issued)>, crate::links::Error> {
        for _ in 0..3 {
            let Some((account_id, login)) = self
                .active_administrator_account()
                .map_err(|_| crate::links::Error::Conflict)?
            else {
                return Ok(None);
            };
            let subject = account_subject(&account_id)?;
            let issued = crate::links::issue_with(
                &self.instance_id,
                LinkKind::SetPassword,
                &subject,
                now_ms,
                |link| {
                    let mut db = self.connection.lock().map_err(|_| {
                        crate::links::Error::Store(crate::links::StoreFault::Poisoned)
                    })?;
                    let tx = db
                        .transaction_with_behavior(TransactionBehavior::Immediate)
                        .map_err(|_| crate::links::Error::Conflict)?;
                    let now_admin =
                        active_admin_in(&tx).map_err(|_| crate::links::Error::Conflict)?;
                    if now_admin.as_ref().map(|(id, _)| id) != Some(&account_id) {
                        return Ok(false);
                    }
                    links::replace(&tx, &link)?;
                    tx.commit().map_err(|_| crate::links::Error::Conflict)?;
                    Ok(true)
                },
            )?;
            if let Some(issued) = issued {
                return Ok(Some((login, issued)));
            }
        }
        Err(crate::links::Error::Conflict)
    }

    /// Issues a one-time `set_password` link for the active account linked to `(org_id, member_id)`.
    pub(crate) fn issue_member_password_reset_link(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, MemberResetIssueError> {
        let account_id = {
            let db = self
                .connection
                .lock()
                .map_err(|_| MemberResetIssueError::Store)?;
            let raw: Option<String> = db
                .query_row(
                    "SELECT a.account_id FROM account_links l JOIN accounts a ON a.account_id=l.account_id WHERE l.org_id=?1 AND l.member_id=?2 AND a.revoked_at_ms IS NULL AND a.password_phc <> ?3",
                    (org_id, member_id, local_auth::LOCAL_AUTHLESS_VERIFIER),
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| MemberResetIssueError::Store)?;
            let Some(raw) = raw else {
                return Err(MemberResetIssueError::NotLinked);
            };
            AccountId::parse(&raw).map_err(|_| MemberResetIssueError::Store)?
        };
        let subject = account_subject(&account_id).map_err(|_| MemberResetIssueError::Store)?;
        let issued = crate::links::issue_with(
            &self.instance_id,
            LinkKind::SetPassword,
            &subject,
            now_ms,
            |link| {
                let mut db = self
                    .connection
                    .lock()
                    .map_err(|_| crate::links::Error::Store(crate::links::StoreFault::Poisoned))?;
                let tx = db
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|_| crate::links::Error::Conflict)?;
                let still_linked: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM account_links l JOIN accounts a ON a.account_id=l.account_id WHERE l.org_id=?1 AND l.member_id=?2 AND l.account_id=?3 AND a.revoked_at_ms IS NULL AND a.password_phc <> ?4)",
                        (org_id, member_id, account_id.as_str(), local_auth::LOCAL_AUTHLESS_VERIFIER),
                        |r| r.get(0),
                    )
                    .map_err(|_| crate::links::Error::Conflict)?;
                if !still_linked {
                    return Ok(false);
                }
                links::replace(&tx, &link)?;
                tx.commit().map_err(|_| crate::links::Error::Conflict)?;
                Ok(true)
            },
        )
        .map_err(|_| MemberResetIssueError::Store)?;
        issued.ok_or(MemberResetIssueError::NotLinked)
    }

    /// Redeems a one-time `set_password` link, updates the account's password hash, clears its
    /// sign-in lockout (SIGN-32), and ends every browser session of that account atomically.
    pub(crate) fn redeem_set_password(
        &self,
        token: &Token,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<SetPasswordOutcome, StoreError> {
        let digest = LinkDigest::of(&self.instance_id, LinkKind::SetPassword, token);
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let now = clock::now(&tx, now_ms).map_err(unavailable)?;
        let subject = match links::redeem_in_with_subject(&tx, LinkKind::SetPassword, digest, now)
            .map_err(unavailable)?
        {
            Ok((_, subject)) => subject,
            Err(refused) => return Ok(SetPasswordOutcome::Link(refused)),
        };
        let Some(account_id) = parse_account_subject(&subject) else {
            return Ok(SetPasswordOutcome::Link(Refused::Unknown));
        };
        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT login, login_key FROM accounts WHERE account_id=?1 AND revoked_at_ms IS NULL AND password_phc <> ?2",
                (account_id.as_str(), local_auth::LOCAL_AUTHLESS_VERIFIER),
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(unavailable)?;
        let Some((login, login_key)) = row else {
            return Ok(SetPasswordOutcome::Link(Refused::Unknown));
        };
        let updated = tx
            .execute(
                "UPDATE accounts SET password_phc=?1, password_changed_at_ms=?2 WHERE account_id=?3 AND revoked_at_ms IS NULL",
                (verifier.expose(), now, account_id.as_str()),
            )
            .map_err(unavailable)?;
        if updated != 1 {
            return Err(StoreError::Unavailable);
        }
        tx.execute(
            "DELETE FROM op_verifiers WHERE created_account=?1",
            [account_id.as_str()],
        )
        .map_err(unavailable)?;
        tx.execute(
            "DELETE FROM login_lockout WHERE login_key=?1",
            [login_key.as_str()],
        )
        .map_err(unavailable)?;
        let ended = delete_account_sessions(&tx, account_id.as_str(), None).map_err(unavailable)?;
        clock::advance_clock(&tx, now).map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(SetPasswordOutcome::Updated {
            account_id,
            login,
            ended,
        })
    }

    /// Changes an account's own password and ends every OTHER browser session of that account,
    /// keeping `keep_session_id` live.
    pub(crate) fn change_own_password(
        &self,
        account_id: &AccountId,
        keep_session_id: &str,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<Vec<Ended>, Error> {
        let mut db = self.connection.lock().map_err(|_| Error::Unsafe)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock::now(&tx, now_ms)?;
        let updated = tx.execute(
            "UPDATE accounts SET password_phc=?1, password_changed_at_ms=?2 WHERE account_id=?3 AND revoked_at_ms IS NULL AND password_phc <> ?4",
            (
                verifier.expose(),
                now,
                account_id.as_str(),
                local_auth::LOCAL_AUTHLESS_VERIFIER,
            ),
        )?;
        if updated != 1 {
            return Err(Error::Conflict);
        }
        tx.execute(
            "DELETE FROM op_verifiers WHERE created_account=?1",
            [account_id.as_str()],
        )?;
        let ended = delete_account_sessions(&tx, account_id.as_str(), Some(keep_session_id))?;
        clock::advance_clock(&tx, now)?;
        tx.commit()?;
        Ok(ended)
    }
}
