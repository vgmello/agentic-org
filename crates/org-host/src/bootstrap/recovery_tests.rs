//! Actual host/worker orchestration with a private cfg(test) presence adapter.
//! This deliberately proves no physical authentication or production signature.
use crate::{
    bootstrap::{BootstrapHost, BootstrapService},
    hostdb::HostDb,
    recovery::TestPresence,
    service::listener::Connection,
};
use org_host_control::{ops, recovery as dto, SessionHandle};
use org_proto::{frame, ApiError, Call, ErrorCode, Request, Response};
use std::{
    io::BufReader,
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
struct Scripted {
    state: Arc<BootstrapService>,
    db: Arc<HostDb>,
    mode: AtomicUsize,
    calls: AtomicUsize,
    ready: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
    finish: std::sync::atomic::AtomicBool,
}
impl TestPresence for Scripted {
    fn local(&self, _: &Connection) -> bool {
        self.mode.load(Ordering::SeqCst) != 0
    }
    fn confirm(&self, cancelled: &dyn Fn() -> bool) -> Result<(), ()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(
            self.state.state.try_lock().is_ok(),
            "global bootstrap controller held during presence"
        );
        assert!(
            self.db.recovery_db_unlocked_for_test(),
            "DB held during presence"
        );
        if self.mode.load(Ordering::SeqCst) == 3 {
            if let Some(ready) = self.ready.lock().unwrap().take() {
                ready.send(()).unwrap();
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !self.finish.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                if cancelled() {
                    return Err(());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
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
    frame::write_message(wire.get_mut(), &Request::new::<C>(current, p).unwrap()).unwrap();
    let response: Response = frame::read_message(wire).unwrap().unwrap();
    assert_eq!(response.id, current);
    response.into_result::<C>().map_err(|e| match e {
        org_proto::CallError::Api(e) => e,
        _ => panic!("typed recovery response invalid"),
    })
}
fn session(s: &SessionHandle) -> SessionHandle {
    SessionHandle::new(s.expose().into()).unwrap()
}
pub(super) fn exercise(
    host: &BootstrapHost,
    wire: &mut BufReader<UnixStream>,
    p: &org_host_control::provision::Params,
    org: &org_host_control::provision::Provisioned,
    id: &mut u64,
    password: &str,
) {
    let (recovery, db) = {
        let state = host.state.state.lock().unwrap();
        (
            state.host.as_ref().unwrap().service.recovery_for_test(),
            host.state.database(&state).unwrap().unwrap(),
        )
    };
    let adapter = Arc::new(Scripted {
        state: Arc::clone(&host.state),
        db: Arc::clone(&db),
        mode: AtomicUsize::new(0),
        calls: AtomicUsize::new(0),
        ready: std::sync::Mutex::new(None),
        finish: std::sync::atomic::AtomicBool::new(false),
    });
    let show = |redeliver| ops::RecoveryReleaseParams {
        session: session(&p.session),
        org_id: org.organization_id.clone(),
        redeliver,
    };
    let ack = || ops::RecoveryAcknowledgeParams {
        session: session(&p.session),
        org_id: org.organization_id.clone(),
    };
    let status = || dto::StatusParams {
        session: session(&p.session),
        org_id: org.organization_id.clone(),
        after_ordinal: None,
    };
    recovery.with_test_presence(adapter.clone(), || {
        // No foreground context: refuse before even invoking the test presence callback.
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
        let waiting = call::<dto::Status>(wire, id, &status()).unwrap();
        assert_eq!(waiting.delivery_ordinal, "0");
        assert!(waiting.audit.is_empty());
        // A second actual native sign-in remains usable while the first OS
        // adapter is waiting; concurrent prompts are refused, never queued.
        let native_path = {
            let state = host.state.state.lock().unwrap();
            state.host.as_ref().unwrap().socket_path().to_owned()
        };
        let mut other = BufReader::new(UnixStream::connect(native_path).unwrap());
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
                password: org_host_control::Password::new(password.into()).unwrap(),
            },
        )
        .unwrap();
        adapter.mode.store(3, Ordering::SeqCst);
        let (ready, started) = std::sync::mpsc::channel();
        *adapter.ready.lock().unwrap() = Some(ready);
        let params = show(false);
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| call::<ops::RecoveryRelease>(wire, id, &params));
            started
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
            let other_params = ops::RecoveryReleaseParams {
                session: session(&signed.session),
                org_id: org.organization_id.clone(),
                redeliver: false,
            };
            assert_eq!(
                call::<ops::RecoveryRelease>(&mut other, &mut other_id, &other_params)
                    .unwrap_err()
                    .code,
                ErrorCode::Unavailable
            );
            let status = call::<dto::Status>(
                &mut other,
                &mut other_id,
                &dto::StatusParams {
                    session: session(&signed.session),
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
        adapter.finish.store(false, Ordering::SeqCst);
        let (ready, started) = std::sync::mpsc::channel();
        *adapter.ready.lock().unwrap() = Some(ready);
        let eof = other.get_ref().try_clone().unwrap();
        let params = ops::RecoveryReleaseParams {
            session: session(&signed.session),
            org_id: org.organization_id.clone(),
            redeliver: false,
        };
        std::thread::scope(|scope| {
            let waiting =
                scope.spawn(|| call::<ops::RecoveryRelease>(&mut other, &mut other_id, &params));
            started
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
            eof.shutdown(std::net::Shutdown::Write).unwrap();
            assert_eq!(
                waiting.join().unwrap().unwrap_err().code,
                ErrorCode::NotAuthorized
            );
        });
        drop(other);
        drop(eof);
        assert_eq!(
            call::<dto::Status>(wire, id, &status())
                .unwrap()
                .delivery_ordinal,
            "0"
        );
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
        let package = call::<dto::Package>(
            wire,
            id,
            &dto::PackageParams {
                session: session(&p.session),
                org_id: org.organization_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(package.instance_id, host.state.id);
        assert_eq!(package.operation_id, p.operation_id);
        assert_eq!(package.office_id, org.office_id);
        assert!(package.package_hex.starts_with("41474f5247434131"));
        let before = db.recovery_observed(&p.operation_id).unwrap();
        assert_eq!(before.ordinal, 3);
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
        let after = call::<dto::Package>(
            wire,
            id,
            &dto::PackageParams {
                session: session(&p.session),
                org_id: org.organization_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(after, package);
        let source = db.recovery_source(&org.organization_id).unwrap();
        let (_, custody) = {
            let state = host.state.state.lock().unwrap();
            state
                .host
                .as_ref()
                .unwrap()
                .service
                .authentication
                .as_ref()
                .unwrap()
                .provisioning_storage()
        };
        assert!(custody
            .load(&crate::host_secrets::Name::parse(&source.recovery_handle).unwrap())
            .unwrap()
            .is_none());
    });
}
