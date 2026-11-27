//! Organization invites: issuing one and redeeming it by registering or attaching an account.

use rusqlite::TransactionBehavior;

use crate::{clock, links, local_auth, HostDb};
use org_host::ports::accounts::{AccountId, NewAccount, StoreError};
use org_host::ports::links::{
    self as port_links, Issued, LinkDigest, LinkKind, Refused, StoreFault, Token,
};

use super::{
    invite_subject, parse_invite_subject, unavailable, InviteAttach, InviteIssueError,
    InviteRegistration,
};

impl HostDb {
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
        let issued = port_links::issue_with(
            &self.instance_id,
            LinkKind::Registration,
            &subject,
            now_ms,
            |link| {
                let mut db = self
                    .connection
                    .lock()
                    .map_err(|_| port_links::Error::Store(StoreFault::Poisoned))?;
                let tx = db
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|_| port_links::Error::Conflict)?;
                if local_auth::local_authless_id(&tx)
                    .map_err(|_| port_links::Error::Conflict)?
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
                    .map_err(|_| port_links::Error::Conflict)?;
                if linked {
                    refusal = Some(InviteIssueError::AlreadyLinked);
                    return Ok(false);
                }
                links::replace(&tx, &link)?;
                tx.commit().map_err(|_| port_links::Error::Conflict)?;
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
}
