//! Actual single-thread host-managed entry. No public socket/path fallback.
use crate::{
    api::VersionApi,
    host::HostControl,
    service::{self, ApiLimits, GatewayConfig},
    tls::TlsCertificates,
    transport::{HttpLimits, ListenMode},
    Hsts,
};
use org_host_control::{
    gateway::{
        GatewayFd, GatewayFirstLine, GatewayHandoff, GatewayReadyForSecrets, LandlockEnforcement,
        LandlockStatus, GATEWAY_HANDOFF_FDS,
    },
    ops::{TlsFetchResult, HOST_SOCK_MARKER},
};
use org_platform::confinement::{self, Enforcement};
use org_socket::rights::{self, ReceiveError, StreamReceiver};
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
    io::Error::other("authenticated web process startup unavailable")
}

/// What a web process that could not confine itself writes to its standard error
/// before it exits, so that the host that started it can log why:
///
/// ```text
/// line   = "agentic-org-web: startup refused: confinement: " reason LF
/// reason = 1*240 ( %x20-7E )
/// ```
///
/// The reason is the confinement adapter's own text: the names of the system call
/// and of its error, never a path, an argument or anything it was sent. It is for
/// display only. The host reads at most this one line, and only from a child that
/// ended before it reported ready; the same grammar is in `org-host`'s `web.rs`.
pub const REFUSAL_PREFIX: &str = "agentic-org-web: startup refused: confinement: ";
/// The longest reason of a refusal line, in bytes.
pub const REFUSAL_REASON_BYTES: usize = 240;
/// What every other failed start writes: one fixed line that says nothing more.
const UNAVAILABLE: &str = "agentic-org-web: authenticated startup unavailable";

/// Why this process could not confine itself: the adapter's reason, as the refusal
/// line carries it.
#[derive(Debug)]
struct ConfinementRefused(String);
impl std::fmt::Display for ConfinementRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ConfinementRefused {}

/// `text` as a refusal line's reason: printable ASCII only (anything else becomes
/// `?`), at most [`REFUSAL_REASON_BYTES`] long, never empty.
fn refusal_reason(text: &str) -> String {
    let reason: String = text
        .chars()
        .take(REFUSAL_REASON_BYTES)
        .map(|c| if matches!(c, ' '..='~') { c } else { '?' })
        .collect();
    if reason.is_empty() {
        "no reason given".to_owned()
    } else {
        reason
    }
}

/// The error of a failed `confine_web`, kept apart from every other startup failure:
/// it alone is reported with its reason.
fn confinement_refused(error: io::Error) -> io::Error {
    io::Error::other(ConfinementRefused(refusal_reason(&error.to_string())))
}

/// The one line the executable writes to its standard error for a failed
/// [`serve`], without its line feed: the refusal line with the adapter's reason when
/// the process could not confine itself, and one fixed line for anything else.
pub fn startup_failure_line(error: &io::Error) -> String {
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ConfinementRefused>())
    {
        Some(ConfinementRefused(reason)) => format!("{REFUSAL_PREFIX}{reason}"),
        None => UNAVAILABLE.to_owned(),
    }
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
/// The ready line for what confinement reported. A Landlock ABI is reported only by an
/// adapter that restricted this process under it in full; seccomp has no wire field.
fn ready(enforcement: Enforcement) -> GatewayReadyForSecrets {
    GatewayReadyForSecrets {
        landlock: enforcement.landlock_abi.map(|abi| LandlockStatus {
            abi,
            enforcement: LandlockEnforcement::FullyEnforced,
        }),
    }
}
fn handoff(
    mut stream: UnixStream,
    enforcement: Enforcement,
) -> io::Result<(OwnedFd, OwnedFd, TcpListener)> {
    stream.set_write_timeout(Some(WAIT))?;
    send(&mut stream, &ready(enforcement))?;
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

/// Resolves once the host connection is gone (control EOF, a failed call or
/// stdin EOF): serving stops and the listener closes with the process.
async fn host_lost(host: Arc<HostControl>) {
    while host.live() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The directory that holds this image's file, and beside it the release's `ui`
/// build: the directory of `image` (what the system says this process executes)
/// when that names a file, as it does for an image started from its path.
///
/// A host on GNU/Linux starts the image from a sealed copy in memory, which has no
/// path: the system then names a file that does not exist. The host's admission
/// gives every child its pinned path as its first argument, `started_as`, on every
/// system, so that names the release instead; it must be absolute and name a file.
/// Nothing a request sends can reach either value.
pub fn release_root(
    image: io::Result<std::path::PathBuf>,
    started_as: Option<std::ffi::OsString>,
) -> io::Result<std::path::PathBuf> {
    let directory = |path: &Path| path.parent().map(Path::to_owned);
    if let Some(root) = image
        .ok()
        .filter(|path| path.is_file())
        .as_deref()
        .and_then(directory)
    {
        return Ok(root);
    }
    started_as
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .as_deref()
        .and_then(directory)
        .ok_or_else(|| io::Error::other("missing release root"))
}

/// Called only by the thin executable before any application concurrency.
/// The immutable verified release owns `assets`; no request can pick this path.
pub fn serve(assets: &Path) -> io::Result<()> {
    confinement::disable_core_dumps()?;
    let line = first_line(&mut io::stdin().lock())?;
    let files = crate::release::load(assets, line.release.as_ref()).map_err(|_| failure())?;
    let (stream, enforcement) =
        confinement::confine_web(Path::new(&line.rendezvous)).map_err(confinement_refused)?;
    let (control, transfer, listener) = handoff(stream, enforcement)?;
    // Concurrency starts only after enforced readiness and the complete handoff.
    let host =
        HostControl::from_handoff(control, transfer, line.instance_id, Arc::new(|| fatal()))?;
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
            // Browser sign-in waits for stored sessions (FC-159, ADR-0013).
            let backend = VersionApi;
            Ok((backend, host_lost(host)))
        },
        serving,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_confinement_refusal_is_reported_with_its_reason_on_one_bounded_line() {
        let refusal = io::Error::other(
            "gateway confinement unavailable: landlock_restrict_self failed with EPERM \
             (Operation not permitted (os error 1))",
        );
        assert_eq!(
            startup_failure_line(&confinement_refused(refusal)),
            "agentic-org-web: startup refused: confinement: gateway confinement unavailable: \
             landlock_restrict_self failed with EPERM (Operation not permitted (os error 1))"
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
