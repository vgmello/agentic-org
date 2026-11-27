use super::*;

/// A caller identified by its image: the audit token `byte` repeated.
fn image(byte: u8) -> Identity {
    Identity::from_macos_audit_token([byte; 32])
}

/// A caller the system names by its user alone: `uid`, and some PID.
fn user(uid: u32) -> Identity {
    Identity::from_linux_credentials(uid, 4242)
}

struct FakePeer {
    sent_after_accept: bool,
    nothing_after: bool,
    accepted: Option<Identity>,
    current: Option<Identity>,
    /// After this many reads of the current token, it changes (another process wrote).
    swap_after: Option<usize>,
    reads: std::cell::Cell<usize>,
    signed: bool,
    #[cfg(debug_assertions)]
    uid: u32,
    #[cfg(debug_assertions)]
    executable: PathBuf,
}

impl Peer for FakePeer {
    fn sent_after_accept(&self) -> bool {
        self.sent_after_accept
    }
    fn nothing_after_request(&self) -> bool {
        self.nothing_after
    }
    fn accepted_identity(&self) -> Option<Identity> {
        self.accepted
    }
    fn current_identity(&self) -> Option<Identity> {
        let reads = self.reads.get() + 1;
        self.reads.set(reads);
        match self.swap_after {
            Some(after) if reads > after => Some(image(9)),
            _ => self.current,
        }
    }
    fn satisfies(&self, _identity: &Identity, _requirement: &str) -> bool {
        self.signed
    }
    #[cfg(debug_assertions)]
    fn uid(&self) -> Option<u32> {
        Some(self.uid)
    }
    #[cfg(debug_assertions)]
    fn executable(&self) -> Option<PathBuf> {
        Some(self.executable.clone())
    }
}

/// A caller that's still the process accepted (the same audit token), sending after the
/// accept: this user, running this test binary.
fn caller(signed: bool) -> FakePeer {
    FakePeer {
        sent_after_accept: true,
        nothing_after: true,
        accepted: Some(image(7)),
        current: Some(image(7)),
        swap_after: None,
        reads: std::cell::Cell::new(0),
        signed,
        #[cfg(debug_assertions)]
        uid: crate::home::euid(),
        #[cfg(debug_assertions)]
        executable: std::env::current_exe().unwrap(),
    }
}

#[test]
fn the_requirement_names_the_bundle_and_team() {
    assert_eq!(
        designated_requirement(APP_BUNDLE_ID, "AB12CD34EF").unwrap(),
        "identifier \"com.vgmello.agentic-org\" and anchor apple generic and \
             certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
             certificate leaf[field.1.2.840.113635.100.6.1.13] exists and \
             certificate leaf[subject.OU] = \"AB12CD34EF\""
    );
    for team in ["", "ab12cd34ef", "AB12CD34E", "AB12CD34EFG", "AB12CD34\"F"] {
        assert!(
            designated_requirement(APP_BUNDLE_ID, team).is_err(),
            "{team}"
        );
    }
    for bundle in ["", ".com.x", "com.x\" or anchor apple", "com x", "com.é"] {
        assert!(
            designated_requirement(bundle, "AB12CD34EF").is_err(),
            "{bundle}"
        );
    }
}

/// As a release build is set up: no team id and no development app. Nobody passes, not even
/// a caller whose code would satisfy any requirement.
#[test]
fn with_no_team_id_and_no_development_app_everyone_is_refused() {
    let policy = AppPolicy::new(APP_BUNDLE_ID, None).unwrap();
    assert!(!policy.allows(&caller(true)));
}

#[test]
fn a_signed_caller_passes() {
    let policy = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
    assert!(policy.allows(&caller(true)));
    assert!(!policy.allows(&caller(false)));
}

/// The exec swap: the caller's audit token changed since its connection was accepted (it
/// `exec`ed the signed app after connecting). Refused, though its code now satisfies the
/// requirement and it is the development app.
#[test]
fn a_caller_whose_audit_token_changed_since_accept_is_refused() {
    let mut swapped = caller(true);
    swapped.current = Some(image(8));
    let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
    assert!(!signed.allows(&swapped));
    #[cfg(debug_assertions)]
    {
        let me = std::env::current_exe().unwrap();
        let dev = AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(&me)
            .unwrap();
        assert!(!dev.allows(&swapped));
    }
    // And one whose token can't be read, then or now.
    let mut unknown = caller(true);
    unknown.current = None;
    assert!(!signed.allows(&unknown));
    unknown.accepted = None;
    assert!(!signed.allows(&unknown));
}

/// Another process touches the connection after the decision was taken: the caller's audit
/// token changes between the decision and the answer. Refused by the final re-check.
#[test]
fn a_caller_whose_audit_token_changes_after_the_decision_is_refused() {
    #[cfg_attr(
        not(debug_assertions),
        allow(
            unused_mut,
            reason = "release builds have no development app policy to append"
        )
    )]
    let mut policies = vec![(
        AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap(),
        Grant::Signed,
    )];
    #[cfg(debug_assertions)]
    {
        let me = std::env::current_exe().unwrap();
        policies.push((
            AppPolicy::new(APP_BUNDLE_ID, None)
                .unwrap()
                .with_dev_app(&me)
                .unwrap(),
            Grant::Development,
        ));
    }
    for (policy, expected) in policies {
        // How many times the decision alone reads the current token.
        let unchanged = caller(true);
        assert!(policy.decide(&unchanged).is_some());
        assert_eq!(policy.grant(&caller(true)), Some(expected));
        let decision_reads = unchanged.reads.get();
        // The token changes right after those reads: the decision still says yes...
        let mut late = caller(true);
        late.swap_after = Some(decision_reads);
        assert!(policy.decide(&late).is_some());
        // ...and the whole check says no (so the development fallback, which logs only
        // for a grant, never logs this call as allowed)...
        let mut late = caller(true);
        late.swap_after = Some(decision_reads);
        assert_eq!(policy.grant(&late), None);
        let mut late = caller(true);
        late.swap_after = Some(decision_reads);
        assert!(!policy.allows(&late));
        assert!(policy.allows(&caller(true)));
    }
}

/// A request already buffered when the connection was accepted: refused, though everything
/// else checks out (it may have been written by the image the process `exec`ed away from).
#[test]
fn a_request_buffered_before_the_accept_is_refused() {
    let mut early = caller(true);
    early.sent_after_accept = false;
    let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
    assert!(!signed.allows(&early));
    #[cfg(debug_assertions)]
    {
        let me = std::env::current_exe().unwrap();
        let dev = AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(&me)
            .unwrap();
        assert!(!dev.allows(&early));
    }
}

/// Anything that arrived after the request (an extra byte, another line), however it got
/// there: refused, though everything else checks out.
#[test]
fn a_request_with_anything_after_it_is_refused() {
    let mut followed = caller(true);
    followed.nothing_after = false;
    let signed = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
    assert!(!signed.allows(&followed));
    #[cfg(debug_assertions)]
    {
        let me = std::env::current_exe().unwrap();
        let dev = AppPolicy::new(APP_BUNDLE_ID, None)
            .unwrap()
            .with_dev_app(&me)
            .unwrap();
        assert!(!dev.allows(&followed));
    }
}

/// A caller the system names by its user alone (it has no code identity for a native
/// client) is admitted exactly when that user is this host's own, in every build, with or
/// without a team id; the checks of the request's place in the stream still apply.
#[test]
fn a_caller_named_by_its_user_alone_passes_only_as_this_hosts_user() {
    let by_user = |uid: u32| {
        let mut peer = caller(false);
        peer.accepted = Some(user(uid));
        peer.current = Some(user(uid));
        peer
    };
    let me = crate::home::euid();
    for policy in [
        AppPolicy::new(APP_BUNDLE_ID, None).unwrap(),
        AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap(),
    ] {
        assert_eq!(policy.grant(&by_user(me)), Some(Grant::User));
        assert!(policy.allows(&by_user(me)));
        // Another user is refused, root among them: nothing admits root here.
        for other in [me.wrapping_add(1), 0] {
            if other != me {
                assert!(!policy.allows(&by_user(other)), "{other}");
            }
        }
        // The stream checks are the same as for a signed caller.
        let mut early = by_user(me);
        early.sent_after_accept = false;
        assert!(!policy.allows(&early));
        let mut followed = by_user(me);
        followed.nothing_after = false;
        assert!(!policy.allows(&followed));
        // Not the connection that was accepted any more.
        let mut other_process = by_user(me);
        other_process.current = Some(Identity::from_linux_credentials(me, 4243));
        assert!(!policy.allows(&other_process));
        let mut unknown = by_user(me);
        unknown.current = None;
        assert!(!policy.allows(&unknown));
    }
    // A caller identified by its image is never admitted for its user.
    assert!(!is_this_user(&image(7)));
    assert!(is_this_user(&user(me)));
}

/// Whether this system names a native client by its user alone, as the platform's
/// admission says (it has no code signature to identify one by). Each test below that
/// differs by system expects exactly the outcome this names.
fn named_by_user_alone() -> bool {
    use org_platform_host::admission::ImageIdentity;
    match crate::test_programs_tests::image_identity() {
        ImageIdentity::VerifiedBytes => true,
        ImageIdentity::CodeSignature => false,
    }
}

/// What the host logs. Where a native client is named by its user alone it says so, says
/// what that admission alone gives (the initial registration link, to whoever names the
/// setup operation) and misses no team id. Elsewhere it says none of that, and a missing
/// team id is a warning.
#[test]
fn the_description_says_how_this_system_admits_a_native_client() {
    let lines = AppPolicy::new(APP_BUNDLE_ID, None).unwrap().describe();
    let admitted: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("admits a native client run by this user"))
        .collect();
    let warned = lines.iter().any(|line| line.starts_with("WARNING"));
    assert_eq!(names_peers_by_user(), named_by_user_alone());
    if named_by_user_alone() {
        assert_eq!(admitted.len(), 1, "{lines:?}");
        let line = admitted[0];
        assert!(
            line.contains("initial administrator registration link")
                && line.contains("names the setup operation")
                && line.contains("with no session"),
            "{line}"
        );
        // It does not claim that admission gives nothing.
        assert!(!line.contains("grants nothing"), "{line}");
        assert!(!warned, "{lines:?}");
    } else {
        assert!(admitted.is_empty(), "{lines:?}");
        assert!(warned, "{lines:?}");
    }
}

/// `nothing_after_request` on a real socket: true only while nothing waits past the
/// request, in the reader's buffer or on the socket.
#[test]
fn a_socket_peer_sees_bytes_after_the_request() {
    use std::io::Write;
    let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
    let peer = |stream, buffered_after| SocketPeer {
        stream,
        accepted: crate::peer::Accepted {
            identity: None,
            buffered: 0,
        },
        received: crate::peer::Received {
            sent_at: 0,
            buffered_after,
        },
    };
    assert!(peer(&a, 0).nothing_after_request());
    assert!(!peer(&a, 1).nothing_after_request());
    (&b).write_all(b"x").unwrap();
    assert!(!peer(&a, 0).nothing_after_request());
}

#[cfg(debug_assertions)]
#[test]
fn a_socket_peer_reports_the_effective_uid_after_shared_admission() {
    let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let peer = SocketPeer {
        stream: &stream,
        accepted: crate::peer::Accepted {
            identity: None,
            buffered: 0,
        },
        received: crate::peer::Received {
            sent_at: 0,
            buffered_after: 0,
        },
    };
    assert_eq!(crate::home::euid(), org_socket::effective_uid());
    assert_eq!(peer.uid(), Some(crate::home::euid()));
}

#[cfg(debug_assertions)]
#[test]
fn a_socket_peer_refuses_a_failed_credential_query() {
    let file = std::fs::File::open("/dev/null").unwrap();
    let stream = std::os::unix::net::UnixStream::from(std::os::fd::OwnedFd::from(file));
    let peer = SocketPeer {
        stream: &stream,
        accepted: crate::peer::Accepted {
            identity: None,
            buffered: 0,
        },
        received: crate::peer::Received {
            sent_at: 0,
            buffered_after: 0,
        },
    };
    assert_eq!(peer.uid(), None);
}

#[cfg(debug_assertions)]
#[test]
fn the_development_fallback_needs_the_same_user_and_path() {
    let me = std::env::current_exe().unwrap();
    let dev = AppPolicy::new(APP_BUNDLE_ID, None)
        .unwrap()
        .with_dev_app(&me)
        .unwrap();
    assert!(dev.allows(&caller(false)));
    let mut other_user = caller(false);
    other_user.uid = crate::home::euid().wrapping_add(1);
    assert!(!dev.allows(&other_user));
    let mut other_exe = caller(false);
    other_exe.executable = "/bin/sh".into();
    assert!(!dev.allows(&other_exe));
    assert!(AppPolicy::new(APP_BUNDLE_ID, None)
        .unwrap()
        .with_dev_app(Path::new("relative"))
        .is_err());
}

/// The development app isn't built yet: the host still starts, with the fallback off.
#[cfg(debug_assertions)]
#[test]
fn a_development_app_that_isnt_built_leaves_the_fallback_off() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("Org.app/Contents/MacOS/Org");
    let dev = AppPolicy::new(APP_BUNDLE_ID, None)
        .unwrap()
        .with_dev_app(&missing)
        .unwrap();
    assert!(dev.dev_app.is_none());
    assert!(!dev.describe().iter().any(|line| line.contains("FALLBACK")));
    let mut there = caller(false);
    there.executable = missing;
    assert!(!dev.allows(&there));
}

/// The app folder, or a file that can't be executed, instead of the app's executable: no
/// caller could ever match it, so the fallback stays off rather than being described as on.
#[cfg(debug_assertions)]
#[test]
fn a_development_app_folder_instead_of_its_executable_leaves_the_fallback_off() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("Org.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    let dev = AppPolicy::new(APP_BUNDLE_ID, None)
        .unwrap()
        .with_dev_app(&bundle)
        .unwrap();
    assert!(dev.dev_app.is_none());
    assert!(!dev.describe().iter().any(|line| line.contains("FALLBACK")));
    // A file inside it that can't be executed: off too.
    let plain = bundle.join("Contents/MacOS/Org");
    std::fs::write(&plain, b"not a program").unwrap();
    let dev = AppPolicy::new(APP_BUNDLE_ID, None)
        .unwrap()
        .with_dev_app(&plain)
        .unwrap();
    assert!(dev.dev_app.is_none());
    assert!(!dev.describe().iter().any(|line| line.contains("FALLBACK")));
}

/// This test process's own identity, through the policy, as the host takes it. Its code
/// satisfies no requirement on any system (it is not the app). Where the system identifies
/// its image, it is refused; where the system names only its user, it is this host's own
/// user and is admitted for that. The Security-framework path itself, both ways, is tested
/// with the adapter (`org_platform_host::admission`).
#[test]
fn the_policy_judges_this_process_by_its_real_identity() {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let identity = crate::peer::identity(&a).unwrap();
    let requirement = designated_requirement(APP_BUNDLE_ID, "AB12CD34EF").unwrap();
    assert!(!code_signing::satisfies_hardened(&identity, &requirement));
    let peer = SocketPeer {
        stream: &a,
        accepted: crate::peer::Accepted {
            identity: Some(identity),
            buffered: 0,
        },
        received: crate::peer::Received {
            sent_at: 0,
            buffered_after: 0,
        },
    };
    let policy = AppPolicy::new(APP_BUNDLE_ID, Some("AB12CD34EF")).unwrap();
    // One outcome per kind of system, by the platform's fact (not by what the identity
    // under test happens to carry).
    if named_by_user_alone() {
        assert!(is_this_user(&identity));
        assert_eq!(policy.grant(&peer), Some(Grant::User));
        assert!(policy.allows(&peer));
    } else {
        assert_eq!(identity.uid, None);
        assert!(!is_this_user(&identity));
        assert_eq!(policy.grant(&peer), None);
        assert!(!policy.allows(&peer));
    }
}

/// A request counts as sent after the accept only if its line starts at or after the bytes
/// buffered then (N0).
#[test]
fn a_socket_peer_compares_the_request_offset_with_what_was_buffered() {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let peer = |sent_at| SocketPeer {
        stream: &a,
        accepted: crate::peer::Accepted {
            identity: None,
            buffered: 10,
        },
        received: crate::peer::Received {
            sent_at,
            buffered_after: 0,
        },
    };
    assert!(!peer(0).sent_after_accept());
    assert!(!peer(9).sent_after_accept());
    assert!(peer(10).sent_after_accept());
    assert!(peer(42).sent_after_accept());
}
