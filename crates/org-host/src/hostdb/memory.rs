//! In-memory implementation of [`HostStore`] (ADR-0018 ENG-2 / ENG-3).
//!
//! Proves that the host registry contract does not depend on SQLite or any SQL engine.
//! Every invariant enforced by the SQLite adapter (`hostdb::sqlite`) is replicated here
//! over plain Rust collections protected by a single mutex.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use org_host_control::ops::TlsRefusal;
use org_office_launch::recovery::PublicRecord;
use org_proto::OrganizationName;

use super::accounts::{Account, Credentials, Link};
use super::journal::{id_time, Attempt, Verifier, DAY};
use super::lockout::{Outcome, Socket, State as LockoutState};
use super::provisioning::{Reservation, Taken};
use super::recovery::{Ledger, State as RecoveryState};
use super::web_tls::{StoredPair, TlsLookup, TlsStored};
use super::{Error, HostStore, StoreFault};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::store::{
    backoff_ms, AccountStore, Attempt as AccountAttempt, Credentials as AccountCredentials,
    NewAccount, Registration, StoreError as AccountStoreError, Throttle, ThrottleKey,
    LOGIN_DELAY_CAP_MS,
};
use crate::host_secrets::{HostSecrets, Name};
use crate::links::{
    issue_with, redeem_rule, Error as LinkError, Issued, LinkDigest, LinkKind, NewLink,
    OneTimeLinks, Pending as PendingLink, Refused, StoreFault as LinkStoreFault, Subject,
};
use crate::sessions::{
    is_live, Admission, BrowserSessions, Ended, NewSession, Origin, SessionDigest, SessionRecord,
    StoreError as SessionStoreError, SESSION_LIMIT,
};

struct AccountRow {
    account_id: AccountId,
    login_key: String,
    login: String,
    verifier: Verifier,
    admin: bool,
    password_changed_at_ms: i64,
    revoked_at_ms: Option<i64>,
}

struct LinkRow {
    link_id: String,
    kind: LinkKind,
    subject: String,
    digest: LinkDigest,
    issued_at_ms: i64,
    expires_at_ms: i64,
    state: &'static str,
    ended_at_ms: Option<i64>,
}

struct SessionRow {
    id: String,
    account_id: String,
    created_ms: i64,
    last_activity_ms: i64,
    cookie: SessionDigest,
    proof: SessionDigest,
    origin: Origin,
}

struct LockoutRow {
    failures: i64,
    until_ms: i64,
    last_failure_ms: i64,
}

struct AuditRow {
    ordinal: i64,
    action: &'static str,
    account: String,
    at_ms: i64,
}

struct JournalRow {
    kind: &'static str,
    fingerprint: [u8; 32],
    account: String,
    state: &'static str,
    result: String,
    expires_at_ms: i64,
}

struct Inner {
    clock_high_ms: i64,
    accounts: BTreeMap<String, AccountRow>,
    account_links: BTreeMap<(String, String), String>,
    org_owners: BTreeMap<String, (String, String)>,
    links: Vec<LinkRow>,
    sessions: BTreeMap<String, SessionRow>,
    lockouts: BTreeMap<(Socket, String), LockoutRow>,
    provisioning: BTreeMap<String, Reservation>,
    recovery: BTreeMap<String, Ledger>,
    recovery_audit: BTreeMap<String, Vec<AuditRow>>,
    web_tls_pair: Option<StoredPair>,
    journal: BTreeMap<String, JournalRow>,
}

impl Inner {
    fn now(&self, wall_ms: i64) -> Result<i64, Error> {
        if wall_ms < 0 || self.clock_high_ms < 0 {
            return Err(Error::Unsafe);
        }
        Ok(wall_ms.max(self.clock_high_ms))
    }

    fn advance_clock(&mut self, now: i64) {
        if now > self.clock_high_ms {
            self.clock_high_ms = now;
        }
    }

    fn is_live_admin(&self, account_id: &str) -> bool {
        self.accounts
            .get(account_id)
            .is_some_and(|a| a.admin && a.revoked_at_ms.is_none())
    }

    fn any_live_admin(&self) -> bool {
        self.accounts
            .values()
            .any(|a| a.admin && a.revoked_at_ms.is_none())
    }

    fn account_snapshot(&self, account_id: &str) -> Option<Account> {
        let row = self.accounts.get(account_id)?;
        let links = self
            .account_links
            .iter()
            .filter(|((acc, _), _)| acc == account_id)
            .map(|((_, org_id), member_id)| Link {
                org_id: org_id.clone(),
                member_id: member_id.clone(),
            })
            .collect();
        Some(Account {
            account_id: row.account_id.clone(),
            login: row.login.clone(),
            admin: row.admin,
            revoked_at_ms: row.revoked_at_ms,
            password_changed_at_ms: row.password_changed_at_ms,
            links,
            time_zone: None,
            last_org_id: None,
        })
    }

    fn replace_link(&mut self, link: &NewLink<'_>) -> Result<(), LinkError> {
        if self.links.iter().any(|r| r.link_id == link.link_id)
            || self.links.iter().any(|r| r.digest == link.digest)
        {
            return Err(LinkError::Store(LinkStoreFault::Constraint));
        }
        for row in &mut self.links {
            if row.kind == link.kind
                && row.subject == link.subject.as_str()
                && row.state == "pending"
            {
                row.state = "cancelled";
                row.ended_at_ms = Some(link.issued_at_ms);
            }
        }
        self.links.push(LinkRow {
            link_id: link.link_id.to_owned(),
            kind: link.kind,
            subject: link.subject.as_str().to_owned(),
            digest: link.digest,
            issued_at_ms: link.issued_at_ms,
            expires_at_ms: link.expires_at_ms,
            state: "pending",
            ended_at_ms: None,
        });
        Ok(())
    }

    fn redeem_link(
        &mut self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<String, Refused> {
        let Some(row) = self
            .links
            .iter_mut()
            .find(|r| r.kind == kind && r.digest == digest)
        else {
            return Err(Refused::Unknown);
        };
        redeem_rule(row.state, row.expires_at_ms, now_ms)?;
        row.state = "used";
        row.ended_at_ms = Some(now_ms);
        Ok(row.link_id.clone())
    }

    fn has_org_reservation(&self, org_id: &str) -> bool {
        self.provisioning
            .values()
            .any(|r| r.prepared.organization_id == org_id)
    }

    fn taken(&self, name: &str, slug: &str) -> Result<Option<Taken>, Error> {
        let key = OrganizationName::parse(name).map_err(|_| Error::Unsafe)?;
        for r in self.provisioning.values() {
            let existing_key =
                OrganizationName::parse(&r.prepared.plan.name).map_err(|_| Error::Unsafe)?;
            if existing_key.key() == key.key() {
                return Ok(Some(Taken::Name));
            }
        }
        for r in self.provisioning.values() {
            if r.prepared.plan.slug == slug {
                return Ok(Some(Taken::Slug));
            }
        }
        Ok(None)
    }

    fn verify_recovery_source(
        &self,
        instance_id: &str,
        binding: &org_office_launch::recovery::Request,
    ) -> Result<Reservation, Error> {
        binding.check().map_err(|_| Error::Unsafe)?;
        let r = self
            .provisioning
            .get(&binding.operation_id)
            .ok_or(Error::Unsafe)?;
        if instance_id != binding.instance_id
            || !r.linked
            || r.key_fingerprint.is_none()
            || r.prepared.office_id != binding.office_id
            || r.prepared.organization_id != binding.organization_id
            || r.recovery_handle != format!("recovery.{}", binding.operation_id)
        {
            return Err(Error::Unsafe);
        }
        Ok(r.clone())
    }

    fn stored_journal_row(
        &self,
        op: &Attempt<'_>,
        now: i64,
    ) -> Result<Option<(&'static str, String)>, Error> {
        if let Some(row) = self.journal.get(op.id) {
            if row.expires_at_ms <= now {
                return Err(Error::Expired);
            }
            if row.kind != op.kind || row.fingerprint != op.fingerprint || row.account != op.account
            {
                return Err(Error::OperationReused);
            }
            if row.state == "reserved" {
                return Err(Error::Conflict);
            }
            return Ok(Some((row.state, row.result.clone())));
        }
        let issued = id_time(op.id)?;
        if issued < now.saturating_sub(29 * DAY) {
            return Err(Error::Expired);
        }
        if issued > now.saturating_add(300_000) {
            return Err(Error::ClockSkew);
        }
        Ok(None)
    }

    fn tls_lookup_inner(&self, op: &Attempt<'_>, now: i64) -> Result<TlsLookup, Error> {
        let Some((state, result)) = self.stored_journal_row(op, now)? else {
            return Ok(TlsLookup::Vacant);
        };
        let stored: TlsStored = serde_json::from_str(&result).map_err(|_| Error::Unsafe)?;
        match (state, &stored) {
            ("completed", TlsStored::Completed { .. })
            | (
                "rejected",
                TlsStored::Rejected {
                    refusal: TlsRefusal::TlsKeyMismatch,
                },
            ) => Ok(TlsLookup::Replay(stored)),
            _ => Err(Error::Unsafe),
        }
    }

    fn maintain_journal(&mut self, now: i64) {
        self.advance_clock(now);
        let mut expired: Vec<(i64, String)> = self
            .journal
            .iter()
            .filter(|(_, r)| r.expires_at_ms <= now && r.state != "reserved")
            .map(|(id, r)| (r.expires_at_ms, id.clone()))
            .collect();
        expired.sort();
        for (_, id) in expired.into_iter().take(256) {
            self.journal.remove(&id);
        }
    }
}

/// Engine-independent in-memory host registry.
pub(crate) struct MemoryHostStore {
    instance_id: String,
    inner: Mutex<Inner>,
}

impl MemoryHostStore {
    pub(crate) fn new(instance_id: &str, clock_high_ms: i64) -> Self {
        Self {
            instance_id: instance_id.to_owned(),
            inner: Mutex::new(Inner {
                clock_high_ms,
                accounts: BTreeMap::new(),
                account_links: BTreeMap::new(),
                org_owners: BTreeMap::new(),
                links: Vec::new(),
                sessions: BTreeMap::new(),
                lockouts: BTreeMap::new(),
                provisioning: BTreeMap::new(),
                recovery: BTreeMap::new(),
                recovery_audit: BTreeMap::new(),
                web_tls_pair: None,
                journal: BTreeMap::new(),
            }),
        }
    }

    pub(crate) fn seed_account(
        &self,
        account_id: &str,
        login: &str,
        verifier: &Verifier,
        admin: bool,
        at_ms: i64,
    ) -> Result<(), Error> {
        let id = AccountId::parse(account_id).map_err(|_| Error::InvalidId)?;
        let key = LoginKey::new(login).map_err(|_| Error::Unsafe)?;
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        if s.accounts.contains_key(id.as_str())
            || s.accounts.values().any(|a| a.login_key == key.as_str())
        {
            return Err(Error::Store(StoreFault::Constraint));
        }
        s.accounts.insert(
            id.as_str().to_owned(),
            AccountRow {
                account_id: id,
                login_key: key.as_str().to_owned(),
                login: login.to_owned(),
                verifier: Verifier::new(verifier.expose().to_owned())?,
                admin,
                password_changed_at_ms: at_ms,
                revoked_at_ms: None,
            },
        );
        Ok(())
    }

    pub(crate) fn revoke_account(&self, id: &AccountId) {
        if let Some(row) = self.inner.lock().unwrap().accounts.get_mut(id.as_str()) {
            row.revoked_at_ms = Some(1);
        }
    }
}

impl OneTimeLinks for MemoryHostStore {
    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn replace_pending(&self, link: NewLink<'_>) -> Result<(), LinkError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| LinkError::Store(LinkStoreFault::Poisoned))?;
        s.replace_link(&link)
    }

    fn redeem(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<String, Refused>, LinkError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| LinkError::Store(LinkStoreFault::Poisoned))?;
        Ok(s.redeem_link(kind, digest, now_ms))
    }

    fn would_redeem(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<(), Refused>, LinkError> {
        let s = self
            .inner
            .lock()
            .map_err(|_| LinkError::Store(LinkStoreFault::Poisoned))?;
        let row = s
            .links
            .iter()
            .find(|r| r.kind == kind && r.digest == digest);
        Ok(row.map_or(Err(Refused::Unknown), |r| {
            redeem_rule(r.state, r.expires_at_ms, now_ms)
        }))
    }

    fn insert_first(&self, link: NewLink<'_>) -> Result<bool, LinkError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| LinkError::Store(LinkStoreFault::Poisoned))?;
        if s.links
            .iter()
            .any(|r| r.kind == link.kind && r.subject == link.subject.as_str())
        {
            return Ok(false);
        }
        s.replace_link(&link)?;
        Ok(true)
    }

    fn pending_row(
        &self,
        kind: LinkKind,
        subject: &Subject,
    ) -> Result<Option<PendingLink>, LinkError> {
        let s = self
            .inner
            .lock()
            .map_err(|_| LinkError::Store(LinkStoreFault::Poisoned))?;
        Ok(s.links
            .iter()
            .find(|r| r.kind == kind && r.subject == subject.as_str() && r.state == "pending")
            .map(|r| PendingLink {
                link_id: r.link_id.clone(),
                issued_at_ms: r.issued_at_ms,
                expires_at_ms: r.expires_at_ms,
            }))
    }
}

impl AccountStore for MemoryHostStore {
    fn register_administrator(
        &self,
        token: &crate::links::Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<Registration, AccountStoreError> {
        let digest = LinkDigest::of(&self.instance_id, LinkKind::Registration, token);
        let mut s = self
            .inner
            .lock()
            .map_err(|_| AccountStoreError::Unavailable)?;
        let now = s.now(now_ms).map_err(|_| AccountStoreError::Unavailable)?;
        let link_idx = s
            .links
            .iter()
            .position(|r| r.kind == LinkKind::Registration && r.digest == digest);
        let Some(idx) = link_idx else {
            return Ok(Registration::Link(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(s.links[idx].state, s.links[idx].expires_at_ms, now) {
            return Ok(Registration::Link(refused));
        }
        if s.any_live_admin() {
            return Ok(Registration::AdministratorExists);
        }
        let key = new.username.key().as_str();
        if s.accounts.values().any(|a| a.login_key == key) {
            return Ok(Registration::LoginTaken);
        }
        let verifier = Verifier::new(new.verifier.expose().to_owned())
            .map_err(|_| AccountStoreError::Unavailable)?;
        s.links[idx].state = "used";
        s.links[idx].ended_at_ms = Some(now);
        s.accounts.insert(
            new.account_id.as_str().to_owned(),
            AccountRow {
                account_id: new.account_id.clone(),
                login_key: key.to_owned(),
                login: new.username.display().to_owned(),
                verifier,
                admin: true,
                password_changed_at_ms: now,
                revoked_at_ms: None,
            },
        );
        s.advance_clock(now);
        Ok(Registration::Registered {
            account_id: new.account_id.clone(),
        })
    }

    fn credentials(&self, key: &LoginKey) -> Result<Option<AccountCredentials>, AccountStoreError> {
        let creds = HostStore::account_credentials(self, key)
            .map_err(|_| AccountStoreError::Unavailable)?;
        Ok(creds.map(|c| AccountCredentials {
            account_id: c.account.account_id,
            login: c.account.login,
            admin: c.account.admin,
            revoked: c.account.revoked_at_ms.is_some(),
            verifier: c.verifier,
        }))
    }

    fn administrator_exists(&self) -> Result<bool, AccountStoreError> {
        HostStore::has_administrator(self).map_err(|_| AccountStoreError::Unavailable)
    }

    fn throttle(&self, key: &ThrottleKey<'_>, now_ms: i64) -> Result<Throttle, AccountStoreError> {
        let wall = u64::try_from(now_ms).map_err(|_| AccountStoreError::Unavailable)?;
        let state = HostStore::login_lockout(self, Socket::Control, key.as_str(), wall)
            .map_err(|_| AccountStoreError::Unavailable)?;
        Ok(Throttle {
            locked: state.is_locked(),
            retry_after_ms: if state.is_locked() {
                state.retry_after_ms()
            } else {
                0
            },
        })
    }

    fn record(
        &self,
        key: &ThrottleKey<'_>,
        attempt: AccountAttempt,
        now_ms: i64,
    ) -> Result<Throttle, AccountStoreError> {
        let wall = u64::try_from(now_ms).map_err(|_| AccountStoreError::Unavailable)?;
        let outcome = match attempt {
            AccountAttempt::Failure => Outcome::Failure,
            AccountAttempt::Success => Outcome::Success,
            AccountAttempt::Reset => Outcome::Reset,
        };
        let state = HostStore::record_login_capped(
            self,
            Socket::Control,
            key.as_str(),
            wall,
            outcome,
            key.cap_ms(),
        )
        .map_err(|_| AccountStoreError::Unavailable)?;
        Ok(Throttle {
            locked: state.is_locked(),
            retry_after_ms: if state.is_locked() {
                state.retry_after_ms()
            } else {
                0
            },
        })
    }
}

impl BrowserSessions for MemoryHostStore {
    fn create(&self, new: &NewSession, now_ms: i64) -> Result<Admission, SessionStoreError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let now = s.now(now_ms).map_err(|_| SessionStoreError::Unavailable)?;
        let mut reaped = Vec::new();
        s.sessions.retain(|_, row| {
            if is_live(row.last_activity_ms, now) {
                true
            } else {
                reaped.push(Ended {
                    id: row.id.clone(),
                    account_id: row.account_id.clone(),
                });
                false
            }
        });
        reaped.sort();
        if s.sessions.len() >= SESSION_LIMIT {
            s.advance_clock(now);
            return Ok(Admission::Full { reaped });
        }
        if !s.accounts.contains_key(&new.account_id)
            || s.sessions.contains_key(&new.id)
            || s.sessions.values().any(|r| r.cookie == new.cookie)
        {
            return Err(SessionStoreError::Unavailable);
        }
        s.sessions.insert(
            new.id.clone(),
            SessionRow {
                id: new.id.clone(),
                account_id: new.account_id.clone(),
                created_ms: now,
                last_activity_ms: now,
                cookie: new.cookie,
                proof: new.proof,
                origin: new.origin.clone(),
            },
        );
        s.advance_clock(now);
        Ok(Admission::Created { reaped })
    }

    fn find(
        &self,
        cookie: &SessionDigest,
        proof: &SessionDigest,
        origin: &Origin,
        now_ms: i64,
    ) -> Result<Option<SessionRecord>, SessionStoreError> {
        let s = self
            .inner
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let now = s.now(now_ms).map_err(|_| SessionStoreError::Unavailable)?;
        let found = s.sessions.values().find(|r| r.cookie == *cookie);
        let Some(row) = found else {
            return Ok(None);
        };
        let valid =
            row.proof.ct_eq(proof) && row.origin == *origin && is_live(row.last_activity_ms, now);
        Ok(valid.then(|| SessionRecord {
            id: row.id.clone(),
            account_id: row.account_id.clone(),
            created_ms: row.created_ms,
            last_activity_ms: row.last_activity_ms,
            origin: row.origin.clone(),
        }))
    }

    fn touch(&self, activity: &[(&str, i64)], now_ms: i64) -> Result<(), SessionStoreError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let now = s.now(now_ms).map_err(|_| SessionStoreError::Unavailable)?;
        for (id, ms) in activity {
            if let Some(row) = s.sessions.get_mut(*id) {
                row.last_activity_ms = row.last_activity_ms.max((*ms).min(now));
            }
        }
        s.advance_clock(now);
        Ok(())
    }

    fn end(&self, id: &str) -> Result<Option<Ended>, SessionStoreError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        Ok(s.sessions.remove(id).map(|row| Ended {
            id: row.id,
            account_id: row.account_id,
        }))
    }

    fn end_account(
        &self,
        account_id: &str,
        except: Option<&str>,
    ) -> Result<Vec<Ended>, SessionStoreError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let mut gone = Vec::new();
        s.sessions.retain(|_, row| {
            if row.account_id == account_id && except != Some(row.id.as_str()) {
                gone.push(Ended {
                    id: row.id.clone(),
                    account_id: row.account_id.clone(),
                });
                false
            } else {
                true
            }
        });
        gone.sort();
        Ok(gone)
    }

    fn reap(&self, now_ms: i64) -> Result<Vec<Ended>, SessionStoreError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let now = s.now(now_ms).map_err(|_| SessionStoreError::Unavailable)?;
        let mut gone = Vec::new();
        s.sessions.retain(|_, row| {
            if is_live(row.last_activity_ms, now) {
                true
            } else {
                gone.push(Ended {
                    id: row.id.clone(),
                    account_id: row.account_id.clone(),
                });
                false
            }
        });
        gone.sort();
        s.advance_clock(now);
        Ok(gone)
    }
}

impl HostStore for MemoryHostStore {
    fn verify_instance(&self, instance: &str) -> Result<(), Error> {
        if self.instance_id == instance {
            Ok(())
        } else {
            Err(Error::Unsafe)
        }
    }

    fn establish_organization_owner(
        &self,
        administrator: &Account,
        org: &str,
        member: &str,
    ) -> Result<(), Error> {
        if !org_office_launch::is_storage_id(org) || !org_proto::is_valid_member_id(member) {
            return Err(Error::Unsafe);
        }
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let id = administrator.account_id();
        if !s.is_live_admin(id) {
            return Err(Error::Unsafe);
        }
        let next_owner = (id.to_owned(), member.to_owned());
        let existing_owner = s.org_owners.get(org).cloned();
        let existing_link = s
            .account_links
            .get(&(id.to_owned(), org.to_owned()))
            .cloned();
        let effective_owner = existing_owner.as_ref().unwrap_or(&next_owner);
        let effective_link = existing_link.as_deref().unwrap_or(member);
        if effective_owner != &next_owner || effective_link != member {
            return Err(Error::Conflict);
        }
        s.org_owners.entry(org.to_owned()).or_insert(next_owner);
        s.account_links
            .entry((id.to_owned(), org.to_owned()))
            .or_insert_with(|| member.to_owned());
        Ok(())
    }

    fn verify_selected_owner(&self, org: &str, account: &str, member: &str) -> Result<(), Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let valid = s
            .org_owners
            .get(org)
            .is_some_and(|(a, m)| a == account && m == member)
            && s.account_links
                .get(&(account.to_owned(), org.to_owned()))
                .is_some_and(|m| m == member)
            && s.accounts
                .get(account)
                .is_some_and(|a| a.revoked_at_ms.is_none());
        if valid {
            Ok(())
        } else {
            Err(Error::Unsafe)
        }
    }

    fn has_administrator(&self) -> Result<bool, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.any_live_admin())
    }

    fn account_credentials(&self, login_key: &LoginKey) -> Result<Option<Credentials>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let Some(row) = s
            .accounts
            .values()
            .find(|a| a.login_key == login_key.as_str())
        else {
            return Ok(None);
        };
        let verifier = Verifier::new(row.verifier.expose().to_owned())?;
        let account = s
            .account_snapshot(row.account_id.as_str())
            .ok_or(Error::Unsafe)?;
        Ok(Some(Credentials { account, verifier }))
    }

    fn account_authority(&self, account_id: &AccountId) -> Result<Option<Account>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.account_snapshot(account_id.as_str()))
    }

    fn issue_unless_administrator(
        &self,
        kind: LinkKind,
        subject: &Subject,
        now_ms: i64,
    ) -> Result<Option<Issued>, LinkError> {
        issue_with(&self.instance_id, kind, subject, now_ms, |link| {
            let mut s = self
                .inner
                .lock()
                .map_err(|_| LinkError::Store(LinkStoreFault::Poisoned))?;
            if s.any_live_admin() {
                return Ok(false);
            }
            s.replace_link(&link)?;
            Ok(true)
        })
    }

    fn login_lockout(
        &self,
        socket: Socket,
        key: &str,
        wall_ms: u64,
    ) -> Result<LockoutState, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let now = s.now(i64::try_from(wall_ms).map_err(|_| Error::Unsafe)?)?;
        let until_ms = s
            .lockouts
            .get(&(socket, key.to_owned()))
            .map_or(now, |r| r.until_ms);
        Ok(LockoutState {
            now_ms: now,
            locked: now < until_ms,
            until_ms,
        })
    }

    fn record_login(
        &self,
        socket: Socket,
        key: &str,
        wall_ms: u64,
        outcome: Outcome,
    ) -> Result<LockoutState, Error> {
        self.record_login_capped(socket, key, wall_ms, outcome, LOGIN_DELAY_CAP_MS)
    }

    fn record_login_capped(
        &self,
        socket: Socket,
        key: &str,
        wall_ms: u64,
        outcome: Outcome,
        cap_ms: i64,
    ) -> Result<LockoutState, Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let now = s.now(i64::try_from(wall_ms).map_err(|_| Error::Unsafe)?)?;
        let map_key = (socket, key.to_owned());
        let existing = s.lockouts.get(&map_key);
        if let Some(until_ms) = existing.map(|r| r.until_ms).filter(|u| now < *u) {
            if outcome != Outcome::Reset {
                return Ok(LockoutState {
                    now_ms: now,
                    locked: true,
                    until_ms,
                });
            }
        }
        let mut until = now;
        if outcome == Outcome::Failure {
            let failures = existing
                .map_or(0, |r| r.failures)
                .checked_add(1)
                .ok_or(Error::Unsafe)?
                .min(63);
            let delay = backoff_ms(failures, cap_ms);
            until = now.checked_add(delay).ok_or(Error::Unsafe)?;
            s.lockouts.insert(
                map_key,
                LockoutRow {
                    failures,
                    until_ms: until,
                    last_failure_ms: now,
                },
            );
        } else {
            s.lockouts.remove(&map_key);
        }
        s.advance_clock(now);
        Ok(LockoutState {
            now_ms: now,
            locked: now < until,
            until_ms: until,
        })
    }

    fn prune_login_lockouts(&self, wall_ms: u64) -> Result<usize, Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let now = s.now(i64::try_from(wall_ms).map_err(|_| Error::Unsafe)?)?;
        let mut expired = Vec::new();
        for (k, row) in &s.lockouts {
            let age = if row.failures < 6 {
                1_800_000
            } else {
                86_400_000
            };
            if now.checked_sub(row.last_failure_ms).ok_or(Error::Unsafe)? >= age {
                expired.push(k.clone());
            }
        }
        let removed = expired.len();
        for k in expired {
            s.lockouts.remove(&k);
        }
        s.advance_clock(now);
        Ok(removed)
    }

    fn provisioning_taken(&self, name: &str, slug: &str) -> Result<Option<Taken>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        s.taken(name, slug)
    }

    fn provisioning_known(&self, operation: &str) -> Result<bool, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.provisioning.contains_key(operation))
    }

    fn provisioning_linked_to(&self, org: &str) -> Result<Option<Reservation>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.provisioning
            .values()
            .find(|r| r.linked && r.prepared.organization_id == org)
            .cloned())
    }

    fn provisioning_for_org(&self, org: &str) -> Result<Option<Reservation>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.provisioning
            .values()
            .find(|r| r.prepared.organization_id == org)
            .cloned())
    }

    fn provisioning_reserved(&self) -> Result<Vec<Reservation>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.provisioning
            .values()
            .filter(|r| !r.linked)
            .cloned()
            .collect())
    }

    fn provisioning_of(&self, account: &str) -> Result<Vec<Reservation>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.provisioning
            .values()
            .filter(|r| r.account == account)
            .cloned()
            .collect())
    }

    fn provisioning_lookup(
        &self,
        account: &str,
        operation: &str,
    ) -> Result<Option<Reservation>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.provisioning
            .get(operation)
            .filter(|r| r.account == account)
            .cloned())
    }

    fn provisioning_offices(&self) -> Result<BTreeSet<String>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let mut offices = BTreeSet::new();
        for r in s.provisioning.values() {
            if !r.prepared.valid() || !offices.insert(r.prepared.office_id.clone()) {
                return Err(Error::Unsafe);
            }
        }
        Ok(offices)
    }

    fn provisioning(
        &self,
        account: &Account,
        operation: &str,
        fingerprint: &[u8; 32],
        wall: i64,
    ) -> Result<Option<Reservation>, Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        if !s.is_live_admin(account.account_id()) {
            return Err(Error::NotAdministrator);
        }
        let now = s.now(wall)?;
        let result = s.provisioning.get(operation).cloned();
        if let Some(r) = &result {
            if r.account != account.account_id() || &r.fingerprint != fingerprint {
                return Err(Error::OperationReused);
            }
        } else {
            let time = org_proto::uuid_v7_time_ms(operation).ok_or(Error::InvalidId)? as i64;
            if time < now.saturating_sub(29 * DAY) {
                return Err(Error::Expired);
            }
            if time > now.saturating_add(300_000) {
                return Err(Error::ClockSkew);
            }
        }
        s.advance_clock(now);
        Ok(result)
    }

    fn reserve_provisioning(
        &self,
        account: &Account,
        frozen: &Reservation,
    ) -> Result<Reservation, Error> {
        check_reservation_shape(account, frozen)?;
        let _time =
            org_proto::uuid_v7_time_ms(&frozen.operation_id).ok_or(Error::InvalidId)? as i64;
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        if !s.is_live_admin(account.account_id()) {
            return Err(Error::NotAdministrator);
        }
        if let Some(prior) = s.provisioning.get(&frozen.operation_id) {
            return check_matching_prior(prior, frozen);
        }
        if s.taken(&frozen.prepared.plan.name, &frozen.prepared.plan.slug)?
            .is_some()
            || s.has_org_reservation(&frozen.prepared.organization_id)
        {
            return Err(Error::Conflict);
        }
        OrganizationName::parse(&frozen.prepared.plan.name).map_err(|_| Error::Unsafe)?;
        s.provisioning
            .insert(frozen.operation_id.clone(), frozen.clone());
        Ok(frozen.clone())
    }

    fn provision_key_stored(&self, r: &Reservation, fingerprint: &str) -> Result<(), Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        if !s.is_live_admin(&r.account) {
            return Err(Error::NotAdministrator);
        }
        let prior = s
            .provisioning
            .get_mut(&r.operation_id)
            .ok_or(Error::Unsafe)?;
        if prior.account != r.account
            || prior.fingerprint != r.fingerprint
            || prior
                .key_fingerprint
                .as_ref()
                .is_some_and(|p| p != fingerprint)
        {
            return Err(Error::Conflict);
        }
        prior.key_fingerprint = Some(fingerprint.to_owned());
        Ok(())
    }

    fn provision_linked(&self, r: &Reservation) -> Result<(), Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        if !s.is_live_admin(&r.account) {
            return Err(Error::NotAdministrator);
        }
        let prior = s
            .provisioning
            .get_mut(&r.operation_id)
            .ok_or(Error::Unsafe)?;
        if prior.account != r.account
            || prior.fingerprint != r.fingerprint
            || prior.key_fingerprint.is_none()
        {
            return Err(Error::Conflict);
        }
        prior.linked = true;
        Ok(())
    }

    fn recovery_state(&self, operation: &str) -> Result<Option<RecoveryState>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.recovery.get(operation).map(|l| l.state))
    }

    fn recovery_observed(&self, operation: &str) -> Result<Ledger, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        s.recovery.get(operation).cloned().ok_or(Error::Unsafe)
    }

    fn recovery_source(&self, org: &str) -> Result<Reservation, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let matches: Vec<_> = s
            .provisioning
            .values()
            .filter(|r| r.linked && r.prepared.organization_id == org)
            .collect();
        if matches.len() != 1 {
            return Err(Error::Unsafe);
        }
        Ok(matches[0].clone())
    }

    fn recovery_status(
        &self,
        expected: &Ledger,
        after: i64,
    ) -> Result<org_host_control::recovery::StatusResult, Error> {
        if after < 0 {
            return Err(Error::InvalidId);
        }
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let l = s
            .recovery
            .get(&expected.binding.operation_id)
            .ok_or(Error::Unsafe)?;
        if l != expected {
            return Err(Error::Conflict);
        }
        let end = after.saturating_add(32).min(l.ordinal);
        let mut audit_rows: Vec<&AuditRow> = s
            .recovery_audit
            .get(&l.binding.operation_id)
            .into_iter()
            .flatten()
            .filter(|a| a.ordinal > after && a.ordinal <= end)
            .collect();
        audit_rows.sort_by_key(|a| {
            (
                a.ordinal,
                match a.action {
                    "show" => 0,
                    "ack_pending" => 1,
                    _ => 2,
                },
            )
        });
        let audit = audit_rows
            .into_iter()
            .map(|a| org_host_control::recovery::Audit {
                ordinal: a.ordinal.to_string(),
                action: a.action.to_owned(),
                account: a.account.clone(),
                at_ms: a.at_ms,
            })
            .collect();
        Ok(org_host_control::recovery::StatusResult {
            version: 1,
            org_id: l.binding.organization_id.clone(),
            state: l.state.into(),
            delivery_ordinal: l.ordinal.to_string(),
            first_shown_ms: l.first_shown_ms,
            last_redelivered_ms: l.last_redelivered_ms,
            redelivery_count: l.redelivery_count.to_string(),
            acknowledged_ms: l.acknowledged_ms,
            acknowledged_by: l.acknowledged_by.clone(),
            audit,
            next_after_ordinal: (end < l.ordinal).then(|| end.to_string()),
            hq_loss_recovery_supported: false,
        })
    }

    fn recovery_prepare(&self, public: &PublicRecord) -> Result<Ledger, Error> {
        public.package().map_err(|_| Error::Unsafe)?;
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        s.verify_recovery_source(&self.instance_id, &public.binding())?;
        if let Some(l) = s.recovery.get(&public.operation_id) {
            if l.binding != public.binding()
                || l.public_key != public.public_key
                || l.package_sha256 != public.package_sha256
            {
                return Err(Error::Conflict);
            }
            return Ok(l.clone());
        }
        let l = Ledger {
            version: 1,
            binding: public.binding(),
            public_key: public.public_key.clone(),
            package_sha256: public.package_sha256.clone(),
            state: RecoveryState::Pending,
            ordinal: 0,
            first_shown_ms: None,
            last_redelivered_ms: None,
            redelivery_count: 0,
            acknowledged_ms: None,
            acknowledged_by: None,
        };
        s.recovery.insert(public.operation_id.clone(), l.clone());
        Ok(l)
    }

    fn recovery_delivered(
        &self,
        expected: &Ledger,
        account: &str,
        redeliver: bool,
        wall: i64,
    ) -> Result<Ledger, Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        if !s.is_live_admin(account) {
            return Err(Error::Unsafe);
        }
        let current = s
            .recovery
            .get(&expected.binding.operation_id)
            .cloned()
            .ok_or(Error::Unsafe)?;
        if &current != expected
            || !matches!(
                current.state,
                RecoveryState::Pending | RecoveryState::Delivered
            )
            || (current.state == RecoveryState::Delivered && !redeliver)
        {
            return Err(Error::Conflict);
        }
        let now = s.now(wall)?;
        let mut next = current;
        next.ordinal = next.ordinal.checked_add(1).ok_or(Error::Unsafe)?;
        next.state = RecoveryState::Delivered;
        next.first_shown_ms.get_or_insert(now);
        if next.ordinal > 1 {
            next.last_redelivered_ms = Some(now);
            next.redelivery_count = next.ordinal - 1;
        }
        s.recovery
            .insert(next.binding.operation_id.clone(), next.clone());
        s.recovery_audit
            .entry(next.binding.operation_id.clone())
            .or_default()
            .push(AuditRow {
                ordinal: next.ordinal,
                action: "show",
                account: account.to_owned(),
                at_ms: now,
            });
        s.advance_clock(now);
        Ok(next)
    }

    fn recovery_ack_intent(
        &self,
        expected: &Ledger,
        account: &str,
        wall: i64,
    ) -> Result<Ledger, Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        if !s.is_live_admin(account) {
            return Err(Error::Unsafe);
        }
        let current = s
            .recovery
            .get(&expected.binding.operation_id)
            .cloned()
            .ok_or(Error::Unsafe)?;
        if &current != expected || current.state != RecoveryState::Delivered {
            return Err(Error::Conflict);
        }
        let now = s.now(wall)?;
        let mut next = current;
        next.state = RecoveryState::AckPending;
        next.acknowledged_by = Some(account.to_owned());
        next.acknowledged_ms = Some(now);
        s.recovery
            .insert(next.binding.operation_id.clone(), next.clone());
        s.recovery_audit
            .entry(next.binding.operation_id.clone())
            .or_default()
            .push(AuditRow {
                ordinal: next.ordinal,
                action: "ack_pending",
                account: account.to_owned(),
                at_ms: now,
            });
        s.advance_clock(now);
        Ok(next)
    }

    fn recovery_reconcile(&self, custody: &dyn HostSecrets) -> Result<(), Error> {
        let ops: Vec<String> = {
            let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
            s.provisioning.keys().cloned().collect()
        };
        for op in ops {
            if org_proto::uuid_v7_time_ms(&op).is_none() {
                return Err(Error::Unsafe);
            }
            let (r, l) = {
                let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
                (
                    s.provisioning.get(&op).cloned().ok_or(Error::Unsafe)?,
                    s.recovery.get(&op).cloned(),
                )
            };
            let name = Name::parse(&r.recovery_handle)?;
            match l.as_ref().map(|l| l.state) {
                Some(RecoveryState::AckPending) => {
                    custody.delete(&name)?;
                    if custody.load(&name)?.is_some() {
                        return Err(Error::Unsafe);
                    }
                    let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
                    let current = s.recovery.get(&op).cloned().ok_or(Error::Unsafe)?;
                    if current.state == RecoveryState::Acknowledged {
                        continue;
                    }
                    if Some(&current) != l.as_ref() {
                        return Err(Error::Conflict);
                    }
                    let mut acked = current;
                    acked.state = RecoveryState::Acknowledged;
                    let ack_by = acked.acknowledged_by.clone().ok_or(Error::Unsafe)?;
                    let ack_ms = acked.acknowledged_ms.ok_or(Error::Unsafe)?;
                    s.recovery.insert(op.clone(), acked.clone());
                    s.recovery_audit.entry(op).or_default().push(AuditRow {
                        ordinal: acked.ordinal,
                        action: "acknowledged",
                        account: ack_by,
                        at_ms: ack_ms,
                    });
                }
                Some(RecoveryState::Acknowledged) => {
                    if custody.load(&name)?.is_some() {
                        return Err(Error::Unsafe);
                    }
                }
                _ => {
                    crate::provisioning::pending_recovery(custody, &r)
                        .map_err(|_| Error::Unsafe)?;
                }
            }
        }
        Ok(())
    }

    fn web_tls_pair(&self) -> Result<Option<StoredPair>, Error> {
        let s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        Ok(s.web_tls_pair.clone())
    }

    fn set_web_tls_pair(&self, pair: &StoredPair) -> Result<(), Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        s.web_tls_pair = Some(pair.clone());
        Ok(())
    }

    fn tls_set_lookup(&self, op: &Attempt<'_>, wall_ms: i64) -> Result<TlsLookup, Error> {
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let now = s.now(wall_ms)?;
        let decision = s.tls_lookup_inner(op, now);
        s.advance_clock(now);
        decision
    }

    fn tls_set_write(
        &self,
        op: &Attempt<'_>,
        wall_ms: i64,
        stored: &TlsStored,
        pair: Option<&StoredPair>,
    ) -> Result<Option<TlsStored>, Error> {
        let valid = matches!(
            (stored, pair),
            (TlsStored::Completed { .. }, Some(_))
                | (
                    TlsStored::Rejected {
                        refusal: TlsRefusal::TlsKeyMismatch
                    },
                    None
                )
        );
        if !valid || op.kind != "tls.set" {
            return Err(Error::Unsafe);
        }
        let mut s = self.inner.lock().map_err(|_| Error::Unsafe)?;
        let now = s.now(wall_ms)?;
        match s.tls_lookup_inner(op, now) {
            Ok(TlsLookup::Vacant) => {}
            Ok(TlsLookup::Replay(existing)) => {
                s.advance_clock(now);
                return Ok(Some(existing));
            }
            Err(error) => {
                s.advance_clock(now);
                return Err(error);
            }
        }
        s.maintain_journal(now);
        let state = match stored {
            TlsStored::Completed { .. } => "completed",
            _ => "rejected",
        };
        if let Some(pair) = pair {
            s.web_tls_pair = Some(pair.clone());
        }
        let result = serde_json::to_string(stored).map_err(|_| Error::Unsafe)?;
        let expires = id_time(op.id)? + 30 * DAY;
        s.journal.insert(
            op.id.to_owned(),
            JournalRow {
                kind: op.kind,
                fingerprint: op.fingerprint,
                account: op.account.to_owned(),
                state,
                result,
                expires_at_ms: expires,
            },
        );
        Ok(None)
    }
}

fn check_reservation_shape(account: &Account, frozen: &Reservation) -> Result<(), Error> {
    if !frozen.prepared.valid()
        || frozen.account != account.account_id()
        || frozen.prepared.creator.employee_id != frozen.owner_member
        || frozen.recovery_handle != format!("recovery.{}", frozen.operation_id)
        || frozen.key_fingerprint.is_some()
        || frozen.linked
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}

fn check_matching_prior(prior: &Reservation, frozen: &Reservation) -> Result<Reservation, Error> {
    if prior.account != frozen.account
        || prior.fingerprint != frozen.fingerprint
        || prior.prepared != frozen.prepared
        || prior.owner_member != frozen.owner_member
    {
        return Err(Error::OperationReused);
    }
    Ok(prior.clone())
}
