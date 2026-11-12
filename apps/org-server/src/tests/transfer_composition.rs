#![allow(unsafe_code)]
//! Run the real root composition in an isolated test child, then inspect cleanup
//! BEFORE that child's process exit can mask a forgotten service or database.
use super::super::*;
use org_office::{Key, NewOffice};
use org_office_launch::v3::{
    self, ChannelKind, Handoff, HandoffFd, HostFirstLine, OfficeLine, RendezvousEvent,
    TransferRecord,
};
use org_proto::{DbKey, GrantKey};
use std::io::{BufReader, Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::time::Duration;
const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const PROBE: &str = "KAI_ROOT_TRANSFER_TEST_CHILD";

fn isolated(case: &str, test: &str) {
    if std::env::var(PROBE).ok().as_deref() == Some(case) {
        let _spawn = transfer::tests::without_process_spawn();
        probe(case == "failure");
        return;
    }
    let output = transfer::tests::process_output(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(PROBE, case),
    )
    .unwrap();
    assert!(
        output.status.success(),
        "isolated root probe failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for line in String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| line.starts_with("measured root-host"))
    {
        eprintln!("{line}");
    }
    if case == "failure" {
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("org-server: cannot start transfer service\n"),
            "Service::start failure returned without the fixed diagnostic: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
fn root_return_joins_transfer_and_live_channels_before_process_exit() {
    isolated("normal", "tests::transfer_composition::root_return_joins_transfer_and_live_channels_before_process_exit");
}
#[test]
fn root_service_start_refusal_reports_fixed_diagnostic_and_releases_office() {
    isolated("failure", "tests::transfer_composition::root_service_start_refusal_reports_fixed_diagnostic_and_releases_office");
}

fn probe(failure: bool) {
    let dir = tempfile::Builder::new().prefix("kr").tempdir().unwrap();
    let folder = dir.path().join(ID);
    let runtime = dir.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    let office_socket = socket_path(&runtime, ID).unwrap();
    let info = NewOffice {
        office_id: ID.into(),
        office_name: "HQ".into(),
        role: org_office::OfficeRole::Hq,
        org_id: ID.into(),
        org_name: "Acme".into(),
    };
    let key = || Key::from_db_key(DbKey::from_bytes([4; 32]));
    let office = Office::create_for_instance(&folder, &info, key(), ID).unwrap();
    let path = dir.path().join("rv.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut input = tempfile::tempfile().unwrap();
    v3::write_first_line(
        &mut input,
        &HostFirstLine::Start(OfficeLine {
            instance_id: ID.into(),
            office_id: ID.into(),
            rendezvous: path.to_str().unwrap().into(),
        }),
    )
    .unwrap();
    input.rewind().unwrap();
    let stdout = tempfile::tempfile().unwrap();
    let mut control = Control::new(
        ControlPipe {
            from_host: input,
            to_host: stdout.try_clone().unwrap(),
        },
        Arc::default(),
    );
    let expected_socket = office_socket.clone();
    let host = std::thread::spawn(move || {
        assert!(org_socket::poll_readable(listener.as_raw_fd(), Duration::from_secs(5)).unwrap());
        let (mut stream, _) = listener.accept().unwrap();
        // Match the fake-host activation fixture: macOS accepted sockets may
        // retain the nonblocking listener flag. A socket timeout does not clear it.
        let before = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) };
        assert!(before >= 0);
        stream.set_nonblocking(false).unwrap();
        let after = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) };
        assert!(after >= 0);
        assert_eq!(after & libc::O_NONBLOCK, 0);
        eprintln!("measured root-host accepted flags: before={before} after={after}");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::with_capacity(1, stream.try_clone().unwrap());
        let ready: serde_json::Value = org_proto::frame::read_message(&mut reader)
            .unwrap()
            .unwrap();
        assert_eq!(ready["event"], "ready_for_secrets");
        let (worker, transfer) = UnixDatagram::pair().unwrap();
        let handoff = Handoff::Start {
            db_key: DbKey::from_bytes([4; 32]),
            grant_key: GrantKey::from_bytes([7; 32]),
            fds: vec![HandoffFd::Transfer],
        };
        stream
            .write_all(&v3::encode_handoff(&handoff).unwrap())
            .unwrap();
        pass_marker(&stream, worker.as_raw_fd());
        assert_eq!(
            v3::read_server_event(&mut reader).unwrap(),
            Some(RendezvousEvent::FdsReceived {
                of: "handoff".into()
            })
        );
        drop(worker);
        let Some(RendezvousEvent::Server(ServerEvent::Ready { socket, .. })) =
            v3::read_server_event(&mut reader).unwrap()
        else {
            panic!("host did not receive Ready");
        };
        assert_eq!(std::path::Path::new(&socket), expected_socket);
        if !failure {
            // The refusal path may already have unlinked after emitting Ready.
            // The normal host still holds the control connection open here.
            assert!(expected_socket.exists(), "Ready socket was never bound");
        }
        if failure {
            assert!(v3::read_server_event(&mut reader).unwrap().is_none());
            return (transfer, None);
        }
        let (source, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let header = v3::encode_transfer_record(&TransferRecord::Channel {
            kind: ChannelKind::Local,
            office_id: ID.into(),
            channel_id: "root-owned".into(),
        })
        .unwrap();
        transfer::tests::send_rights(&transfer, &header, &[source.as_raw_fd()]);
        assert_eq!(
            v3::read_server_event(&mut reader).unwrap(),
            Some(RendezvousEvent::FdsReceived {
                of: "root-owned".into()
            })
        );
        drop(source);
        transfer
            .send(
                &v3::encode_transfer_record(&TransferRecord::Revoked {
                    session_ids: vec!["queued-at-shutdown".into()],
                })
                .unwrap(),
            )
            .unwrap();
        stream.shutdown(std::net::Shutdown::Both).unwrap();
        // The JoinHandle's result retains the transfer peer until after the
        // root returns; macOS peer-close must not terminate this cleanup probe.
        (transfer, Some(peer))
    });
    assert!(matches!(
        control.read_for_office(FirstLine::Start, Some(ID)).unwrap(),
        Some(startup::Request::V3 { .. })
    ));
    let lifetime = control.lifetime().unwrap();
    if failure {
        transfer::tests::refuse_next_spawn();
    }
    let joining = if failure {
        None
    } else {
        let (entered, at_exit) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        transfer::tests::hold_service_exit(entered, released);
        let socket = office_socket.clone();
        Some(std::thread::spawn(move || {
            // The receiver exits only after Service::drop sets stop. Hold its
            // actual thread exit so the root must still be joining it here.
            at_exit.recv_timeout(Duration::from_secs(5)).unwrap();
            let still_bound = socket.exists();
            release.send(()).unwrap();
            assert!(
                still_bound,
                "root unlinked its socket before joining the transfer service"
            );
        }))
    };
    let code = serve_office(
        control,
        lifetime,
        &runtime,
        office,
        serve::LaunchAuth::Grants {
            _key: GrantKey::from_bytes([7; 32]),
        },
        Some(ID.into()),
    );
    if let Some(joining) = joining {
        joining.join().unwrap();
    }
    let (_host_transfer, peer) = host.join().unwrap();
    assert_eq!(code, i32::from(failure));
    assert_eq!(stdout.metadata().unwrap().len(), 0);
    assert!(!office_socket.exists());
    if let Some(mut peer) = peer {
        assert_eq!(
            peer.read(&mut [0]).unwrap(),
            0,
            "root returned while a channel was still owned"
        );
    }
    drop(
        Office::open_for_instance(&folder, key(), ID)
            .expect("root returned before closing Office databases/lock"),
    );
}
fn pass_marker(stream: &UnixStream, fd: std::os::fd::RawFd) {
    let mut marker = [v3::FD_MARKER];
    let mut data = libc::iovec {
        iov_base: marker.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: all pointers borrow live aligned storage covering exactly one fd.
    unsafe {
        let space = libc::CMSG_SPACE(std::mem::size_of_val(&fd) as _) as usize;
        let mut control = vec![0usize; space.div_ceil(std::mem::size_of::<usize>())];
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut data;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = space as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(&fd) as _) as _;
        std::ptr::copy_nonoverlapping(&fd, libc::CMSG_DATA(header).cast(), 1);
        assert_eq!(libc::sendmsg(stream.as_raw_fd(), &message, 0), 1);
    }
}
