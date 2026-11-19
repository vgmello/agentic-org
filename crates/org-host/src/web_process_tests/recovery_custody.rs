//! Recovery-custody release through the actual registered host, ported from the
//! deleted reverse-bootstrap proof (`bootstrap/recovery_tests.rs`). A private
//! cfg(test) presence adapter stands in for the OS prompt; this proves no
//! physical authentication or production signature.
//!
//! It proves: presence runs with the registry connection unlocked and other
//! connections still served; no confirmation without a local foreground
//! context; concurrent prompts are refused, not queued; a waiting prompt is
//! cancelled by its connection's EOF; release, redelivery, acknowledgement and
//! the package follow the persisted ledger; and acknowledgement deletes the
//! custody copy.
use super::{answer, create_hq, start_signed_in, Started, PASSWORD};
use crate::{hostdb::HostDb, recovery::TestPresence, service::listener::Connection};
use org_host_control::{ops, recovery as dto, SessionHandle};
use org_proto::{ApiError, Call, ErrorCode};
use std::{
    io::BufReader,
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Presence modes: 0 not local, 1 local but refused, 2 confirmed,
/// 3 wait until `finish` or cancellation.
struct Scripted {
    db: Arc<HostDb>,
    mode: AtomicUsize,
    calls: AtomicUsize,
    ready: Mutex<Option<mpsc::Sender<()>>>,
    finish: AtomicBool,
    /// Set when a mode-3 prompt observed `cancelled()`, so a test can tell a
    /// real cancellation from the 5 s deadline (both refuse the release).
    cancelled_seen: AtomicBool,
}
impl TestPresence for Scripted {
    fn local(&self, _: &Connection) -> bool {
        self.mode.load(Ordering::SeqCst) != 0
    }
    fn confirm(&self, cancelled: &dyn Fn() -> bool) -> Result<(), ()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(
            self.db.recovery_db_unlocked_for_test(),
            "registry connection held during presence"
        );
        if self.mode.load(Ordering::SeqCst) == 3 {
            if let Some(ready) = self.ready.lock().unwrap().take() {
                ready.send(()).unwrap();
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.finish.load(Ordering::SeqCst) && Instant::now() < deadline {
                if cancelled() {
                    self.cancelled_seen.store(true, Ordering::SeqCst);
                    return Err(());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            return Err(());
        }
        if self.mode.load(Ordering::SeqCst) == 1 || cancelled() {
            Err(())
        } else {
            Ok(())
        }
    }
}

fn call<C: Call>(
    wire: &mut BufReader<UnixStream>,
    id: &mut u64,
    p: &C::Params,
) -> Result<C::Result, ApiError> {
    let current = *id;
    *id += 1;
    answer::<C>(wire, current, p)
}
fn copy(s: &SessionHandle) -> SessionHandle {
    SessionHandle::new(s.expose().into()).unwrap()
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual worker listeners require permitted local bind"]
fn actual_registered_host_releases_recovery_custody_only_after_unlocked_presence() {
    // `_temp` is bound first so it drops last, after the host and its children.
    let Started {
        temp: _temp,
        address,
        host,
        socket,
        mut wire,
        session,
        ..
    } = start_signed_in();
    let wire = &mut wire;
    let mut next = 3;
    let id = &mut next;
    let org = create_hq(&host, address);
    let recovery = host.service.recovery_for_test();
    let (db, custody) = host
        .service
        .authentication
        .as_ref()
        .expect("registered host authentication")
        .provisioning_storage();
    let instance_id = host.instance.as_ref().unwrap().id().to_owned();
    let adapter = Arc::new(Scripted {
        db: Arc::clone(&db),
        mode: AtomicUsize::new(0),
        calls: AtomicUsize::new(0),
        ready: Mutex::new(None),
        finish: AtomicBool::new(false),
        cancelled_seen: AtomicBool::new(false),
    });
    let show = |redeliver| ops::RecoveryReleaseParams {
        session: copy(&session),
        org_id: org.organization_id.clone(),
        redeliver,
    };
    let ack = || ops::RecoveryAcknowledgeParams {
        session: copy(&session),
        org_id: org.organization_id.clone(),
    };
    let status = || dto::StatusParams {
        session: copy(&session),
        org_id: org.organization_id.clone(),
        after_ordinal: None,
    };
    recovery.with_test_presence(adapter.clone(), || {
        // No foreground context: refuse before even invoking the presence callback.
        assert_eq!(
            call::<ops::RecoveryRelease>(wire, id, &show(false))
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);
        adapter.mode.store(1, Ordering::SeqCst);
        assert_eq!(
            call::<ops::RecoveryRelease>(wire, id, &show(false))
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
        let waiting = call::<dto::Status>(wire, id, &status()).unwrap();
        assert_eq!(waiting.delivery_ordinal, "0");
        assert!(waiting.audit.is_empty());
        // A second actual native sign-in remains usable while the first presence
        // prompt is waiting; concurrent prompts are refused, never queued.
        let mut other = BufReader::new(UnixStream::connect(&socket).unwrap());
        let mut other_id = 1;
        call::<org_proto::Hello>(
            &mut other,
            &mut other_id,
            &org_proto::HelloParams::default(),
        )
        .unwrap();
        let signed = call::<ops::AccountVerify>(
            &mut other,
            &mut other_id,
            &ops::AccountVerifyParams {
                login: "Owner".into(),
                password: org_host_control::Password::new(PASSWORD.into()).unwrap(),
            },
        )
        .unwrap();
        adapter.mode.store(3, Ordering::SeqCst);
        let (ready, started) = mpsc::channel();
        *adapter.ready.lock().unwrap() = Some(ready);
        let params = show(false);
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| call::<ops::RecoveryRelease>(wire, id, &params));
            started.recv_timeout(Duration::from_secs(3)).unwrap();
            let other_params = ops::RecoveryReleaseParams {
                session: copy(&signed.session),
                org_id: org.organization_id.clone(),
                redeliver: false,
            };
            assert_eq!(
                call::<ops::RecoveryRelease>(&mut other, &mut other_id, &other_params)
                    .unwrap_err()
                    .code,
                ErrorCode::Unavailable
            );
            // The registry is readable from another connection mid-prompt.
            let status = call::<dto::Status>(
                &mut other,
                &mut other_id,
                &dto::StatusParams {
                    session: copy(&signed.session),
                    org_id: org.organization_id.clone(),
                    after_ordinal: None,
                },
            )
            .unwrap();
            assert_eq!(status.delivery_ordinal, "0");
            adapter.finish.store(true, Ordering::SeqCst);
            assert_eq!(
                waiting.join().unwrap().unwrap_err().code,
                ErrorCode::NotAuthorized
            );
        });
        // The waiting connection's EOF cancels its prompt.
        adapter.finish.store(false, Ordering::SeqCst);
        adapter.cancelled_seen.store(false, Ordering::SeqCst);
        let eof_started = Instant::now();
        let (ready, started) = mpsc::channel();
        *adapter.ready.lock().unwrap() = Some(ready);
        let eof = other.get_ref().try_clone().unwrap();
        let params = ops::RecoveryReleaseParams {
            session: copy(&signed.session),
            org_id: org.organization_id.clone(),
            redeliver: false,
        };
        std::thread::scope(|scope| {
            let waiting =
                scope.spawn(|| call::<ops::RecoveryRelease>(&mut other, &mut other_id, &params));
            started.recv_timeout(Duration::from_secs(3)).unwrap();
            eof.shutdown(std::net::Shutdown::Write).unwrap();
            assert_eq!(
                waiting.join().unwrap().unwrap_err().code,
                ErrorCode::NotAuthorized
            );
        });
        assert!(
            adapter.cancelled_seen.load(Ordering::SeqCst),
            "the prompt ended by its deadline, not by the connection's EOF"
        );
        assert!(eof_started.elapsed() < Duration::from_secs(4));
        drop(other);
        drop(eof);
        assert_eq!(
            call::<dto::Status>(wire, id, &status())
                .unwrap()
                .delivery_ordinal,
            "0"
        );
        // Confirmed: released once, then only by explicit redelivery.
        adapter.mode.store(2, Ordering::SeqCst);
        let first = call::<ops::RecoveryRelease>(wire, id, &show(false)).unwrap();
        let shown = call::<dto::Status>(wire, id, &status()).unwrap();
        assert_eq!(shown.state, dto::DeliveryState::Delivered);
        assert_eq!(shown.delivery_ordinal, "1");
        let calls = adapter.calls.load(Ordering::SeqCst);
        assert_eq!(
            call::<ops::RecoveryRelease>(wire, id, &show(false))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(adapter.calls.load(Ordering::SeqCst), calls);
        // Another release attempt discarded the old same-run ACK permission.
        assert_eq!(
            call::<ops::RecoveryAcknowledge>(wire, id, &ack())
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        let second = call::<ops::RecoveryRelease>(wire, id, &show(true)).unwrap();
        assert!(first.code.expose() == second.code.expose());
        // An invalid session cannot borrow the permission, and consumes it.
        let wrong = ops::RecoveryAcknowledgeParams {
            session: SessionHandle::new("unknown-private-fixture-session".into()).unwrap(),
            org_id: org.organization_id.clone(),
        };
        assert_eq!(
            call::<ops::RecoveryAcknowledge>(wire, id, &wrong)
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(
            call::<ops::RecoveryAcknowledge>(wire, id, &ack())
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        let third = call::<ops::RecoveryRelease>(wire, id, &show(true)).unwrap();
        assert!(first.code.expose() == third.code.expose());
        let package_params = || dto::PackageParams {
            session: copy(&session),
            org_id: org.organization_id.clone(),
        };
        let package = call::<dto::Package>(wire, id, &package_params()).unwrap();
        assert_eq!(package.instance_id, instance_id);
        assert_eq!(package.operation_id, org.operation_id);
        assert_eq!(package.office_id, org.office_id);
        assert!(package.package_hex.starts_with("41474f5247434131"));
        let before = db.recovery_observed(&org.operation_id).unwrap();
        assert_eq!(before.ordinal, 3);
        // Acknowledged: idempotent, final, and the custody copy is gone.
        let _: org_proto::Empty = call::<ops::RecoveryAcknowledge>(wire, id, &ack()).unwrap();
        let done = call::<dto::Status>(wire, id, &status()).unwrap();
        assert_eq!(done.state, dto::DeliveryState::Acknowledged);
        assert_eq!(done.audit.len(), 5);
        let _: org_proto::Empty = call::<ops::RecoveryAcknowledge>(wire, id, &ack()).unwrap();
        let again = call::<dto::Status>(wire, id, &status()).unwrap();
        assert_eq!(again, done);
        let calls = adapter.calls.load(Ordering::SeqCst);
        assert_eq!(
            call::<ops::RecoveryRelease>(wire, id, &show(true))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(adapter.calls.load(Ordering::SeqCst), calls);
        let after = call::<dto::Package>(wire, id, &package_params()).unwrap();
        assert_eq!(after, package);
        let source = db.recovery_source(&org.organization_id).unwrap();
        assert!(custody
            .load(&crate::host_secrets::Name::parse(&source.recovery_handle).unwrap())
            .unwrap()
            .is_none());
    });
}
