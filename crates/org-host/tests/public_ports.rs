//! External consumer verification for `org_host::ports` (`TASK-231` AC #2).
//!
//! This integration test compiles as a separate crate outside `org-host` and imports ONLY
//! `org_host::ports` (no private internals, no `rusqlite`, no SQLite fixtures). It proves an
//! external crate can implement all six consumer-owned port traits — `HostStore`, `AccountStore`,
//! `BrowserSessions`, `OneTimeLinks`, `HostSecrets`, and `OfficeSecretsKeys` — and drive host
//! persistence and custody operations through trait objects.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use org_host::ports::accounts::{
    backoff_ms, check_new_password, AccountId, Attempt as AccountAttempt, ClientAddress,
    Credentials as AccountCredentials, LoginKey, NewAccount, PasswordRule, Registration,
    StoreError as AccountStoreError, Throttle, ThrottleKey, Username, Verifier,
};
use org_host::ports::host_secrets::{Error as SecretError, Name, Value, MAX_BYTES};
use org_host::ports::host_store::{
    accounts::{Account, Credentials as HostCredentials, Link},
    journal::{
        id_time, Attempt as JournalAttempt, Effect, Lookup, PruneOutcome, Stored, Write, DAY,
        MAX_PRUNE_BATCHES, PRUNE_BATCH,
    },
    lockout::{Outcome, Socket, State as LockoutState},
    multi_person::{
        account_subject, invite_subject, parse_account_subject, parse_invite_subject,
        AdminHandoverOutcome, InviteAttach, InviteIssueError, InviteRegistration,
        MemberResetIssueError, OrgOwnerHandoverOutcome, RemoveAccessOutcome, SetPasswordOutcome,
    },
    provisioning::{Reservation, Taken},
    recovery::{Ledger, State as RecoveryState},
    web_tls::{Source, StoredPair, TlsLookup, TlsStored},
    ApiError, CreatorV1, Error as HostStoreError, ErrorCode, HostOpResult, Journaled,
    OrgAccessEntry, PlanV1, PreparedV1, PublicRecord, RecoveryAudit, RecoveryBinding,
    RecoveryStatusResult, TlsRefusal,
};
use org_host::ports::keys::{is_storage_id, KeyStoreError, OfficeSecretsKey};
use org_host::ports::links::{
    issue_with, redeem_rule, Error as LinkError, Issued, LinkDigest, LinkKind, NewLink, Pending,
    Refused, Subject, Token,
};
use org_host::ports::sessions::{
    is_live, Admission, Ended, NewSession, Origin, Scheme, SessionDigest, SessionRecord,
    StoreError as SessionStoreError, SESSION_LIMIT,
};
use org_host::ports::{
    AccountStore, BrowserSessions, HostSecrets, HostStore, OfficeSecretsKeys, OneTimeLinks,
};
use serde_json::json;

const INSTANCE_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
const OTHER_INSTANCE_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b1";
const ORG_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";
const ORG_ID_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071d0";
const OFFICE_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c1";
const MEMBER_OWNER: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c2";
const MEMBER_BOB: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c3";
const MEMBER_BOB_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c4";
const OP_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607110";
const OP_ID_2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607111";
const OP_ID_3: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607112";
const ADMIN_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1";
const BOB_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2";
const PHC_1: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const PHC_2: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA";

#[derive(Default)]
struct ExternalHostSecrets(Mutex<BTreeMap<Name, Vec<u8>>>);

impl HostSecrets for ExternalHostSecrets {
    fn put(&self, name: &Name, value: &Value) -> Result<(), SecretError> {
        let mut map = self.0.lock().unwrap();
        // Add-only items (`tls.*`, `writer-device`) preserve the first stored value.
        if name.is_add_only() && map.contains_key(name) {
            return Ok(());
        }
        map.insert(name.clone(), value.expose().to_vec());
        Ok(())
    }

    fn load(&self, name: &Name) -> Result<Option<Value>, SecretError> {
        let map = self.0.lock().unwrap();
        match map.get(name) {
            Some(bytes) => Ok(Some(Value::new(bytes.clone())?)),
            None => Ok(None),
        }
    }

    fn delete(&self, name: &Name) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }

    fn list(&self) -> Result<Vec<Name>, SecretError> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}

#[derive(Default)]
struct ExternalOfficeKeys(Mutex<BTreeMap<String, [u8; 32]>>);

impl OfficeSecretsKeys for ExternalOfficeKeys {
    fn create(&self, office_id: &str, key: &OfficeSecretsKey) -> Result<(), KeyStoreError> {
        if !is_storage_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        let mut map = self.0.lock().unwrap();
        if map.contains_key(office_id) {
            return Err(KeyStoreError::Exists);
        }
        map.insert(office_id.to_owned(), *key.expose_bytes());
        Ok(())
    }

    fn load(&self, office_id: &str) -> Result<Option<OfficeSecretsKey>, KeyStoreError> {
        if !is_storage_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        let map = self.0.lock().unwrap();
        Ok(map.get(office_id).map(|b| OfficeSecretsKey::from_bytes(*b)))
    }

    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError> {
        if !is_storage_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        self.0.lock().unwrap().remove(office_id);
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>, KeyStoreError> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}

#[derive(Clone)]
struct StoredLink {
    link_id: String,
    kind: LinkKind,
    subject: String,
    digest: LinkDigest,
    issued_at_ms: i64,
    expires_at_ms: i64,
    used_at_ms: Option<i64>,
    cancelled_at_ms: Option<i64>,
}

impl StoredLink {
    fn status(&self) -> &'static str {
        if self.used_at_ms.is_some() {
            "used"
        } else if self.cancelled_at_ms.is_some() {
            "cancelled"
        } else {
            "pending"
        }
    }
}

struct StoredAccount {
    account_id: AccountId,
    login: String,
    login_key: String,
    verifier: Option<String>,
    admin: bool,
    revoked_at_ms: Option<i64>,
    password_changed_at_ms: i64,
    time_zone: Option<String>,
    last_org_id: Option<String>,
}

struct StoredSession {
    record: SessionRecord,
    cookie: SessionDigest,
    proof: SessionDigest,
}

#[derive(Default)]
struct State {
    clock_ms: i64,
    links: Vec<StoredLink>,
    accounts: BTreeMap<String, StoredAccount>,
    org_links: BTreeMap<(String, String), (String, bool)>,
    sessions: BTreeMap<String, StoredSession>,
    throttles: BTreeMap<String, (i64, i64)>,
    lockouts: BTreeMap<(Socket, String), (u32, u64, u64)>,
    journal: BTreeMap<String, ([u8; 32], Stored, i64)>,
    reservations: BTreeMap<String, Reservation>,
    recoveries: BTreeMap<String, (Ledger, Vec<RecoveryAudit>)>,
    tls_pair: Option<StoredPair>,
    tls_journal: BTreeMap<String, ([u8; 32], TlsStored)>,
}

struct ExternalHostStore {
    instance_id: String,
    state: Mutex<State>,
}

impl ExternalHostStore {
    fn new(instance_id: &str, now_ms: i64) -> Self {
        Self {
            instance_id: instance_id.to_owned(),
            state: Mutex::new(State {
                clock_ms: now_ms,
                ..State::default()
            }),
        }
    }
}

impl State {
    fn advance(&mut self, now_ms: i64) -> i64 {
        self.clock_ms = self.clock_ms.max(now_ms);
        self.clock_ms
    }

    fn has_active_admin(&self) -> bool {
        self.accounts
            .values()
            .any(|a| a.admin && a.revoked_at_ms.is_none() && a.verifier.is_some())
    }

    fn snapshot(&self, account: &StoredAccount) -> Account {
        let links = self
            .org_links
            .iter()
            .filter(|(_, (acct_id, _))| acct_id == account.account_id.as_str())
            .map(|((org_id, member_id), _)| Link::new(org_id.clone(), member_id.clone()))
            .collect();
        Account::new(
            account.account_id.clone(),
            account.login.clone(),
            account.admin,
            account.revoked_at_ms,
            account.password_changed_at_ms,
            links,
        )
        .with_preferences(account.time_zone.clone(), account.last_org_id.clone())
    }

    fn insert_link(&mut self, link: NewLink<'_>) {
        for existing in &mut self.links {
            if existing.kind == link.kind
                && existing.subject == link.subject.as_str()
                && existing.used_at_ms.is_none()
                && existing.cancelled_at_ms.is_none()
            {
                existing.cancelled_at_ms = Some(link.issued_at_ms);
            }
        }
        self.links.push(StoredLink {
            link_id: link.link_id.to_owned(),
            kind: link.kind,
            subject: link.subject.as_str().to_owned(),
            digest: link.digest,
            issued_at_ms: link.issued_at_ms,
            expires_at_ms: link.expires_at_ms,
            used_at_ms: None,
            cancelled_at_ms: None,
        });
    }

    fn end_account_sessions(&mut self, account_id: &str, except: Option<&str>) -> Vec<Ended> {
        let ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|(id, s)| {
                s.record.account_id == account_id && except.is_none_or(|keep| keep != id.as_str())
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut ended = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(removed) = self.sessions.remove(&id) {
                ended.push(Ended {
                    id: removed.record.id,
                    account_id: removed.record.account_id,
                });
            }
        }
        ended.sort();
        ended
    }
}

impl OneTimeLinks for ExternalHostStore {
    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn replace_pending(&self, link: NewLink<'_>) -> Result<(), LinkError> {
        self.state.lock().unwrap().insert_link(link);
        Ok(())
    }

    fn redeem(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<String, Refused>, LinkError> {
        let mut state = self.state.lock().unwrap();
        let Some(row) = state
            .links
            .iter_mut()
            .find(|row| row.kind == kind && row.digest == digest)
        else {
            return Ok(Err(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(row.status(), row.expires_at_ms, now_ms) {
            return Ok(Err(refused));
        }
        row.used_at_ms = Some(now_ms);
        Ok(Ok(row.link_id.clone()))
    }

    fn would_redeem(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<(), Refused>, LinkError> {
        let state = self.state.lock().unwrap();
        let Some(row) = state
            .links
            .iter()
            .find(|row| row.kind == kind && row.digest == digest)
        else {
            return Ok(Err(Refused::Unknown));
        };
        Ok(redeem_rule(row.status(), row.expires_at_ms, now_ms))
    }

    fn peek_subject(
        &self,
        kind: LinkKind,
        digest: LinkDigest,
        now_ms: i64,
    ) -> Result<Result<String, Refused>, LinkError> {
        let state = self.state.lock().unwrap();
        let Some(row) = state
            .links
            .iter()
            .find(|row| row.kind == kind && row.digest == digest)
        else {
            return Ok(Err(Refused::Unknown));
        };
        match redeem_rule(row.status(), row.expires_at_ms, now_ms) {
            Ok(()) => Ok(Ok(row.subject.clone())),
            Err(refused) => Ok(Err(refused)),
        }
    }

    fn insert_first(&self, link: NewLink<'_>) -> Result<bool, LinkError> {
        let mut state = self.state.lock().unwrap();
        if state
            .links
            .iter()
            .any(|row| row.kind == link.kind && row.subject == link.subject.as_str())
        {
            return Ok(false);
        }
        state.insert_link(link);
        Ok(true)
    }

    fn pending_row(&self, kind: LinkKind, subject: &Subject) -> Result<Option<Pending>, LinkError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .links
            .iter()
            .find(|row| {
                row.kind == kind
                    && row.subject == subject.as_str()
                    && row.used_at_ms.is_none()
                    && row.cancelled_at_ms.is_none()
            })
            .map(|row| Pending {
                link_id: row.link_id.clone(),
                issued_at_ms: row.issued_at_ms,
                expires_at_ms: row.expires_at_ms,
            }))
    }
}

impl AccountStore for ExternalHostStore {
    fn register_administrator(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<Registration, AccountStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        let digest = LinkDigest::of(&self.instance_id, LinkKind::Registration, token);
        let Some(pos) = state
            .links
            .iter()
            .position(|row| row.kind == LinkKind::Registration && row.digest == digest)
        else {
            return Ok(Registration::Link(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(
            state.links[pos].status(),
            state.links[pos].expires_at_ms,
            now,
        ) {
            return Ok(Registration::Link(refused));
        }
        if state.has_active_admin() {
            return Ok(Registration::AdministratorExists);
        }
        let login_key = new.username.key().as_str().to_owned();
        if state.accounts.values().any(|a| a.login_key == login_key) {
            return Ok(Registration::LoginTaken);
        }
        state.links[pos].used_at_ms = Some(now);
        state.accounts.insert(
            new.account_id.as_str().to_owned(),
            StoredAccount {
                account_id: new.account_id.clone(),
                login: new.username.display().to_owned(),
                login_key,
                verifier: Some(new.verifier.expose().to_owned()),
                admin: true,
                revoked_at_ms: None,
                password_changed_at_ms: now,
                time_zone: None,
                last_org_id: None,
            },
        );
        Ok(Registration::Registered {
            account_id: new.account_id.clone(),
        })
    }

    fn credentials(&self, key: &LoginKey) -> Result<Option<AccountCredentials>, AccountStoreError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .accounts
            .values()
            .find(|a| a.login_key == key.as_str())
            .and_then(|a| {
                let verifier = Verifier::new(a.verifier.clone()?).ok()?;
                Some(AccountCredentials {
                    account_id: a.account_id.clone(),
                    login: a.login.clone(),
                    admin: a.admin,
                    revoked: a.revoked_at_ms.is_some(),
                    verifier,
                })
            }))
    }

    fn administrator_exists(&self) -> Result<bool, AccountStoreError> {
        Ok(self.state.lock().unwrap().has_active_admin())
    }

    fn throttle(&self, key: &ThrottleKey<'_>, now_ms: i64) -> Result<Throttle, AccountStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        let until = state
            .throttles
            .get(key.as_str())
            .map_or(0, |&(_, until)| until);
        let retry = until.saturating_sub(now).max(0);
        Ok(Throttle {
            locked: retry > 0,
            retry_after_ms: retry,
        })
    }

    fn record(
        &self,
        key: &ThrottleKey<'_>,
        attempt: AccountAttempt,
        now_ms: i64,
    ) -> Result<Throttle, AccountStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        match attempt {
            AccountAttempt::Success | AccountAttempt::Reset => {
                state.throttles.remove(key.as_str());
                Ok(Throttle {
                    locked: false,
                    retry_after_ms: 0,
                })
            }
            AccountAttempt::Failure => {
                let entry = state.throttles.entry(key.as_str().to_owned()).or_default();
                if entry.1 > now {
                    return Ok(Throttle {
                        locked: true,
                        retry_after_ms: entry.1 - now,
                    });
                }
                entry.0 += 1;
                let delay = backoff_ms(entry.0, key.cap_ms());
                entry.1 = now + delay;
                Ok(Throttle {
                    locked: delay > 0,
                    retry_after_ms: delay,
                })
            }
        }
    }
}

impl BrowserSessions for ExternalHostStore {
    fn create(&self, new: &NewSession, now_ms: i64) -> Result<Admission, SessionStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        if !state.accounts.contains_key(&new.account_id) {
            return Err(SessionStoreError::Unavailable);
        }
        let expired: Vec<String> = state
            .sessions
            .iter()
            .filter(|(_, s)| !is_live(s.record.last_activity_ms, now))
            .map(|(id, _)| id.clone())
            .collect();
        let mut reaped = Vec::with_capacity(expired.len());
        for id in expired {
            if let Some(s) = state.sessions.remove(&id) {
                reaped.push(Ended {
                    id: s.record.id,
                    account_id: s.record.account_id,
                });
            }
        }
        reaped.sort();
        if state.sessions.len() >= SESSION_LIMIT {
            return Ok(Admission::Full { reaped });
        }
        state.sessions.insert(
            new.id.clone(),
            StoredSession {
                record: SessionRecord {
                    id: new.id.clone(),
                    account_id: new.account_id.clone(),
                    created_ms: now,
                    last_activity_ms: now,
                    origin: new.origin.clone(),
                },
                cookie: new.cookie,
                proof: new.proof,
            },
        );
        Ok(Admission::Created { reaped })
    }

    fn find(
        &self,
        cookie: &SessionDigest,
        proof: &SessionDigest,
        origin: &Origin,
        now_ms: i64,
    ) -> Result<Option<SessionRecord>, SessionStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        Ok(state
            .sessions
            .values()
            .find(|s| {
                s.cookie.ct_eq(cookie)
                    && s.proof.ct_eq(proof)
                    && &s.record.origin == origin
                    && is_live(s.record.last_activity_ms, now)
            })
            .map(|s| s.record.clone()))
    }

    fn touch(&self, activity: &[(&str, i64)], now_ms: i64) -> Result<(), SessionStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        for &(id, at) in activity {
            if let Some(session) = state.sessions.get_mut(id) {
                let capped = at.min(now);
                if capped > session.record.last_activity_ms {
                    session.record.last_activity_ms = capped;
                }
            }
        }
        Ok(())
    }

    fn end(&self, id: &str) -> Result<Option<Ended>, SessionStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .sessions
            .remove(id)
            .map(|s| Ended {
                id: s.record.id,
                account_id: s.record.account_id,
            }))
    }

    fn end_account(
        &self,
        account_id: &str,
        except: Option<&str>,
    ) -> Result<Vec<Ended>, SessionStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .end_account_sessions(account_id, except))
    }

    fn reap(&self, now_ms: i64) -> Result<Vec<Ended>, SessionStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        let expired: Vec<String> = state
            .sessions
            .iter()
            .filter(|(_, s)| !is_live(s.record.last_activity_ms, now))
            .map(|(id, _)| id.clone())
            .collect();
        let mut ended = Vec::with_capacity(expired.len());
        for id in expired {
            if let Some(s) = state.sessions.remove(&id) {
                ended.push(Ended {
                    id: s.record.id,
                    account_id: s.record.account_id,
                });
            }
        }
        ended.sort();
        Ok(ended)
    }
}

impl HostStore for ExternalHostStore {
    fn verify_instance(&self, instance: &str) -> Result<(), HostStoreError> {
        if self.instance_id == instance {
            Ok(())
        } else {
            Err(HostStoreError::Unsafe)
        }
    }

    fn establish_organization_owner(
        &self,
        administrator: &Account,
        org: &str,
        member: &str,
    ) -> Result<(), HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let Some(acct) = state.accounts.get(administrator.account_id()) else {
            return Err(HostStoreError::Unsafe);
        };
        if !acct.admin || acct.revoked_at_ms.is_some() {
            return Err(HostStoreError::Unsafe);
        }
        if let Some(((existing_org, existing_member), (existing_acct, _))) = state
            .org_links
            .iter()
            .find(|((o, _), (_, is_owner))| o == org && *is_owner)
        {
            return if existing_org == org
                && existing_acct == administrator.account_id()
                && existing_member == member
            {
                Ok(())
            } else {
                Err(HostStoreError::Conflict)
            };
        }
        state.org_links.insert(
            (org.to_owned(), member.to_owned()),
            (administrator.account_id().to_owned(), true),
        );
        Ok(())
    }

    fn verify_selected_owner(
        &self,
        org: &str,
        account: &str,
        member: &str,
    ) -> Result<(), HostStoreError> {
        let state = self.state.lock().unwrap();
        let live = state
            .accounts
            .get(account)
            .is_some_and(|a| a.admin && a.revoked_at_ms.is_none());
        let linked = state
            .org_links
            .get(&(org.to_owned(), member.to_owned()))
            .is_some_and(|(acct, is_owner)| acct == account && *is_owner);
        if live && linked {
            Ok(())
        } else {
            Err(HostStoreError::Unsafe)
        }
    }

    fn has_administrator(&self) -> Result<bool, HostStoreError> {
        Ok(self.state.lock().unwrap().has_active_admin())
    }

    fn account_credentials(
        &self,
        login_key: &LoginKey,
    ) -> Result<Option<HostCredentials>, HostStoreError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .accounts
            .values()
            .find(|a| a.login_key == login_key.as_str())
            .and_then(|a| {
                let verifier = Verifier::new(a.verifier.clone()?).ok()?;
                Some(HostCredentials::new(state.snapshot(a), verifier))
            }))
    }

    fn account_authority(&self, account_id: &AccountId) -> Result<Option<Account>, HostStoreError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .accounts
            .get(account_id.as_str())
            .map(|a| state.snapshot(a)))
    }

    fn set_account_preferences(
        &self,
        account_id: &AccountId,
        time_zone: Option<Option<&str>>,
        last_org_id: Option<Option<&str>>,
    ) -> Result<(Option<String>, Option<String>), HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let linked_orgs: BTreeSet<String> = state
            .org_links
            .iter()
            .filter(|(_, (acct, _))| acct == account_id.as_str())
            .map(|((org, _), _)| org.clone())
            .collect();
        let Some(acct) = state.accounts.get_mut(account_id.as_str()) else {
            return Err(HostStoreError::Unsafe);
        };
        if let Some(tz) = time_zone {
            acct.time_zone = tz.map(ToOwned::to_owned);
        }
        if let Some(org) = last_org_id {
            acct.last_org_id = org
                .filter(|id| linked_orgs.contains(*id))
                .map(ToOwned::to_owned);
        }
        Ok((acct.time_zone.clone(), acct.last_org_id.clone()))
    }

    fn init_local_authless_if_empty(
        &self,
        now_ms: i64,
    ) -> Result<Option<AccountId>, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        if !state.accounts.is_empty() {
            return Ok(None);
        }
        let id = AccountId::parse(ADMIN_ID).map_err(|_| HostStoreError::InvalidId)?;
        state.accounts.insert(
            id.as_str().to_owned(),
            StoredAccount {
                account_id: id.clone(),
                login: "local".into(),
                login_key: "local".into(),
                verifier: None,
                admin: true,
                revoked_at_ms: None,
                password_changed_at_ms: now_ms,
                time_zone: None,
                last_org_id: None,
            },
        );
        Ok(Some(id))
    }

    fn local_authless_account(&self) -> Result<Option<AccountId>, HostStoreError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .accounts
            .values()
            .find(|a| a.admin && a.revoked_at_ms.is_none() && a.verifier.is_none())
            .map(|a| a.account_id.clone()))
    }

    fn enable_local_auth(
        &self,
        username: &Username,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<AccountId, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let Some(acct) = state
            .accounts
            .values_mut()
            .find(|a| a.admin && a.revoked_at_ms.is_none() && a.verifier.is_none())
        else {
            return Err(HostStoreError::Conflict);
        };
        acct.login = username.display().to_owned();
        acct.login_key = username.key().as_str().to_owned();
        acct.verifier = Some(verifier.expose().to_owned());
        acct.password_changed_at_ms = now_ms;
        Ok(acct.account_id.clone())
    }

    fn active_administrator_account(&self) -> Result<Option<(AccountId, String)>, HostStoreError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .accounts
            .values()
            .find(|a| a.admin && a.revoked_at_ms.is_none() && a.verifier.is_some())
            .map(|a| (a.account_id.clone(), a.login.clone())))
    }

    fn active_account_login(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<String>, HostStoreError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .accounts
            .get(account_id.as_str())
            .filter(|a| a.revoked_at_ms.is_none() && a.verifier.is_some())
            .map(|a| a.login.clone()))
    }

    fn org_owner(&self, org_id: &str) -> Result<Option<(AccountId, String)>, HostStoreError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .org_links
            .iter()
            .find(|((o, _), (_, owner))| o == org_id && *owner)
            .and_then(|((_, member_id), (acct_id, _))| {
                Some((AccountId::parse(acct_id).ok()?, member_id.clone()))
            }))
    }

    fn list_org_access(
        &self,
        org_id: &str,
        now_ms: i64,
    ) -> Result<Vec<OrgAccessEntry>, HostStoreError> {
        let state = self.state.lock().unwrap();
        let mut entries = BTreeMap::<String, OrgAccessEntry>::new();
        for ((o, member_id), (acct_id, owner)) in &state.org_links {
            if o != org_id {
                continue;
            }
            if let Some(acct) = state
                .accounts
                .get(acct_id)
                .filter(|a| a.revoked_at_ms.is_none() && a.verifier.is_some())
            {
                let reset_subject =
                    account_subject(&acct.account_id).map_err(|_| HostStoreError::Unsafe)?;
                let pending_password_reset = state.links.iter().any(|l| {
                    l.kind == LinkKind::SetPassword
                        && l.subject == reset_subject.as_str()
                        && redeem_rule(l.status(), l.expires_at_ms, now_ms).is_ok()
                });
                entries.insert(
                    member_id.clone(),
                    OrgAccessEntry {
                        member_id: member_id.clone(),
                        account_id: Some(acct.account_id.as_str().to_owned()),
                        login: Some(acct.login.clone()),
                        admin: acct.admin,
                        owner: *owner,
                        pending_invite: false,
                        pending_password_reset,
                    },
                );
            }
        }
        for link in &state.links {
            if link.kind == LinkKind::Registration
                && redeem_rule(link.status(), link.expires_at_ms, now_ms).is_ok()
            {
                if let Some((o, member_id)) = parse_invite_subject(&link.subject) {
                    if o == org_id && !entries.contains_key(member_id) {
                        entries.insert(
                            member_id.to_owned(),
                            OrgAccessEntry {
                                member_id: member_id.to_owned(),
                                account_id: None,
                                login: None,
                                admin: false,
                                owner: false,
                                pending_invite: true,
                                pending_password_reset: false,
                            },
                        );
                    }
                }
            }
        }
        Ok(entries.into_values().collect())
    }

    fn issue_org_invite(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, InviteIssueError> {
        let subject = invite_subject(org_id, member_id).map_err(|_| InviteIssueError::Invalid)?;
        let mut state = self.state.lock().unwrap();
        if !state.has_active_admin() {
            return Err(InviteIssueError::AuthDisabled);
        }
        if state
            .org_links
            .contains_key(&(org_id.to_owned(), member_id.to_owned()))
        {
            return Err(InviteIssueError::AlreadyLinked);
        }
        issue_with(
            &self.instance_id,
            LinkKind::Registration,
            &subject,
            now_ms,
            |new_link| {
                state.insert_link(new_link);
                Ok(true)
            },
        )
        .map_err(|_| InviteIssueError::Store)?
        .ok_or(InviteIssueError::Store)
    }

    fn redeem_invite_register(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<InviteRegistration, AccountStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        let digest = LinkDigest::of(&self.instance_id, LinkKind::Registration, token);
        let Some(pos) = state
            .links
            .iter()
            .position(|l| l.kind == LinkKind::Registration && l.digest == digest)
        else {
            return Ok(InviteRegistration::Link(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(
            state.links[pos].status(),
            state.links[pos].expires_at_ms,
            now,
        ) {
            return Ok(InviteRegistration::Link(refused));
        }
        let Some((org_id, member_id)) = parse_invite_subject(&state.links[pos].subject) else {
            return Ok(InviteRegistration::NotAnInvite);
        };
        let (org_id, member_id) = (org_id.to_owned(), member_id.to_owned());
        if state
            .org_links
            .contains_key(&(org_id.clone(), member_id.clone()))
        {
            return Ok(InviteRegistration::AlreadyLinked);
        }
        let login_key = new.username.key().as_str().to_owned();
        if state.accounts.values().any(|a| a.login_key == login_key) {
            return Ok(InviteRegistration::LoginTaken);
        }
        state.links[pos].used_at_ms = Some(now);
        state.accounts.insert(
            new.account_id.as_str().to_owned(),
            StoredAccount {
                account_id: new.account_id.clone(),
                login: new.username.display().to_owned(),
                login_key,
                verifier: Some(new.verifier.expose().to_owned()),
                admin: false,
                revoked_at_ms: None,
                password_changed_at_ms: now,
                time_zone: None,
                last_org_id: None,
            },
        );
        state.org_links.insert(
            (org_id.clone(), member_id.clone()),
            (new.account_id.as_str().to_owned(), false),
        );
        Ok(InviteRegistration::Registered {
            account_id: new.account_id.clone(),
            org_id,
            member_id,
        })
    }

    fn redeem_invite_attach(
        &self,
        token: &Token,
        account_id: &AccountId,
        now_ms: i64,
    ) -> Result<InviteAttach, AccountStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        let digest = LinkDigest::of(&self.instance_id, LinkKind::Registration, token);
        let Some(pos) = state
            .links
            .iter()
            .position(|l| l.kind == LinkKind::Registration && l.digest == digest)
        else {
            return Ok(InviteAttach::Link(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(
            state.links[pos].status(),
            state.links[pos].expires_at_ms,
            now,
        ) {
            return Ok(InviteAttach::Link(refused));
        }
        let Some((org_id, member_id)) = parse_invite_subject(&state.links[pos].subject) else {
            return Ok(InviteAttach::NotAnInvite);
        };
        let (org_id, member_id) = (org_id.to_owned(), member_id.to_owned());
        if !state
            .accounts
            .get(account_id.as_str())
            .is_some_and(|a| a.revoked_at_ms.is_none() && a.verifier.is_some())
        {
            return Ok(InviteAttach::AccountUnavailable);
        }
        if state
            .org_links
            .contains_key(&(org_id.clone(), member_id.clone()))
        {
            return Ok(InviteAttach::AlreadyLinked);
        }
        if state
            .org_links
            .iter()
            .any(|((o, _), (acct, _))| o == &org_id && acct == account_id.as_str())
        {
            return Ok(InviteAttach::AlreadyMember);
        }
        state.links[pos].used_at_ms = Some(now);
        state.org_links.insert(
            (org_id.clone(), member_id.clone()),
            (account_id.as_str().to_owned(), false),
        );
        Ok(InviteAttach::Attached { org_id, member_id })
    }

    fn issue_admin_password_reset_link(
        &self,
        now_ms: i64,
    ) -> Result<Option<(String, Issued)>, LinkError> {
        let mut state = self.state.lock().unwrap();
        let Some((id, login)) = state
            .accounts
            .values()
            .find(|a| a.admin && a.revoked_at_ms.is_none() && a.verifier.is_some())
            .map(|a| (a.account_id.clone(), a.login.clone()))
        else {
            return Ok(None);
        };
        let subject = account_subject(&id)?;
        let issued = issue_with(
            &self.instance_id,
            LinkKind::SetPassword,
            &subject,
            now_ms,
            |new_link| {
                state.insert_link(new_link);
                Ok(true)
            },
        )?
        .ok_or(LinkError::Invalid)?;
        Ok(Some((login, issued)))
    }

    fn issue_member_password_reset_link(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, MemberResetIssueError> {
        let mut state = self.state.lock().unwrap();
        let Some((acct_id, _)) = state
            .org_links
            .get(&(org_id.to_owned(), member_id.to_owned()))
            .cloned()
        else {
            return Err(MemberResetIssueError::NotLinked);
        };
        let Some(id) = state
            .accounts
            .get(&acct_id)
            .filter(|a| a.revoked_at_ms.is_none() && a.verifier.is_some())
            .map(|a| a.account_id.clone())
        else {
            return Err(MemberResetIssueError::NotLinked);
        };
        let subject = account_subject(&id).map_err(|_| MemberResetIssueError::Store)?;
        issue_with(
            &self.instance_id,
            LinkKind::SetPassword,
            &subject,
            now_ms,
            |new_link| {
                state.insert_link(new_link);
                Ok(true)
            },
        )
        .map_err(|_| MemberResetIssueError::Store)?
        .ok_or(MemberResetIssueError::Store)
    }

    fn redeem_set_password(
        &self,
        token: &Token,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<SetPasswordOutcome, AccountStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        let digest = LinkDigest::of(&self.instance_id, LinkKind::SetPassword, token);
        let Some(pos) = state
            .links
            .iter()
            .position(|l| l.kind == LinkKind::SetPassword && l.digest == digest)
        else {
            return Ok(SetPasswordOutcome::Link(Refused::Unknown));
        };
        if let Err(refused) = redeem_rule(
            state.links[pos].status(),
            state.links[pos].expires_at_ms,
            now,
        ) {
            return Ok(SetPasswordOutcome::Link(refused));
        }
        let Some(account_id) = parse_account_subject(&state.links[pos].subject) else {
            return Ok(SetPasswordOutcome::Link(Refused::Unknown));
        };
        let Some(acct) = state.accounts.get_mut(account_id.as_str()) else {
            return Ok(SetPasswordOutcome::Link(Refused::Unknown));
        };
        acct.verifier = Some(verifier.expose().to_owned());
        acct.password_changed_at_ms = now;
        let login = acct.login.clone();
        state.links[pos].used_at_ms = Some(now);
        let ended = state.end_account_sessions(account_id.as_str(), None);
        Ok(SetPasswordOutcome::Updated {
            account_id,
            login,
            ended,
        })
    }

    fn change_own_password(
        &self,
        account_id: &AccountId,
        keep_session_id: &str,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<Vec<Ended>, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(now_ms);
        let Some(acct) = state.accounts.get_mut(account_id.as_str()) else {
            return Err(HostStoreError::Unsafe);
        };
        acct.verifier = Some(verifier.expose().to_owned());
        acct.password_changed_at_ms = now;
        Ok(state.end_account_sessions(account_id.as_str(), Some(keep_session_id)))
    }

    fn remove_org_access(
        &self,
        org_id: &str,
        member_id: &str,
        _now_ms: i64,
    ) -> Result<RemoveAccessOutcome, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        if let Some((acct_id, is_owner)) = state
            .org_links
            .get(&(org_id.to_owned(), member_id.to_owned()))
            .cloned()
        {
            if is_owner {
                return Ok(RemoveAccessOutcome::IsOwner);
            }
            state
                .org_links
                .remove(&(org_id.to_owned(), member_id.to_owned()));
            let ended = state.end_account_sessions(&acct_id, None);
            let account_id = AccountId::parse(&acct_id).ok();
            return Ok(RemoveAccessOutcome::Removed { account_id, ended });
        }
        Ok(RemoveAccessOutcome::NotFound)
    }

    fn handover_org_owner(
        &self,
        org_id: &str,
        from_account: &AccountId,
        to_member_id: &str,
    ) -> Result<OrgOwnerHandoverOutcome, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let Some(prev_member) = state
            .org_links
            .iter()
            .find(|((o, _), (a, owner))| o == org_id && a == from_account.as_str() && *owner)
            .map(|((_, m), _)| m.clone())
        else {
            return Ok(OrgOwnerHandoverOutcome::NotOwner);
        };
        let Some((to_acct_str, _)) = state
            .org_links
            .get(&(org_id.to_owned(), to_member_id.to_owned()))
            .cloned()
        else {
            return Ok(OrgOwnerHandoverOutcome::InvalidTarget);
        };
        let to_account_id =
            AccountId::parse(&to_acct_str).map_err(|_| HostStoreError::InvalidId)?;
        state
            .org_links
            .get_mut(&(org_id.to_owned(), prev_member.clone()))
            .unwrap()
            .1 = false;
        state
            .org_links
            .get_mut(&(org_id.to_owned(), to_member_id.to_owned()))
            .unwrap()
            .1 = true;
        Ok(OrgOwnerHandoverOutcome::Transferred {
            previous_member_id: prev_member,
            to_account_id,
            to_member_id: to_member_id.to_owned(),
        })
    }

    fn restore_org_owner(
        &self,
        org_id: &str,
        account: &AccountId,
        member_id: &str,
    ) -> Result<(), HostStoreError> {
        let mut state = self.state.lock().unwrap();
        for ((o, m), (acct, owner)) in &mut state.org_links {
            if o == org_id {
                *owner = acct == account.as_str() && m == member_id;
            }
        }
        Ok(())
    }

    fn handover_administrator(
        &self,
        from_account: &AccountId,
        to_account: &AccountId,
    ) -> Result<AdminHandoverOutcome, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        if !state
            .accounts
            .get(from_account.as_str())
            .is_some_and(|a| a.admin && a.revoked_at_ms.is_none())
        {
            return Ok(AdminHandoverOutcome::NotAdmin);
        }
        if !state
            .accounts
            .get(to_account.as_str())
            .is_some_and(|a| a.revoked_at_ms.is_none() && a.verifier.is_some())
        {
            return Ok(AdminHandoverOutcome::InvalidTarget);
        }
        state.accounts.get_mut(from_account.as_str()).unwrap().admin = false;
        state.accounts.get_mut(to_account.as_str()).unwrap().admin = true;
        Ok(AdminHandoverOutcome::Transferred)
    }

    fn issue_unless_administrator(
        &self,
        kind: LinkKind,
        subject: &Subject,
        now_ms: i64,
    ) -> Result<Option<Issued>, LinkError> {
        let mut state = self.state.lock().unwrap();
        if state.has_active_admin() {
            return Ok(None);
        }
        issue_with(&self.instance_id, kind, subject, now_ms, |new_link| {
            state.insert_link(new_link);
            Ok(true)
        })
    }

    fn journal_lookup(
        &self,
        op: &JournalAttempt<'_>,
        wall_ms: i64,
    ) -> Result<Lookup, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(wall_ms);
        let Some((fp, stored, expires_at_ms)) = state.journal.get(op.id) else {
            return Ok(Lookup::Vacant);
        };
        if *expires_at_ms <= now {
            return Err(HostStoreError::Expired);
        }
        if fp != &op.fingerprint {
            return Err(HostStoreError::OperationReused);
        }
        Ok(Lookup::Replay(stored.clone()))
    }

    fn journal_commit(
        &self,
        op: &JournalAttempt<'_>,
        wall_ms: i64,
        effect: Effect,
    ) -> Result<Write, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let now = state.advance(wall_ms);
        if let Some((fp, stored, expires_at_ms)) = state.journal.get(op.id) {
            if *expires_at_ms <= now {
                return Err(HostStoreError::Expired);
            }
            if fp != &op.fingerprint {
                return Err(HostStoreError::OperationReused);
            }
            return Ok(Write::Existing(Lookup::Replay(stored.clone())));
        }
        match &effect.stored {
            Stored::Completed { .. } => {}
            Stored::Rejected { refusal }
                if matches!(
                    refusal.code,
                    ErrorCode::AlreadyExists | ErrorCode::InvalidName
                ) => {}
            _ => return Err(HostStoreError::Unsafe),
        }
        let expires_at_ms = id_time(op.id)? + 30 * DAY;
        state.journal.insert(
            op.id.to_owned(),
            (op.fingerprint, effect.stored.clone(), expires_at_ms),
        );
        Ok(Write::Committed(effect.stored))
    }

    fn journal_prune_expired(&self, wall_ms: i64) -> Result<PruneOutcome, HostStoreError> {
        if wall_ms < 0 {
            return Err(HostStoreError::Unsafe);
        }
        let mut removed: u64 = 0;
        for _ in 0..MAX_PRUNE_BATCHES {
            let batch = {
                let mut state = self.state.lock().unwrap();
                let now = state.advance(wall_ms);
                let mut expired: Vec<(i64, String)> = state
                    .journal
                    .iter()
                    .filter(|(_, (_, _, expires))| *expires <= now)
                    .map(|(id, (_, _, expires))| (*expires, id.clone()))
                    .collect();
                expired.sort();
                let mut count = 0usize;
                for (_, id) in expired.into_iter().take(usize::from(PRUNE_BATCH)) {
                    state.journal.remove(&id);
                    count += 1;
                }
                count
            };
            removed = removed.saturating_add(u64::try_from(batch).unwrap_or(u64::MAX));
            if batch < usize::from(PRUNE_BATCH) {
                return Ok(PruneOutcome {
                    removed,
                    more_remaining: false,
                });
            }
        }
        Ok(PruneOutcome {
            removed,
            more_remaining: true,
        })
    }

    fn login_lockout(
        &self,
        socket: Socket,
        key: &str,
        now_ms: u64,
    ) -> Result<LockoutState, HostStoreError> {
        let state = self.state.lock().unwrap();
        let until = state
            .lockouts
            .get(&(socket, key.to_owned()))
            .map_or(0, |&(_, until, _)| until);
        Ok(LockoutState::new(
            now_ms as i64,
            until > now_ms,
            until as i64,
        ))
    }

    fn record_login(
        &self,
        socket: Socket,
        key: &str,
        now_ms: u64,
        outcome: Outcome,
    ) -> Result<LockoutState, HostStoreError> {
        self.record_login_capped(socket, key, now_ms, outcome, 900_000)
    }

    fn record_login_capped(
        &self,
        socket: Socket,
        key: &str,
        now_ms: u64,
        outcome: Outcome,
        cap_ms: i64,
    ) -> Result<LockoutState, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        match outcome {
            Outcome::Success | Outcome::Reset => {
                state.lockouts.remove(&(socket, key.to_owned()));
                Ok(LockoutState::new(now_ms as i64, false, now_ms as i64))
            }
            Outcome::Failure => {
                let entry = state
                    .lockouts
                    .entry((socket, key.to_owned()))
                    .or_insert((0, 0, now_ms));
                entry.0 += 1;
                entry.2 = now_ms;
                let delay = backoff_ms(i64::from(entry.0), cap_ms) as u64;
                entry.1 = now_ms + delay;
                Ok(LockoutState::new(now_ms as i64, delay > 0, entry.1 as i64))
            }
        }
    }

    fn prune_login_lockouts(&self, now_ms: u64) -> Result<usize, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let before = state.lockouts.len();
        state.lockouts.retain(|_, (_, until, updated)| {
            *until > now_ms || now_ms.saturating_sub(*updated) < 1_800_000
        });
        Ok(before - state.lockouts.len())
    }

    fn provisioning_taken(&self, name: &str, slug: &str) -> Result<Option<Taken>, HostStoreError> {
        let state = self.state.lock().unwrap();
        for r in state.reservations.values() {
            if r.prepared.plan.name.eq_ignore_ascii_case(name) {
                return Ok(Some(Taken::Name));
            }
            if r.prepared.plan.slug.eq_ignore_ascii_case(slug) {
                return Ok(Some(Taken::Slug));
            }
        }
        Ok(None)
    }

    fn provisioning_known(&self, operation_id: &str) -> Result<bool, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .reservations
            .contains_key(operation_id))
    }

    fn provisioning(
        &self,
        administrator: &Account,
        operation_id: &str,
        fingerprint: &[u8; 32],
        _now_ms: i64,
    ) -> Result<Option<Reservation>, HostStoreError> {
        let state = self.state.lock().unwrap();
        let Some(r) = state.reservations.get(operation_id) else {
            return Ok(None);
        };
        if r.account != administrator.account_id() || &r.fingerprint != fingerprint {
            return Err(HostStoreError::OperationReused);
        }
        Ok(Some(r.clone()))
    }

    fn reserve_provisioning(
        &self,
        administrator: &Account,
        r: &Reservation,
    ) -> Result<Reservation, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        if !state
            .accounts
            .get(administrator.account_id())
            .is_some_and(|a| a.admin && a.revoked_at_ms.is_none())
        {
            return Err(HostStoreError::NotAdministrator);
        }
        if let Some(existing) = state.reservations.get(&r.operation_id) {
            return Ok(existing.clone());
        }
        state.reservations.insert(r.operation_id.clone(), r.clone());
        Ok(r.clone())
    }

    fn provision_key_stored(
        &self,
        r: &Reservation,
        key_fingerprint: &str,
    ) -> Result<(), HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let Some(row) = state.reservations.get_mut(&r.operation_id) else {
            return Err(HostStoreError::Conflict);
        };
        row.key_fingerprint = Some(key_fingerprint.to_owned());
        Ok(())
    }

    fn provision_linked(&self, r: &Reservation) -> Result<(), HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let Some(row) = state.reservations.get_mut(&r.operation_id) else {
            return Err(HostStoreError::Conflict);
        };
        if row.key_fingerprint.is_none() {
            return Err(HostStoreError::Conflict);
        }
        row.linked = true;
        Ok(())
    }

    fn provisioning_reserved(&self) -> Result<Vec<Reservation>, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .reservations
            .values()
            .filter(|r| !r.linked)
            .cloned()
            .collect())
    }

    fn provisioning_of(&self, account: &str) -> Result<Vec<Reservation>, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .reservations
            .values()
            .filter(|r| r.account == account)
            .cloned()
            .collect())
    }

    fn provisioning_lookup(
        &self,
        account: &str,
        operation_id: &str,
    ) -> Result<Option<Reservation>, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .reservations
            .get(operation_id)
            .filter(|r| r.account == account)
            .cloned())
    }

    fn provisioning_offices(&self) -> Result<BTreeSet<String>, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .reservations
            .values()
            .map(|r| r.prepared.office_id.clone())
            .collect())
    }

    fn provisioning_linked_to(
        &self,
        organization_id: &str,
    ) -> Result<Option<Reservation>, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .reservations
            .values()
            .find(|r| r.linked && r.prepared.organization_id == organization_id)
            .cloned())
    }

    fn provisioning_for_org(
        &self,
        organization_id: &str,
    ) -> Result<Option<Reservation>, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .reservations
            .values()
            .find(|r| r.prepared.organization_id == organization_id)
            .cloned())
    }

    fn recovery_source(&self, organization_id: &str) -> Result<Reservation, HostStoreError> {
        self.provisioning_linked_to(organization_id)?
            .ok_or(HostStoreError::Empty)
    }

    fn recovery_state(&self, operation_id: &str) -> Result<Option<RecoveryState>, HostStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .recoveries
            .get(operation_id)
            .map(|(l, _)| l.state))
    }

    fn recovery_prepare(&self, public: &PublicRecord) -> Result<Ledger, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        if let Some((ledger, _)) = state.recoveries.get(&public.operation_id) {
            return Ok(ledger.clone());
        }
        let ledger = Ledger {
            version: 1,
            binding: RecoveryBinding {
                instance_id: public.instance_id.clone(),
                organization_id: public.organization_id.clone(),
                office_id: public.office_id.clone(),
                operation_id: public.operation_id.clone(),
                version: public.escrow_version,
            },
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
        state
            .recoveries
            .insert(public.operation_id.clone(), (ledger.clone(), Vec::new()));
        Ok(ledger)
    }

    fn recovery_observed(&self, operation_id: &str) -> Result<Ledger, HostStoreError> {
        self.state
            .lock()
            .unwrap()
            .recoveries
            .get(operation_id)
            .map(|(l, _)| l.clone())
            .ok_or(HostStoreError::Empty)
    }

    fn recovery_delivered(
        &self,
        current: &Ledger,
        account: &str,
        redeliver: bool,
        now_ms: i64,
    ) -> Result<Ledger, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let (ledger, audit) = state
            .recoveries
            .get_mut(&current.binding.operation_id)
            .ok_or(HostStoreError::Empty)?;
        ledger.state = RecoveryState::Delivered;
        ledger.ordinal += 1;
        if redeliver {
            ledger.redelivery_count += 1;
            ledger.last_redelivered_ms = Some(now_ms);
        } else {
            ledger.first_shown_ms = Some(now_ms);
        }
        audit.push(RecoveryAudit {
            ordinal: ledger.ordinal.to_string(),
            action: "show".into(),
            account: account.to_owned(),
            at_ms: now_ms,
        });
        Ok(ledger.clone())
    }

    fn recovery_ack_intent(
        &self,
        current: &Ledger,
        account: &str,
        now_ms: i64,
    ) -> Result<Ledger, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        let (ledger, audit) = state
            .recoveries
            .get_mut(&current.binding.operation_id)
            .ok_or(HostStoreError::Empty)?;
        ledger.state = RecoveryState::AckPending;
        ledger.acknowledged_by = Some(account.to_owned());
        audit.push(RecoveryAudit {
            ordinal: ledger.ordinal.to_string(),
            action: "ack_pending".into(),
            account: account.to_owned(),
            at_ms: now_ms,
        });
        Ok(ledger.clone())
    }

    fn recovery_reconcile(&self, custody: &dyn HostSecrets) -> Result<(), HostStoreError> {
        let pending_ops: Vec<(String, String)> = {
            let state = self.state.lock().unwrap();
            state
                .recoveries
                .iter()
                .filter(|(_, (l, _))| l.state == RecoveryState::AckPending)
                .filter_map(|(op, _)| {
                    state
                        .reservations
                        .get(op)
                        .map(|r| (op.clone(), r.recovery_handle.clone()))
                })
                .collect()
        };
        for (op, handle) in pending_ops {
            let name = Name::parse(&handle).map_err(|_| HostStoreError::Unsafe)?;
            custody.delete(&name)?;
            let mut state = self.state.lock().unwrap();
            if let Some((ledger, audit)) = state.recoveries.get_mut(&op) {
                let at_ms = ledger.first_shown_ms.unwrap_or(0) + 1;
                ledger.state = RecoveryState::Acknowledged;
                ledger.acknowledged_ms = Some(at_ms);
                audit.push(RecoveryAudit {
                    ordinal: ledger.ordinal.to_string(),
                    action: "acknowledged".into(),
                    account: ledger.acknowledged_by.clone().unwrap_or_default(),
                    at_ms,
                });
            }
        }
        Ok(())
    }

    fn recovery_status(
        &self,
        ledger: &Ledger,
        after: i64,
    ) -> Result<RecoveryStatusResult, HostStoreError> {
        let state = self.state.lock().unwrap();
        let (_, audit) = state
            .recoveries
            .get(&ledger.binding.operation_id)
            .ok_or(HostStoreError::Empty)?;
        let filtered: Vec<_> = audit
            .iter()
            .filter(|a| a.ordinal.parse::<i64>().unwrap_or(0) > after)
            .cloned()
            .collect();
        Ok(RecoveryStatusResult {
            version: 1,
            org_id: ledger.binding.organization_id.clone(),
            state: ledger.state.into(),
            delivery_ordinal: ledger.ordinal.to_string(),
            first_shown_ms: ledger.first_shown_ms,
            last_redelivered_ms: ledger.last_redelivered_ms,
            redelivery_count: ledger.redelivery_count.to_string(),
            acknowledged_ms: ledger.acknowledged_ms,
            acknowledged_by: ledger.acknowledged_by.clone(),
            audit: filtered,
            next_after_ordinal: None,
            hq_loss_recovery_supported: false,
        })
    }

    fn web_tls_pair(&self) -> Result<Option<StoredPair>, HostStoreError> {
        Ok(self.state.lock().unwrap().tls_pair.clone())
    }

    fn set_web_tls_pair(&self, pair: &StoredPair) -> Result<(), HostStoreError> {
        self.state.lock().unwrap().tls_pair = Some(pair.clone());
        Ok(())
    }

    fn tls_set_lookup(
        &self,
        op: &JournalAttempt<'_>,
        _wall_ms: i64,
    ) -> Result<TlsLookup, HostStoreError> {
        let state = self.state.lock().unwrap();
        let Some((fp, stored)) = state.tls_journal.get(op.id) else {
            return Ok(TlsLookup::Vacant);
        };
        if fp != &op.fingerprint {
            return Err(HostStoreError::OperationReused);
        }
        Ok(TlsLookup::Replay(stored.clone()))
    }

    fn tls_set_write(
        &self,
        op: &JournalAttempt<'_>,
        _wall_ms: i64,
        stored: &TlsStored,
        pair: Option<&StoredPair>,
    ) -> Result<Option<TlsStored>, HostStoreError> {
        let mut state = self.state.lock().unwrap();
        if let Some((fp, existing)) = state.tls_journal.get(op.id) {
            if fp != &op.fingerprint {
                return Err(HostStoreError::OperationReused);
            }
            return Ok(Some(existing.clone()));
        }
        state
            .tls_journal
            .insert(op.id.to_owned(), (op.fingerprint, stored.clone()));
        if let Some(pair) = pair {
            state.tls_pair = Some(pair.clone());
        }
        Ok(None)
    }
}

#[test]
fn external_host_secrets_and_office_keys_ports() {
    let secrets: Arc<dyn HostSecrets> = Arc::new(ExternalHostSecrets::default());
    assert!(Name::parse("not-a-valid-secret-name").is_err());
    assert!(Value::new(vec![0u8; MAX_BYTES + 1]).is_err());

    let reg_name = Name::parse("registry").unwrap();
    let tls_name = Name::parse(&format!("tls.{}", "ab".repeat(32))).unwrap();
    assert!(!reg_name.is_add_only());
    assert!(tls_name.is_add_only());

    let val = Value::new(b"secret-material".to_vec()).unwrap();
    let replacement = Value::new(b"replacement".to_vec()).unwrap();
    assert_eq!(format!("{val:?}"), "<redacted>");
    secrets.put(&reg_name, &val).unwrap();
    assert_eq!(
        secrets.load(&reg_name).unwrap().unwrap().expose(),
        b"secret-material"
    );

    // Add-only TLS items preserve their first stored value on duplicate put.
    secrets.put(&tls_name, &val).unwrap();
    secrets.put(&tls_name, &replacement).unwrap();
    assert_eq!(
        secrets.load(&tls_name).unwrap().unwrap().expose(),
        b"secret-material"
    );
    assert_eq!(secrets.list().unwrap(), vec![reg_name.clone(), tls_name]);
    secrets.delete(&reg_name).unwrap();
    assert!(secrets.load(&reg_name).unwrap().is_none());

    let keys: Arc<dyn OfficeSecretsKeys> = Arc::new(ExternalOfficeKeys::default());
    let key = OfficeSecretsKey::from_bytes([0x5a; 32]);
    assert!(matches!(
        keys.create("../bad", &key),
        Err(KeyStoreError::NotPlainId)
    ));
    keys.create(OFFICE_ID, &key).unwrap();
    assert!(matches!(
        keys.create(OFFICE_ID, &key),
        Err(KeyStoreError::Exists)
    ));
    assert_eq!(
        keys.load(OFFICE_ID).unwrap().unwrap().expose_bytes(),
        &[0x5a; 32]
    );
    assert_eq!(keys.list().unwrap(), vec![OFFICE_ID.to_owned()]);
    keys.delete(OFFICE_ID).unwrap();
    keys.delete(OFFICE_ID).unwrap();
    assert!(keys.load(OFFICE_ID).unwrap().is_none());
}

#[test]
fn external_host_store_implements_all_four_registry_ports() {
    let now = 1_700_000_000_000_i64;
    let store: Arc<dyn HostStore> = Arc::new(ExternalHostStore::new(INSTANCE_ID, now));
    let secrets: Arc<dyn HostSecrets> = Arc::new(ExternalHostSecrets::default());

    // Supertrait upcasting works cleanly from `&dyn HostStore`.
    let links: &dyn OneTimeLinks = &*store;
    let accounts: &dyn AccountStore = &*store;
    let sessions: &dyn BrowserSessions = &*store;

    assert_eq!(store.verify_instance(INSTANCE_ID), Ok(()));
    assert_eq!(
        store.verify_instance(OTHER_INSTANCE_ID),
        Err(HostStoreError::Unsafe)
    );
    assert_eq!(check_new_password("short"), Err(PasswordRule::TooShort));

    // 1. OneTimeLinks + AccountStore: initial admin link -> register administrator.
    let admin_subject = Subject::new("administrator").unwrap();
    let initial = links
        .issue_first(LinkKind::Registration, &admin_subject, now)
        .unwrap()
        .expect("first link");
    assert_eq!(
        links
            .peek_link_subject(LinkKind::Registration, &initial.token, now)
            .unwrap(),
        Ok("administrator".into())
    );
    assert_eq!(
        links
            .redeemable(LinkKind::Registration, &initial.token, now)
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        links
            .pending(LinkKind::Registration, &admin_subject, now)
            .unwrap()
            .unwrap()
            .link_id,
        initial.link_id
    );

    let admin_id = AccountId::parse(ADMIN_ID).unwrap();
    let new_admin = NewAccount {
        account_id: admin_id.clone(),
        username: Username::new("Alice").unwrap(),
        verifier: Verifier::new(PHC_1.into()).unwrap(),
    };
    assert_eq!(
        accounts
            .register_administrator(&initial.token, &new_admin, now + 1)
            .unwrap(),
        Registration::Registered {
            account_id: admin_id.clone(),
        }
    );
    assert_eq!(accounts.administrator_exists(), Ok(true));
    assert_eq!(store.has_administrator(), Ok(true));
    assert!(store
        .issue_unless_administrator(LinkKind::Registration, &admin_subject, now + 2)
        .unwrap()
        .is_none());

    let login_key = LoginKey::new("alice").unwrap();
    let creds = accounts.credentials(&login_key).unwrap().unwrap();
    assert_eq!(creds.account_id, admin_id);
    assert!(creds.admin && !creds.revoked);

    // Throttle schedule on AccountStore.
    let addr = ClientAddress::of("127.0.0.1".parse().unwrap());
    let key = ThrottleKey::Address(&addr);
    for i in 0..5 {
        assert!(
            !accounts
                .record(&key, AccountAttempt::Failure, now + 10 + i)
                .unwrap()
                .locked
        );
    }
    let locked = accounts
        .record(&key, AccountAttempt::Failure, now + 15)
        .unwrap();
    assert!(locked.locked);
    assert!(accounts.throttle(&key, now + 15).unwrap().locked);
    assert!(
        !accounts
            .record(&key, AccountAttempt::Reset, now + 16)
            .unwrap()
            .locked
    );

    // 2. BrowserSessions: create, find, touch, end, end_account, reap.
    let origin = Origin::new(Scheme::Http, "127.0.0.1", 7420).unwrap();
    let cookie = SessionDigest::cookie(b"cookie-1");
    let proof = SessionDigest::proof(b"proof-1");
    let s1 = NewSession::new(admin_id.as_str(), cookie, proof, origin.clone()).unwrap();
    assert_eq!(
        sessions.create(&s1, now + 20).unwrap(),
        Admission::Created { reaped: vec![] }
    );
    let found = sessions
        .find(&cookie, &proof, &origin, now + 21)
        .unwrap()
        .unwrap();
    assert_eq!(found.id, s1.id);
    sessions.touch(&[(&s1.id, now + 25)], now + 25).unwrap();
    assert_eq!(
        sessions
            .find(&cookie, &proof, &origin, now + 26)
            .unwrap()
            .unwrap()
            .last_activity_ms,
        now + 25
    );
    assert_eq!(sessions.end(&s1.id).unwrap().unwrap().id, s1.id);
    assert!(sessions.reap(now + 30).unwrap().is_empty());

    // 3. Multi-person invites, password resets, handovers, preferences, and access removal.
    let admin = store.account_authority(&admin_id).unwrap().unwrap();
    store
        .establish_organization_owner(&admin, ORG_ID, MEMBER_OWNER)
        .unwrap();
    store
        .establish_organization_owner(&admin, ORG_ID_2, MEMBER_OWNER)
        .unwrap();
    store
        .verify_selected_owner(ORG_ID, ADMIN_ID, MEMBER_OWNER)
        .unwrap();
    assert_eq!(
        store
            .set_account_preferences(&admin_id, Some(Some("UTC")), Some(Some(ORG_ID)))
            .unwrap(),
        (Some("UTC".into()), Some(ORG_ID.into()))
    );

    let invite = store
        .issue_org_invite(ORG_ID, MEMBER_BOB, now + 40)
        .unwrap();
    assert_eq!(store.list_org_access(ORG_ID, now + 40).unwrap().len(), 2);
    let bob_id = AccountId::parse(BOB_ID).unwrap();
    let bob_new = NewAccount {
        account_id: bob_id.clone(),
        username: Username::new("Bob").unwrap(),
        verifier: Verifier::new(PHC_1.into()).unwrap(),
    };
    assert_eq!(
        store
            .redeem_invite_register(&invite.token, &bob_new, now + 41)
            .unwrap(),
        InviteRegistration::Registered {
            account_id: bob_id.clone(),
            org_id: ORG_ID.into(),
            member_id: MEMBER_BOB.into(),
        }
    );
    let invite_2 = store
        .issue_org_invite(ORG_ID_2, MEMBER_BOB_2, now + 42)
        .unwrap();
    assert_eq!(
        store
            .redeem_invite_attach(&invite_2.token, &bob_id, now + 43)
            .unwrap(),
        InviteAttach::Attached {
            org_id: ORG_ID_2.into(),
            member_id: MEMBER_BOB_2.into(),
        }
    );

    let reset_link = store
        .issue_member_password_reset_link(ORG_ID, MEMBER_BOB, now + 44)
        .unwrap();
    assert!(matches!(
        store
            .redeem_set_password(
                &reset_link.token,
                &Verifier::new(PHC_2.into()).unwrap(),
                now + 45
            )
            .unwrap(),
        SetPasswordOutcome::Updated { ref login, .. } if login == "Bob"
    ));
    assert!(store
        .change_own_password(
            &bob_id,
            "keep-none",
            &Verifier::new(PHC_1.into()).unwrap(),
            now + 46,
        )
        .unwrap()
        .is_empty());
    let (admin_login, _admin_reset) = store
        .issue_admin_password_reset_link(now + 47)
        .unwrap()
        .expect("admin reset link");
    assert_eq!(admin_login, "Alice");

    assert!(matches!(
        store
            .handover_org_owner(ORG_ID, &admin_id, MEMBER_BOB)
            .unwrap(),
        OrgOwnerHandoverOutcome::Transferred { .. }
    ));
    store
        .restore_org_owner(ORG_ID, &admin_id, MEMBER_OWNER)
        .unwrap();
    assert_eq!(
        store.handover_administrator(&admin_id, &bob_id).unwrap(),
        AdminHandoverOutcome::Transferred
    );
    assert_eq!(
        store.handover_administrator(&bob_id, &admin_id).unwrap(),
        AdminHandoverOutcome::Transferred
    );
    assert!(matches!(
        store
            .remove_org_access(ORG_ID, MEMBER_BOB, now + 48)
            .unwrap(),
        RemoveAccessOutcome::Removed { .. }
    ));

    // 4. Journal, lockouts, provisioning, recovery reconciliation, and web TLS.
    struct ExternalRestartOp<'a> {
        operation_id: &'a str,
        office_id: &'a str,
    }
    impl Journaled for ExternalRestartOp<'_> {
        const METHOD: &'static str = "office.restart";
        fn operation_id(&self) -> &str {
            self.operation_id
        }
        fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
            serde_json::to_string(&json!({ "office_id": self.office_id }))
        }
    }
    let restart_op = ExternalRestartOp {
        operation_id: OP_ID,
        office_id: OFFICE_ID,
    };
    let attempt = JournalAttempt::new(&restart_op, ADMIN_ID).unwrap();
    assert!(matches!(
        store.journal_lookup(&attempt, now + 50).unwrap(),
        Lookup::Vacant
    ));
    let stored = HostOpResult::Completed {
        result: json!({"ok": true}),
    };
    assert!(matches!(
        store
            .journal_commit(
                &attempt,
                now + 50,
                Effect {
                    stored: stored.clone(),
                    verifier: None,
                },
            )
            .unwrap(),
        Write::Committed(_)
    ));
    assert!(matches!(
        store.journal_lookup(&attempt, now + 51).unwrap(),
        Lookup::Replay(ref s) if s == &stored
    ));
    // Refusal-code validation uses `org_host::ports::host_store::{ApiError, ErrorCode}`.
    let refusal_op = ExternalRestartOp {
        operation_id: OP_ID_3,
        office_id: OFFICE_ID,
    };
    let refusal_attempt = JournalAttempt::new(&refusal_op, ADMIN_ID).unwrap();
    assert!(matches!(
        store.journal_commit(
            &refusal_attempt,
            now + 52,
            Effect {
                stored: HostOpResult::Rejected {
                    refusal: ApiError::new(ErrorCode::NotAuthorized, "not stored"),
                },
                verifier: None,
            },
        ),
        Err(HostStoreError::Unsafe)
    ));
    assert!(matches!(
        store
            .journal_commit(
                &refusal_attempt,
                now + 52,
                Effect {
                    stored: HostOpResult::Rejected {
                        refusal: ApiError::new(ErrorCode::InvalidName, "bad name"),
                    },
                    verifier: None,
                },
            )
            .unwrap(),
        Write::Committed(_)
    ));

    assert!(!store
        .login_lockout(Socket::Local, "alice", 1000)
        .unwrap()
        .is_locked());
    store
        .record_login(Socket::Local, "alice", 1000, Outcome::Failure)
        .unwrap();
    assert_eq!(store.prune_login_lockouts(1000 + 1_800_000).unwrap(), 1);

    let reservation = Reservation {
        operation_id: OP_ID.into(),
        account: ADMIN_ID.into(),
        fingerprint: [0x21; 32],
        prepared: PreparedV1 {
            plan: PlanV1 {
                version: 1,
                name: "Acme".into(),
                slug: "acme".into(),
                repository_remote_url: "https://example.test/acme.git".into(),
                state_branch: "org/state".into(),
            },
            organization_id: ORG_ID.into(),
            office_id: OFFICE_ID.into(),
            repository_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c5".into(),
            home_team_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c6".into(),
            hr_employee_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c7".into(),
            creator: CreatorV1::from_login(MEMBER_OWNER.into(), "Alice"),
        },
        owner_member: MEMBER_OWNER.into(),
        recovery_handle: format!("recovery.{OP_ID}"),
        key_fingerprint: None,
        linked: false,
    };
    store.reserve_provisioning(&admin, &reservation).unwrap();
    assert_eq!(
        store.provisioning_taken("Acme", "other"),
        Ok(Some(Taken::Name))
    );
    store.provision_key_stored(&reservation, "fp-1").unwrap();
    store.provision_linked(&reservation).unwrap();
    assert!(store.provisioning_offices().unwrap().contains(OFFICE_ID));

    let recovery_name = Name::parse(&reservation.recovery_handle).unwrap();
    secrets
        .put(
            &recovery_name,
            &Value::new(b"staged-recovery".to_vec()).unwrap(),
        )
        .unwrap();
    let public = PublicRecord {
        version: 1,
        instance_id: INSTANCE_ID.into(),
        office_id: OFFICE_ID.into(),
        organization_id: ORG_ID.into(),
        operation_id: OP_ID.into(),
        escrow_version: 1,
        public_key: "04".to_owned() + &"07".repeat(64),
        package_sha256: "aa".repeat(32),
        package_hex: "00".into(),
    };
    let prepared = store.recovery_prepare(&public).unwrap();
    let delivered = store
        .recovery_delivered(&prepared, ADMIN_ID, false, now + 60)
        .unwrap();
    let _ack_pending = store
        .recovery_ack_intent(&delivered, ADMIN_ID, now + 61)
        .unwrap();
    store.recovery_reconcile(&*secrets).unwrap();
    assert!(secrets.load(&recovery_name).unwrap().is_none());
    let acked = store.recovery_observed(OP_ID).unwrap();
    assert_eq!(acked.state, RecoveryState::Acknowledged);
    let status = store.recovery_status(&acked, 0).unwrap();
    assert_eq!(status.delivery_ordinal, "1");

    let pair = StoredPair {
        source: Source::Supplied,
        cert_pem: "pem".into(),
        pair_fp: [0xee; 32],
        set_at_ms: now + 70,
    };
    let tls_attempt = JournalAttempt {
        id: OP_ID_2,
        kind: "tls.set",
        fingerprint: [0x33; 32],
        account: ADMIN_ID,
    };
    assert_eq!(
        store.tls_set_lookup(&tls_attempt, now + 70),
        Ok(TlsLookup::Vacant)
    );
    let rejected = TlsStored::Rejected {
        refusal: TlsRefusal::TlsKeyMismatch,
    };
    assert_eq!(
        store.tls_set_write(&tls_attempt, now + 70, &rejected, Some(&pair)),
        Ok(None)
    );
    assert_eq!(store.web_tls_pair(), Ok(Some(pair)));

    let prune_at = id_time(OP_ID).unwrap() + 30 * DAY;
    assert_eq!(
        store.journal_prune_expired(prune_at).unwrap(),
        PruneOutcome {
            removed: 2,
            more_remaining: false,
        }
    );
    assert_eq!(
        store.journal_prune_expired(prune_at).unwrap(),
        PruneOutcome {
            removed: 0,
            more_remaining: false,
        }
    );
}

#[test]
fn external_host_store_local_authless_lifecycle() {
    let now = 1_700_000_000_000_i64;
    let store: Arc<dyn HostStore> = Arc::new(ExternalHostStore::new(INSTANCE_ID, now));
    assert_eq!(store.auth_enabled(), Ok(true));
    let id = store
        .init_local_authless_if_empty(now)
        .unwrap()
        .expect("created local authless account");
    assert_eq!(store.local_authless_account(), Ok(Some(id.clone())));
    assert_eq!(store.auth_enabled(), Ok(false));
    assert_eq!(
        store
            .enable_local_auth(
                &Username::new("Owner").unwrap(),
                &Verifier::new(PHC_1.into()).unwrap(),
                now + 5,
            )
            .unwrap(),
        id
    );
    assert_eq!(store.local_authless_account(), Ok(None));
    assert_eq!(store.auth_enabled(), Ok(true));
}
