//! Actual single-thread host-managed entry. No public socket/path fallback.
use crate::{
    service::{self, ApiLimits, GatewayConfig},
    sessions::{BrowserSessions, HostControl, SessionApi},
    tls::TlsCertificates,
    transport::{HttpLimits, ListenMode},
    Hsts,
};
use org_host_control::{
    gateway::{
        GatewayFd, GatewayFirstLine, GatewayHandoff, GatewayReadyForSecrets, GATEWAY_HANDOFF_FDS,
    },
    ops::{TlsFetchResult, HOST_SOCK_MARKER},
};
use org_socket::{
    confinement,
    rights::{self, ReceiveError, StreamReceiver},
};
use std::{
    io::{self, BufRead, Read, Write},
    net::TcpListener,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::{UnixDatagram, UnixStream},
    },
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_rustls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

const WAIT: Duration = Duration::from_secs(5);
const HEADER: usize = 1024;
fn failure() -> io::Error {
    io::Error::other("authenticated gateway startup unavailable")
}
fn fatal() -> ! {
    std::process::exit(1)
}

fn first_line(reader: &mut impl BufRead) -> io::Result<GatewayFirstLine> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((HEADER + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > HEADER || bytes.pop() != Some(b'\n') {
        return Err(failure());
    }
    serde_json::from_slice(&bytes).map_err(|_| failure())
}
fn send(stream: &mut UnixStream, message: &impl serde::Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(message).map_err(|_| failure())?;
    if bytes.len() >= HEADER {
        return Err(failure());
    }
    bytes.push(b'\n');
    stream.write_all(&bytes)
}
fn byte(
    receiver: &mut StreamReceiver,
    count: usize,
    until: Instant,
    fence: &Mutex<()>,
) -> io::Result<(u8, Vec<OwnedFd>)> {
    loop {
        match receiver.receive_byte(count, fence) {
            Ok(value) => return Ok(value),
            Err(ReceiveError::WouldBlock) => {
                let remaining = until
                    .checked_duration_since(Instant::now())
                    .ok_or_else(failure)?;
                let fd = receiver.fd().ok_or_else(failure)?;
                if !org_socket::poll_readable(fd.as_raw_fd(), remaining)? {
                    return Err(failure());
                }
            }
            // Fatal ancillary framing requires process termination, even before
            // secret handoff: macOS may have installed unreported descriptors.
            Err(ReceiveError::Fatal(_)) => fatal(),
        }
    }
}
/// Decodes the descriptors exactly as the host sends them: the marker byte
/// carries [`GATEWAY_HANDOFF_FDS`] in order, [Transfer, Listener], and nothing
/// else. Each descriptor's socket type is checked before any ACK is sent.
fn descriptors(marker: u8, fds: Vec<OwnedFd>) -> io::Result<(UnixDatagram, TcpListener)> {
    if marker != HOST_SOCK_MARKER || fds.len() != GATEWAY_HANDOFF_FDS.len() {
        return Err(failure());
    }
    let mut transfer = None;
    let mut listener = None;
    for (kind, fd) in GATEWAY_HANDOFF_FDS.into_iter().zip(fds) {
        match kind {
            GatewayFd::Transfer => transfer = Some(rights::unix_datagram(fd)?),
            GatewayFd::Listener => listener = Some(rights::tcp_listener(fd)?),
        }
    }
    Ok((transfer.ok_or_else(failure)?, listener.ok_or_else(failure)?))
}
fn handoff(mut stream: UnixStream) -> io::Result<(OwnedFd, OwnedFd, TcpListener)> {
    stream.set_write_timeout(Some(WAIT))?;
    send(&mut stream, &GatewayReadyForSecrets { landlock: None })?;
    let mut receiver = StreamReceiver::new(stream.try_clone()?);
    let until = Instant::now() + WAIT;
    let fence = Mutex::new(()); // still the only thread; no concurrent spawn exists
    let mut header = Vec::new();
    loop {
        let (value, descriptors) = byte(&mut receiver, 0, until, &fence)?;
        drop(descriptors);
        if value == b'\n' {
            break;
        }
        if header.len() >= HEADER - 1 {
            return Err(failure());
        }
        header.push(value);
    }
    let _: GatewayHandoff = serde_json::from_slice(&header).map_err(|_| failure())?;
    let (marker, fds) = byte(&mut receiver, GATEWAY_HANDOFF_FDS.len(), until, &fence)?;
    let (transfer, listener) = descriptors(marker, fds)?;
    // Type checks precede ACK, and sole receiver has consumed exactly through
    // the marker. Its duplicate is dropped before the actor becomes the reader.
    drop(receiver);
    send(
        &mut stream,
        &serde_json::json!({"v":org_host_control::API_VERSION,"event":"fds.received","of":"handoff"}),
    )?;
    Ok((stream.into(), transfer.into(), listener))
}

/// Called only by the thin executable before any application concurrency.
/// The immutable verified release owns `assets`; no request can pick this path.
pub fn serve(assets: &Path) -> io::Result<()> {
    confinement::disable_core_dumps()?;
    let line = first_line(&mut io::stdin().lock())?;
    let files = crate::release::load(assets, line.release.as_ref()).map_err(|_| failure())?;
    confinement::restrict_gateway(Path::new(&line.rendezvous))?;
    let stream = UnixStream::connect(&line.rendezvous)?;
    let (control, transfer, listener) = handoff(stream)?;
    // Concurrency starts only after enforced readiness and the complete handoff.
    let host = HostControl::from_handoff(
        control,
        transfer,
        line.instance_id.clone(),
        Arc::new(|| fatal()),
    )?;
    let (mode, origin, hsts, secure) = match host.fetch_listener().map_err(|_| failure())? {
        TlsFetchResult::HttpLoopback { origin, hsts } => {
            (ListenMode::HttpLoopback, origin, hsts, false)
        }
        TlsFetchResult::Https {
            cert,
            key,
            origin,
            hsts,
        } => {
            let chain = CertificateDer::pem_slice_iter(cert.as_bytes())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| failure())?;
            let key =
                PrivateKeyDer::from_pem_slice(key.expose().as_bytes()).map_err(|_| failure())?;
            let tls = TlsCertificates::new(chain, key).map_err(|_| failure())?;
            (ListenMode::Https(Arc::new(tls)), origin, hsts, true)
        }
        _ => return Err(failure()),
    };
    let hsts = match hsts {
        org_host_control::listen::Hsts::Off => Hsts::Off,
        org_host_control::listen::Hsts::On => Hsts::On,
        org_host_control::listen::Hsts::Clear => Hsts::Clear,
    };
    let config = GatewayConfig {
        mode,
        origin,
        hsts,
        http: HttpLimits::default(),
        api: ApiLimits::default(),
    };
    service::run_gateway(listener, config, files, move || {
        let sessions = BrowserSessions::new(Arc::clone(&host), line.instance_id, secure)
            .map_err(|_| failure())?;
        let backend = SessionApi::new(sessions);
        let shutdown = async move {
            while host.live() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        Ok((backend, shutdown))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn transfer() -> OwnedFd {
        UnixDatagram::pair().unwrap().0.into()
    }
    fn listener() -> OwnedFd {
        TcpListener::bind("127.0.0.1:0").unwrap().into()
    }

    #[test]
    fn handoff_decodes_exactly_the_transfer_then_the_listener() {
        let (transfer, listener) =
            descriptors(HOST_SOCK_MARKER, vec![transfer(), listener()]).unwrap();
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
}
