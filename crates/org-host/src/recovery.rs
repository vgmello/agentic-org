//! REC-DEL-1 native orchestration. The production constructor always uses the
//! SDK adapter; no DTO, environment option or development path supplies presence.
pub(crate) mod presence;
use crate::{
    app_auth::AppPolicy,
    broadcast::Broadcast,
    host_secrets::HostSecrets,
    hostdb::{
        recovery::{Ledger, State},
        HostDb,
    },
    offices::Offices,
    peer::Received,
    service::{authentication::Authentication, listener::Connection},
    supervisor::channels::Endpoint,
};
use org_host_control::{ops, recovery as dto, SessionHandle};
use org_office_launch::recovery::PublicRecord;
use org_proto::{ApiError, ErrorCode, Request};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Weak,
};

/// Owned by exactly one connection. Not Clone/Debug/Serialize, and never a
/// caller-visible capability. A dropped/revoked session invalidates it in place.
pub(crate) struct Permission {
    session: [u8; 32],
    live: Weak<AtomicBool>,
    account: String,
    ledger: Ledger,
}
pub(crate) struct Recovery {
    auth: Arc<Authentication>,
    db: Arc<HostDb>,
    custody: Arc<dyn HostSecrets>,
    offices: Arc<Offices>,
    broadcast: Arc<Broadcast>,
    instance: String,
    policy: AppPolicy,
    inflight: AtomicBool,
    #[cfg(test)]
    test_presence: std::sync::Mutex<Option<Arc<dyn TestPresence>>>,
}
struct Reservation<'a>(&'a AtomicBool);
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
fn denied() -> ApiError {
    ApiError::new(
        ErrorCode::NotAuthorized,
        "local native administrator confirmation required",
    )
}
fn unavailable() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "recovery state unresolved; inspect persisted status before retrying",
    )
}
fn storage(_: crate::hostdb::Error) -> ApiError {
    unavailable()
}
fn invalid() -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, "invalid recovery request")
}
fn wall() -> Result<i64, ApiError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|n| i64::try_from(n.as_millis()).ok())
        .ok_or_else(unavailable)
}
pub(crate) fn handles(request: &Request) -> bool {
    request.is::<ops::RecoveryRelease>()
        || request.is::<ops::RecoveryAcknowledge>()
        || request.is::<dto::Status>()
        || request.is::<dto::Package>()
}
impl Recovery {
    pub(crate) fn new(
        auth: Arc<Authentication>,
        offices: Arc<Offices>,
        broadcast: Arc<Broadcast>,
        instance: &str,
        policy: AppPolicy,
    ) -> Self {
        let (db, custody) = auth.provisioning_storage();
        Self {
            auth,
            db,
            custody,
            offices,
            broadcast,
            instance: instance.into(),
            policy,
            inflight: AtomicBool::new(false),
            #[cfg(test)]
            test_presence: std::sync::Mutex::new(None),
        }
    }
    fn local(&self, c: &Connection) -> bool {
        #[cfg(test)]
        {
            let adapter = self.test_presence.lock().unwrap().clone();
            if let Some(adapter) = adapter {
                return adapter.local(c);
            }
        }
        presence::local(c)
    }
    fn confirm(&self, cancelled: impl Fn() -> bool) -> Result<(), ()> {
        #[cfg(test)]
        {
            let adapter = self.test_presence.lock().unwrap().clone();
            if let Some(adapter) = adapter {
                return adapter.confirm(&cancelled);
            }
        }
        presence::confirm(cancelled)
    }
    /// Private disposable test seam. No request/environment/build-release option
    /// can select it; production always calls the actual SDK functions above.
    #[cfg(test)]
    pub(crate) fn with_test_presence<T>(
        &self,
        adapter: Arc<dyn TestPresence>,
        run: impl FnOnce() -> T,
    ) -> T {
        struct Reset<'a>(&'a std::sync::Mutex<Option<Arc<dyn TestPresence>>>);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                *self.0.lock().unwrap() = None;
            }
        }
        assert!(self
            .test_presence
            .lock()
            .unwrap()
            .replace(adapter)
            .is_none());
        let _reset = Reset(&self.test_presence);
        run()
    }
    fn native(&self, c: &Connection, received: Received) -> Result<(), ApiError> {
        if c.channel_kind() != org_office_launch::ChannelKind::Local || !c.recovery_peer_idle() {
            return Err(denied());
        }
        #[cfg(target_os = "macos")]
        if self.policy.allows(&crate::app_auth::SocketPeer {
            stream: c.socket(),
            accepted: c.accepted(),
            received,
        }) {
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (&self.policy, received);
        Err(denied())
    }
    fn authority(
        &self,
        c: &Connection,
        r: Received,
        session: &SessionHandle,
        local: bool,
    ) -> Result<String, ApiError> {
        self.native(c, r)?;
        let a = self.auth.authorize(c, session)?;
        if !a.admin() || (local && !self.local(c)) {
            return Err(denied());
        }
        Ok(a.account_id().into())
    }
    fn public(&self, org: &str) -> Result<(PublicRecord, Ledger, Arc<Endpoint>), ApiError> {
        let reservation = self.db.recovery_source(org).map_err(storage)?;
        let office = self
            .offices
            .offices()
            .into_iter()
            .find(|o| {
                o.id == reservation.prepared.office_id
                    && o.organization_id == org
                    && o.role == org_proto::OfficeRole::Hq
                    && o.status == org_proto::OfficeStatus::Active
            })
            .ok_or_else(unavailable)?;
        let endpoint = self.offices.endpoint(&office.id).ok_or_else(unavailable)?;
        let request = org_office_launch::recovery::Request {
            version: 1,
            instance_id: self.instance.clone(),
            office_id: office.id,
            organization_id: org.into(),
            operation_id: reservation.operation_id,
        };
        let public = endpoint
            .recovery_public(&request)
            .map_err(|_| unavailable())?;
        let ledger = self.db.recovery_prepare(&public).map_err(storage)?;
        Ok((public, ledger, endpoint))
    }
    fn publish(&self, org: &str) {
        self.broadcast.publish::<dto::Changed>(|| {
            let source = self.db.recovery_source(org).ok()?;
            // Read persisted facts again under broadcast ordering; never emit a
            // pre-read stale state after a later ACK/redelivery event.
            let status = self.db.recovery_observed(&source.operation_id).ok()?;
            Some(dto::ChangedData {
                version: 1,
                org_id: org.into(),
                recovery: status.state.into(),
                delivery_ordinal: status.ordinal.to_string(),
                first_shown_ms: status.first_shown_ms,
                last_redelivered_ms: status.last_redelivered_ms,
                redelivery_count: status.redelivery_count.to_string(),
                acknowledged_ms: status.acknowledged_ms,
            })
        });
    }
    pub(crate) fn status(
        &self,
        c: &Connection,
        r: Received,
        p: dto::StatusParams,
    ) -> Result<dto::StatusResult, ApiError> {
        self.authority(c, r, &p.session, false)?;
        let after = p.after_ordinal.as_deref().unwrap_or("0");
        if after.is_empty()
            || (after.len() > 1 && after.starts_with('0'))
            || !after.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(invalid());
        }
        let after = after.parse::<i64>().map_err(|_| invalid())?;
        let (_, ledger, _) = self.public(&p.org_id)?;
        self.authority(c, r, &p.session, false)?;
        self.db.recovery_status(&ledger, after).map_err(storage)
    }
    pub(crate) fn package(
        &self,
        c: &Connection,
        r: Received,
        p: dto::PackageParams,
    ) -> Result<dto::PackageResult, ApiError> {
        self.authority(c, r, &p.session, true)?;
        let (public, _, _) = self.public(&p.org_id)?;
        self.authority(c, r, &p.session, true)?;
        Ok(dto::PackageResult {
            version: public.version,
            instance_id: public.instance_id,
            office_id: public.office_id,
            organization_id: public.organization_id,
            operation_id: public.operation_id,
            escrow_version: public.escrow_version,
            public_key: public.public_key,
            package_sha256: public.package_sha256,
            package_hex: public.package_hex,
        })
    }
    pub(crate) fn release(
        &self,
        c: &Connection,
        r: Received,
        p: ops::RecoveryReleaseParams,
    ) -> Result<ops::RecoveryReleaseResult, ApiError> {
        c.clear_recovery_permission();
        let account = self.authority(c, r, &p.session, true)?;
        self.inflight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "recovery confirmation already in progress",
                )
            })?;
        let _reserved = Reservation(&self.inflight);
        let (public, ledger, endpoint) = self.public(&p.org_id)?;
        if !matches!(ledger.state, State::Pending | State::Delivered)
            || (ledger.state == State::Delivered && !p.redeliver)
        {
            return Err(ApiError::new(ErrorCode::Conflict,"recovery already shown or acknowledged; inspect persisted status; showing again requires explicit redeliver"));
        }
        let (session, live) = self.auth.recovery_session_binding(c, &p.session)?;
        self.confirm(|| {
            !c.recovery_peer_idle()
                || live.upgrade().is_none_or(|l| !l.load(Ordering::SeqCst))
                || self.auth.authorize(c, &p.session).is_err()
        })
        .map_err(|_| denied())?;
        if self.authority(c, r, &p.session, true)? != account {
            return Err(denied());
        }
        let (again, current, current_endpoint) = self.public(&p.org_id)?;
        if again != public || current != ledger || !Arc::ptr_eq(&endpoint, &current_endpoint) {
            return Err(unavailable());
        }
        let source = self.db.recovery_source(&p.org_id).map_err(storage)?;
        let code = crate::provisioning::pending_recovery(&*self.custody, &source)?;
        self.authority(c, r, &p.session, true)?;
        let delivered = self
            .db
            .recovery_delivered(&ledger, &account, p.redeliver, wall()?)
            .map_err(storage)?;
        self.publish(&p.org_id);
        // A post-commit authority/EOF failure leaves a durable Shown fact. It
        // cannot be reported as a cancelled/unseen delivery or undo that fact.
        if self.authority(c, r, &p.session, true)? != account
            || live.upgrade().is_none_or(|l| !l.load(Ordering::SeqCst))
        {
            return Err(denied());
        }
        *c.recovery_permission.lock().map_err(|_| unavailable())? = Some(Permission {
            session,
            live,
            account,
            ledger: delivered,
        });
        Ok(ops::RecoveryReleaseResult { code })
    }
    pub(crate) fn acknowledge(
        &self,
        c: &Connection,
        r: Received,
        p: ops::RecoveryAcknowledgeParams,
    ) -> Result<org_proto::Empty, ApiError> {
        let permission = c
            .recovery_permission
            .lock()
            .map_err(|_| unavailable())?
            .take();
        let account = self.authority(c, r, &p.session, true)?;
        let (_, ledger, _) = self.public(&p.org_id)?;
        if !matches!(ledger.state, State::Acknowledged | State::AckPending) {
            let permission = permission.ok_or_else(denied)?;
            let (session, live) = self.auth.recovery_session_binding(c, &p.session)?;
            if permission.account != account
                || permission.session != session
                || !permission.live.ptr_eq(&live)
                || live.upgrade().is_none_or(|l| !l.load(Ordering::SeqCst))
                || permission.ledger != ledger
            {
                return Err(denied());
            }
            self.authority(c, r, &p.session, true)?;
            self.db
                .recovery_ack_intent(&ledger, &account, wall()?)
                .map_err(storage)?;
            self.publish(&p.org_id);
        }
        self.authority(c, r, &p.session, true)?;
        self.db
            .recovery_reconcile(&*self.custody)
            .map_err(storage)?;
        self.publish(&p.org_id);
        Ok(org_proto::Empty {})
    }
}

#[cfg(test)]
pub(crate) trait TestPresence: Send + Sync {
    fn local(&self, connection: &Connection) -> bool;
    fn confirm(&self, cancelled: &dyn Fn() -> bool) -> Result<(), ()>;
}
