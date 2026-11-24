//! Multi-person account SQLite operations (`TASK-38` / A6):
//! organization invitations, existing-account invitation attachment (SIGN-35),
//! set-password links (`admin reset` and People › New sign-in link), own password change,
//! access removal, organization owner handover, and instance administrator handover.

use std::collections::{BTreeMap, BTreeSet};

use org_host_control::browser_accounts::OrgAccessEntry;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use super::super::{administrator, clock, links, local_auth, Error, HostDb};
use crate::accounts::identity::AccountId;
use crate::accounts::store::{NewAccount, StoreError};
use crate::hostdb::journal::Verifier;
use crate::links::{Issued, LinkDigest, LinkKind, Refused, Subject, Token};
use crate::sessions::Ended;

const INVITE_PREFIX: &str = "invite:";
const ACCOUNT_PREFIX: &str = "account:";

pub(crate) fn invite_subject(
    org_id: &str,
    member_id: &str,
) -> Result<Subject, crate::links::Error> {
    Subject::new(&format!("{INVITE_PREFIX}{org_id}:{member_id}"))
}

pub(crate) fn account_subject(account_id: &AccountId) -> Result<Subject, crate::links::Error> {
    Subject::new(&format!("{ACCOUNT_PREFIX}{}", account_id.as_str()))
}

/// Parses `"invite:<org_id>:<member_id>"` into `(org_id, member_id)`.
pub(crate) fn parse_invite_subject(subject: &str) -> Option<(&str, &str)> {
    let rest = subject.strip_prefix(INVITE_PREFIX)?;
    let (org_id, member_id) = rest.split_once(':')?;
    if org_office_launch::is_storage_id(org_id) && org_proto::is_valid_member_id(member_id) {
        Some((org_id, member_id))
    } else {
        None
    }
}

/// Parses `"account:<account_id>"` into `AccountId`.
pub(crate) fn parse_account_subject(subject: &str) -> Option<AccountId> {
    let raw = subject.strip_prefix(ACCOUNT_PREFIX)?;
    AccountId::parse(raw).ok()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InviteIssueError {
    AlreadyLinked,
    AuthDisabled,
    Invalid,
    Store,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InviteRegistration {
    Registered {
        account_id: AccountId,
        org_id: String,
        member_id: String,
    },
    Link(Refused),
    NotAnInvite,
    AlreadyLinked,
    LoginTaken,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InviteAttach {
    Attached { org_id: String, member_id: String },
    Link(Refused),
    NotAnInvite,
    AlreadyLinked,
    AlreadyMember,
    AccountUnavailable,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MemberResetIssueError {
    NotLinked,
    Store,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SetPasswordOutcome {
    Updated {
        account_id: AccountId,
        login: String,
        ended: Vec<Ended>,
    },
    Link(Refused),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RemoveAccessOutcome {
    Removed {
        account_id: Option<AccountId>,
        ended: Vec<Ended>,
    },
    IsOwner,
    NotFound,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OrgOwnerHandoverOutcome {
    Transferred {
        previous_member_id: String,
        to_account_id: AccountId,
        to_member_id: String,
    },
    NotOwner,
    InvalidTarget,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AdminHandoverOutcome {
    Transferred,
    NotAdmin,
    InvalidTarget,
}

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

    /// Issues a one-time invitation link for `(org_id, member_id)`, cancelling any prior pending
    /// invite for that member. Refuses if the member is already linked to an active account.
    pub(crate) fn issue_org_invite(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, InviteIssueError> {
        if !org_office_launch::is_storage_id(org_id) || !org_proto::is_valid_member_id(member_id) {
            return Err(InviteIssueError::Invalid);
        }
        let subject = invite_subject(org_id, member_id).map_err(|_| InviteIssueError::Invalid)?;
        let mut refusal: Option<InviteIssueError> = None;
        let issued = crate::links::issue_with(
            &self.instance_id,
            LinkKind::Registration,
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
                if local_auth::local_authless_id(&tx)
                    .map_err(|_| crate::links::Error::Conflict)?
                    .is_some()
                {
                    refusal = Some(InviteIssueError::AuthDisabled);
                    return Ok(false);
                }
                let linked: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM account_links l JOIN accounts a ON a.account_id=l.account_id WHERE l.org_id=?1 AND l.member_id=?2 AND a.revoked_at_ms IS NULL AND a.password_phc <> ?3)",
                        (org_id, member_id, local_auth::LOCAL_AUTHLESS_VERIFIER),
                        |r| r.get(0),
                    )
                    .map_err(|_| crate::links::Error::Conflict)?;
                if linked {
                    refusal = Some(InviteIssueError::AlreadyLinked);
                    return Ok(false);
                }
                links::replace(&tx, &link)?;
                tx.commit().map_err(|_| crate::links::Error::Conflict)?;
                Ok(true)
            },
        )
        .map_err(|_| InviteIssueError::Store)?;
        issued.ok_or_else(|| refusal.unwrap_or(InviteIssueError::AlreadyLinked))
    }

    /// Redeems an organization invitation link by creating a new non-admin account and linking it
    /// to `(org_id, member_id)` atomically.
    pub(crate) fn redeem_invite_register(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<InviteRegistration, StoreError> {
        let digest = LinkDigest::of(&self.instance_id, LinkKind::Registration, token);
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let now = clock::now(&tx, now_ms).map_err(unavailable)?;
        if local_auth::local_authless_id(&tx)
            .map_err(unavailable)?
            .is_some()
        {
            return Ok(InviteRegistration::Link(Refused::Unknown));
        }
        let subject = match links::redeem_in_with_subject(&tx, LinkKind::Registration, digest, now)
            .map_err(unavailable)?
        {
            Ok((_, subject)) => subject,
            Err(refused) => return Ok(InviteRegistration::Link(refused)),
        };
        let Some((org_id, member_id)) = parse_invite_subject(&subject) else {
            return Ok(InviteRegistration::NotAnInvite);
        };
        let org_id = org_id.to_owned();
        let member_id = member_id.to_owned();
        let already_linked: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM account_links l JOIN accounts a ON a.account_id=l.account_id WHERE l.org_id=?1 AND l.member_id=?2 AND a.revoked_at_ms IS NULL)",
                (&org_id, &member_id),
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if already_linked {
            return Ok(InviteRegistration::AlreadyLinked);
        }
        let key = new.username.key().as_str();
        let taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE login_key=?1)",
                [key],
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if taken {
            return Ok(InviteRegistration::LoginTaken);
        }
        tx.execute(
            "INSERT INTO accounts VALUES (?1,?2,?3,?4,0,?5,?5,NULL)",
            (
                new.account_id.as_str(),
                key,
                new.username.display(),
                new.verifier.expose(),
                now,
            ),
        )
        .map_err(unavailable)?;
        tx.execute(
            "INSERT INTO account_links VALUES (?1,?2,?3)",
            (new.account_id.as_str(), &org_id, &member_id),
        )
        .map_err(unavailable)?;
        clock::advance_clock(&tx, now).map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(InviteRegistration::Registered {
            account_id: new.account_id.clone(),
            org_id,
            member_id,
        })
    }

    /// Redeems an organization invitation link for an existing account (SIGN-35), linking it to
    /// `(org_id, member_id)` atomically.
    pub(crate) fn redeem_invite_attach(
        &self,
        token: &Token,
        account_id: &AccountId,
        now_ms: i64,
    ) -> Result<InviteAttach, StoreError> {
        let digest = LinkDigest::of(&self.instance_id, LinkKind::Registration, token);
        let mut db = self.connection.lock().map_err(unavailable)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let now = clock::now(&tx, now_ms).map_err(unavailable)?;
        if local_auth::local_authless_id(&tx)
            .map_err(unavailable)?
            .is_some()
        {
            return Ok(InviteAttach::Link(Refused::Unknown));
        }
        let subject = match links::redeem_in_with_subject(&tx, LinkKind::Registration, digest, now)
            .map_err(unavailable)?
        {
            Ok((_, subject)) => subject,
            Err(refused) => return Ok(InviteAttach::Link(refused)),
        };
        let Some((org_id, member_id)) = parse_invite_subject(&subject) else {
            return Ok(InviteAttach::NotAnInvite);
        };
        let org_id = org_id.to_owned();
        let member_id = member_id.to_owned();
        let account_active: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND revoked_at_ms IS NULL AND password_phc <> ?2)",
                (account_id.as_str(), local_auth::LOCAL_AUTHLESS_VERIFIER),
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if !account_active {
            return Ok(InviteAttach::AccountUnavailable);
        }
        let already_member: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM account_links WHERE account_id=?1 AND org_id=?2)",
                (account_id.as_str(), &org_id),
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if already_member {
            return Ok(InviteAttach::AlreadyMember);
        }
        let member_taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM account_links l JOIN accounts a ON a.account_id=l.account_id WHERE l.org_id=?1 AND l.member_id=?2 AND a.revoked_at_ms IS NULL)",
                (&org_id, &member_id),
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if member_taken {
            return Ok(InviteAttach::AlreadyLinked);
        }
        tx.execute(
            "INSERT INTO account_links VALUES (?1,?2,?3)",
            (account_id.as_str(), &org_id, &member_id),
        )
        .map_err(unavailable)?;
        clock::advance_clock(&tx, now).map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(InviteAttach::Attached { org_id, member_id })
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
mod tests {
    use super::*;
    use crate::accounts::rules::Username;
    use crate::hostdb::lockout::{Outcome, Socket};
    use crate::hostdb::sqlite::tests::fixture;
    use crate::sessions::{BrowserSessions, NewSession, Origin, Scheme, SessionDigest};

    const PHC_A: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PHC_B: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn verifier(phc: &str) -> Verifier {
        Verifier::new(phc.to_owned()).unwrap()
    }

    #[test]
    fn invite_register_attach_and_password_reset_flows() {
        let (_home, _secrets, db) = fixture();
        let owner_id =
            AccountId::parse(crate::hostdb::sqlite::tests::admin_fixture::ACCOUNT).unwrap();
        let org1 = crate::ids::uuid_v7().unwrap();
        let org2 = crate::ids::uuid_v7().unwrap();
        let owner_member = crate::ids::uuid_v7().unwrap();
        let sam_member = crate::ids::uuid_v7().unwrap();
        let sam_member_org2 = crate::ids::uuid_v7().unwrap();

        db.seed_owner_for_test(owner_id.as_str(), &org1, &owner_member)
            .unwrap();
        db.seed_owner_for_test(owner_id.as_str(), &org2, &owner_member)
            .unwrap();

        // 1. Issue invite, re-issue cancels previous, redeem creates UUIDv7 non-admin account.
        let inv1 = db.issue_org_invite(&org1, &sam_member, 10_000).unwrap();
        let inv2 = db.issue_org_invite(&org1, &sam_member, 10_100).unwrap();
        let access = db.list_org_access(&org1, 10_200).unwrap();
        assert!(access
            .iter()
            .any(|e| e.member_id == sam_member && e.pending_invite && e.account_id.is_none()));

        let new_sam = NewAccount::new(Username::new("Sam").unwrap(), verifier(PHC_A)).unwrap();
        assert!(org_proto::is_uuid_v7(new_sam.account_id.as_str()));
        assert_eq!(
            db.redeem_invite_register(&inv1.token, &new_sam, 10_300)
                .unwrap(),
            InviteRegistration::Link(Refused::Cancelled)
        );
        let reg = db
            .redeem_invite_register(&inv2.token, &new_sam, 10_400)
            .unwrap();
        assert_eq!(
            reg,
            InviteRegistration::Registered {
                account_id: new_sam.account_id.clone(),
                org_id: org1.clone(),
                member_id: sam_member.clone(),
            }
        );
        let sam_id = new_sam.account_id.clone();

        // Already-linked member cannot be invited again.
        assert_eq!(
            db.issue_org_invite(&org1, &sam_member, 10_500).unwrap_err(),
            InviteIssueError::AlreadyLinked
        );

        // 2. SIGN-35: attach Sam's existing account to org2 via invite link.
        let inv_org2 = db
            .issue_org_invite(&org2, &sam_member_org2, 10_600)
            .unwrap();
        assert_eq!(
            db.redeem_invite_attach(&inv_org2.token, &sam_id, 10_700)
                .unwrap(),
            InviteAttach::Attached {
                org_id: org2.clone(),
                member_id: sam_member_org2.clone(),
            }
        );

        // 3. Own password change ends OTHER sessions while keeping the current session.
        let origin = Origin::new(Scheme::Http, "127.0.0.1", 7420).unwrap();
        let s1 = NewSession::new(
            sam_id.as_str(),
            SessionDigest::cookie(b"sam-c1"),
            SessionDigest::proof(b"sam-p1"),
            origin.clone(),
        )
        .unwrap();
        let s2 = NewSession::new(
            sam_id.as_str(),
            SessionDigest::cookie(b"sam-c2"),
            SessionDigest::proof(b"sam-p2"),
            origin,
        )
        .unwrap();
        db.create(&s1, 11_000).unwrap();
        db.create(&s2, 11_000).unwrap();
        let ended = db
            .change_own_password(&sam_id, &s1.id, &verifier(PHC_B), 11_500)
            .unwrap();
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].id, s2.id);

        // 4. Member set-password link clears lockout and ends ALL sessions.
        let sam_key = crate::accounts::identity::LoginKey::new("Sam").unwrap();
        for n in 0..8 {
            db.record_login(
                Socket::Control,
                sam_key.as_str(),
                12_000 + n,
                Outcome::Failure,
            )
            .unwrap();
        }
        assert!(db
            .login_lockout(Socket::Control, sam_key.as_str(), 12_010)
            .unwrap()
            .is_locked());
        let reset = db
            .issue_member_password_reset_link(&org1, &sam_member, 12_100)
            .unwrap();
        let outcome = db
            .redeem_set_password(&reset.token, &verifier(PHC_A), 12_200)
            .unwrap();
        match outcome {
            SetPasswordOutcome::Updated {
                account_id,
                login,
                ended,
            } => {
                assert_eq!(account_id, sam_id);
                assert_eq!(login, "Sam");
                assert_eq!(ended.len(), 1);
                assert_eq!(ended[0].id, s1.id);
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
        assert!(!db
            .login_lockout(Socket::Control, sam_key.as_str(), 12_201)
            .unwrap()
            .is_locked());
    }

    #[test]
    fn owner_and_admin_handover_and_access_removal() {
        let (_home, _secrets, db) = fixture();
        let owner_id =
            AccountId::parse(crate::hostdb::sqlite::tests::admin_fixture::ACCOUNT).unwrap();
        let org1 = crate::ids::uuid_v7().unwrap();
        let owner_member = crate::ids::uuid_v7().unwrap();
        let sam_member = crate::ids::uuid_v7().unwrap();
        db.seed_owner_for_test(owner_id.as_str(), &org1, &owner_member)
            .unwrap();

        let inv = db.issue_org_invite(&org1, &sam_member, 10_000).unwrap();
        let new_sam = NewAccount::new(Username::new("Sam").unwrap(), verifier(PHC_A)).unwrap();
        let sam_id = match db
            .redeem_invite_register(&inv.token, &new_sam, 10_100)
            .unwrap()
        {
            InviteRegistration::Registered { account_id, .. } => account_id,
            other => panic!("unexpected registration: {other:?}"),
        };

        // Removing owner access is refused with IsOwner; handover + restore + handover unlocks it.
        assert_eq!(
            db.remove_org_access(&org1, &owner_member, 13_000).unwrap(),
            RemoveAccessOutcome::IsOwner
        );
        assert_eq!(
            db.handover_org_owner(&org1, &owner_id, &sam_member)
                .unwrap(),
            OrgOwnerHandoverOutcome::Transferred {
                previous_member_id: owner_member.clone(),
                to_account_id: sam_id.clone(),
                to_member_id: sam_member.clone(),
            }
        );
        assert_eq!(
            db.org_owner(&org1).unwrap(),
            Some((sam_id.clone(), sam_member.clone()))
        );
        db.restore_org_owner(&org1, &owner_id, &owner_member)
            .unwrap();
        assert_eq!(
            db.org_owner(&org1).unwrap(),
            Some((owner_id.clone(), owner_member.clone()))
        );
        assert_eq!(
            db.handover_org_owner(&org1, &owner_id, &sam_member)
                .unwrap(),
            OrgOwnerHandoverOutcome::Transferred {
                previous_member_id: owner_member.clone(),
                to_account_id: sam_id.clone(),
                to_member_id: sam_member.clone(),
            }
        );

        // Administrator handover swaps admin flag atomically.
        assert_eq!(
            db.handover_administrator(&owner_id, &sam_id).unwrap(),
            AdminHandoverOutcome::Transferred
        );
        assert!(!db.account_authority(&owner_id).unwrap().unwrap().admin());
        assert!(db.account_authority(&sam_id).unwrap().unwrap().admin());
        assert!(db.administrator_exists().unwrap());

        // Removing old owner's access unlinks org1 and ends sessions, keeping account unrevoked (SIGN-9).
        let origin = Origin::new(Scheme::Http, "127.0.0.1", 7420).unwrap();
        let o_sess = NewSession::new(
            owner_id.as_str(),
            SessionDigest::cookie(b"own-c1"),
            SessionDigest::proof(b"own-p1"),
            origin,
        )
        .unwrap();
        db.create(&o_sess, 13_100).unwrap();
        let removed = db.remove_org_access(&org1, &owner_member, 13_200).unwrap();
        match removed {
            RemoveAccessOutcome::Removed { account_id, ended } => {
                assert_eq!(account_id, Some(owner_id.clone()));
                assert_eq!(ended.len(), 1);
                assert_eq!(ended[0].id, o_sess.id);
            }
            other => panic!("unexpected remove outcome: {other:?}"),
        }
        let owner_after = db.account_authority(&owner_id).unwrap().unwrap();
        assert!(owner_after.revoked_at_ms().is_none());
        assert!(!owner_after.links().iter().any(|l| l.org_id() == org1));
    }
}
