//! Actual single-thread host-managed entry. No public socket/path fallback.
use crate::{
    api::SessionApi,
    host::{EventSink, HostControl},
    service::{self, ApiLimits, WebConfig},
    session::SessionCache,
    sse::EventHub,
    tls::TlsCertificates,
    transport::{HttpLimits, ListenMode},
    Hsts,
};
use org_host_control::{
    ops::{TlsFetchResult, HOST_SOCK_MARKER},
    web::{
        LandlockEnforcement, LandlockStatus, WebFd, WebFirstLine, WebHandoff, WebReadyForSecrets,
        WEB_HANDOFF_FDS,
    },
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
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
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
/// What a web process that confined itself without Landlock, where its confinement
/// adapter has that layer and the kernel or a profile did not let it be applied,
/// writes to its standard error once, before its ready line:
///
/// ```text
/// line   = "agentic-org-web: confinement: landlock not applied: " reason LF
/// reason = 1*240 ( %x20-7E )
/// ```
///
/// The process is then confined by its seccomp filter alone and serves as usual.
/// The ready line says only that no Landlock domain is in force; it cannot carry
/// why, so the reason travels here. It is the adapter's own text, as in a refusal
/// line, and is for display only: the host logs it once per start. The same
/// grammar is in `org-host`'s `web.rs`.
pub const LANDLOCK_NOT_APPLIED_PREFIX: &str =
    "agentic-org-web: confinement: landlock not applied: ";
/// What a web process whose UI build could not be loaded writes to its standard error
/// before it exits, before it has confined itself or received anything from its host:
///
/// ```text
/// line = "agentic-org-web: startup refused: ui build: " kind LF
/// kind = the build check that failed, by its fixed name (`InvalidKey`, `UnreadableBuild`, ...)
/// ```
///
/// The name says which rule the release's `ui/` folder broke (a file whose name is not one a
/// build has, such as a macOS `._` file, is `InvalidKey`), never a path or a file's bytes.
pub const BUILD_REFUSAL_PREFIX: &str = "agentic-org-web: startup refused: ui build: ";
/// What every other failed start writes: one fixed line that says nothing more.
const UNAVAILABLE: &str = "agentic-org-web: authenticated startup unavailable";

/// Why the release's UI build could not be loaded: the build check's fixed name.
#[derive(Debug)]
struct BuildRefused(crate::assets::BuildError);
impl std::fmt::Display for BuildRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
impl std::error::Error for BuildRefused {}

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

/// `text` as the reason of a line this process writes for its host: printable ASCII
/// only (anything else becomes `?`), at most [`REFUSAL_REASON_BYTES`] long, never
/// empty.
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
/// the process could not confine itself, the UI build's refusal with the check that
/// failed, and one fixed line for anything else.
pub fn startup_failure_line(error: &io::Error) -> String {
    let inner = error.get_ref();
    if let Some(ConfinementRefused(reason)) =
        inner.and_then(|inner| inner.downcast_ref::<ConfinementRefused>())
    {
        return format!("{REFUSAL_PREFIX}{reason}");
    }
    if let Some(refused) = inner.and_then(|inner| inner.downcast_ref::<BuildRefused>()) {
        return format!("{BUILD_REFUSAL_PREFIX}{refused}");
    }
    UNAVAILABLE.to_owned()
}
/// The line for a Landlock layer that was not applied, without its line feed.
pub fn landlock_not_applied_line(reason: &str) -> String {
    format!("{LANDLOCK_NOT_APPLIED_PREFIX}{}", refusal_reason(reason))
}
fn fatal() -> ! {
    std::process::exit(1)
}

fn first_line(reader: &mut impl BufRead) -> io::Result<WebFirstLine> {
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
/// carries [`WEB_HANDOFF_FDS`] in order, [Transfer, Listener], and nothing
/// else. Each descriptor's socket type is checked before any ACK is sent.
fn descriptors(marker: u8, fds: Vec<OwnedFd>) -> io::Result<(UnixDatagram, TcpListener)> {
    if marker != HOST_SOCK_MARKER || fds.len() != WEB_HANDOFF_FDS.len() {
        return Err(failure());
    }
    let mut transfer = None;
    let mut listener = None;
    for (kind, fd) in WEB_HANDOFF_FDS.into_iter().zip(fds) {
        match kind {
            WebFd::Transfer => transfer = Some(rights::unix_datagram(fd)?),
            WebFd::Listener => listener = Some(rights::tcp_listener(fd)?),
        }
    }
    Ok((transfer.ok_or_else(failure)?, listener.ok_or_else(failure)?))
}
/// The ready line for what confinement reported. A Landlock ABI is reported only by an
/// adapter that restricted this process under it in full; seccomp has no wire field.
fn ready(enforcement: Enforcement) -> WebReadyForSecrets {
    WebReadyForSecrets {
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
    let _: WebHandoff = serde_json::from_slice(&header).map_err(|_| failure())?;
    let (marker, fds) = byte(&mut receiver, WEB_HANDOFF_FDS.len(), until, &fence)?;
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

/// Applies the host's `session.ended`, `account.changed`, and `auth.mode_changed`
/// pushes on the reader thread.
/// No session request precedes serving, so the generation is set before any
/// event can name a cached session; an event before then has nothing to drop.
#[cfg(test)]
pub(crate) fn session_events(
    cache: Arc<SessionCache>,
    generation: Arc<OnceLock<u64>>,
) -> EventSink {
    session_events_with_access(cache, generation, Arc::new(AtomicBool::new(true)))
}

pub(crate) fn session_events_with_access(
    cache: Arc<SessionCache>,
    generation: Arc<OnceLock<u64>>,
    auth_enabled: Arc<AtomicBool>,
) -> EventSink {
    use org_host_control::browser::{AuthModeChanged, SessionEnded, SESSION_ENDED};
    use org_host_control::organization::AccountChangedData;
    Arc::new(move |name: &str, data: &serde_json::Value| {
        if name == org_host_control::event::AUTH_MODE_CHANGED {
            if let Ok(changed) = serde_json::from_value::<AuthModeChanged>(data.clone()) {
                auth_enabled.store(changed.auth_enabled, Ordering::Release);
            }
            return;
        }
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

/// `sessions`, and `tls.renew` passed on to `renewed` (to [`follow_renewals`]): the reader
/// thread cannot call the host itself.
fn with_renewals(sessions: EventSink, renewed: mpsc::Sender<()>) -> EventSink {
    Arc::new(move |name: &str, data: &serde_json::Value| {
        if name == org_host_control::event::TLS_RENEW {
            let _ = renewed.send(());
        } else {
            sessions(name, data);
        }
    })
}

/// What a `tls.fetch` after `tls.renew` means for an `https` listener serving `current`
/// (its origin and `hsts`).
#[derive(Debug)]
enum Renewal {
    /// The pair to use for new handshakes.
    Swap(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>),
    /// Keep serving the current pair (`unavailable`, or a pair that does not load), for
    /// the reason named ([`renewal_kept_line`]).
    Keep(&'static str),
    /// The host answered another listener: a host bug. The process exits without swapping.
    Exit,
}

fn renewal(
    current: &(
        org_host_control::listen::ListenOrigin,
        org_host_control::listen::Hsts,
    ),
    fetched: TlsFetchResult,
) -> Renewal {
    match fetched {
        TlsFetchResult::Https {
            cert,
            key,
            origin,
            hsts,
        } if (origin.clone(), hsts) == *current => {
            let chain =
                CertificateDer::pem_slice_iter(cert.as_bytes()).collect::<Result<Vec<_>, _>>();
            match (
                chain,
                PrivateKeyDer::from_pem_slice(key.expose().as_bytes()),
            ) {
                (Ok(chain), Ok(key)) if !chain.is_empty() => Renewal::Swap(chain, key),
                _ => Renewal::Keep("unusable_pair"),
            }
        }
        TlsFetchResult::Unavailable(_) => Renewal::Keep("unavailable"),
        _ => Renewal::Exit,
    }
}

/// The start of the one line the web process writes when a `tls.renew` did not replace its
/// pair. The host forwards the line's reason to its log when it is one of the closed set
/// (`org-host`'s `web::RENEWAL_KEPT_PREFIX`):
///
/// ```text
/// line   = "agentic-org-web: tls renewal kept the current pair: " reason LF
/// reason = "fetch_failed" / "unavailable" / "unusable_pair" / "replace_failed"
/// ```
///
/// It carries no key material and no certificate bytes.
pub const RENEWAL_KEPT_PREFIX: &str = "agentic-org-web: tls renewal kept the current pair: ";

/// The line [`RENEWAL_KEPT_PREFIX`] describes, for `reason`.
pub fn renewal_kept_line(reason: &str) -> String {
    format!("{RENEWAL_KEPT_PREFIX}{reason}\n")
}

/// After each `tls.renew`, fetches the pair again on its own thread and swaps it for new
/// handshakes ([`TlsCertificates::replace`]). A failed call, an unusable pair or a failed
/// swap keeps the current one and writes one line saying so ([`renewal_kept_line`]); a
/// different listener ends the process.
fn follow_renewals(
    host: Arc<HostControl>,
    tls: Arc<TlsCertificates>,
    current: (
        org_host_control::listen::ListenOrigin,
        org_host_control::listen::Hsts,
    ),
    renewals: mpsc::Receiver<()>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("web-tls-renewal".into())
        .spawn(move || {
            while renewals.recv().is_ok() {
                // Renewals that arrived meanwhile are one fetch.
                while renewals.try_recv().is_ok() {}
                let kept = |reason: &str| {
                    let _ = io::stderr().write_all(renewal_kept_line(reason).as_bytes());
                };
                let Ok(fetched) = host.fetch_listener() else {
                    kept("fetch_failed");
                    continue;
                };
                match renewal(&current, fetched) {
                    Renewal::Swap(chain, key) => {
                        if tls.replace(chain, key).is_err() {
                            kept("replace_failed");
                        }
                    }
                    Renewal::Keep(reason) => kept(reason),
                    Renewal::Exit => fatal(),
                }
            }
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
    let files = crate::release::load(assets, line.release.as_ref())
        .map_err(|refused| io::Error::other(BuildRefused(refused)))?;
    let confined =
        confinement::confine_web(Path::new(&line.rendezvous)).map_err(confinement_refused)?;
    if let Some(reason) = &confined.landlock_not_applied {
        // Before the ready line, in one write, so that the host has it when it has
        // verified the report. A host that does not read it loses only the reason.
        let line = format!("{}\n", landlock_not_applied_line(reason));
        let _ = io::stderr().write_all(line.as_bytes());
    }
    let (control, transfer, listener) = handoff(confined.stream, confined.enforcement)?;
    // Concurrency starts only after enforced readiness and the complete handoff.
    // The cache exists before the connection so the reader can apply every
    // `session.ended` event, in arrival order, from the first frame on.
    let cache = Arc::new(SessionCache::default());
    let hub = EventHub::default();
    let generation = Arc::new(OnceLock::new());
    let auth_enabled = Arc::new(AtomicBool::new(true));
    let (renewed, renewals) = mpsc::channel();
    let events_hub = hub.clone();
    let session_sink = session_events_with_access(
        Arc::clone(&cache),
        Arc::clone(&generation),
        Arc::clone(&auth_enabled),
    );
    let combined_sink: EventSink = Arc::new(move |name: &str, data: &serde_json::Value| {
        session_sink(name, data);
        events_hub.observe_host_event(name, data);
    });
    let host = HostControl::from_handoff(
        control,
        transfer,
        line.instance_id,
        Arc::new(|| fatal()),
        with_renewals(combined_sink, renewed),
    )?;
    let _ = generation.set(host.generation());
    let fetched = host.fetch_listener().map_err(|_| failure())?;
    let scheme = fetched.listen().ok_or_else(failure)?.0;
    if scheme != org_host_control::listen::ListenMode::Https {
        if let Ok(view) =
            host.request::<org_host_control::browser::InstanceView>(&Default::default())
        {
            auth_enabled.store(view.auth_enabled, Ordering::Release);
        }
    }
    let (mode, origin, hsts) = match fetched {
        TlsFetchResult::HttpLoopback { origin, hsts } => (ListenMode::HttpLoopback, origin, hsts),
        TlsFetchResult::HttpPublished { origin, hsts } => (ListenMode::HttpPublished, origin, hsts),
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
            let tls = Arc::new(TlsCertificates::new(chain, key).map_err(|_| failure())?);
            follow_renewals(
                Arc::clone(&host),
                Arc::clone(&tls),
                (origin.clone(), hsts),
                renewals,
            )?;
            (ListenMode::Https(tls), origin, hsts)
        }
        _ => return Err(failure()),
    };
    let hsts = match hsts {
        org_host_control::listen::Hsts::Off => Hsts::Off,
        org_host_control::listen::Hsts::On => Hsts::On,
        org_host_control::listen::Hsts::Clear => Hsts::Clear,
    };
    let browser_origin = origin.clone();
    let config = WebConfig {
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
            let backend = SessionApi::with_access(
                Arc::clone(&host),
                cache,
                &browser_origin,
                scheme,
                hub,
                auth_enabled,
            );
            Ok((backend, host_lost(host)))
        },
        serving,
    )
}

#[cfg(test)]
mod tests;
