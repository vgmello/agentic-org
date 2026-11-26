//! What the web process is started from: its configuration, its admitted image and the
//! HTTPS gate that starts it only while the stored pair verifies.

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use org_host_control::listen::ListenConfig;
use org_platform_host::admission::Admission;

use crate::{
    server_check::{NoAdmission, ServerCheck},
    sync::lock,
};

use super::stderr::shown;

pub(crate) struct Configuration {
    pub program: PathBuf,
    pub instance: String,
    pub runtime: PathBuf,
    pub listen: ListenConfig,
    pub check: ServerCheck,
    pub critical_paths: Vec<PathBuf>,
    pub release: Option<org_host_control::web::WebRelease>,
    /// The `https` listener's pair (`crate::web_tls`); `None` for plain HTTP.
    pub tls: Option<Arc<crate::web_tls::WebTls>>,
    /// Invoked when the running web process reports a transient `tls.renew` fetch failure
    /// (`fetch_failed` or `unavailable`), so the host can schedule a bounded re-announcement.
    pub on_renewal_kept: Option<Arc<dyn Fn(&'static str) + Send + Sync>>,
}

/// The selected release's web process image pin (`Err`: the release has none) and the
/// release identity bound into the web process's first line.
pub(crate) type SelectedRelease<E> = (
    Result<crate::server_check::Pin, E>,
    org_host_control::web::WebRelease,
);

/// The web process admission policy of a build (`debug`: `cfg!(debug_assertions)`) with app
/// team id `team_id` and the selected `release`, if any: the check every start uses and
/// the release binding of its first line. A malformed team id or a release without a
/// web process image hash refuses every start; a selected release pins its exact image.
pub(crate) fn admission<E>(
    debug: bool,
    team_id: Option<&str>,
    release: Option<SelectedRelease<E>>,
) -> (ServerCheck, Option<org_host_control::web::WebRelease>) {
    let check = ServerCheck::web_for_build(debug, team_id)
        .unwrap_or(ServerCheck::WebRefused("malformed app team id"));
    match release {
        None => (check, None),
        Some((Ok(pin), binding)) => (check.pinned(pin), Some(binding)),
        Some((Err(_), binding)) => (
            ServerCheck::WebRefused("the selected release has no agentic-org-web image hash"),
            Some(binding),
        ),
    }
}

/// The web process's admission, built once in a host's lifetime.
///
/// A system that executes verified bytes reads, checks and seals the web process's image
/// when the admission is built: every web process this host starts, each restart included,
/// then runs exactly the bytes read at that moment, and no restart reads or hashes the
/// program again. Where the loaded image is checked instead, building reads nothing and
/// this changes nothing.
///
/// It has the properties of the office servers' admission (`supervisor::child::Images`),
/// with its own state and its own words:
///
/// - the build begins with the supervisor, on its own thread, beside the host's start;
/// - a start waits for a build that is under way: there is never more than one;
/// - a build that fails is logged, fails that start with its reason, and is asked for again
///   by the next start, so the supervisor's limit of consecutive failed starts ends the
///   retries;
/// - an admission, once built, is never built again.
pub(crate) struct Image {
    check: ServerCheck,
    program: PathBuf,
    built: Mutex<Option<Arc<Admission>>>,
}

impl Image {
    /// The admission of `check`'s web process started from `program`, not built yet.
    pub(super) fn new(check: &ServerCheck, program: &Path) -> Arc<Self> {
        Arc::new(Self {
            check: check.clone(),
            program: program.to_owned(),
            built: Mutex::new(None),
        })
    }

    /// Begins the build on its own thread. Without a thread to build it on, the first
    /// start builds it.
    pub(super) fn begin(self: &Arc<Self>) {
        let building = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("web-image".into())
            .spawn(move || building.build_at_start());
    }

    /// The build a host makes at its start, with the reason logged when there is none.
    pub(super) fn build_at_start(&self) {
        if let Err(none) = self.admission() {
            crate::host_log::log(&format!(
                "the web process cannot be started: {}; each start asks again",
                crate::host_log::untrusted(&none.to_string())
            ));
        }
    }

    /// The host's admission of its web process, built now if no earlier attempt
    /// succeeded. A caller waits for a build that is under way.
    pub(super) fn admission(&self) -> Result<Arc<Admission>, NoAdmission> {
        let mut built = lock(&self.built);
        if let Some(admission) = built.as_ref() {
            return Ok(Arc::clone(admission));
        }
        let admission = Arc::new(self.check.admission(&self.program)?);
        if let Some(line) = self.check.describe_admitted(&self.program, &admission) {
            crate::host_log::log(&line);
        }
        *built = Some(Arc::clone(&admission));
        Ok(admission)
    }
}

/// The refusal of a system on which the web process cannot be confined: fixed
/// wording, then the adapter's reason (which call failed with which error, or that
/// the system has no adapter).
pub(super) fn unconfinable(reason: io::Error) -> io::Error {
    io::Error::other(format!(
        "the web process requires process confinement, which is unavailable here: {}",
        shown(&reason.to_string())
    ))
}

/// The pre-spawn check of an `https` listener (X-02 §2) at `now` (Unix seconds), after its
/// listener fields, origin and `hsts`, which the snapshot passed when it was read: a stored
/// pair is present (`tls_missing`), covers the origin (`tls_name_mismatch`), is valid now and
/// its key can be read. The refusal names the reason for the host log.
pub(super) fn https_gate(tls: Option<&crate::web_tls::WebTls>, now: i64) -> io::Result<()> {
    use crate::web_tls::Unservable;
    let refused = |reason: &str| {
        io::Error::other(format!(
            "the web process requires a verified TLS pair for its https listener: {reason}"
        ))
    };
    match tls.map(|tls| tls.servable(now)) {
        Some(Ok(())) => Ok(()),
        None | Some(Err(Unservable::Missing)) => Err(refused("tls_missing")),
        Some(Err(Unservable::Failure(_))) => Err(refused(
            "tls_name_mismatch: the stored certificate does not name the origin's host",
        )),
        Some(Err(Unservable::NotValid)) => Err(refused(
            "tls_not_valid: the stored certificate is not valid now; the administrator \
             replaces it (tls.set)",
        )),
        Some(Err(Unservable::Storage)) => Err(refused(
            "tls_missing: the stored pair or its key cannot be read",
        )),
    }
}

/// How often the `https` gate is asked again while the supervisor polls its eligibility.
const HTTPS_GATE_EVERY: Duration = Duration::from_secs(2);

/// `eligible`, and the `https` gate ([`https_gate`]) for `tls`: the web process starts only
/// while the stored pair can be served, and the supervisor waits, without counting a failed
/// start, while it cannot (a supplied pair that expired waits for the administrator's
/// `tls.set`); a running one is stopped when it no longer can. To avoid repeating the gate
/// check on every 50 ms supervisor poll, the last boolean verdict is cached for up to
/// [`HTTPS_GATE_EVERY`] (2 s), after which `https_gate` re-evaluates the stored pair and its
/// validity window against the current clock; each change of verdict is logged once.
pub(super) fn gated(
    eligible: Arc<dyn Fn() -> bool + Send + Sync>,
    tls: Arc<crate::web_tls::WebTls>,
) -> Arc<dyn Fn() -> bool + Send + Sync> {
    let last: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    Arc::new(move || {
        if !eligible() {
            return false;
        }
        let mut last = lock(&last);
        if let Some((at, served)) = *last {
            if at.elapsed() < HTTPS_GATE_EVERY {
                return served;
            }
        }
        let verdict = match crate::web_tls::now() {
            Some(now) => https_gate(Some(&tls), now),
            None => Err(io::Error::other("the clock is unavailable")),
        };
        let served = verdict.is_ok();
        if last.map(|(_, was)| was) != Some(served) {
            match &verdict {
                Err(why) => crate::host_log::log(&format!(
                    "the web process is not started: {}",
                    crate::host_log::untrusted(&why.to_string())
                )),
                Ok(()) if last.is_some() => crate::host_log::log(
                    "the stored TLS pair can be served: the web process is started",
                ),
                Ok(()) => {}
            }
        }
        *last = Some((Instant::now(), served));
        served
    })
}
