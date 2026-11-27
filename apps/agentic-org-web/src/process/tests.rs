use super::*;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::sync::Mutex;

/// After `tls.renew`: a pair for the same origin and `hsts` is swapped in; `unavailable`
/// or a pair that does not load keeps the current one; another listener is a host bug
/// and the process exits.
#[test]
fn a_renewal_swaps_only_a_pair_for_the_same_listener() {
    use org_host_control::listen::{Hsts, ListenFailure, ListenOrigin, Uncovered};
    use org_host_control::TlsPrivateKey;
    let origin = ListenOrigin::parse("https://org.example:8443").unwrap();
    let current = (origin.clone(), Hsts::Off);
    let cert = include_bytes!("../../tests/fixtures/test-cert-a.der");
    let key = include_bytes!("../../tests/fixtures/test-key-a.der");
    let pem = |label: &str, der: &[u8]| {
        use base64::prelude::*;
        format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            BASE64_STANDARD.encode(der)
        )
    };
    let https = |origin: &ListenOrigin, hsts, key_pem: String| TlsFetchResult::Https {
        cert: pem("CERTIFICATE", cert),
        key: TlsPrivateKey::new(key_pem).unwrap(),
        origin: origin.clone(),
        hsts,
    };
    assert!(matches!(
        renewal(&current, https(&origin, Hsts::Off, pem("PRIVATE KEY", key))),
        Renewal::Swap(chain, _) if chain.len() == 1
    ));
    assert!(matches!(
        renewal(&current, https(&origin, Hsts::Off, "not a key".into())),
        Renewal::Keep("unusable_pair")
    ));
    assert!(matches!(
        renewal(
            &current,
            TlsFetchResult::Unavailable(ListenFailure::TlsNameMismatch {
                uncovered: Uncovered::one(origin.clone())
            })
        ),
        Renewal::Keep("unavailable")
    ));
    // The one line a kept pair writes: the host's prefix, a known reason, nothing else.
    let line = renewal_kept_line("unusable_pair");
    assert_eq!(
        line,
        "agentic-org-web: tls renewal kept the current pair: unusable_pair\n"
    );
    let elsewhere = ListenOrigin::parse("https://elsewhere.example:8443").unwrap();
    for changed in [
        https(&origin, Hsts::On, pem("PRIVATE KEY", key)),
        https(&elsewhere, Hsts::Off, pem("PRIVATE KEY", key)),
        TlsFetchResult::HttpLoopback {
            origin: ListenOrigin::parse("http://127.0.0.1:8443").unwrap(),
            hsts: Hsts::Off,
        },
    ] {
        assert!(matches!(renewal(&current, changed), Renewal::Exit));
    }
}

/// `tls.renew` goes to the renewal thread and nothing else; every other event still
/// reaches the session sink.
#[test]
fn tls_renew_is_passed_on_and_other_events_reach_the_sessions() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    let sessions: EventSink = Arc::new(move |name: &str, _: &serde_json::Value| {
        recorded.lock().unwrap().push(name.to_owned())
    });
    let (renewed, renewals) = mpsc::channel();
    let events = with_renewals(sessions, renewed);
    events(org_host_control::event::TLS_RENEW, &serde_json::json!({}));
    events("session.ended", &serde_json::json!({}));
    assert!(renewals.try_recv().is_ok());
    assert!(renewals.try_recv().is_err());
    assert_eq!(*seen.lock().unwrap(), ["session.ended"]);
}

#[test]
fn a_confinement_refusal_is_reported_with_its_reason_on_one_bounded_line() {
    let refusal = io::Error::other(
        "gateway confinement unavailable: seccomp failed with EPERM (Operation not \
             permitted (os error 1))",
    );
    assert_eq!(
        startup_failure_line(&confinement_refused(refusal)),
        "agentic-org-web: startup refused: confinement: gateway confinement unavailable: \
             seccomp failed with EPERM (Operation not permitted (os error 1))"
    );
    // Control characters, line breaks and anything outside ASCII never reach the
    // line, and a long reason is cut.
    let forged = confinement_refused(io::Error::other(format!(
        "a\nagentic-org-web: startup refused: b\r\t\u{1b}[31m\u{202e}é{}",
        "x".repeat(400)
    )));
    let line = startup_failure_line(&forged);
    let reason = line.strip_prefix(REFUSAL_PREFIX).unwrap();
    assert_eq!(reason.len(), REFUSAL_REASON_BYTES);
    assert!(
        reason.bytes().all(|b| (0x20..=0x7e).contains(&b)),
        "{reason}"
    );
    assert!(reason.starts_with("a?agentic-org-web: startup refused: b???[31m??xxx"));
    assert_eq!(
        startup_failure_line(&confinement_refused(io::Error::other(""))),
        format!("{REFUSAL_PREFIX}no reason given")
    );
}

#[test]
fn the_release_root_is_the_images_directory_or_that_of_the_pinned_path() {
    let release = tempfile::tempdir().unwrap();
    let program = release.path().join("agentic-org-web");
    std::fs::write(&program, b"image").unwrap();
    let root = |image: io::Result<&Path>, started_as: Option<&Path>| {
        release_root(
            image.map(Path::to_owned),
            started_as.map(|path| path.as_os_str().to_owned()),
        )
    };
    // Started from its path: the image's own file decides, whatever the argument.
    assert_eq!(root(Ok(&program), None).unwrap(), release.path());
    assert_eq!(
        root(Ok(&program), Some(Path::new("/elsewhere/agentic-org-web"))).unwrap(),
        release.path()
    );
    // Started from a sealed copy in memory: the system names no file, and the
    // pinned path the host started it by names the release.
    let sealed = Path::new("/memfd:agentic-org-web (deleted)");
    assert_eq!(root(Ok(sealed), Some(&program)).unwrap(), release.path());
    assert_eq!(
        root(Err(io::Error::other("no image")), Some(&program)).unwrap(),
        release.path()
    );
    // Never a relative path, a missing file, a directory or nothing.
    for started_as in [
        None,
        Some(Path::new("agentic-org-web")),
        Some(Path::new("./agentic-org-web")),
        Some(release.path().join("missing").as_path()),
        Some(release.path()),
    ] {
        let error = root(Ok(sealed), started_as).unwrap_err();
        assert_eq!(error.to_string(), "missing release root", "{started_as:?}");
    }
}

#[test]
fn a_landlock_layer_that_was_not_applied_is_one_bounded_printable_line() {
    assert_eq!(
        landlock_not_applied_line(
            "landlock_create_ruleset failed with EPERM: the Landlock version query was refused"
        ),
        "agentic-org-web: confinement: landlock not applied: landlock_create_ruleset \
             failed with EPERM: the Landlock version query was refused"
    );
    // Whatever the reason is, the line is one line of printable ASCII, and its
    // reason is at most as long as a refusal's and never empty.
    let forged = landlock_not_applied_line(&format!(
        "a\nagentic-org-web: startup refused: confinement: b\r\t\u{1b}[31m\u{202e}é{}",
        "x".repeat(400)
    ));
    let reason = forged.strip_prefix(LANDLOCK_NOT_APPLIED_PREFIX).unwrap();
    assert_eq!(reason.len(), REFUSAL_REASON_BYTES);
    assert!(reason.starts_with("a?agentic-org-web: startup refused: confinement: b??"));
    assert!(reason.bytes().all(|byte| (0x20..=0x7e).contains(&byte)));
    assert_eq!(
        landlock_not_applied_line(""),
        format!("{LANDLOCK_NOT_APPLIED_PREFIX}no reason given")
    );
    // It is not a refusal line, and a refusal line is not one of these.
    assert!(!LANDLOCK_NOT_APPLIED_PREFIX.starts_with(REFUSAL_PREFIX));
    assert!(!REFUSAL_PREFIX.starts_with(LANDLOCK_NOT_APPLIED_PREFIX));
}

/// X5 review, fourth round (V-c): a UI build that cannot be loaded says which check it
/// failed, by the check's fixed name alone, so the host's log can say why the process
/// ended before it connected.
#[test]
fn a_ui_build_that_cannot_be_loaded_is_reported_by_its_check() {
    use crate::assets::BuildError;
    for (refused, kind) in [
        (BuildError::InvalidKey, "InvalidKey"),
        (BuildError::UnreadableBuild, "UnreadableBuild"),
        (BuildError::UnauthenticatedRelease, "UnauthenticatedRelease"),
    ] {
        assert_eq!(
            startup_failure_line(&io::Error::other(BuildRefused(refused))),
            format!("agentic-org-web: startup refused: ui build: {kind}")
        );
    }
    assert!(!BUILD_REFUSAL_PREFIX.starts_with(REFUSAL_PREFIX));
}

#[test]
fn every_other_failed_start_is_one_fixed_line() {
    for error in [
        failure(),
        io::Error::other("host hello failed"),
        io::Error::from_raw_os_error(2),
        io::Error::other(format!("{REFUSAL_PREFIX}forged")),
    ] {
        assert_eq!(
            startup_failure_line(&error),
            "agentic-org-web: authenticated startup unavailable"
        );
    }
    assert_eq!(
        REFUSAL_PREFIX,
        "agentic-org-web: startup refused: confinement: "
    );
    assert_eq!(REFUSAL_REASON_BYTES, 240);
}
#[test]
fn launch_input_is_one_strict_bounded_complete_frame() {
    let valid = br#"{"v":1,"rendezvous":"/private/tmp/r","instance_id":"INSTANCE"}"#;
    let mut frame = valid.to_vec();
    frame.push(b'\n');
    assert_eq!(
        first_line(&mut std::io::Cursor::new(&frame))
            .unwrap()
            .instance_id,
        "INSTANCE"
    );
    for source in [
        r#"{"v":1,"v":1,"rendezvous":"/r","instance_id":"INSTANCE"}"#,
        r#"{"v":1,"rendezvous":"/r","rendez\u0076ous":"/r","instance_id":"INSTANCE"}"#,
        r#"{"v":1,"rendezvous":"/r","instance_id":"INSTANCE","extra":true}"#,
        r#"{"v":2,"rendezvous":"/r","instance_id":"INSTANCE"}"#,
        r#"{"v":1,"rendezvous":"relative","instance_id":"INSTANCE"}"#,
        r#"{"v":1,"rendezvous":"/r\u0000","instance_id":"INSTANCE"}"#,
        r#"{"v":1,"rendezvous":"/r","instance_id":"../other"}"#,
    ] {
        assert!(first_line(&mut std::io::Cursor::new(format!("{source}\n"))).is_err());
    }
    assert!(
        first_line(&mut std::io::Cursor::new(valid)).is_err(),
        "EOF is not a full frame"
    );
    assert!(first_line(&mut std::io::Cursor::new(vec![b' '; HEADER + 1])).is_err());
}

/// A host answering hello for `INSTANCE` until its connection ends.
fn connected() -> (Arc<HostControl>, UnixStream) {
    const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let (web, peer) = UnixStream::pair().unwrap();
    let stop = peer.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut reader = io::BufReader::new(peer);
        while let Ok(Some(request)) =
            org_proto::frame::read_message::<serde_json::Value>(&mut reader)
        {
            let result =
                org_proto::HelloResult::host("test", "disposable").with_instance_id(INSTANCE);
            let reply = serde_json::json!({"id":request["id"],"result":result});
            if org_proto::frame::write_message(reader.get_mut(), &reply).is_err() {
                break;
            }
        }
    });
    let control = HostControl::from_handoff(
        web.into(),
        transfer(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
        Arc::new(|_: &str, _: &serde_json::Value| {}),
    )
    .unwrap();
    (control, stop)
}
/// Whether serving's shutdown future resolves within two seconds.
fn ends_serving(host: &Arc<HostControl>) -> bool {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(2), host_lost(Arc::clone(host)))
                .await
                .is_ok()
        })
}

#[test]
fn the_host_closing_stdin_ends_serving() {
    let (host, _control) = connected();
    let (input, from_host) = UnixStream::pair().unwrap();
    watch_stdin(input, Arc::clone(&host)).unwrap();
    // Later lines are ignored; the open pipe keeps the host connection.
    (&from_host).write_all(b"{\"v\":1}\n").unwrap();
    assert!(!ends_serving(&host), "an open stdin is not a lost host");
    drop(from_host);
    assert!(ends_serving(&host), "stdin EOF is a lost host");
    assert!(host.fetch_listener().is_err());
}

#[test]
fn control_eof_ends_serving() {
    let (host, control) = connected();
    assert!(
        !ends_serving(&host),
        "a live control connection keeps serving"
    );
    control.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(ends_serving(&host), "control EOF is a lost host");
}

fn transfer() -> OwnedFd {
    UnixDatagram::pair().unwrap().0.into()
}
fn listener() -> OwnedFd {
    TcpListener::bind("127.0.0.1:0").unwrap().into()
}

#[test]
fn handoff_decodes_exactly_the_transfer_then_the_listener() {
    let (transfer, listener) = descriptors(HOST_SOCK_MARKER, vec![transfer(), listener()]).unwrap();
    assert!(listener.local_addr().unwrap().ip().is_loopback());
    assert!(transfer.local_addr().is_ok());
}

#[test]
fn handoff_refuses_any_other_descriptor_count_order_or_marker() {
    for fds in [
        vec![],
        vec![transfer()],
        vec![listener()],
        vec![transfer(), listener(), transfer()],
        vec![listener(), transfer()],
        vec![transfer(), transfer()],
    ] {
        assert!(descriptors(HOST_SOCK_MARKER, fds).is_err());
    }
    assert!(descriptors(HOST_SOCK_MARKER ^ 1, vec![transfer(), listener()]).is_err());
}
