//! Actual single-thread host-managed entry. No public socket/path fallback.
use crate::{
    api::SessionApi,
    host::{EventSink, HostControl},
    service::{self, ApiLimits, GatewayConfig},
    session::SessionCache,
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
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio_rustls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

const WAIT: Duration = Duration::from_secs(5);
const HEADER: usize = 1024;
fn failure() -> io::Error {
    io::Error::other("authenticated web process startup unavailable")
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

/// The host holds the write end of stdin for the web process's whole life and
/// never writes after the first line. Its EOF (or a read error) means the host is
/// gone: the connection is closed, so serving ends and the listener closes, even
/// if the control connection's EOF were never observed. Later bytes are ignored,
/// so nothing can swap the launch input mid-life.
fn watch_stdin(mut input: impl Read + Send + 'static, host: Arc<HostControl>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("web-host-stdin".into())
        .spawn(move || {
            let mut sink = [0u8; 256];
            loop {
                match input.read(&mut sink) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            host.close();
        })
        .map(drop)
}

/// Applies the host's `session.ended` and `account.changed` pushes to the cache on the
/// reader thread.
/// No session request precedes serving, so the generation is set before any
/// event can name a cached session; an event before then has nothing to drop.
pub(crate) fn session_events(
    cache: Arc<SessionCache>,
    generation: Arc<OnceLock<u64>>,
) -> EventSink {
    use org_host_control::browser::{SessionEnded, SESSION_ENDED};
    use org_host_control::organization::AccountChangedData;
    Arc::new(move |name: &str, data: &serde_json::Value| {
        // An organization was linked: the account's cached sessions predate it.
        if name == org_host_control::event::ACCOUNT_CHANGED {
            if let (Some(generation), Ok(changed)) = (
                generation.get(),
                serde_json::from_value::<AccountChangedData>(data.clone()),
            ) {
                cache.account_changed(&changed.account_id, *generation);
            }
            return;
        }
        if name != SESSION_ENDED {
            return;
        }
        let (Some(generation), Ok(ended)) = (
            generation.get(),
            serde_json::from_value::<SessionEnded>(data.clone()),
        ) else {
            return;
        };
        cache.ended(&ended.session_id, *generation);
    })
}

/// Resolves once the host connection is gone (control EOF, a failed call or
/// stdin EOF): serving stops and the listener closes with the process.
async fn host_lost(host: Arc<HostControl>) {
    while host.live() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
    // The cache exists before the connection so the reader can apply every
    // `session.ended` event, in arrival order, from the first frame on.
    let cache = Arc::new(SessionCache::default());
    let generation = Arc::new(OnceLock::new());
    let host = HostControl::from_handoff(
        control,
        transfer,
        line.instance_id,
        Arc::new(|| fatal()),
        session_events(Arc::clone(&cache), Arc::clone(&generation)),
    )?;
    let _ = generation.set(host.generation());
    let (mode, origin, hsts) = match host.fetch_listener().map_err(|_| failure())? {
        TlsFetchResult::HttpLoopback { origin, hsts } => (ListenMode::HttpLoopback, origin, hsts),
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
            (ListenMode::Https(Arc::new(tls)), origin, hsts)
        }
        _ => return Err(failure()),
    };
    let hsts = match hsts {
        org_host_control::listen::Hsts::Off => Hsts::Off,
        org_host_control::listen::Hsts::On => Hsts::On,
        org_host_control::listen::Hsts::Clear => Hsts::Clear,
    };
    let browser_origin = origin.clone();
    let config = GatewayConfig {
        mode,
        origin,
        hsts,
        http: HttpLimits::default(),
        api: ApiLimits::default(),
    };
    watch_stdin(io::stdin(), Arc::clone(&host))?;
    let reporting = Arc::clone(&host);
    // Sent exactly once, after HTTP initialization and before the first accept,
    // on the runtime's driving thread while no request is served yet.
    let serving = move || reporting.report_serving().map_err(|_| failure());
    service::run_web_reporting(
        listener,
        config,
        files,
        move || {
            let backend = SessionApi::new(Arc::clone(&host), cache, &browser_origin);
            Ok((backend, host_lost(host)))
        },
        serving,
    )
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
