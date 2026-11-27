//! Launch HQ `--create` server stand-in for `web_process_tests/organizations.rs`.
//! Re-executes this integration test binary with `--exact create_server_helper::hq_create_server`.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use org_office_launch::{OfficeFile, ServerEvent};

const CREATE_PID: &str = "ORG_HOST_TEST_CREATE_PID";
const CREATE_HANG: &str = "ORG_HOST_TEST_CREATE_HANG";

pub(crate) fn write_create_server(path: &Path, pid_file: &Path, hang: bool) {
    let hang = if hang {
        format!("{CREATE_HANG}=1 ")
    } else {
        String::new()
    };
    let body = format!(
        "{CREATE_PID}='{}' {hang}exec '{}' --exact create_server_helper::hq_create_server \
             --ignored --test-threads=1 >/dev/null",
        pid_file.display(),
        std::env::current_exe().unwrap().display()
    );
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
#[ignore = "run only as the launch HQ create server of a process test"]
fn hq_create_server() {
    use org_office_launch::HostFirstLine;
    use std::os::unix::net::UnixStream;
    let Some(pid_file) = std::env::var_os(CREATE_PID) else {
        return;
    };
    fs::write(&pid_file, std::process::id().to_string()).unwrap();
    let mut stdin = std::io::BufReader::new(std::io::stdin());
    let Ok(Some(HostFirstLine::Create(create))) =
        org_office_launch::read_first_line(&mut stdin, org_office_launch::FirstLine::Create)
    else {
        std::process::exit(2);
    };
    let mut stream = UnixStream::connect(&create.office.rendezvous).unwrap();
    org_proto::frame::write_message(&mut stream, &org_office_launch::ReadyForSecrets).unwrap();
    let fence = std::sync::Mutex::new(());
    let mut receiver = org_socket::rights::StreamReceiver::new(stream.try_clone().unwrap());
    let mut receive = |count: usize, marker: bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let got = if marker {
                receiver
                    .receive_marker(count, org_office_launch::FD_MARKER, &fence)
                    .map(|fds| (org_office_launch::FD_MARKER, fds))
            } else {
                receiver.receive_byte(count, &fence)
            };
            match got {
                Ok(value) => return value,
                Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::yield_now()
                }
                Err(_) => std::process::exit(4),
            }
        }
    };
    let mut header = Vec::new();
    loop {
        let (b, _) = receive(0, false);
        if b == b'\n' {
            break;
        }
        header.push(b);
    }
    let Ok(org_office_launch::Handoff::Create { fds, .. }) =
        org_office_launch::decode_handoff(&header)
    else {
        std::process::exit(5);
    };
    let (_, mut transfer) = receive(fds.len(), true);
    drop(receiver);
    stream.set_nonblocking(false).unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &org_office_launch::RendezvousEvent::FdsReceived {
            of: "handoff".into(),
        },
    )
    .unwrap();
    let Some(transfer) = transfer.pop() else {
        std::process::exit(6);
    };
    let mut control =
        org_socket::rights::DatagramReceiver::new(std::os::unix::net::UnixDatagram::from(transfer));
    let mut payload = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
    let deadline = Instant::now() + Duration::from_secs(5);
    let received = loop {
        match control.receive_up_to(&mut payload, 1, &fence) {
            Ok(received) => break received,
            Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(_) => std::process::exit(7),
        }
    };
    let Ok(org_office_launch::TransferRecord::WriterControl {
        office_id,
        channel_id,
    }) = org_office_launch::decode_transfer_record(&payload[..received.bytes])
    else {
        std::process::exit(8);
    };
    if office_id != create.office.office_id || received.descriptors.len() != 1 {
        std::process::exit(9);
    }
    let Some(writer) = received.descriptors.into_iter().next() else {
        std::process::exit(9);
    };
    let writer = UnixStream::from(writer);
    writer.set_nonblocking(false).unwrap();
    writer
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut writer = std::io::BufReader::new(writer);
    let Ok(Some(admission)) =
        org_office_launch::writer::read::<org_office_launch::writer::Admission>(&mut writer)
    else {
        std::process::exit(11);
    };
    if !admission.check()
        || admission.identity.office_id != create.office.office_id
        || admission.identity.organization_id != create.org_id
    {
        std::process::exit(12);
    }
    let mut seen = std::path::PathBuf::from(&pid_file).into_os_string();
    seen.push(".admission");
    fs::write(seen, serde_json::to_vec(&admission).unwrap()).unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &org_office_launch::RendezvousEvent::FdsReceived { of: channel_id },
    )
    .unwrap();
    if std::env::var_os(CREATE_HANG).is_some() {
        std::thread::sleep(Duration::from_secs(600));
        std::process::exit(0);
    }
    let Ok(office) = OfficeFile::selected(
        &create.office.office_id,
        &create.office_name,
        create.role,
        &create.org_id,
        &create.org_name,
        "2026-11-08T00:00:00.000Z",
    ) else {
        std::process::exit(10);
    };
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &org_office_launch::RendezvousEvent::Server(ServerEvent::Created { office }),
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(30));
    std::process::exit(0);
}
