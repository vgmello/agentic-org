use super::*;

#[test]
fn revocations_are_exact_idempotent_retained_and_batch_overflow_is_atomic() {
    let mut state = Revocations::default();
    state.apply_batch(vec![]).unwrap();
    assert!(state.0.is_empty());
    state
        .apply_batch(vec![
            "opaque:id".into(),
            "opaque:id".into(),
            "OPAQUE:id".into(),
        ])
        .unwrap();
    assert_eq!(state.0.len(), 2);
    assert!(state.0.contains("opaque:id") && state.0.contains("OPAQUE:id"));
    let rest: Vec<_> = (2..4095).map(|n| format!("id-{n}")).collect();
    state.apply_batch(rest).unwrap();
    assert_eq!(state.0.len(), 4095);
    state.apply_batch(vec!["id-4095".into()]).unwrap();
    assert_eq!(state.0.len(), 4096);
    let before = state.0.clone();
    assert!(state.apply_batch(vec!["adjacent-new".into()]).is_err());
    assert_eq!(state.0, before, "4097 must refuse atomically");
    state
        .apply_batch(
            before
                .iter()
                .cloned()
                .chain(before.iter().cloned())
                .collect(),
        )
        .unwrap();
    assert_eq!(state.0, before, "duplicates must not consume capacity");
    assert!(state
        .apply_batch(vec![
            "new-prefix".into(),
            "opaque:id".into(),
            "new-tail".into()
        ])
        .is_err());
    assert_eq!(
        state.0, before,
        "overflow must retain every old ID and apply no prefix"
    );
    state.apply_batch(vec![]).unwrap();
    assert_eq!(state.0, before, "no eviction or expiry before worker exit");
}

#[test]
fn actual_transfer_applies_fifo_records_then_refuses_atomic_overflow_without_a_prefix() {
    use org_office_launch::v3::{self, TransferRecord};
    use std::os::unix::net::UnixDatagram;
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    let state = Arc::new(Mutex::new(Revocations::default()));
    state
        .lock()
        .unwrap()
        .apply_batch((0..4094).map(|n| format!("id-{n}")).collect())
        .unwrap();
    let (host, child) = UnixDatagram::pair().unwrap();
    for ids in [
        vec!["first".into(), "id-1".into(), "first".into()],
        vec!["second".into()],
        vec![
            "overflow-prefix".into(),
            "second".into(),
            "overflow-tail".into(),
        ],
        vec!["unreachable-after-overflow".into()],
    ] {
        let body =
            v3::encode_transfer_record(&TransferRecord::Revoked { session_ids: ids }).unwrap();
        host.send(&body).unwrap();
    }
    host.send(b"terminal-invalid").unwrap();
    let transport = crate::startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(child),
        fence: Arc::default(),
    };
    let result = super::super::run(
        transport,
        Arc::clone(&state),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(result.is_err());
    let observed = &state.lock().unwrap().0;
    assert_eq!(observed.len(), 4096);
    assert!(observed.contains("first") && observed.contains("second"));
    assert!(observed.contains("id-0") && observed.contains("id-4093"));
    assert!(
        !observed.contains("overflow-prefix")
            && !observed.contains("overflow-tail")
            && !observed.contains("unreachable-after-overflow")
    );
}

#[test]
fn stream_handoff_with_concatenated_records_refuses_without_applying_a_prefix() {
    use org_office_launch::v3::{self, TransferRecord};
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::{UnixDatagram, UnixStream};
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    let (mut host, child) = UnixStream::pair().unwrap();
    let mut body = Vec::new();
    for id in ["prefix", "tail"] {
        body.extend(
            v3::encode_transfer_record(&TransferRecord::Revoked {
                session_ids: vec![id.into()],
            })
            .unwrap(),
        );
    }
    // Deliberately violate the trusted-host datagram precondition. Queue both
    // records before receiving; EOF bounds the test if only a prefix is decoded.
    host.write_all(&body).unwrap();
    host.shutdown(std::net::Shutdown::Write).unwrap();
    let state = Arc::new(Mutex::new(Revocations::default()));
    let transport = crate::startup::Transfer {
        receiver: org_socket::rights::DatagramReceiver::new(UnixDatagram::from(OwnedFd::from(
            child,
        ))),
        fence: Arc::default(),
    };
    let result = super::super::run(
        transport,
        Arc::clone(&state),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(result.is_err());
    assert!(
        state.lock().unwrap().0.is_empty(),
        "a concatenated stream applied a prefix"
    );
}
