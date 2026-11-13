use super::*;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;

const WAIT: Duration = Duration::from_secs(10);

fn connected() -> (tempfile::TempDir, Client, UnixStream) {
    let dir = tempfile::Builder::new().prefix("cl").tempdir().unwrap();
    let path = dir.path().join("s");
    let listener = UnixListener::bind(&path).unwrap();
    let client = Client::connect(&path).unwrap();
    let (peer, _) = listener.accept().unwrap();
    (dir, client, peer)
}

#[test]
fn dropping_a_client_with_a_backpressured_writer_shuts_down_before_peer_eof() {
    let (_dir, client, mut peer) = connected();
    let small: libc::c_int = 4096;
    // SAFETY: the client owns this live socket; the buffer is a correctly sized int.
    assert_eq!(
        unsafe {
            libc::setsockopt(
                client.shared.stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&small as *const libc::c_int).cast(),
                std::mem::size_of_val(&small) as _,
            )
        },
        0
    );
    let line = vec![b'x'; 8 * 1024 * 1024];
    let total = line.len();
    let state = Arc::new(AtomicU8::new(QUEUED));
    client.shared.queued_bytes.store(total, Ordering::SeqCst);
    client
        .outbound
        .as_ref()
        .unwrap()
        .send(Outgoing {
            line,
            deadline: Instant::now() + WAIT,
            state: Arc::clone(&state),
        })
        .unwrap();
    // Bytes reached the real peer, which deliberately has not drained the small socket.
    assert!(org_socket::poll_readable(peer.as_raw_fd(), WAIT).unwrap());
    assert_eq!(state.load(Ordering::SeqCst), WRITING);
    peer.set_nonblocking(true).unwrap();
    let start = Instant::now();
    drop(client);
    let until = start + Duration::from_secs(2);
    let mut received = 0;
    let mut buffer = [0; 4096];
    loop {
        let remaining = until.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "client drop did not produce bounded EOF"
        );
        match peer.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => received += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                assert!(org_socket::poll_readable(peer.as_raw_fd(), remaining).unwrap());
            }
            other => panic!("peer did not reach EOF after client drop: {other:?}"),
        }
    }
    assert!(
        received > 0 && received < total,
        "writer was not backpressured"
    );
}

fn delayed_helper(
    client: &mut Client,
    writer: bool,
) -> (
    mpsc::Sender<()>,
    std::sync::Weak<UnixStream>,
    mpsc::Receiver<()>,
) {
    client.close_and_join(Instant::now() + WAIT).unwrap();
    let socket = Arc::new(client.shared.stream.try_clone().unwrap());
    let weak = Arc::downgrade(&socket);
    let (entered, started) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (completed, completion) = mpsc::channel();
    let name = if writer {
        "org-client-writer"
    } else {
        "org-client-reader"
    };
    let helper = Helper::spawn(name, move || {
        entered.send(()).unwrap();
        let outcome = released.recv_timeout(WAIT);
        drop(socket);
        completed.send(()).unwrap();
        outcome.unwrap();
    })
    .unwrap();
    if writer {
        client._writer = Some(helper);
    } else {
        client.reader = Some(helper);
    }
    started.recv_timeout(WAIT).unwrap();
    (release, weak, completion)
}

struct ObservedFence<'a> {
    _guard: std::sync::MutexGuard<'a, ()>,
    helper: std::sync::Weak<UnixStream>,
    released: mpsc::Sender<bool>,
}
impl Drop for ObservedFence<'_> {
    fn drop(&mut self) {
        let _ = self.released.send(self.helper.upgrade().is_none());
    }
}

fn timeout_retains_helper_until_explicit_retry(writer: bool) {
    let case_lock = Mutex::new(());
    let case_guard = case_lock.lock().unwrap();
    let (_dir, mut client, _peer) = connected();
    let (release, socket, completion) = delayed_helper(&mut client, writer);
    let (fence_released, fence_observed) = mpsc::channel();
    let fence = ObservedFence {
        _guard: case_guard,
        helper: socket.clone(),
        released: fence_released,
    };
    let outcome = client.close_and_join(Instant::now());
    let retained = socket.upgrade().is_some();
    let handle_retained = if writer {
        client._writer.is_some()
    } else {
        client.reader.is_some()
    };
    let _ = release.send(());
    completion.recv_timeout(WAIT).unwrap();
    let retry = client.close_and_join(Instant::now() + WAIT);
    let joined = client._writer.is_none() && client.reader.is_none();
    // Even a join-omission counterfactual must clean up before its assertion fails.
    for helper in [client._writer.take(), client.reader.take()]
        .into_iter()
        .flatten()
    {
        helper.join().unwrap();
    }
    let error = outcome.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    let name = if writer {
        "org-client-writer"
    } else {
        "org-client-reader"
    };
    assert!(error.to_string().contains(name), "wrong helper: {error}");
    assert!(retained && handle_retained, "timeout lost helper ownership");
    retry.unwrap();
    assert!(
        socket.upgrade().is_none(),
        "helper descriptor survived the explicit fence"
    );
    assert!(joined);
    client.close_and_join(Instant::now()).unwrap();
    drop(client);
    drop(fence);
    assert!(
        fence_observed.recv_timeout(WAIT).unwrap(),
        "case fence released while the helper still owned its descriptor"
    );
    assert!(case_lock.try_lock().is_ok());
}

#[test]
fn close_and_join_names_a_delayed_writer_and_retains_it_until_retry() {
    timeout_retains_helper_until_explicit_retry(true);
}

#[test]
fn close_and_join_names_a_delayed_reader_and_retains_it_until_retry() {
    timeout_retains_helper_until_explicit_retry(false);
}

#[test]
fn explicit_teardown_joins_idle_and_half_closed_connections() {
    for half_closed in [false, true] {
        let (_dir, mut client, peer) = connected();
        if half_closed {
            peer.shutdown(std::net::Shutdown::Write).unwrap();
        }
        client.close_and_join(Instant::now() + WAIT).unwrap();
        assert!(client._writer.is_none() && client.reader.is_none());
        assert_eq!(Arc::strong_count(&client.shared), 1);
    }
}

#[test]
fn explicit_teardown_reports_helper_panics_instead_of_claiming_a_clean_fence() {
    for writer in [false, true] {
        let (_dir, mut client, _peer) = connected();
        client.close_and_join(Instant::now() + WAIT).unwrap();
        let name = if writer {
            "org-client-writer"
        } else {
            "org-client-reader"
        };
        let helper = Helper::spawn(name, || panic!("controlled helper panic")).unwrap();
        if writer {
            client._writer = Some(helper);
        } else {
            client.reader = Some(helper);
        }
        let error = client.close_and_join(Instant::now() + WAIT).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Other);
        assert!(error.to_string().contains(name) && error.to_string().contains("panicked"));
        client.close_and_join(Instant::now() + WAIT).unwrap();
    }
}

#[test]
fn drop_policy_stays_bounded_and_does_not_prove_delayed_helper_completion() {
    for writer in [false, true] {
        let (_dir, mut client, _peer) = connected();
        let (release, socket, completion) = delayed_helper(&mut client, writer);
        let start = Instant::now();
        drop(client);
        let elapsed = start.elapsed();
        let retained = socket.upgrade().is_some();
        release.send(()).unwrap();
        // The descriptor's last owner must finish before this test reports its result.
        completion.recv_timeout(WAIT).unwrap();
        assert!(socket.upgrade().is_none());
        assert!(
            elapsed < Duration::from_secs(2),
            "Drop policy changed: {elapsed:?}"
        );
        assert!(
            retained,
            "fixture did not reproduce the missing explicit fence"
        );
    }
}

#[test]
fn retained_completion_receivers_preserve_client_send_and_sync() {
    fn assert_shareable<T: Send + Sync>() {}
    assert_shareable::<Client>();
}

#[test]
fn explicit_teardown_attempts_both_helpers_and_reports_the_first_error() {
    for writer_timeout in [false, true] {
        let (_dir, mut client, _peer) = connected();
        client.close_and_join(Instant::now() + WAIT).unwrap();
        let (release, released) = mpsc::channel();
        client._writer = Some(
            Helper::spawn("org-client-writer", move || {
                if writer_timeout {
                    released.recv_timeout(WAIT).unwrap();
                } else {
                    panic!("controlled writer panic");
                }
            })
            .unwrap(),
        );
        let reader =
            Helper::spawn("org-client-reader", || panic!("controlled reader panic")).unwrap();
        // Its completion is already available even when the common deadline expires.
        let until = Instant::now() + WAIT;
        while !reader.is_finished() {
            assert!(Instant::now() < until, "controlled reader did not finish");
            std::thread::yield_now();
        }
        client.reader = Some(reader);
        let deadline = if writer_timeout {
            Instant::now()
        } else {
            Instant::now() + WAIT
        };
        let outcome = client.close_and_join(deadline);
        let reader_joined = client.reader.is_none();
        let writer_retained = client._writer.is_some();
        let _ = release.send(());
        // Clean up even when the counterfactual short-circuits after the writer's error.
        let _ = client.close_and_join(Instant::now() + WAIT);
        for helper in [client._writer.take(), client.reader.take()]
            .into_iter()
            .flatten()
        {
            let _ = helper.join();
        }
        let error = outcome.unwrap_err();
        assert_eq!(
            error.kind(),
            if writer_timeout {
                ErrorKind::TimedOut
            } else {
                ErrorKind::Other
            }
        );
        assert!(error.to_string().contains("org-client-writer"));
        assert!(
            reader_joined,
            "writer error prevented the completed reader's join"
        );
        assert!(
            !error.to_string().contains("reader not joined"),
            "joined reader was reported as unfinished"
        );
        assert_eq!(writer_retained, writer_timeout);
    }
}

#[test]
fn writer_failure_reports_a_reader_that_could_not_be_joined() {
    for writer_timeout in [false, true] {
        let (_dir, mut client, _peer) = connected();
        client.close_and_join(Instant::now() + WAIT).unwrap();
        let (release_writer, writer_released) = mpsc::channel();
        let writer = Helper::spawn("org-client-writer", move || {
            if writer_timeout {
                writer_released.recv_timeout(WAIT).unwrap();
            } else {
                panic!("controlled writer panic");
            }
        })
        .unwrap();
        if !writer_timeout {
            let until = Instant::now() + WAIT;
            while !writer.is_finished() {
                assert!(Instant::now() < until, "controlled writer did not finish");
                std::thread::yield_now();
            }
        }
        client._writer = Some(writer);
        let (release_reader, reader_released) = mpsc::channel();
        client.reader = Some(
            Helper::spawn("org-client-reader", move || {
                reader_released.recv_timeout(WAIT).unwrap();
            })
            .unwrap(),
        );
        let started = Instant::now();
        let outcome = client.close_and_join(started);
        let elapsed = started.elapsed();
        let reader_retained = client.reader.is_some();
        let _ = release_writer.send(());
        release_reader.send(()).unwrap();
        client.close_and_join(Instant::now() + WAIT).unwrap();
        let error = outcome.unwrap_err();
        assert_eq!(
            error.kind(),
            if writer_timeout {
                ErrorKind::TimedOut
            } else {
                ErrorKind::Other
            }
        );
        assert!(error.to_string().contains("org-client-writer"));
        assert!(
            error.to_string().contains("reader not joined"),
            "missing unfinished-reader diagnostic: {error}"
        );
        assert!(reader_retained);
        assert!(client._writer.is_none() && client.reader.is_none());
        assert!(
            elapsed < Duration::from_secs(2),
            "teardown ignored its expired deadline"
        );
    }
}
