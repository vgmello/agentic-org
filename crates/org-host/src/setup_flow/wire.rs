//! Reaching the registered instance's `host.sock`: connecting, verifying the instance and
//! waiting until it is ready (step 6).

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_host_control::{InstanceStatus, InstanceStatusParams, InstanceStatusResult, WebState};
use org_proto::{Hello, HelloParams};

use crate::human::Wire;
use crate::instance::registry::Registry;
use crate::instance::setup::{Journal, Paths, Record};

use super::output::fail;

pub(super) fn connect(socket: &Path, instance_id: &str) -> Result<Wire, String> {
    connect_verified(socket, instance_id, |stream| {
        crate::human::verify_host(stream).map(drop)
    })
}

/// Connects to `socket`, has `verify` judge who listens there, and greets instance
/// `instance_id`.
pub(super) fn connect_verified(
    socket: &Path,
    instance_id: &str,
    verify: impl FnOnce(&UnixStream) -> Result<(), String>,
) -> Result<Wire, String> {
    let stream = UnixStream::connect(socket).map_err(|e| fail("connecting to the service", e))?;
    verify(&stream)?;
    let mut wire = Wire::new(stream)?;
    let hello = wire
        .call::<Hello>(
            &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
            None,
        )
        .map_err(|e| fail("greeting the service", e))?;
    if hello.instance_id.as_deref() != Some(instance_id) {
        return Err("the service answering is not this instance".into());
    }
    Ok(wire)
}

/// Polls until the service answers for `instance_id` with its browser service running, or
/// the deadline passes (X5 review, fourth round, P2-1): a host that answers while its web
/// process has not started, or failed, is not ready, and nothing may say the browser can be
/// used. A failed browser service is restarted by its host, so it is asked again; `failed`
/// says when a failure is not one a restart can mend (another program holds the address).
pub(super) fn ready(
    socket: &Path,
    instance_id: &str,
    within: Duration,
    failed: impl Fn() -> Option<String>,
) -> Result<InstanceStatusResult, String> {
    wait_ready(
        || {
            connect(socket, instance_id).and_then(|mut wire| {
                wire.call::<InstanceStatus>(&InstanceStatusParams {}, None)
                    .map_err(|e| fail("reading the instance status", e))
            })
        },
        instance_id,
        within,
        failed,
    )
}

/// [`ready`], with each question to the service (`ask`) given.
pub(super) fn wait_ready(
    mut ask: impl FnMut() -> Result<InstanceStatusResult, String>,
    instance_id: &str,
    within: Duration,
    failed: impl Fn() -> Option<String>,
) -> Result<InstanceStatusResult, String> {
    let deadline = Instant::now() + within;
    let mut browser = None;
    // Since when a failed browser service's address is held: by its own child for a moment
    // after the failure (the host reaps it), so only one held for a while is another's.
    let mut held_since: Option<Instant> = None;
    loop {
        let why = match ask() {
            Ok(status) if status.instance_id != instance_id => {
                return Err("the service answering is not this instance".into())
            }
            Ok(status) if status.gateway.state == WebState::Running => return Ok(status),
            Ok(status) => {
                let held = (status.gateway.state == WebState::Failed)
                    .then(&failed)
                    .flatten();
                match (held, held_since) {
                    (Some(why), Some(since)) if since.elapsed() >= HELD_FOR => return Err(why),
                    (Some(_), None) => held_since = Some(Instant::now()),
                    (Some(_), Some(_)) => {}
                    (None, _) => held_since = None,
                }
                browser = Some(status.gateway.state);
                None
            }
            Err(why) => Some(why),
        };
        if Instant::now() >= deadline {
            let seconds = within.as_secs();
            return Err(match (why, browser) {
                (None, Some(state)) => format!(
                    "the service runs, but its browser service did not start within {seconds}s \
                     (it is {}; the service's log says why); setup can be run again and \
                     continues where it stopped",
                    match state {
                        WebState::Failed => "failing",
                        WebState::Starting => "still starting",
                        _ => "not configured",
                    }
                ),
                (why, _) => format!(
                    "the service did not become ready within {seconds}s ({}); setup can be run \
                     again and continues where it stopped",
                    why.unwrap_or_default()
                ),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// How long a failed browser service's address must stay held before [`wait_ready`] names
/// another program as the reason.
pub(super) const HELD_FOR: Duration = Duration::from_secs(1);

/// Finds the registered instance `name`: its ID and `host.sock`.
pub(super) fn registered(
    paths: &Paths,
    name: Option<&str>,
) -> Result<(Record, String, PathBuf), String> {
    let (record, id, socket, _) = registered_with_origin(paths, name)?;
    Ok((record, id, socket))
}

/// Like [`registered`], also resolving the active browser origin from `instance.json` when
/// present (reflecting opt-in HTTPS upgrades), falling back to the setup record's origin.
pub(crate) fn registered_with_origin(
    paths: &Paths,
    name: Option<&str>,
) -> Result<
    (
        Record,
        String,
        PathBuf,
        org_host_control::listen::ListenOrigin,
    ),
    String,
> {
    let record =
        Journal::snapshot(&paths.base, name).map_err(|e| fail("reading the setup record", e))?;
    let registry = Registry::open(&paths.locator, &paths.user_temp)
        .map_err(|e| fail("opening the instance locator", e))?;
    // A service that is just starting holds the locator's lock for a moment (it reserves
    // its instance again): the registry waits for it rather than failing.
    let (id, temp, browser) = registry
        .locate_with_browser(name, &record.input.root)
        .map_err(|e| fail("locating the instance", e))?;
    if record.instance_id.as_deref() != Some(id.as_str()) {
        return Err("the instance is not the one setup recorded; run agentic-org setup".into());
    }
    let origin = browser
        .filter(|active| {
            crate::browser_listener::browser_matches_setup(active, &record.input.listen)
        })
        .map(|active| active.origin().clone())
        .unwrap_or_else(|| record.input.listen.origin().clone());
    Ok((record, id, temp.join("host.sock"), origin))
}

/// `work` on a thread of its own, waited for until `limit` has passed: then this returns,
/// and the thread is left to end with the process (a command that ends right after). It is
/// how a blocking connect, greeting and call are bound by one deadline.
pub(super) fn within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (sender, answer) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("admin-reset".into())
        .spawn(move || {
            let _ = sender.send(work());
        })
        .map_err(|e| fail("asking the service", e))?;
    answer.recv_timeout(limit).unwrap_or_else(|_| {
        Err(format!(
            "the service did not answer within {}s",
            limit.as_secs()
        ))
    })
}
